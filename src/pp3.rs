use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::model::{ProfileAdjustments, SharpeningSettings};

/// Write a partial RawTherapee processing profile for XMP-side image settings.
///
/// RawTherapee `.pp3` files can be incomplete and are layered by
/// `rawtherapee-cli` in command-line order. This writer only emits sections for
/// Lightroom/Camera Raw settings that mini-film has parsed from XMP and that are
/// better handled by RawTherapee's image pipeline than by a Hald CLUT. The Hald
/// remains limited to the RGBTable lookup, while RawTherapee receives tone,
/// color, curve, and sharpening approximations before mini-film applies the
/// Hald and grain.
pub fn write_rawtherapee_profile(
    path: &Path,
    adjustments: &ProfileAdjustments,
    sharpening: SharpeningSettings,
) -> Result<Option<PathBuf>> {
    if adjustments.is_default() && !sharpening.present {
        return Ok(None);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let text = rawtherapee_profile_text(adjustments, sharpening);
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    Ok(Some(path.to_path_buf()))
}

pub fn write_rawtherapee_contrast_clarity_profile(
    path: &Path,
    contrast: Option<f32>,
    clarity: Option<f32>,
) -> Result<Option<PathBuf>> {
    let text = rawtherapee_contrast_clarity_profile_text(contrast, clarity);
    if text.is_empty() {
        return Ok(None);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    Ok(Some(path.to_path_buf()))
}

pub fn write_rawtherapee_resize_profile(path: &Path, long_edge: u32) -> Result<PathBuf> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(path, rawtherapee_resize_profile_text(long_edge))
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(path.to_path_buf())
}

/// Render a RawTherapee `.pp3` string from parsed Lightroom-style settings.
///
/// The generated profile is deliberately partial. `rawtherapee-cli -p` starts
/// from neutral/default values and applies the keys present here, so mini-film
/// does not need to clone RawTherapee's full profile schema. Basic Lightroom
/// sliders map to RawTherapee's Exposure tool where there are direct controls;
/// point and parametric curves become RT curve data; HSL/calibration sliders are
/// represented as hue-relative Lab curves; and Lightroom sharpening becomes RT
/// capture sharpening/unsharp settings. An absent sharpening setting is omitted
/// so a later partial profile cannot reset sharpening from an earlier layer.
/// Unsupported or weakly-known Lightroom controls are left out instead of being
/// baked into the Hald.
pub fn rawtherapee_profile_text(
    adjustments: &ProfileAdjustments,
    sharpening: SharpeningSettings,
) -> String {
    let mut out = String::new();

    write_exposure_section(&mut out, adjustments);
    out.push_str(&rawtherapee_tone_equalizer_profile_text(adjustments));
    if adjustments.clarity != 0.0 {
        out.push_str(&rawtherapee_local_contrast_profile_text(
            adjustments.clarity,
        ));
    }
    write_luminance_section(&mut out, adjustments);
    write_color_curve_section(&mut out, adjustments);
    write_vibrance_section(&mut out, adjustments);
    if sharpening.present {
        write_sharpening_section(&mut out, sharpening);
    }
    write_color_management_section(&mut out);
    write_raw_section(&mut out);

    out
}

pub fn rawtherapee_hald_clut_profile_text(hald: &Path) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "[Film Simulation]");
    let _ = writeln!(out, "Enabled=true");
    let _ = writeln!(out, "ClutFilename={}", hald.display());
    let _ = writeln!(out, "Strength=100");
    let _ = writeln!(out);
    out
}

pub fn rawtherapee_contrast_clarity_profile_text(
    contrast: Option<f32>,
    clarity: Option<f32>,
) -> String {
    let mut out = String::new();
    if let Some(contrast) = contrast {
        let _ = writeln!(out, "[Exposure]");
        let _ = writeln!(out, "Auto=false");
        let _ = writeln!(out, "Contrast={}", fmt_slider(contrast));
        let _ = writeln!(out);
    }
    if let Some(clarity) = clarity {
        out.push_str(&rawtherapee_local_contrast_profile_text(clarity));
    }
    out
}

