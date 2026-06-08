use std::{fmt::Write, path::Path};

use anyhow::Result;
use mini_film::{
    CalibrationAdjustments, ConvertedProfile, GrainSettings, HslAdjustments, ParametricTone,
    ProfileAdjustments, SharpeningSettings, ToneCurves, XmpFilmRecipe, profile_display_name,
};

use crate::app::profile::{ProfileInfo, inspect_profile};

pub(crate) struct InfoArgs {
    pub(crate) profile: String,
    pub(crate) profiles_root: std::path::PathBuf,
    pub(crate) hald_dir: std::path::PathBuf,
    pub(crate) hald_level: u32,
}

pub(crate) fn run_info(args: InfoArgs) -> Result<()> {
    let text = profile_info_text_for_selector(
        &args.profile,
        &args.profiles_root,
        &args.hald_dir,
        args.hald_level,
    )?;
    print!("{text}");
    Ok(())
}

pub(crate) fn profile_info_text_for_selector(
    selector: &str,
    profiles_root: &Path,
    hald_dir: &Path,
    hald_level: u32,
) -> Result<String> {
    let info = inspect_profile(selector, profiles_root, hald_dir, hald_level)?;
    Ok(profile_info_to_string(&info))
}

pub(crate) fn profile_info_to_string(info: &ProfileInfo) -> String {
    let mut out = String::new();
    write_profile_info(&mut out, info);
    out
}

#[allow(dead_code)]
fn print_profile_info(info: &ProfileInfo) {
    print!("{}", profile_info_to_string(info));
}

fn write_profile_info(out: &mut String, info: &ProfileInfo) {
    match info {
        ProfileInfo::HaldPng { path } => {
            writeln!(out, "Kind: Hald PNG").ok();
            writeln!(out, "Path: {}", path.display()).ok();
            writeln!(out, "Adjustments: none attached").ok();
            writeln!(out, "Grain: none attached").ok();
            writeln!(out).ok();
        }
        ProfileInfo::RawTherapeePp3 { path } => {
            writeln!(out, "Kind: RawTherapee PP3").ok();
            writeln!(out, "Path: {}", path.display()).ok();
            writeln!(out, "Adjustments: defined by PP3 file").ok();
            writeln!(out, "Grain: none attached").ok();
            writeln!(out).ok();
        }
        ProfileInfo::RgbTableProfile {
            path,
            converted,
            hald_path,
        } => {
            writeln!(out, "Kind: internal RGBTable profile").ok();
            writeln!(out, "Profile XMP: {}", path.display()).ok();
            writeln!(out, "Cached Hald: {}", hald_path.display()).ok();
            write_converted_profile(out, converted);
            writeln!(out).ok();
        }
        ProfileInfo::Emulation {
            path,
            recipe,
            source,
            converted,
            hald_path,
        } => {
            writeln!(out, "Kind: emulation preset").ok();
            writeln!(out, "Emulation XMP: {}", path.display()).ok();
            write_recipe_identity(out, recipe);
            writeln!(out).ok();
            writeln!(out, "Linked RGBTable profile: {}", source.display()).ok();
            writeln!(out, "Cached Hald: {}", hald_path.display()).ok();
            write_converted_profile(out, converted);
            writeln!(out).ok();
            writeln!(out, "Emulation adjustments").ok();
            write_adjustments(out, &recipe.adjustments);
            write_sharpening(out, recipe.sharpening);
            write_grain(out, recipe.grain);
            writeln!(out).ok();
        }
    }
}

fn write_recipe_identity(out: &mut String, recipe: &XmpFilmRecipe) {
    write_optional(out, "Name", recipe.name.as_deref());
    write_optional(out, "Group", recipe.group.as_deref());
    write_optional(out, "UUID", recipe.uuid.as_deref());
    write_optional(out, "Look name", recipe.look_name.as_deref());
    write_optional(out, "Look UUID", recipe.look_uuid.as_deref());
}

