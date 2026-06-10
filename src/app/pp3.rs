use std::{fmt::Write as _, fs, path::PathBuf};

use anyhow::{Context, Result};
use mini_film::{rawtherapee_hald_clut_profile_text, rawtherapee_profile_text};

const BASE_COLOR_NOISE_LUMA: u16 = 14;
const BASE_COLOR_NOISE_LDETAIL: u16 = 34;
const BASE_COLOR_NOISE_CHROMA: u16 = 9;
const HIGH_COLOR_NOISE_LUMA: u16 = 30;
const HIGH_COLOR_NOISE_LDETAIL: u16 = 52;
const HIGH_COLOR_NOISE_CHROMA: u16 = 18;
const VERY_HIGH_COLOR_NOISE_LUMA: u16 = 44;
const VERY_HIGH_COLOR_NOISE_LDETAIL: u16 = 64;
const VERY_HIGH_COLOR_NOISE_CHROMA: u16 = 28;

use crate::app::profile::{ProfileInfo, inspect_profile};
use crate::cli::LensCorrections;

pub(crate) struct NoiseRemovalSettings {
    luma: u16,
    ldetail: u16,
    chroma: u16,
}

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

/// Write a partial RawTherapee profile section enabling directional pyramid
/// color-noise reduction for a given capture ISO.
pub(crate) fn write_rawtherapee_color_noise_profile(
    path: &PathBuf,
    iso: u32,
) -> Result<Option<PathBuf>> {
    let settings = color_noise_settings_for_iso(iso);
    if settings.luma == 0 && settings.ldetail == 0 && settings.chroma == 0 {
        return Ok(None);
    }

    let parent = path.parent().context("color-noise profile has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    fs::write(path, rawtherapee_color_noise_profile_text(&settings))
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(Some(path.to_path_buf()))
}

fn color_noise_settings_for_iso(iso: u32) -> NoiseRemovalSettings {
    if iso >= 25_600 {
        NoiseRemovalSettings {
            luma: VERY_HIGH_COLOR_NOISE_LUMA,
            ldetail: VERY_HIGH_COLOR_NOISE_LDETAIL,
            chroma: VERY_HIGH_COLOR_NOISE_CHROMA,
        }
    } else if iso >= 6_400 {
        NoiseRemovalSettings {
            luma: HIGH_COLOR_NOISE_LUMA,
            ldetail: HIGH_COLOR_NOISE_LDETAIL,
            chroma: HIGH_COLOR_NOISE_CHROMA,
        }
    } else if iso >= 1 {
        NoiseRemovalSettings {
            luma: BASE_COLOR_NOISE_LUMA,
            ldetail: BASE_COLOR_NOISE_LDETAIL,
            chroma: BASE_COLOR_NOISE_CHROMA,
        }
    } else {
        NoiseRemovalSettings {
            luma: 0,
            ldetail: 0,
            chroma: 0,
        }
    }
}

fn rawtherapee_color_noise_profile_text(settings: &NoiseRemovalSettings) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "[Directional Pyramid Denoising]");
    let _ = writeln!(out, "Enabled=true");
    let _ = writeln!(out, "Enhance=false");
    let _ = writeln!(out, "Median=true");
    let _ = writeln!(out, "Luma={}", settings.luma);
    let _ = writeln!(out, "Ldetail={}", settings.ldetail);
    let _ = writeln!(out, "Chroma={}", settings.chroma);
    let _ = writeln!(out, "Method=Lab");
    let _ = writeln!(out, "LMethod=SLI");
    let _ = writeln!(out, "CMethod=AUT");
    let _ = writeln!(out, "C2Method=AUTO");
    let _ = writeln!(out, "SMethod=shal");
    let _ = writeln!(out, "MedMethod=55");
    let _ = writeln!(out, "RGBMethod=soft");
    let _ = writeln!(out, "MethodMed=Lpab");
    let _ = writeln!(out, "Redchro=0");
    let _ = writeln!(out, "Bluechro=0");
    let _ = writeln!(out, "Gamma=1.7");
    let _ = writeln!(out, "Passes=1");
    let _ = writeln!(out, "LCurve=0;");
    let _ = writeln!(out, "CCCurve=0;");
    let _ = writeln!(out);
    out
}

/// Write a partial RawTherapee profile section enabling requested lens corrections.
pub(crate) fn write_rawtherapee_lens_corrections_profile(
    path: &PathBuf,
    lens: LensCorrections,
) -> Result<Option<PathBuf>> {
    if !lens.is_enabled() {
        return Ok(None);
    }

    let parent = path
        .parent()
        .context("lens correction profile has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    fs::write(path, rawtherapee_lens_corrections_profile_text(&lens))
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(Some(path.to_path_buf()))
}

fn rawtherapee_lens_corrections_profile_text(lens: &LensCorrections) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "[LensProfile]");
    let _ = writeln!(out, "LcMode=lfauto");
    let _ = writeln!(out, "UseDistortion={}", lens.distortion);
    let _ = writeln!(out, "UseVignette={}", lens.vignetting);
    let _ = writeln!(out, "UseCA={}", lens.ca);
    let _ = writeln!(out);
    out
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

    #[test]
    fn color_noise_profile_increases_strength_with_iso() {
        let base = color_noise_settings_for_iso(2_000);
        let high = color_noise_settings_for_iso(8_000);
        let very_high = color_noise_settings_for_iso(30_000);

        assert_eq!(base.luma, BASE_COLOR_NOISE_LUMA);
        assert!(high.luma > base.luma);
        assert!(very_high.luma > high.luma);
        assert!(very_high.chroma > high.chroma);
    }

    #[test]
    fn lens_corrections_profile_turns_off_unused_corrections_off() {
        let text = rawtherapee_lens_corrections_profile_text(&crate::cli::LensCorrections {
            distortion: true,
            ca: false,
            vignetting: true,
        });

        assert!(text.contains("[LensProfile]"));
        assert!(text.contains("UseDistortion=true"));
        assert!(text.contains("UseCA=false"));
        assert!(text.contains("UseVignette=true"));
    }
}
