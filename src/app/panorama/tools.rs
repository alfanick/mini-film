use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use anyhow::{Context, Result, anyhow, bail};

const REQUIRED_TOOLS: &[&str] = &[
    "pto_gen",
    "cpfind",
    "cpclean",
    "pto_var",
    "autooptimiser",
    "pano_modify",
    "hugin_executor",
    "nona",
    "enblend",
];

#[derive(Clone, Debug)]
pub(crate) struct HuginToolchain {
    bin_dir: Option<PathBuf>,
    pto_gen: PathBuf,
    cpfind: PathBuf,
    cpclean: PathBuf,
    pto_var: PathBuf,
    autooptimiser: PathBuf,
    pano_modify: PathBuf,
    hugin_executor: PathBuf,
    fingerprint: String,
}

#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct PanoramaCapability {
    pub(crate) available: bool,
    pub(crate) reason: Option<String>,
}

impl PanoramaCapability {
    pub(crate) fn probe(bin_dir: Option<&Path>) -> Self {
        match HuginToolchain::discover(bin_dir) {
            Ok(_) => Self {
                available: true,
                reason: None,
            },
            Err(error) => Self {
                available: false,
                reason: Some(error.to_string()),
            },
        }
    }
}

impl HuginToolchain {
    pub(crate) fn discover(explicit_bin_dir: Option<&Path>) -> Result<Self> {
        let bin_dir = explicit_bin_dir
            .map(Path::to_path_buf)
            .or_else(|| env::var_os("MINI_FILM_HUGIN_BIN_DIR").map(PathBuf::from));
        if let Some(bin_dir) = &bin_dir
            && !bin_dir.is_dir()
        {
            bail!(
                "Hugin binary directory does not exist: {}",
                bin_dir.display()
            );
        }

        let resolve = |name: &str| {
            bin_dir
                .as_ref()
                .map(|directory| directory.join(name))
                .unwrap_or_else(|| PathBuf::from(name))
        };
        let mut missing = Vec::new();
        let mut versions = Vec::new();
        for name in REQUIRED_TOOLS {
            let path = resolve(name);
            match probe_tool(&path) {
                Ok(version) => versions.push(format!("{name}:{version}")),
                Err(error) => missing.push(format!("{name} ({error})")),
            }
        }
        if !missing.is_empty() {
            bail!(
                "panorama mode requires Hugin CLI tools; missing or unusable: {}",
                missing.join(", ")
            );
        }

        let pto_gen = resolve("pto_gen");
        let cpfind = resolve("cpfind");
        let cpclean = resolve("cpclean");
        let pto_var = resolve("pto_var");
        let autooptimiser = resolve("autooptimiser");
        let pano_modify = resolve("pano_modify");
        let hugin_executor = resolve("hugin_executor");
        Ok(Self {
            bin_dir,
            pto_gen,
            cpfind,
            cpclean,
            pto_var,
            autooptimiser,
            pano_modify,
            hugin_executor,
            fingerprint: versions.join("|"),
        })
    }

    pub(crate) fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub(crate) fn pto_gen(&self) -> &Path {
        &self.pto_gen
    }

    pub(crate) fn cpfind(&self) -> &Path {
        &self.cpfind
    }

    pub(crate) fn cpclean(&self) -> &Path {
        &self.cpclean
    }

    pub(crate) fn pto_var(&self) -> &Path {
        &self.pto_var
    }

    pub(crate) fn autooptimiser(&self) -> &Path {
        &self.autooptimiser
    }

    pub(crate) fn pano_modify(&self) -> &Path {
        &self.pano_modify
    }

    pub(crate) fn hugin_executor(&self) -> &Path {
        &self.hugin_executor
    }

    pub(crate) fn run<I, S>(
        &self,
        label: &str,
        binary: &Path,
        args: I,
        jobs: usize,
    ) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let mut command = Command::new(binary);
        command
            .args(args.into_iter().map(Into::into))
            .env("OMP_NUM_THREADS", jobs.max(1).to_string());
        if let Some(bin_dir) = &self.bin_dir {
            let existing = env::var_os("PATH").unwrap_or_default();
            let paths = std::iter::once(bin_dir.clone())
                .chain(env::split_paths(&existing))
                .collect::<Vec<_>>();
            let path = env::join_paths(paths).context("building Hugin PATH")?;
            command.env("PATH", path);
        }
        let output = command
            .output()
            .with_context(|| format!("running {label} at {}", binary.display()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            bail!(
                "{label} failed with status {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                stdout.trim(),
                stderr.trim()
            );
        }
        Ok(output)
    }
}

fn probe_tool(path: &Path) -> Result<String> {
    let output = Command::new(path)
        .arg("--version")
        .output()
        .or_else(|_| Command::new(path).arg("--help").output())
        .map_err(|error| anyhow!(error))?;
    let text = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    let first = String::from_utf8_lossy(text)
        .lines()
        .next()
        .unwrap_or("available")
        .trim()
        .to_string();
    Ok(if first.is_empty() {
        "available".to_string()
    } else {
        first
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::{fs, io::Write};

    fn write_tool(path: &Path) {
        let mut file = fs::File::create(path).unwrap();
        writeln!(file, "#!/bin/sh").unwrap();
        writeln!(file, "echo test-tool 1.0").unwrap();
        #[cfg(unix)]
        {
            let mut permissions = file.metadata().unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions).unwrap();
        }
    }

    #[test]
    fn capability_requires_complete_hugin_suite() {
        let dir = tempfile::tempdir().unwrap();
        write_tool(&dir.path().join("pto_gen"));
        let capability = PanoramaCapability::probe(Some(dir.path()));
        assert!(!capability.available);
        assert!(capability.reason.unwrap().contains("cpfind"));

        for tool in REQUIRED_TOOLS.iter().skip(1) {
            write_tool(&dir.path().join(tool));
        }
        let capability = PanoramaCapability::probe(Some(dir.path()));
        assert!(capability.available, "{:?}", capability.reason);
    }
}