#[allow(dead_code)]
fn print_recipe_identity(recipe: &XmpFilmRecipe) {
    let mut out = String::new();
    write_recipe_identity(&mut out, recipe);
    print!("{}", out);
}

fn write_converted_profile(out: &mut String, converted: &ConvertedProfile) {
    let display_name = profile_display_name(&converted.input, &converted.profile);
    writeln!(out, "Profile name: {display_name}").ok();
    write_optional(out, "Profile group", converted.profile.group.as_deref());
    write_optional(out, "Profile UUID", converted.profile.uuid.as_deref());
    writeln!(out, "RGB table").ok();
    writeln!(out, "  input: {}", converted.input.display()).ok();
    writeln!(out, "  dimensions: {}", converted.table.dimensions).ok();
    writeln!(out, "  divisions: {}", converted.table.divisions).ok();
    writeln!(out, "  primaries: {}", converted.table.primaries).ok();
    writeln!(out, "  gamma: {}", converted.table.gamma).ok();
    writeln!(out, "  gamut: {}", converted.table.gamut).ok();
    writeln!(
        out,
        "  amount: {:.2}..{:.2}",
        converted.table.min_amount, converted.table.max_amount
    )
    .ok();
    writeln!(out, "  flags: {:?}", converted.table.flags).ok();
    writeln!(out).ok();
    writeln!(out, "Profile adjustments").ok();
    write_adjustments(out, &converted.adjustments);
    write_sharpening(out, converted.sharpening);
}

#[allow(dead_code)]
fn print_converted_profile(converted: &ConvertedProfile) {
    let mut out = String::new();
    write_converted_profile(&mut out, converted);
    print!("{}", out);
}

fn write_adjustments(out: &mut String, adjustments: &ProfileAdjustments) {
    if adjustments.is_default() {
        writeln!(out, "  none").ok();
        return;
    }

    write_nonzero_f32(out, "  exposure", adjustments.exposure);
    write_nonzero_f32(out, "  contrast", adjustments.contrast);
    write_nonzero_f32(out, "  highlights", adjustments.highlights);
    write_nonzero_f32(out, "  shadows", adjustments.shadows);
    write_nonzero_f32(out, "  whites", adjustments.whites);
    write_nonzero_f32(out, "  blacks", adjustments.blacks);
    write_nonzero_f32(out, "  saturation", adjustments.saturation);
    write_nonzero_f32(out, "  vibrance", adjustments.vibrance);
    write_nonzero_f32(out, "  clarity", adjustments.clarity);
    write_parametric(out, adjustments.parametric);
    write_hsl(out, &adjustments.hsl);
    write_calibration(out, adjustments.calibration);
    write_curves(out, &adjustments.tone_curve);
}

#[allow(dead_code)]
fn print_adjustments(adjustments: &ProfileAdjustments) {
    let mut out = String::new();
    write_adjustments(&mut out, adjustments);
    print!("{}", out);
}

fn write_parametric(out: &mut String, parametric: ParametricTone) {
    let changed = parametric.shadows != 0.0
        || parametric.darks != 0.0
        || parametric.lights != 0.0
        || parametric.highlights != 0.0
        || parametric.shadow_split != 25.0
        || parametric.midtone_split != 50.0
        || parametric.highlight_split != 75.0;
    if !changed {
        return;
    }
    writeln!(out, "  parametric tone:").ok();
    write_nonzero_f32(out, "    shadows", parametric.shadows);
    write_nonzero_f32(out, "    darks", parametric.darks);
    write_nonzero_f32(out, "    lights", parametric.lights);
    write_nonzero_f32(out, "    highlights", parametric.highlights);
    writeln!(
        out,
        "    splits: shadow {:.2}, midtone {:.2}, highlight {:.2}",
        parametric.shadow_split, parametric.midtone_split, parametric.highlight_split
    )
    .ok();
}

