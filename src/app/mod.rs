pub(crate) mod apply;
pub(crate) mod batch;
pub(crate) mod export;
pub(crate) mod info;
pub(crate) mod pp3;
pub(crate) mod profile;
pub(crate) mod progress;
pub(crate) mod raw;
pub(crate) mod sampler;
pub(crate) mod util;

use std::path::Path;

use anyhow::Result;
use mini_film::{HaldOptions, convert_path, profile_info_line, try_convert_dir};

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
