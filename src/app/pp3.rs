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