pub fn rawtherapee_local_contrast_profile_text(clarity: f32) -> String {
    let amount = clarity.clamp(-100.0, 100.0) / 100.0;
    let mut out = String::new();
    let _ = writeln!(out, "[Local Contrast]");
    let _ = writeln!(out, "Enabled={}", clarity != 0.0);
    let _ = writeln!(out, "Radius=80");
    let _ = writeln!(out, "Amount={}", fmt_f32(amount));
    let _ = writeln!(out, "Darkness=1");
    let _ = writeln!(out, "Lightness=1");
    let _ = writeln!(out);
    out
}

pub fn rawtherapee_resize_profile_text(long_edge: u32) -> String {
    let long_edge = long_edge.max(1);
    let mut out = String::new();
    let _ = writeln!(out, "[Resize]");
    let _ = writeln!(out, "Enabled=true");
    let _ = writeln!(out, "AppliesTo=Full image");
    let _ = writeln!(out, "Method=Lanczos");
    let _ = writeln!(out, "DataSpecified=3");
    let _ = writeln!(out, "Width={long_edge}");
    let _ = writeln!(out, "Height={long_edge}");
    let _ = writeln!(out, "LongEdge={long_edge}");
    let _ = writeln!(out, "ShortEdge={long_edge}");
    let _ = writeln!(out, "AllowUpscaling=false");
    let _ = writeln!(out);
    out
}

fn write_exposure_section(out: &mut String, adjustments: &ProfileAdjustments) {
    let _ = writeln!(out, "[Exposure]");
    let _ = writeln!(out, "Auto=false");
    let _ = writeln!(out, "Clip=0.02");
    let _ = writeln!(out, "Compensation={}", fmt_f32(adjustments.exposure));
    let _ = writeln!(out, "Contrast={}", fmt_slider(adjustments.contrast));
    let _ = writeln!(out, "Saturation={}", fmt_slider(adjustments.saturation));
    let _ = writeln!(out, "CurveFromHistogramMatching=false");
    let _ = writeln!(out, "CurveMode=Standard");
    let _ = writeln!(out, "CurveMode2=Standard");
    let _ = writeln!(
        out,
        "Curve={}",
        rt_curve(&adjustments.tone_curve.composite, 255.0)
    );
    let _ = writeln!(out, "Curve2=0;");
    let _ = writeln!(out);
}

/// Map Lightroom-style tonal-region sliders directly to RawTherapee's five-band
/// tone equalizer. Band 2 is RawTherapee's midtones control and has no matching
/// basic Lightroom slider, so it remains neutral.
pub fn rawtherapee_tone_equalizer_profile_text(adjustments: &ProfileAdjustments) -> String {
    let enabled = adjustments.blacks != 0.0
        || adjustments.shadows != 0.0
        || adjustments.highlights != 0.0
        || adjustments.whites != 0.0;
    let mut out = String::new();
    let _ = writeln!(out, "[ToneEqualizer]");
    let _ = writeln!(out, "Enabled={enabled}");
    let _ = writeln!(out, "Band0={}", fmt_slider(adjustments.blacks));
    let _ = writeln!(out, "Band1={}", fmt_slider(adjustments.shadows));
    let _ = writeln!(out, "Band2=0");
    let _ = writeln!(out, "Band3={}", fmt_slider(adjustments.highlights));
    let _ = writeln!(out, "Band4={}", fmt_slider(adjustments.whites));
    let _ = writeln!(out, "Regularization=0");
    let _ = writeln!(out, "Pivot=0");
    let _ = writeln!(out);
    out
}

fn write_luminance_section(out: &mut String, adjustments: &ProfileAdjustments) {
    let curve = parametric_curve(adjustments);
    let enabled = curve != "0;";

    let _ = writeln!(out, "[Luminance Curve]");
    let _ = writeln!(out, "Enabled={enabled}");
    let _ = writeln!(out, "Brightness=0");
    let _ = writeln!(out, "Contrast=0");
    let _ = writeln!(out, "Chromaticity=0");
    let _ = writeln!(out, "AvoidColorShift=false");
    let _ = writeln!(out, "RedAndSkinTonesProtection=0");
    let _ = writeln!(out, "LCredsk=true");
    let _ = writeln!(out, "LCurve={curve}");
    let _ = writeln!(out, "aCurve=0;");
    let _ = writeln!(out, "bCurve=0;");
    let _ = writeln!(out, "ccCurve=0;");
    let _ = writeln!(out, "chCurve={}", hue_curve(&adjustments.hsl.hue, 0.30));
    let _ = writeln!(
        out,
        "lhCurve={}",
        hue_curve(&adjustments.hsl.luminance, 0.01)
    );
    let _ = writeln!(out, "hhCurve=0;");
    let _ = writeln!(out, "LcCurve=0;");
    let _ = writeln!(out, "ClCurve=0;");
    let _ = writeln!(out);
}

