pub(crate) mod apply;
pub(crate) mod batch;
pub(crate) mod batch_assets;
pub(crate) mod batch_daemon;
pub(crate) mod codex;
pub(crate) mod desktop;
pub(crate) mod dng;
pub(crate) mod export;
pub(crate) mod info;
pub(crate) mod nikon;
pub(crate) mod nikon_wtu;
pub(crate) mod panorama;
pub(crate) mod pp3;
pub(crate) mod profile;
pub(crate) mod progress;
pub(crate) mod raw;
pub(crate) mod retouch;
pub(crate) mod review;
pub(crate) mod review_assets;
pub(crate) mod sampler;
pub(crate) mod sampler_assets;
pub(crate) mod system_stats;
pub(crate) mod timestamps;
pub(crate) mod update;
pub(crate) mod util;

use std::path::Path;

use anyhow::Result;
use mini_film::{HaldOptions, convert_path, profile_info_line, try_convert_dir};

pub(crate) use update::run_update;

pub(crate) fn run_hald(
    input: &Path,
    output: &Path,
    hald_level: u32,
    overwrite: bool,
    info_only: bool,
) -> Result<()> {
    let options = HaldOptions {
        hald_level,
        overwrite,
        info_only,
    };

    if input.is_dir() {
        let (converted, summary) = try_convert_dir(input, output, options)?;
        for profile in converted {
            eprintln!("{}", profile_info_line(&profile));
        }
        eprintln!(
            "converted {}, skipped {}",
            summary.converted, summary.skipped
        );
    } else {
        for profile in convert_path(input, output, options)? {
            eprintln!("{}", profile_info_line(&profile));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn run_hald_handles_empty_input_dir() {
        let input = tempdir().unwrap();
        let output = tempdir().unwrap();
        run_hald(input.path(), output.path(), 16, false, false).unwrap();
    }

    #[test]
    fn run_hald_fails_for_missing_single_file() {
        let input = tempdir().unwrap();
        let missing = input.path().join("does-not-exist.xmp");
        let output = input.path().join("out.png");
        assert!(run_hald(&missing, &output, 16, false, false).is_err());
    }
}
