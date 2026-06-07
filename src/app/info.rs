use std::path::PathBuf;

use anyhow::Result;
use mini_film::{
    CalibrationAdjustments, ConvertedProfile, GrainSettings, HslAdjustments, ParametricTone,
    ProfileAdjustments, SharpeningSettings, ToneCurves, XmpFilmRecipe, profile_display_name,
};

use crate::app::profile::{ProfileInfo, inspect_profile};

pub(crate) struct InfoArgs {
    pub(crate) profile: String,
    pub(crate) profiles_root: PathBuf,
    pub(crate) hald_dir: PathBuf,
    pub(crate) hald_level: u32,
}

pub(crate) fn run_info(args: InfoArgs) -> Result<()> {
    let info = inspect_profile(
        &args.profile,
        &args.profiles_root,
        &args.hald_dir,
        args.hald_level,
    )?;
    print_profile_info(&info);
    Ok(())
}

fn print_profile_info(info: &ProfileInfo) {
    match info {
        ProfileInfo::HaldPng { path } => {
            println!("Kind: Hald PNG");
            println!("Path: {}", path.display());
            println!("Adjustments: none attached");
            println!("Grain: none attached");
        }
        ProfileInfo::RawTherapeePp3 { path } => {
            println!("Kind: RawTherapee PP3");
            println!("Path: {}", path.display());
            println!("Adjustments: defined by PP3 file");
            println!("Grain: none attached");
        }
        ProfileInfo::RgbTableProfile {
            path,
            converted,
            hald_path,
        } => {
            println!("Kind: internal RGBTable profile");
            println!("Profile XMP: {}", path.display());
            println!("Cached Hald: {}", hald_path.display());
            print_converted_profile(converted);
        }
        ProfileInfo::Emulation {
            path,
            recipe,
            source,
            converted,
            hald_path,
        } => {
            println!("Kind: emulation preset");
            println!("Emulation XMP: {}", path.display());
            print_recipe_identity(recipe);
            println!();
            println!("Linked RGBTable profile: {}", source.display());
            println!("Cached Hald: {}", hald_path.display());
            print_converted_profile(converted);
            println!();
            println!("Emulation adjustments");
            print_adjustments(&recipe.adjustments);
            print_sharpening(recipe.sharpening);
            print_grain(recipe.grain);
        }
    }
}

fn print_recipe_identity(recipe: &XmpFilmRecipe) {
    print_optional("Name", recipe.name.as_deref());
    print_optional("Group", recipe.group.as_deref());
    print_optional("UUID", recipe.uuid.as_deref());
    print_optional("Look name", recipe.look_name.as_deref());
    print_optional("Look UUID", recipe.look_uuid.as_deref());
}

fn print_converted_profile(converted: &ConvertedProfile) {
    let display_name = profile_display_name(&converted.input, &converted.profile);
    println!("Profile name: {display_name}");
    print_optional("Profile group", converted.profile.group.as_deref());
    print_optional("Profile UUID", converted.profile.uuid.as_deref());
    println!("RGB table");
    println!("  input: {}", converted.input.display());
    println!("  dimensions: {}", converted.table.dimensions);
    println!("  divisions: {}", converted.table.divisions);
    println!("  primaries: {}", converted.table.primaries);
    println!("  gamma: {}", converted.table.gamma);
    println!("  gamut: {}", converted.table.gamut);
    println!(
        "  amount: {:.2}..{:.2}",
        converted.table.min_amount, converted.table.max_amount
    );
    println!("  flags: {:?}", converted.table.flags);
    println!();
    println!("Profile adjustments");
    print_adjustments(&converted.adjustments);
    print_sharpening(converted.sharpening);
}

fn print_adjustments(adjustments: &ProfileAdjustments) {
    if adjustments.is_default() {
        println!("  none");
        return;
    }

    print_nonzero_f32("  exposure", adjustments.exposure);
    print_nonzero_f32("  contrast", adjustments.contrast);
    print_nonzero_f32("  highlights", adjustments.highlights);
    print_nonzero_f32("  shadows", adjustments.shadows);
    print_nonzero_f32("  whites", adjustments.whites);
    print_nonzero_f32("  blacks", adjustments.blacks);
    print_nonzero_f32("  saturation", adjustments.saturation);
    print_nonzero_f32("  vibrance", adjustments.vibrance);
    print_nonzero_f32("  clarity", adjustments.clarity);
    print_parametric(adjustments.parametric);
    print_hsl(&adjustments.hsl);
    print_calibration(adjustments.calibration);
    print_curves(&adjustments.tone_curve);
}

fn print_parametric(parametric: ParametricTone) {
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
    println!("  parametric tone:");
    print_nonzero_f32("    shadows", parametric.shadows);
    print_nonzero_f32("    darks", parametric.darks);
    print_nonzero_f32("    lights", parametric.lights);
    print_nonzero_f32("    highlights", parametric.highlights);
    println!(
        "    splits: shadow {:.2}, midtone {:.2}, highlight {:.2}",
        parametric.shadow_split, parametric.midtone_split, parametric.highlight_split
    );
}

fn print_hsl(hsl: &HslAdjustments) {
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
            println!("  hsl {label}: {}", changed.join(", "));
        }
    }
}

fn print_calibration(calibration: CalibrationAdjustments) {
    let changed = calibration.red_hue != 0.0
        || calibration.red_saturation != 0.0
        || calibration.green_hue != 0.0
        || calibration.green_saturation != 0.0
        || calibration.blue_hue != 0.0
        || calibration.blue_saturation != 0.0;
    if !changed {
        return;
    }
    println!("  calibration:");
    print_nonzero_f32("    red hue", calibration.red_hue);
    print_nonzero_f32("    red saturation", calibration.red_saturation);
    print_nonzero_f32("    green hue", calibration.green_hue);
    print_nonzero_f32("    green saturation", calibration.green_saturation);
    print_nonzero_f32("    blue hue", calibration.blue_hue);
    print_nonzero_f32("    blue saturation", calibration.blue_saturation);
}

fn print_curves(curves: &ToneCurves) {
    print_curve("  tone curve", &curves.composite);
    print_curve("  red curve", &curves.red);
    print_curve("  green curve", &curves.green);
    print_curve("  blue curve", &curves.blue);
}

fn print_curve(label: &str, points: &[(f32, f32)]) {
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
    println!("{label}: {}{}", preview.join(" | "), suffix);
}

fn print_sharpening(sharpening: SharpeningSettings) {
    println!("Sharpening");
    if !sharpening.present {
        println!("  none");
        return;
    }
    println!("  enabled: {}", sharpening.is_enabled());
    println!("  amount: {:.2}", sharpening.amount);
    println!("  radius: {:.2}", sharpening.radius);
    println!("  detail: {:.2}", sharpening.detail);
    println!("  masking: {:.2}", sharpening.masking);
}

fn print_grain(grain: GrainSettings) {
    println!("Grain");
    if !grain.is_enabled() {
        println!("  none");
        return;
    }
    println!("  amount: {}", grain.amount);
    println!("  size: {}", grain.size);
    println!("  frequency: {}", grain.frequency);
}

fn print_optional(label: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        println!("{label}: {value}");
    }
}

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
}