fn write_color_curve_section(out: &mut String, adjustments: &ProfileAdjustments) {
    let red = rt_curve(&adjustments.tone_curve.red, 255.0);
    let green = rt_curve(&adjustments.tone_curve.green, 255.0);
    let blue = rt_curve(&adjustments.tone_curve.blue, 255.0);

    let _ = writeln!(out, "[RGB Curves]");
    let _ = writeln!(out, "LumaMode=false");
    let _ = writeln!(out, "rCurve={red}");
    let _ = writeln!(out, "gCurve={green}");
    let _ = writeln!(out, "bCurve={blue}");
    let _ = writeln!(out);
}

fn write_vibrance_section(out: &mut String, adjustments: &ProfileAdjustments) {
    let has_hsl_saturation = adjustments.hsl.saturation.iter().any(|value| *value != 0.0);
    let has_calibration = adjustments.calibration.red_hue != 0.0
        || adjustments.calibration.red_saturation != 0.0
        || adjustments.calibration.green_hue != 0.0
        || adjustments.calibration.green_saturation != 0.0
        || adjustments.calibration.blue_hue != 0.0
        || adjustments.calibration.blue_saturation != 0.0;
    let enabled = adjustments.vibrance != 0.0 || has_hsl_saturation || has_calibration;
    let saturated = adjustments.vibrance + average(&adjustments.hsl.saturation);
    let pastels = adjustments.vibrance * 0.75;
    let calibration_sat = (adjustments.calibration.red_saturation
        + adjustments.calibration.green_saturation
        + adjustments.calibration.blue_saturation)
        / 3.0;
    let hue_shift = (adjustments.calibration.red_hue
        + adjustments.calibration.green_hue
        + adjustments.calibration.blue_hue)
        / 3.0;

    if !enabled {
        return;
    }

    let _ = writeln!(out, "[Vibrance]");
    let _ = writeln!(out, "Enabled=true");
    let _ = writeln!(
        out,
        "Pastels={}",
        fmt_slider(pastels + calibration_sat * 0.25)
    );
    let _ = writeln!(
        out,
        "Saturated={}",
        fmt_slider(saturated + calibration_sat * 0.25)
    );
    let _ = writeln!(out, "ProtectSkins=false");
    let _ = writeln!(out, "AvoidColorShift=true");
    let _ = writeln!(out, "SkinTonesCurve={}", calibration_curve(hue_shift));
    let _ = writeln!(out);
}

fn write_sharpening_section(out: &mut String, sharpening: SharpeningSettings) {
    let _ = writeln!(out, "[Sharpening]");
    let _ = writeln!(out, "Enabled={}", sharpening.is_enabled());
    if sharpening.is_enabled() {
        let _ = writeln!(out, "Method=usm");
        let _ = writeln!(out, "Radius={}", fmt_f32(sharpening.radius.clamp(0.1, 3.0)));
        let _ = writeln!(
            out,
            "Amount={}",
            fmt_f32(sharpening.amount.clamp(0.0, 150.0))
        );
        let _ = writeln!(
            out,
            "Threshold={}",
            fmt_f32(sharpening.masking.clamp(0.0, 100.0) / 100.0)
        );
        let _ = writeln!(out, "OnlyEdges=false");
        let _ = writeln!(out, "EdgedetectionRadius=1.9");
        let _ = writeln!(
            out,
            "EdgeTolerance={}",
            fmt_f32(sharpening.detail.clamp(0.0, 100.0))
        );
        let _ = writeln!(out, "HalocontrolEnabled=true");
        let _ = writeln!(out, "HalocontrolAmount=50");
    }
    let _ = writeln!(out);
}

