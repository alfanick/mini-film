use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use mini_film::{
    rawtherapee_contrast_clarity_profile_text, rawtherapee_hald_clut_profile_text,
    rawtherapee_profile_text,
};

const BASE_COLOR_NOISE_LUMA: u16 = 14;
const BASE_COLOR_NOISE_LDETAIL: u16 = 34;
const BASE_COLOR_NOISE_CHROMA: u16 = 9;
const HIGH_COLOR_NOISE_LUMA: u16 = 30;
const HIGH_COLOR_NOISE_LDETAIL: u16 = 52;
const HIGH_COLOR_NOISE_CHROMA: u16 = 18;
const VERY_HIGH_COLOR_NOISE_LUMA: u16 = 44;
const VERY_HIGH_COLOR_NOISE_LDETAIL: u16 = 64;
const VERY_HIGH_COLOR_NOISE_CHROMA: u16 = 28;
pub(crate) const RAW_RENDER_PIPELINE_KEY: &str = "raw-render-v8-adobe-lcp";

use crate::app::lcp::ResolvedLensCorrection;
use crate::app::profile::{ProfileInfo, combined_contrast_clarity, inspect_profile};
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

pub(crate) fn write_rawtherapee_auto_matched_curve_profile(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .context("auto-matched curve profile has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    fs::write(path, rawtherapee_auto_matched_curve_profile_text())
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(path.to_path_buf())
}

pub(crate) fn write_rawtherapee_dcp_profile(path: &Path, dcp_profile: &Path) -> Result<PathBuf> {
    let parent = path.parent().context("DCP profile has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    fs::write(path, rawtherapee_dcp_profile_text(dcp_profile))
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(path.to_path_buf())
}

pub(crate) fn write_rawtherapee_srgb_output_profile(path: &Path) -> Result<PathBuf> {
    let parent = path.parent().context("sRGB output profile has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    fs::write(path, rawtherapee_srgb_output_profile_text())
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(path.to_path_buf())
}

pub(crate) fn write_rawtherapee_disable_sharpening_profile(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .context("disable-sharpening profile has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    fs::write(path, rawtherapee_disable_sharpening_profile_text())
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(path.to_path_buf())
}

fn rawtherapee_disable_sharpening_profile_text() -> &'static str {
    "[Sharpening]\n\
Enabled=false\n\
\n\
[SharpenEdge]\n\
Enabled=false\n\
\n\
[SharpenMicro]\n\
Enabled=false\n\
\n\
[PostDemosaicSharpening]\n\
Enabled=false\n\
\n\
[PostResizeSharpening]\n\
Enabled=false\n\
"
}

fn rawtherapee_auto_matched_curve_profile_text() -> String {
    let mut out = String::new();
    let _ = writeln!(out, "[Exposure]");
    let _ = writeln!(out, "Auto=false");
    let _ = writeln!(out, "HistogramMatching=true");
    let _ = writeln!(out, "CurveFromHistogramMatching=false");
    let _ = writeln!(out);
    out
}

fn rawtherapee_dcp_profile_text(dcp_profile: &Path) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "[Color Management]");
    let _ = writeln!(out, "InputProfile=file:{}", dcp_profile.display());
    let _ = writeln!(out, "ToneCurve=true");
    let _ = writeln!(out, "ApplyLookTable=true");
    let _ = writeln!(out, "ApplyBaselineExposureOffset=true");
    let _ = writeln!(out, "ApplyHueSatMap=true");
    let _ = writeln!(out, "DCPIlluminant=0");
    let _ = writeln!(out);
    out
}

fn rawtherapee_srgb_output_profile_text() -> &'static str {
    "[Color Management]\n\
WorkingProfile=ProPhoto\n\
WorkingTRC=none\n\
OutputProfile=RTv4_sRGB\n\
OutputProfileIntent=Relative\n\
OutputBPC=true\n"
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
            let (contrast, clarity) =
                combined_contrast_clarity(&converted.adjustments, &recipe.adjustments);
            out.push_str(&rawtherapee_contrast_clarity_profile_text(
                contrast, clarity,
            ));
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
    correction: &ResolvedLensCorrection,
) -> Result<Option<PathBuf>> {
    if !lens.is_enabled() || matches!(correction, ResolvedLensCorrection::Disabled) {
        return Ok(None);
    }

    let parent = path
        .parent()
        .context("lens correction profile has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    fs::write(
        path,
        rawtherapee_lens_corrections_profile_text(&lens, correction),
    )
    .with_context(|| format!("writing {}", path.display()))?;
    Ok(Some(path.to_path_buf()))
}

