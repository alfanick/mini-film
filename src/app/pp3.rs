use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use mini_film::{rawtherapee_hald_clut_profile_text, rawtherapee_profile_text};

use crate::app::profile::{ProfileInfo, inspect_profile};

pub(crate) struct Pp3Args {
    pub(crate) profile: String,
    pub(crate) output: PathBuf,
    pub(crate) profiles_root: PathBuf,
    pub(crate) hald_dir: PathBuf,
    pub(crate) hald_level: u32,
}

pub(crate) fn run_pp3(args: Pp3Args) -> Result<()> {
    let info = inspect_profile(
        &args.profile,
        &args.profiles_root,
        &args.hald_dir,
        args.hald_level,
    )?;
    let text = pp3_text(&info)?;
    write_pp3_output(&args.output, &text)?;
    Ok(())
}

fn write_pp3_output(output: &PathBuf, text: &str) -> Result<()> {
    if output == std::path::Path::new("/dev/stdout") {
        print!("{text}");
        return Ok(());
    }
    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(output, text).with_context(|| format!("writing {}", output.display()))
}

fn pp3_text(info: &ProfileInfo) -> Result<String> {
    let mut out = String::new();
    match info {
        ProfileInfo::HaldPng { path } => {
            out.push_str(&rawtherapee_hald_clut_profile_text(path));
        }
        ProfileInfo::RawTherapeePp3 { path } => {
            out.push_str(
                &fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?,
            );
        }
        ProfileInfo::RgbTableProfile {
            converted,
            hald_path,
            ..
        } => {
            push_adjustment_profile(&mut out, &converted.adjustments, converted.sharpening);
            out.push_str(&rawtherapee_hald_clut_profile_text(hald_path));
        }
        ProfileInfo::Emulation {
            recipe,
            converted,
            hald_path,
            ..
        } => {
            push_adjustment_profile(&mut out, &converted.adjustments, converted.sharpening);
            push_adjustment_profile(&mut out, &recipe.adjustments, recipe.sharpening);
            out.push_str(&rawtherapee_hald_clut_profile_text(hald_path));
        }
    }
    Ok(out)
}

fn push_adjustment_profile(
    out: &mut String,
    adjustments: &mini_film::ProfileAdjustments,
    sharpening: mini_film::SharpeningSettings,
) {
    if adjustments.is_default() && !sharpening.is_enabled() {
        return;
    }
    out.push_str(&rawtherapee_profile_text(adjustments, sharpening));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pp3_text_for_hald_profile_generates_film_simulation_section() {
        let text = pp3_text(&ProfileInfo::HaldPng {
            path: PathBuf::from("/tmp/look.hald.png"),
        })
        .unwrap();

        assert!(text.contains("[Film Simulation]\n"));
        assert!(text.contains("ClutFilename=/tmp/look.hald.png\n"));
        assert!(text.contains("Strength=100\n"));
    }

    #[test]
    fn pp3_text_for_rawtherapee_profile_reads_existing_file_verbatim() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("human.pp3");
        fs::write(&path, "[Exposure]\nCompensation=0.25\n").unwrap();

        let text = pp3_text(&ProfileInfo::RawTherapeePp3 { path }).unwrap();
        assert_eq!(text, "[Exposure]\nCompensation=0.25\n");
    }

    #[test]
    fn write_pp3_output_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("nested/generated.pp3");

        write_pp3_output(&output, "profile text\n").unwrap();

        assert_eq!(fs::read_to_string(output).unwrap(), "profile text\n");
    }
}