fn write_color_management_section(out: &mut String) {
    let _ = writeln!(out, "[Color Management]");
    let _ = writeln!(out, "ToneCurve=false");
    let _ = writeln!(out, "ApplyLookTable=true");
    let _ = writeln!(out, "ApplyBaselineExposureOffset=true");
    let _ = writeln!(out, "ApplyHueSatMap=true");
    let _ = writeln!(out, "DCPIlluminant=0");
    let _ = writeln!(out);
}

fn write_raw_section(out: &mut String) {
    let _ = writeln!(out, "[RAW]");
    let _ = writeln!(out, "CA=true");
    let _ = writeln!(out);
}

fn rt_curve(points: &[(f32, f32)], scale: f32) -> String {
    if curve_is_identity(points) {
        return "0;".to_string();
    }

    let mut normalized = points.to_vec();
    normalized.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut curve = String::from("3;");
    for (x, y) in normalized {
        let _ = write!(
            curve,
            "{};{};",
            fmt_f32((x / scale).clamp(0.0, 1.0)),
            fmt_f32((y / scale).clamp(0.0, 1.0))
        );
    }
    curve
}

fn parametric_curve(adjustments: &ProfileAdjustments) -> String {
    let tone = adjustments.parametric;
    if tone.shadows == 0.0
        && tone.darks == 0.0
        && tone.lights == 0.0
        && tone.highlights == 0.0
        && tone.shadow_split == 25.0
        && tone.midtone_split == 50.0
        && tone.highlight_split == 75.0
    {
        return "0;".to_string();
    }

    let points = [
        (0.0, 0.0 + tone.shadows / 100.0 * 0.25),
        (
            (tone.shadow_split / 100.0).clamp(0.0, 1.0),
            tone.darks / 100.0 * 0.20 + tone.shadows / 100.0 * 0.10,
        ),
        (
            (tone.midtone_split / 100.0).clamp(0.0, 1.0),
            tone.darks / 100.0 * 0.10 + tone.lights / 100.0 * 0.10,
        ),
        (
            (tone.highlight_split / 100.0).clamp(0.0, 1.0),
            tone.lights / 100.0 * 0.20 + tone.highlights / 100.0 * 0.10,
        ),
        (1.0, 1.0 + tone.highlights / 100.0 * 0.25),
    ];

    let mut curve = String::from("3;");
    for (x, delta) in points {
        let y = (x + delta).clamp(0.0, 1.0);
        let _ = write!(curve, "{};{};", fmt_f32(x), fmt_f32(y));
    }
    curve
}

fn hue_curve(values: &[f32; 8], scale: f32) -> String {
    if values.iter().all(|value| *value == 0.0) {
        return "0;".to_string();
    }

    let centers = [0.0, 30.0, 60.0, 120.0, 180.0, 240.0, 280.0, 320.0];
    let mut curve = String::from("3;");
    for (center, value) in centers.iter().zip(values) {
        let x = center / 360.0;
        let y = (0.5 + value * scale).clamp(0.0, 1.0);
        let _ = write!(curve, "{};{};", fmt_f32(x), fmt_f32(y));
    }
    curve
}

fn calibration_curve(hue_shift: f32) -> String {
    if hue_shift == 0.0 {
        "0;".to_string()
    } else {
        let y = (0.5 + hue_shift / 360.0).clamp(0.0, 1.0);
        format!("3;0;{};1;{};", fmt_f32(y), fmt_f32(y))
    }
}

fn average(values: &[f32; 8]) -> f32 {
    values.iter().sum::<f32>() / values.len() as f32
}

fn curve_is_identity(points: &[(f32, f32)]) -> bool {
    points.is_empty() || points.iter().all(|(x, y)| (*x - *y).abs() < f32::EPSILON)
}

fn fmt_f32(value: f32) -> String {
    let mut value = format!("{value:.6}");
    while value.contains('.') && value.ends_with('0') {
        value.pop();
    }
    if value.ends_with('.') {
        value.pop();
    }
    value
}