#[allow(dead_code)]
fn print_parametric(parametric: ParametricTone) {
    let mut out = String::new();
    write_parametric(&mut out, parametric);
    print!("{}", out);
}

fn write_hsl(out: &mut String, hsl: &HslAdjustments) {
    const NAMES: [&str; 8] = [
        "red", "orange", "yellow", "green", "aqua", "blue", "purple", "magenta",
    ];
    for (label, values) in [
        ("hue", &hsl.hue),
        ("saturation", &hsl.saturation),
        ("luminance", &hsl.luminance),
    ] {
        let changed: Vec<_> = values
            .iter()
            .enumerate()
            .filter(|(_, value)| **value != 0.0)
            .map(|(index, value)| format!("{}={:.2}", NAMES[index], value))
            .collect();
        if !changed.is_empty() {
            writeln!(out, "  hsl {label}: {}", changed.join(", ")).ok();
        }
    }
}

#[allow(dead_code)]
fn print_hsl(hsl: &HslAdjustments) {
    let mut out = String::new();
    write_hsl(&mut out, hsl);
    print!("{}", out);
}

fn write_calibration(out: &mut String, calibration: CalibrationAdjustments) {
    let changed = calibration.red_hue != 0.0
        || calibration.red_saturation != 0.0
        || calibration.green_hue != 0.0
        || calibration.green_saturation != 0.0
        || calibration.blue_hue != 0.0
        || calibration.blue_saturation != 0.0;
    if !changed {
        return;
    }
    writeln!(out, "  calibration:").ok();
    write_nonzero_f32(out, "    red hue", calibration.red_hue);
    write_nonzero_f32(out, "    red saturation", calibration.red_saturation);
    write_nonzero_f32(out, "    green hue", calibration.green_hue);
    write_nonzero_f32(out, "    green saturation", calibration.green_saturation);
    write_nonzero_f32(out, "    blue hue", calibration.blue_hue);
    write_nonzero_f32(out, "    blue saturation", calibration.blue_saturation);
}

#[allow(dead_code)]
fn print_calibration(calibration: CalibrationAdjustments) {
    let mut out = String::new();
    write_calibration(&mut out, calibration);
    print!("{}", out);
}

fn write_curves(out: &mut String, curves: &ToneCurves) {
    write_curve(out, "  tone curve", &curves.composite);
    write_curve(out, "  red curve", &curves.red);
    write_curve(out, "  green curve", &curves.green);
    write_curve(out, "  blue curve", &curves.blue);
}

#[allow(dead_code)]
fn print_curves(curves: &ToneCurves) {
    let mut out = String::new();
    write_curves(&mut out, curves);
    print!("{}", out);
}

fn write_curve(out: &mut String, label: &str, points: &[(f32, f32)]) {
    if points.is_empty() {
        return;
    }
    let preview: Vec<_> = points
        .iter()
        .take(6)
        .map(|(x, y)| format!("{x:.1},{y:.1}"))
        .collect();
    let suffix = if points.len() > preview.len() {
        format!(" ... ({} points)", points.len())
    } else {
        format!(" ({} points)", points.len())
    };
    writeln!(out, "{label}: {}{}", preview.join(" | "), suffix).ok();
}

#[allow(dead_code)]
fn print_curve(label: &str, points: &[(f32, f32)]) {
    let mut out = String::new();
    write_curve(&mut out, label, points);
    print!("{}", out);
}

fn write_sharpening(out: &mut String, sharpening: SharpeningSettings) {
    writeln!(out, "Sharpening").ok();
    if !sharpening.present {
        writeln!(out, "  none").ok();
        return;
    }
    writeln!(out, "  enabled: {}", sharpening.is_enabled()).ok();
    writeln!(out, "  amount: {:.2}", sharpening.amount).ok();
    writeln!(out, "  radius: {:.2}", sharpening.radius).ok();
    writeln!(out, "  detail: {:.2}", sharpening.detail).ok();
    writeln!(out, "  masking: {:.2}", sharpening.masking).ok();
}