fn rawtherapee_lens_corrections_profile_text(
    lens: &LensCorrections,
    correction: &ResolvedLensCorrection,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "[LensProfile]");
    match correction {
        ResolvedLensCorrection::AdobeLcp(profile) => {
            let _ = writeln!(out, "LcMode=lcp");
            let _ = writeln!(out, "LCPFile={}", profile.path.display());
        }
        ResolvedLensCorrection::DngMetadata { .. } => {
            let _ = writeln!(out, "LcMode=metadata");
        }
        ResolvedLensCorrection::LensfunAuto => {
            let _ = writeln!(out, "LcMode=lfauto");
        }
        ResolvedLensCorrection::Disabled => return String::new(),
    }
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
    if adjustments.is_default() && !sharpening.present {
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
    fn auto_matched_curve_profile_requests_fresh_histogram_matching() {
        let text = rawtherapee_auto_matched_curve_profile_text();

        assert_eq!(
            text,
            "[Exposure]\nAuto=false\nHistogramMatching=true\nCurveFromHistogramMatching=false\n\n"
        );
    }

    #[test]
    fn dcp_profile_uses_canonical_rawtherapee_path_and_adobe_tone() {
        let text = rawtherapee_dcp_profile_text(Path::new(
            "/wine/ProgramData/Adobe/CameraRaw/CameraProfiles/Adobe Standard/Nikon Z 7 2 Adobe Standard.dcp",
        ));

        assert!(text.contains(
            "InputProfile=file:/wine/ProgramData/Adobe/CameraRaw/CameraProfiles/Adobe Standard/Nikon Z 7 2 Adobe Standard.dcp\n"
        ));
        assert!(text.contains("ToneCurve=true\n"));
        assert!(text.contains("ApplyLookTable=true\n"));
        assert!(text.contains("ApplyBaselineExposureOffset=true\n"));
        assert!(text.contains("ApplyHueSatMap=true\n"));
        assert!(text.contains("DCPIlluminant=0\n"));
        assert!(!text.contains("HistogramMatching"));
    }

    #[test]
    fn srgb_output_profile_makes_diffusion_color_space_explicit() {
        let text = rawtherapee_srgb_output_profile_text();

        assert!(text.contains("WorkingProfile=ProPhoto\n"));
        assert!(text.contains("WorkingTRC=none\n"));
        assert!(text.contains("OutputProfile=RTv4_sRGB\n"));
        assert!(text.contains("OutputProfileIntent=Relative\n"));
        assert!(text.contains("OutputBPC=true\n"));
    }

    #[test]
    fn disable_sharpening_profile_turns_off_every_rawtherapee_sharpening_stage() {
        let text = rawtherapee_disable_sharpening_profile_text();

        for section in [
            "Sharpening",
            "SharpenEdge",
            "SharpenMicro",
            "PostDemosaicSharpening",
            "PostResizeSharpening",
        ] {
            assert!(text.contains(&format!("[{section}]\nEnabled=false\n")));
        }
        assert_eq!(text.matches("Enabled=false").count(), 5);
        assert!(!text.contains("Enabled=true"));
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
        let text = rawtherapee_lens_corrections_profile_text(
            &crate::cli::LensCorrections {
                distortion: true,
                ca: false,
                vignetting: true,
            },
            &ResolvedLensCorrection::LensfunAuto,
        );

        assert!(text.contains("[LensProfile]"));
        assert!(text.contains("UseDistortion=true"));
        assert!(text.contains("UseCA=false"));
        assert!(text.contains("UseVignette=true"));
    }

    #[test]
    fn lens_corrections_profile_uses_dng_metadata_mode() {
        let text = rawtherapee_lens_corrections_profile_text(
            &crate::cli::LensCorrections::all(),
            &ResolvedLensCorrection::DngMetadata {
                fingerprint: "opcode-list-3".to_string(),
            },
        );

        assert!(text.contains("LcMode=metadata"));
        assert!(!text.contains("LCPFile="));
    }

    #[test]
    fn lens_corrections_profile_uses_absolute_adobe_lcp_path() {
        let profile = crate::app::lcp::LcpProfile {
            path: PathBuf::from("/profiles/Nikon lens - RAW.lcp"),
            filename: "Nikon lens - RAW.lcp".to_string(),
            fingerprint: "abc".to_string(),
        };
        let text = rawtherapee_lens_corrections_profile_text(
            &crate::cli::LensCorrections::all(),
            &ResolvedLensCorrection::AdobeLcp(profile),
        );

        assert!(text.contains("LcMode=lcp"));
        assert!(text.contains("LCPFile=/profiles/Nikon lens - RAW.lcp"));
        assert!(!text.contains("LcMode=metadata"));
        assert!(!text.contains("LcMode=lfauto"));
    }
}