fn fmt_slider(value: f32) -> i32 {
    value.round().clamp(-100.0, 100.0) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ParametricTone, ProfileAdjustments};

    #[test]
    fn neutral_profile_omits_absent_sharpening() {
        let profile = rawtherapee_profile_text(
            &ProfileAdjustments::default(),
            SharpeningSettings::default(),
        );
        assert!(profile.contains("[Exposure]\n"));
        assert!(profile.contains("Curve=0;\n"));
        assert!(!profile.contains("[Sharpening]\n"));
    }

    #[test]
    fn explicit_disabled_sharpening_is_emitted() {
        let profile = rawtherapee_profile_text(
            &ProfileAdjustments::default(),
            SharpeningSettings {
                present: true,
                amount: 0.0,
                radius: 1.0,
                detail: 0.0,
                masking: 0.0,
            },
        );

        assert!(profile.contains("[Sharpening]\nEnabled=false\n"));
    }

    #[test]
    fn explicit_disabled_sharpening_writes_a_partial_profile() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("disabled-sharpening.pp3");

        let written = write_rawtherapee_profile(
            &path,
            &ProfileAdjustments::default(),
            SharpeningSettings {
                present: true,
                amount: 0.0,
                radius: 1.0,
                detail: 0.0,
                masking: 0.0,
            },
        )
        .unwrap();

        assert_eq!(written, Some(path.clone()));
        assert!(
            std::fs::read_to_string(path)
                .unwrap()
                .contains("[Sharpening]\nEnabled=false\n")
        );
    }

    #[test]
    fn adjusted_profile_contains_rt_tone_and_color_keys() {
        let adjustments = ProfileAdjustments {
            exposure: 0.5,
            contrast: 20.0,
            highlights: -30.0,
            shadows: 40.0,
            whites: 25.0,
            blacks: -5.0,
            vibrance: 25.0,
            parametric: ParametricTone {
                shadows: -10.0,
                darks: 5.0,
                lights: 7.0,
                highlights: -3.0,
                ..ParametricTone::default()
            },
            ..ProfileAdjustments::default()
        };
        let sharpening = SharpeningSettings {
            present: true,
            amount: 40.0,
            radius: 1.2,
            detail: 20.0,
            masking: 10.0,
        };

        let profile = rawtherapee_profile_text(&adjustments, sharpening);
        assert!(profile.contains("Compensation=0.5\n"));
        assert!(profile.contains("Contrast=20\n"));
        assert!(profile.contains(
            "[ToneEqualizer]\nEnabled=true\nBand0=-5\nBand1=40\nBand2=0\nBand3=-30\nBand4=25\n"
        ));
        assert!(!profile.contains("HighlightCompr="));
        assert!(!profile.contains("ShadowCompr="));
        assert!(profile.contains("[Vibrance]\nEnabled=true\n"));
        assert!(profile.contains("[Sharpening]\nEnabled=true\n"));
        assert!(profile.contains("Amount=40\n"));
    }

    #[test]
    fn clarity_uses_local_contrast_without_changing_luminance_contrast() {
        let adjustments = ProfileAdjustments {
            clarity: 25.0,
            ..ProfileAdjustments::default()
        };

        let profile = rawtherapee_profile_text(&adjustments, SharpeningSettings::default());

        assert!(profile.contains(
            "[Local Contrast]\nEnabled=true\nRadius=80\nAmount=0.25\nDarkness=1\nLightness=1\n"
        ));
        assert!(profile.contains("[Luminance Curve]\nEnabled=false\nBrightness=0\nContrast=0\n"));
        assert!(
            profile.contains("[Exposure]\nAuto=false\nClip=0.02\nCompensation=0\nContrast=0\n")
        );
    }

    #[test]
    fn local_contrast_preserves_signed_clarity_and_can_disable_an_earlier_layer() {
        assert_eq!(
            rawtherapee_local_contrast_profile_text(-35.0),
            "[Local Contrast]\nEnabled=true\nRadius=80\nAmount=-0.35\nDarkness=1\nLightness=1\n\n"
        );
        assert_eq!(
            rawtherapee_contrast_clarity_profile_text(Some(0.0), Some(0.0)),
            "[Exposure]\nAuto=false\nContrast=0\n\n[Local Contrast]\nEnabled=false\nRadius=80\nAmount=0\nDarkness=1\nLightness=1\n\n"
        );
    }

    #[test]
    fn tone_equalizer_preserves_both_directions_of_each_tonal_slider() {
        let adjustments = ProfileAdjustments {
            highlights: 35.0,
            shadows: -40.0,
            whites: -25.0,
            blacks: 15.0,
            ..ProfileAdjustments::default()
        };

        let profile = rawtherapee_tone_equalizer_profile_text(&adjustments);

        assert_eq!(
            profile,
            "[ToneEqualizer]\nEnabled=true\nBand0=15\nBand1=-40\nBand2=0\nBand3=35\nBand4=-25\nRegularization=0\nPivot=0\n\n"
        );
    }

    #[test]
    fn vibrance_section_uses_integer_slider_values_for_rawtherapee() {
        let mut adjustments = ProfileAdjustments {
            vibrance: 2.25,
            ..ProfileAdjustments::default()
        };
        adjustments.hsl.saturation[3] = 9.0;
        adjustments.hsl.saturation[5] = 10.0;

        let profile = rawtherapee_profile_text(&adjustments, SharpeningSettings::default());

        assert!(profile.contains("[Vibrance]\nEnabled=true\n"));
        assert!(profile.contains("Pastels=2\n"));
        assert!(profile.contains("Saturated=5\n"));
        assert!(!profile.contains("Saturated=4.625\n"));
    }

    #[test]
    fn rawtherapee_hald_profile_points_film_simulation_at_hald() {
        let profile = rawtherapee_hald_clut_profile_text(Path::new("/tmp/profile.hald.png"));
        assert!(profile.contains("[Film Simulation]\n"));
        assert!(profile.contains("Enabled=true\n"));
        assert!(profile.contains("ClutFilename=/tmp/profile.hald.png\n"));
        assert!(profile.contains("Strength=100\n"));
    }

    #[test]
    fn resize_profile_uses_long_edge_without_upscaling() {
        let profile = rawtherapee_resize_profile_text(0);
        assert!(profile.contains("[Resize]\nEnabled=true\n"));
        assert!(profile.contains("Width=1\n"));
        assert!(profile.contains("Height=1\n"));
        assert!(profile.contains("LongEdge=1\n"));
        assert!(profile.contains("AllowUpscaling=false\n"));
    }

    #[test]
    fn rt_curve_sorts_and_normalizes_points() {
        let curve = rt_curve(&[(255.0, 255.0), (0.0, 0.0), (128.0, 64.0)], 255.0);
        assert_eq!(curve, "3;0;0;0.501961;0.25098;1;1;");
    }

    #[test]
    fn generated_hue_curves_wrap_around_color_wheel() {
        let mut values = [0.0; 8];
        values[0] = 10.0;
        let curve = hue_curve(&values, 0.01);
        assert!(curve.starts_with("3;0;0.6;"));
        assert!(curve.contains("0.083333;0.5;"));
        assert!(curve.ends_with("0.888889;0.5;"));
    }

    #[test]
    fn parametric_curve_uses_split_positions_and_values() {
        let adjustments = ProfileAdjustments {
            parametric: ParametricTone {
                shadows: -10.0,
                darks: -5.0,
                lights: 8.0,
                highlights: 12.0,
                shadow_split: 20.0,
                midtone_split: 55.0,
                highlight_split: 80.0,
            },
            ..ProfileAdjustments::default()
        };
        let curve = parametric_curve(&adjustments);
        assert!(curve.contains("0;0;"));
        assert!(curve.contains("0.2;0.18"));
        assert!(curve.contains("0.55;0.553"));
        assert!(curve.contains("0.8;0.828"));
    }

    #[test]
    fn average_and_fmt_f32_are_stable_for_generated_profiles() {
        assert_eq!(average(&[1.0, 2.0, 3.0, 4.0, 0.0, 0.0, 0.0, 0.0]), 1.25);
        assert_eq!(fmt_f32(2.0), "2");
        assert_eq!(fmt_f32(2.5), "2.5");
        assert_eq!(fmt_slider(2.5), 3);
        assert_eq!(fmt_slider(200.0), 100);
    }
}
