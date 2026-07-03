use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result};
use mini_film::{rawtherapee_hald_clut_profile_text, rawtherapee_profile_text};
use serde_json::Value;

const BASE_COLOR_NOISE_LUMA: u16 = 14;
const BASE_COLOR_NOISE_LDETAIL: u16 = 34;
const BASE_COLOR_NOISE_CHROMA: u16 = 9;
const HIGH_COLOR_NOISE_LUMA: u16 = 30;
const HIGH_COLOR_NOISE_LDETAIL: u16 = 52;
const HIGH_COLOR_NOISE_CHROMA: u16 = 18;
const VERY_HIGH_COLOR_NOISE_LUMA: u16 = 44;
const VERY_HIGH_COLOR_NOISE_LDETAIL: u16 = 64;
const VERY_HIGH_COLOR_NOISE_CHROMA: u16 = 28;
pub(crate) const RAW_RENDER_PIPELINE_KEY: &str = "raw-render-v2-active-d-lighting";

use crate::app::profile::{ProfileInfo, inspect_profile};
use crate::cli::LensCorrections;

pub(crate) struct NoiseRemovalSettings {
    luma: u16,
    ldetail: u16,
    chroma: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActiveDLighting {
    Low,
    Normal,
    High,
    ExtraHigh,
    ExtraHigh1,
    ExtraHigh2,
    ExtraHigh3,
    ExtraHigh4,
    Auto,
}

struct ActiveDLightingSettings {
    compensation: f32,
    brightness: i16,
    contrast: i16,
    highlight_compression: u16,
    shadow_compression: u16,
    epd_strength: f32,
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

pub(crate) fn write_rawtherapee_active_d_lighting_profile(
    path: &PathBuf,
    raw: &Path,
) -> Result<Option<PathBuf>> {
    let Some(active_d_lighting) = extract_nikon_active_d_lighting(raw)? else {
        return Ok(None);
    };

    let parent = path
        .parent()
        .context("Active D-Lighting profile has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    fs::write(
        path,
        rawtherapee_active_d_lighting_profile_text(active_d_lighting),
    )
    .with_context(|| format!("writing {}", path.display()))?;
    Ok(Some(path.to_path_buf()))
}

fn extract_nikon_active_d_lighting(raw: &Path) -> Result<Option<ActiveDLighting>> {
    let output = Command::new("exiftool")
        .arg("-q")
        .arg("-q")
        .arg("-j")
        .arg("-n")
        .arg("-Nikon:ActiveD-Lighting")
        .arg(raw)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .with_context(|| format!("running exiftool for {}", raw.display()))?;
    if !output.status.success() {
        return Ok(None);
    }
    let mut values = serde_json::from_slice::<Vec<Value>>(&output.stdout)
        .with_context(|| format!("parsing exiftool JSON for {}", raw.display()))?;
    let Some(object) = values.pop().and_then(|value| value.as_object().cloned()) else {
        return Ok(None);
    };
    Ok(active_d_lighting_from_json(object.get("ActiveD-Lighting")))
}

fn active_d_lighting_from_json(value: Option<&Value>) -> Option<ActiveDLighting> {
    match value {
        Some(Value::Number(number)) => number.as_u64().and_then(active_d_lighting_from_code),
        Some(Value::String(text)) => active_d_lighting_from_text(text),
        _ => None,
    }
}

fn active_d_lighting_from_code(code: u64) -> Option<ActiveDLighting> {
    match code {
        0 => None,
        1 => Some(ActiveDLighting::Low),
        3 => Some(ActiveDLighting::Normal),
        5 => Some(ActiveDLighting::High),
        7 => Some(ActiveDLighting::ExtraHigh),
        8 => Some(ActiveDLighting::ExtraHigh1),
        9 => Some(ActiveDLighting::ExtraHigh2),
        10 => Some(ActiveDLighting::ExtraHigh3),
        11 => Some(ActiveDLighting::ExtraHigh4),
        65_535 => Some(ActiveDLighting::Auto),
        _ => None,
    }
}

fn active_d_lighting_from_text(text: &str) -> Option<ActiveDLighting> {
    match text.trim().to_ascii_lowercase().as_str() {
        "off" => None,
        "low" => Some(ActiveDLighting::Low),
        "normal" => Some(ActiveDLighting::Normal),
        "high" => Some(ActiveDLighting::High),
        "extra high" => Some(ActiveDLighting::ExtraHigh),
        "extra high 1" => Some(ActiveDLighting::ExtraHigh1),
        "extra high 2" => Some(ActiveDLighting::ExtraHigh2),
        "extra high 3" => Some(ActiveDLighting::ExtraHigh3),
        "extra high 4" => Some(ActiveDLighting::ExtraHigh4),
        "auto" => Some(ActiveDLighting::Auto),
        _ => None,
    }
}

fn rawtherapee_active_d_lighting_profile_text(active_d_lighting: ActiveDLighting) -> String {
    let settings = active_d_lighting_settings(active_d_lighting);
    let mut out = String::new();
    let _ = writeln!(out, "[Exposure]");
    let _ = writeln!(out, "Auto=false");
    let _ = writeln!(out, "Clip=0.02");
    let _ = writeln!(out, "Compensation={}", fmt_adl_f32(settings.compensation));
    let _ = writeln!(out, "Brightness={}", settings.brightness);
    let _ = writeln!(out, "Contrast={}", settings.contrast);
    let _ = writeln!(out, "HighlightCompr={}", settings.highlight_compression);
    let _ = writeln!(out, "ShadowCompr={}", settings.shadow_compression);
    let _ = writeln!(out, "HighlightComprThreshold=0");
    let _ = writeln!(out, "CurveFromHistogramMatching=false");
    let _ = writeln!(out);
    let _ = writeln!(out, "[HLRecovery]");
    let _ = writeln!(out, "Enabled=true");
    let _ = writeln!(out, "Method=Coloropp");
    let _ = writeln!(out);
    let _ = writeln!(out, "[EPD]");
    let _ = writeln!(out, "Enabled=true");
    let _ = writeln!(out, "Strength={}", fmt_adl_f32(settings.epd_strength));
    let _ = writeln!(out, "Gamma=1");
    let _ = writeln!(out, "EdgeStopping=1.4");
    let _ = writeln!(out, "Scale=0.5");
    let _ = writeln!(out, "ReweightingIterates=0");
    let _ = writeln!(out);
    out
}

fn active_d_lighting_settings(active_d_lighting: ActiveDLighting) -> ActiveDLightingSettings {
    match active_d_lighting {
        ActiveDLighting::Low => ActiveDLightingSettings {
            compensation: 0.08,
            brightness: 1,
            contrast: -2,
            highlight_compression: 18,
            shadow_compression: 14,
            epd_strength: 0.08,
        },
        ActiveDLighting::Normal | ActiveDLighting::Auto => ActiveDLightingSettings {
            compensation: 0.16,
            brightness: 2,
            contrast: -4,
            highlight_compression: 30,
            shadow_compression: 26,
            epd_strength: 0.14,
        },
        ActiveDLighting::High => ActiveDLightingSettings {
            compensation: 0.24,
            brightness: 3,
            contrast: -7,
            highlight_compression: 45,
            shadow_compression: 40,
            epd_strength: 0.22,
        },
        ActiveDLighting::ExtraHigh | ActiveDLighting::ExtraHigh1 => ActiveDLightingSettings {
            compensation: 0.32,
            brightness: 4,
            contrast: -10,
            highlight_compression: 58,
            shadow_compression: 52,
            epd_strength: 0.30,
        },
        ActiveDLighting::ExtraHigh2 => ActiveDLightingSettings {
            compensation: 0.40,
            brightness: 5,
            contrast: -12,
            highlight_compression: 68,
            shadow_compression: 62,
            epd_strength: 0.38,
        },
        ActiveDLighting::ExtraHigh3 => ActiveDLightingSettings {
            compensation: 0.48,
            brightness: 6,
            contrast: -15,
            highlight_compression: 78,
            shadow_compression: 72,
            epd_strength: 0.48,
        },
        ActiveDLighting::ExtraHigh4 => ActiveDLightingSettings {
            compensation: 0.56,
            brightness: 7,
            contrast: -18,
            highlight_compression: 88,
            shadow_compression: 82,
            epd_strength: 0.60,
        },
    }
}

fn fmt_adl_f32(value: f32) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded == 0.0 {
        "0".to_string()
    } else {
        format!("{rounded:.2}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
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

    #[test]
    fn active_d_lighting_parser_accepts_nikon_codes_and_names() {
        assert_eq!(active_d_lighting_from_code(0), None);
        assert_eq!(active_d_lighting_from_code(1), Some(ActiveDLighting::Low));
        assert_eq!(
            active_d_lighting_from_code(65_535),
            Some(ActiveDLighting::Auto)
        );
        assert_eq!(
            active_d_lighting_from_text("Extra High 2"),
            Some(ActiveDLighting::ExtraHigh2)
        );
    }

    #[test]
    fn active_d_lighting_profile_writes_rt_tone_mapping_approximation() {
        let text = rawtherapee_active_d_lighting_profile_text(ActiveDLighting::Low);

        assert!(text.contains("[Exposure]\n"));
        assert!(text.contains("Compensation=0.08\n"));
        assert!(text.contains("HighlightCompr=18\n"));
        assert!(text.contains("ShadowCompr=14\n"));
        assert!(text.contains("[HLRecovery]\nEnabled=true\n"));
        assert!(text.contains("[EPD]\n"));
        assert!(text.contains("Strength=0.08\n"));
    }
}