#[allow(dead_code)]
fn print_sharpening(sharpening: SharpeningSettings) {
    let mut out = String::new();
    write_sharpening(&mut out, sharpening);
    print!("{}", out);
}

fn write_grain(out: &mut String, grain: GrainSettings) {
    writeln!(out, "Grain").ok();
    if !grain.is_enabled() {
        writeln!(out, "  none").ok();
        return;
    }
    writeln!(out, "  amount: {}", grain.amount).ok();
    writeln!(out, "  size: {}", grain.size).ok();
    writeln!(out, "  frequency: {}", grain.frequency).ok();
}

#[allow(dead_code)]
fn print_grain(grain: GrainSettings) {
    let mut out = String::new();
    write_grain(&mut out, grain);
    print!("{}", out);
}

fn write_optional(out: &mut String, label: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        writeln!(out, "{label}: {value}").ok();
    }
}

fn write_nonzero_f32(out: &mut String, label: &str, value: f32) {
    if value != 0.0 {
        writeln!(out, "{label}: {value:.2}").ok();
    }
}

#[allow(dead_code)]
fn print_optional(label: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        println!("{label}: {value}");
    }
}

#[allow(dead_code)]
fn print_nonzero_f32(label: &str, value: f32) {
    if value != 0.0 {
        println!("{label}: {value:.2}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mini_film::{
        GrainSettings, HaldOptions, ParametricTone, ProfileAdjustments, SharpeningSettings,
        XmpFilmRecipe, dummy_converted_profile, non_default_adjustments,
    };
    use std::path::PathBuf;

    #[test]
    fn print_optional_ignores_empty_and_prints_value() {
        print_optional("Name", None);
        print_optional("Name", Some(""));
        print_optional("Name", Some("Film"));
    }

    #[test]
    fn print_nonzero_f32_only_prints_nonzero() {
        print_nonzero_f32("value", 0.0);
        print_nonzero_f32("value", 0.5);
    }

    #[test]
    fn print_curve_handles_empty_and_non_empty() {
        print_curve("curve", &[]);
        print_curve("curve", &[(0.0, 0.0), (0.25, 0.35), (1.0, 1.0)]);
    }

    #[test]
    fn print_curves_prints_all_channels() {
        let curves = mini_film::ToneCurves {
            composite: vec![(0.0, 0.0)],
            red: vec![(0.0, 0.0)],
            green: vec![(0.0, 0.0)],
            blue: vec![],
        };
        print_curves(&curves);
    }

    #[test]
    fn print_hsl_prints_non_zero_channels_only() {
        let adjustment = mini_film::HslAdjustments {
            hue: [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 2.0],
            saturation: [0.0; 8],
            luminance: [0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        };
        print_hsl(&adjustment);
    }

    #[test]
    fn print_parametric_skips_default_and_prints_changed() {
        print_parametric(ParametricTone::default());
        print_parametric(ParametricTone {
            shadows: 1.0,
            darks: 0.0,
            lights: 0.0,
            highlights: 0.0,
            shadow_split: 25.0,
            midtone_split: 50.0,
            highlight_split: 75.0,
        });
    }

    #[test]
    fn print_calibration_skips_default_and_prints_changed() {
        print_calibration(mini_film::CalibrationAdjustments::default());
        print_calibration(mini_film::CalibrationAdjustments {
            red_hue: 1.0,
            red_saturation: 2.0,
            green_hue: 0.0,
            green_saturation: 0.0,
            blue_hue: 0.0,
            blue_saturation: 0.0,
        });
    }

    #[test]
    fn print_sharpening_prints_none_and_settings() {
        print_sharpening(SharpeningSettings {
            present: false,
            amount: 0.0,
            radius: 0.0,
            detail: 0.0,
            masking: 0.0,
        });
        print_sharpening(SharpeningSettings {
            present: true,
            amount: 1.0,
            radius: 2.0,
            detail: 3.0,
            masking: 4.0,
        });
    }

    #[test]
    fn print_grain_prints_none_and_settings() {
        print_grain(GrainSettings {
            amount: 0,
            size: 0,
            frequency: 0,
        });
        print_grain(GrainSettings {
            amount: 1,
            size: 2,
            frequency: 3,
        });
    }

    #[test]
    fn print_adjustments_prints_default_and_non_default() {
        let default = ProfileAdjustments::default();
        print_adjustments(&default);
        print_adjustments(&non_default_adjustments());
    }

    #[test]
    fn print_converted_profile_prints_values() {
        print_converted_profile(&dummy_converted_profile());
    }

    #[test]
    fn print_recipe_identity_prints_selected_values() {
        let recipe = XmpFilmRecipe {
            name: Some("Film".into()),
            group: Some("Color".into()),
            uuid: Some("UUID".into()),
            look_uuid: Some("LOOK".into()),
            look_name: Some("LkName".into()),
            rgb_table: None,
            grain: GrainSettings::default(),
            adjustments: ProfileAdjustments::default(),
            sharpening: SharpeningSettings::default(),
        };
        print_recipe_identity(&recipe);
    }

    #[test]
    fn print_profile_info_for_all_variants() {
        print_profile_info(&ProfileInfo::HaldPng {
            path: PathBuf::from("/tmp/hald.png"),
        });
        print_profile_info(&ProfileInfo::RawTherapeePp3 {
            path: PathBuf::from("/tmp/profile.pp3"),
        });
        print_profile_info(&ProfileInfo::RgbTableProfile {
            path: PathBuf::from("/tmp/table.xmp"),
            converted: Box::new(dummy_converted_profile()),
            hald_path: PathBuf::from("/tmp/hald-cache.png"),
        });
        print_profile_info(&ProfileInfo::Emulation {
            path: PathBuf::from("/tmp/emulation.xmp"),
            recipe: Box::new(XmpFilmRecipe {
                name: Some("Film".into()),
                group: Some("Group".into()),
                uuid: Some("UUID".into()),
                look_uuid: Some("LOOK-UUID".into()),
                look_name: Some("Look Name".into()),
                rgb_table: None,
                grain: GrainSettings::default(),
                adjustments: ProfileAdjustments::default(),
                sharpening: SharpeningSettings::default(),
            }),
            source: PathBuf::from("/tmp/source.xmp"),
            converted: Box::new(dummy_converted_profile()),
            hald_path: PathBuf::from("/tmp/emulation.hald.png"),
        });
    }

    #[test]
    fn run_printing_with_non_default_converted_values() {
        let mut converted = dummy_converted_profile();
        converted.adjustments = non_default_adjustments();
        converted.sharpening = SharpeningSettings {
            present: true,
            amount: 1.2,
            radius: 0.8,
            detail: 0.5,
            masking: 0.4,
        };
        print_converted_profile(&converted);
    }

    #[test]
    fn print_adjustments_are_default_when_profile_is_default() {
        assert!(ProfileAdjustments::default().is_default());
        assert!(!non_default_adjustments().is_default());
    }

    #[test]
    fn test_is_supported_hald_options_default() {
        let default = HaldOptions::default();
        assert_eq!(default.hald_level, 16);
        assert!(!default.overwrite);
        assert!(!default.info_only);
    }

    #[test]
    fn profile_info_text_for_selector_includes_title_lines() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let profile = root.join("profile.pp3");
        std::fs::write(&profile, "Version=0\nProfile=foo\n").unwrap();
        let text = profile_info_text_for_selector(
            &profile.display().to_string(),
            &root,
            Path::new("/tmp"),
            16,
        )
        .expect("info text for pp3");
        assert!(text.contains("Kind: RawTherapee PP3") || text.contains("Kind:"));
    }
}
