use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};

use crate::cli::JpegSubsampling;

/// Develop one RAW file with RawTherapee.
///
/// RawTherapee is the only RAW engine. TIFF-bound work uses a 16-bit TIFF
/// intermediate, while JPEG-bound work can use an 8-bit JPEG intermediate, and
/// batch/sampler can suppress RawTherapee's stdout/stderr to keep progress
/// output readable.
pub(crate) fn run_raw_develop(
    rawtherapee: &Path,
    profiles: &[PathBuf],
    raw: &Path,
    output_tiff: &Path,
    quiet: bool,
) -> Result<()> {
    run_rawtherapee(
        rawtherapee,
        profiles,
        raw,
        output_tiff,
        RawOutput::Tiff16,
        quiet,
    )
}

pub(crate) fn run_raw_develop_jpeg(
    rawtherapee: &Path,
    profiles: &[PathBuf],
    raw: &Path,
    output_jpeg: &Path,
    quality: u8,
    subsampling: JpegSubsampling,
    quiet: bool,
) -> Result<()> {
    run_rawtherapee(
        rawtherapee,
        profiles,
        raw,
        output_jpeg,
        RawOutput::Jpeg8 {
            quality,
            subsampling,
        },
        quiet,
    )
}

enum RawOutput {
    Tiff16,
    Jpeg8 {
        quality: u8,
        subsampling: JpegSubsampling,
    },
}

/// Invoke RawTherapee CLI and require it to create the requested intermediate.
///
/// RawTherapee can print warnings or fail in ways that are not useful for later
/// pipeline steps, so this wrapper creates the destination directory, requests
/// overwrite, selects either 16-bit TIFF or 8-bit JPEG output, optionally
/// silences logs for batch, then checks both process status and actual
/// output-file existence.
fn run_rawtherapee(
    rawtherapee: &Path,
    profiles: &[PathBuf],
    raw: &Path,
    output: &Path,
    output_format: RawOutput,
    quiet: bool,
) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let mut command = Command::new(rawtherapee);
    command.arg("-q").arg("-Y");
    for profile in profiles {
        command.arg("-p").arg(profile);
    }
    command.arg("-o").arg(output);
    match output_format {
        RawOutput::Tiff16 => {
            command.arg("-t").arg("-b16");
        }
        RawOutput::Jpeg8 {
            quality,
            subsampling,
        } => {
            command
                .arg(format!("-j{}", quality.clamp(1, 100)))
                .arg(format!("-js{}", rawtherapee_subsampling(subsampling)));
        }
    }
    command.arg("-c").arg(raw);
    if quiet {
        command.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let status = command
        .status()
        .with_context(|| format!("running {}", rawtherapee.display()))?;

    if !status.success() {
        bail!("rawtherapee failed with status {status}");
    }
    if !output.exists() {
        bail!("rawtherapee finished without creating {}", output.display());
    }

    Ok(())
}

fn rawtherapee_subsampling(subsampling: JpegSubsampling) -> u8 {
    match subsampling {
        JpegSubsampling::S420 => 1,
        JpegSubsampling::S422 => 2,
        JpegSubsampling::S444 => 3,
    }
}

#[cfg(test)]
mod tests {
    const RAWTHAPE_HELPER_SCRIPT: &str = include_str!("../../scripts/tests/rawtherapee_helper.sh");

    use super::*;
    use std::{fs, io::Write, os::unix::fs::PermissionsExt};

    #[test]
    fn jpeg_subsampling_maps_to_rawtherapee_js_values() {
        assert_eq!(rawtherapee_subsampling(JpegSubsampling::S420), 1);
        assert_eq!(rawtherapee_subsampling(JpegSubsampling::S422), 2);
        assert_eq!(rawtherapee_subsampling(JpegSubsampling::S444), 3);
    }

    fn write_helper_script(
        path: &std::path::Path,
        create_output: bool,
        output_image: Option<&std::path::Path>,
        exit_code: i32,
    ) -> Result<PathBuf> {
        let log_file = path.with_file_name("command.log");
        let rendered = RAWTHAPE_HELPER_SCRIPT
            .replace("__LOG_FILE__", &log_file.display().to_string())
            .replace(
                "__OUTPUT_IMAGE__",
                &output_image
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
            )
            .replace("__CREATE_OUTPUT__", if create_output { "1" } else { "0" })
            .replace("__EXIT_CODE__", &exit_code.to_string());
        let mut file = fs::File::create(path)?;
        file.write_all(rendered.as_bytes())?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)?;
        Ok(path.with_file_name("command.log"))
    }

    #[test]
    fn run_raw_develop_tiff_invokes_expected_rawtherapee_args() {
        let temp = tempfile::tempdir().unwrap();
        let raw = temp.path().join("frame.RW2");
        fs::write(&raw, b"raw").unwrap();
        let raw_log = write_helper_script(&temp.path().join("rawtherapee"), true, None, 0).unwrap();
        let out = temp.path().join("out.tif");
        run_raw_develop(&temp.path().join("rawtherapee"), &[], &raw, &out, true).unwrap();

        let log = fs::read_to_string(raw_log).unwrap();
        assert!(log.contains("-q"));
        assert!(log.contains("-Y"));
        assert!(log.contains("-t"));
        assert!(log.contains("-b16"));
        assert!(log.contains(&out.to_string_lossy().to_string()));
    }

    #[test]
    fn run_raw_develop_jpeg_invokes_expected_rawtherapee_args() {
        let temp = tempfile::tempdir().unwrap();
        let raw = temp.path().join("frame.CR2");
        fs::write(&raw, b"raw").unwrap();
        let raw_log = write_helper_script(&temp.path().join("rawtherapee"), true, None, 0).unwrap();
        let out = temp.path().join("out.jpg");

        run_raw_develop_jpeg(
            &temp.path().join("rawtherapee"),
            &[
                PathBuf::from("profile-a.pp3"),
                PathBuf::from("profile-b.pp3"),
            ],
            &raw,
            &out,
            86,
            JpegSubsampling::S422,
            false,
        )
        .unwrap();

        let log = fs::read_to_string(raw_log).unwrap();
        assert!(log.contains("-j86"));
        assert!(log.contains("-js2"));
        assert!(log.contains("-q"));
        assert!(log.contains("-Y"));
        assert!(log.contains("-p"));
        assert!(log.contains("-c"));
        assert!(log.contains("-o"));
        assert!(log.contains("profile-a.pp3"));
        assert!(log.contains(&out.to_string_lossy().to_string()));
    }

    #[test]
    fn run_raw_develop_errors_when_output_not_created() {
        let temp = tempfile::tempdir().unwrap();
        let raw = temp.path().join("frame.NEF");
        fs::write(&raw, b"raw").unwrap();
        write_helper_script(&temp.path().join("rawtherapee"), false, None, 0).unwrap();

        let result = run_raw_develop(
            &temp.path().join("rawtherapee"),
            &[],
            &raw,
            &temp.path().join("out.tif"),
            true,
        );
        assert!(result.is_err());
    }

    #[test]
    fn run_raw_develop_reports_rawtherapee_failure() {
        let temp = tempfile::tempdir().unwrap();
        let raw = temp.path().join("frame.DNG");
        fs::write(&raw, b"raw").unwrap();
        write_helper_script(&temp.path().join("rawtherapee"), false, None, 1).unwrap();

        let result = run_raw_develop(
            &temp.path().join("rawtherapee"),
            &[],
            &raw,
            &temp.path().join("out.tif"),
            true,
        );
        assert!(result.is_err());
    }
}
