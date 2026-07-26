use std::{
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Output},
    thread,
    time::Duration,
};

use anyhow::{Context, Result, anyhow};

use crate::app::dng::{DngFallbackConfig, PreparedRawSource};
use crate::app::util::is_raw_input_file;
use crate::cli::JpegSubsampling;

#[derive(Clone, Debug)]
pub(crate) struct RawDevelopOutcome {
    pub(crate) source: PreparedRawSource,
}

/// Develop one RAW file with RawTherapee.
///
/// RawTherapee is the only RAW engine. TIFF-bound work uses a 16-bit TIFF
/// intermediate, while JPEG-bound work can use an 8-bit JPEG intermediate, and
/// batch/sampler can suppress RawTherapee's stdout/stderr to keep progress
/// output readable.
pub(crate) fn run_raw_develop(
    rawtherapee: &Path,
    profiles: &[PathBuf],
    source: PreparedRawSource,
    output_tiff: &Path,
    lcp_root: Option<&Path>,
    quiet: bool,
    dng_fallback: &DngFallbackConfig,
) -> Result<RawDevelopOutcome> {
    run_rawtherapee(
        rawtherapee,
        profiles,
        source,
        output_tiff,
        RawOutput::Tiff16,
        lcp_root,
        quiet,
        dng_fallback,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_raw_develop_jpeg(
    rawtherapee: &Path,
    profiles: &[PathBuf],
    source: PreparedRawSource,
    output_jpeg: &Path,
    quality: u8,
    subsampling: JpegSubsampling,
    lcp_root: Option<&Path>,
    quiet: bool,
    dng_fallback: &DngFallbackConfig,
) -> Result<RawDevelopOutcome> {
    run_rawtherapee(
        rawtherapee,
        profiles,
        source,
        output_jpeg,
        RawOutput::Jpeg8 {
            quality,
            subsampling,
        },
        lcp_root,
        quiet,
        dng_fallback,
    )
}

#[derive(Clone, Copy)]
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
#[allow(clippy::too_many_arguments)]
fn run_rawtherapee(
    rawtherapee: &Path,
    profiles: &[PathBuf],
    mut source: PreparedRawSource,
    output: &Path,
    output_format: RawOutput,
    lcp_root: Option<&Path>,
    quiet: bool,
    dng_fallback: &DngFallbackConfig,
) -> Result<RawDevelopOutcome> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let lcp_home = if let Some(lcp_root) = lcp_root {
        Some(configure_rawtherapee_lcp_root(lcp_root)?)
    } else {
        None
    };

    let first = rawtherapee_output_with_retry(
        rawtherapee,
        profiles,
        source.active(),
        output,
        output_format,
        lcp_home.as_ref().map(|home| home.path()),
        quiet,
    );
    if let Err(error) = first {
        if !error.is_input_decode_failure()
            || source.was_replaced()
            || !is_raw_input_file(source.requested())
        {
            return Err(error.into_anyhow(rawtherapee));
        }
        source = dng_fallback
            .prepare_after_decode_failure(&source)
            .with_context(|| {
                format!(
                    "preparing Adobe DNG fallback after RawTherapee could not decode {}",
                    source.requested().display()
                )
            })?;
        let _ = fs::remove_file(output);
        rawtherapee_output_with_retry(
            rawtherapee,
            profiles,
            source.active(),
            output,
            output_format,
            lcp_home.as_ref().map(|home| home.path()),
            quiet,
        )
        .map_err(|error| error.into_anyhow(rawtherapee))
        .with_context(|| {
            format!(
                "RawTherapee could not develop Adobe DNG fallback {}",
                source.active().display()
            )
        })?;
    }

    Ok(RawDevelopOutcome { source })
}

fn rawtherapee_output_with_retry(
    rawtherapee: &Path,
    profiles: &[PathBuf],
    raw: &Path,
    output: &Path,
    output_format: RawOutput,
    lcp_config_home: Option<&Path>,
    quiet: bool,
) -> std::result::Result<Output, RawTherapeeAttemptError> {
    const MAX_ATTEMPTS: usize = 6;
    for attempt in 1..=MAX_ATTEMPTS {
        let mut command = rawtherapee_command(
            rawtherapee,
            profiles,
            raw,
            output,
            output_format,
            lcp_config_home,
            quiet,
        );
        match command.output() {
            Ok(output_result) => {
                if !quiet {
                    let _ = io::stdout().write_all(&output_result.stdout);
                    let _ = io::stderr().write_all(&output_result.stderr);
                }
                if !output_result.status.success() {
                    return Err(RawTherapeeAttemptError::Failed {
                        status: output_result.status,
                        stdout: output_result.stdout,
                        stderr: output_result.stderr,
                    });
                }
                if !output.exists() {
                    return Err(RawTherapeeAttemptError::MissingOutput(output.to_path_buf()));
                }
                return Ok(output_result);
            }
            Err(error) if is_executable_busy(&error) && attempt < MAX_ATTEMPTS => {
                thread::sleep(Duration::from_millis(25 * attempt as u64));
            }
            Err(error) => {
                return Err(RawTherapeeAttemptError::Launch(error));
            }
        }
    }
    unreachable!("rawtherapee retry loop always returns")
}

fn rawtherapee_command(
    rawtherapee: &Path,
    profiles: &[PathBuf],
    raw: &Path,
    output: &Path,
    output_format: RawOutput,
    lcp_config_home: Option<&Path>,
    _quiet: bool,
) -> Command {
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
    if let Some(lcp_home) = lcp_config_home {
        let xdg_config_home = lcp_home.join(".config");
        command
            .env("HOME", lcp_home)
            .env("XDG_CONFIG_HOME", xdg_config_home);
    }
    command
}

#[derive(Debug)]
enum RawTherapeeAttemptError {
    Launch(io::Error),
    Failed {
        status: ExitStatus,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    MissingOutput(PathBuf),
}

impl RawTherapeeAttemptError {
    fn is_input_decode_failure(&self) -> bool {
        let Self::Failed { status, stderr, .. } = self else {
            return false;
        };
        let stderr = String::from_utf8_lossy(stderr).to_ascii_lowercase();
        stderr.contains("error loading file")
            || stderr.contains("cannot load file")
            || stderr.contains("failed to load file")
            || stderr.contains("unsupported raw")
            || (status.code() == Some(254) && stderr.contains("load"))
    }

    fn into_anyhow(self, rawtherapee: &Path) -> anyhow::Error {
        match self {
            Self::Launch(error) => {
                anyhow!(error).context(format!("running {}", rawtherapee.display()))
            }
            Self::Failed {
                status,
                stdout,
                stderr,
            } => anyhow!(
                "rawtherapee failed with status {status}\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&stdout),
                String::from_utf8_lossy(&stderr)
            ),
            Self::MissingOutput(output) => {
                anyhow!("rawtherapee finished without creating {}", output.display())
            }
        }
    }
}

fn configure_rawtherapee_lcp_root(lcp_root: &Path) -> Result<tempfile::TempDir> {
    let home = tempfile::Builder::new()
        .prefix("mini-film-rawtherapee-lcp-")
        .tempdir()
        .context("creating temporary RawTherapee config directory")?;
    let options_path = home
        .path()
        .join(".config")
        .join("RawTherapee")
        .join("options");
    if let Some(parent) = options_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(
        options_path,
        format!("[General]\nLensProfilesPath={}\n", lcp_root.display()),
    )?;
    Ok(home)
}

fn is_executable_busy(error: &io::Error) -> bool {
    error.raw_os_error() == Some(26)
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
        let temp_path = path.with_extension("tmp");
        let mut file = fs::File::create(&temp_path)?;
        file.write_all(rendered.as_bytes())?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        let mut permissions = fs::metadata(&temp_path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&temp_path, permissions)?;
        fs::rename(&temp_path, path)?;
        Ok(path.with_file_name("command.log"))
    }

    fn write_executable(path: &Path, text: &str) {
        let mut file = fs::File::create(path).unwrap();
        file.write_all(text.as_bytes()).unwrap();
        file.sync_all().unwrap();
        let mut permissions = file.metadata().unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    #[test]
    fn run_raw_develop_tiff_invokes_expected_rawtherapee_args() {
        let temp = tempfile::tempdir().unwrap();
        let raw = temp.path().join("frame.RW2");
        fs::write(&raw, b"raw").unwrap();
        let raw_log = write_helper_script(&temp.path().join("rawtherapee"), true, None, 0).unwrap();
        let out = temp.path().join("out.tif");
        run_raw_develop(
            &temp.path().join("rawtherapee"),
            &[],
            PreparedRawSource::unchanged(&raw),
            &out,
            None,
            true,
            &DngFallbackConfig::default(),
        )
        .unwrap();

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
            PreparedRawSource::unchanged(&raw),
            &out,
            86,
            JpegSubsampling::S422,
            None,
            false,
            &DngFallbackConfig::default(),
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
            PreparedRawSource::unchanged(&raw),
            &temp.path().join("out.tif"),
            None,
            true,
            &DngFallbackConfig::default(),
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
            PreparedRawSource::unchanged(&raw),
            &temp.path().join("out.tif"),
            None,
            true,
            &DngFallbackConfig::default(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn rawtherapee_decode_failure_retries_with_adobe_dng() {
        let temp = tempfile::tempdir().unwrap();
        let raw = temp.path().join("frame.raf");
        fs::write(&raw, b"unsupported raw").unwrap();
        let out = temp.path().join("out.tif");
        let rawtherapee = temp.path().join("rawtherapee");
        write_executable(
            &rawtherapee,
            &format!(
                r#"#!/bin/sh
case "$*" in
  *.dng*)
    printf '%s\n' 'developed dng' > '{}'
    exit 0
    ;;
  *)
    printf '%s\n' 'Error loading file' >&2
    exit 254
    ;;
esac
"#,
                out.display()
            ),
        );
        let exiftool = temp.path().join("exiftool");
        write_executable(
            &exiftool,
            r#"#!/bin/sh
printf '%s\n' '[{"FileType":"DNG","DNGVersion":"1.7.1.0","Compression":7,"BitsPerSample":16,"NewRawImageDigest":"0123456789abcdef0123456789abcdef","OriginalRawFileName":"frame.raf","ImageWidth":100,"ImageHeight":80}]'
"#,
        );
        let wine = temp.path().join("wine");
        write_executable(
            &wine,
            r#"#!/bin/sh
set -eu
destination=$(printf '%s' "$5" | sed 's#^Z:##; s#\\#/#g')
mkdir -p "$destination"
printf '%s\n' 'converted dng' > "$destination/frame.dng"
"#,
        );
        let converter = temp.path().join("Adobe DNG Converter.exe");
        fs::write(&converter, b"converter").unwrap();
        let prefix = temp.path().join("wine-prefix");
        fs::create_dir_all(&prefix).unwrap();
        let fallback = DngFallbackConfig::new(Some(converter), Some(wine), Some(prefix))
            .with_exiftool(exiftool);

        let outcome = run_raw_develop(
            &rawtherapee,
            &[],
            PreparedRawSource::unchanged(&raw),
            &out,
            None,
            true,
            &fallback,
        )
        .unwrap();
        assert_eq!(outcome.source.active(), temp.path().join("frame.dng"));
        assert!(out.is_file());
        assert!(raw.is_file());
        fallback
            .finish_successful_development(&outcome.source)
            .unwrap();
        assert!(!raw.exists());
        assert!(outcome.source.active().is_file());
    }
}
