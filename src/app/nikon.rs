use std::path::PathBuf;

use anyhow::{Result, bail};
use mini_film::{
    ProfileAdjustments, SharpeningSettings, fit_nikon_picture_control,
    fit_nikon_picture_control_from_hald, profile_display_name, write_ncp, write_report,
};

use crate::app::profile::{ProfileInfo, inspect_profile};

pub(crate) struct NikonArgs {
    pub(crate) profile: String,
    pub(crate) output: PathBuf,
    pub(crate) report: Option<PathBuf>,
    pub(crate) name: Option<String>,
    pub(crate) profiles_root: PathBuf,
    pub(crate) hald_dir: PathBuf,
    pub(crate) hald_level: u32,
}

/// Create a best-effort Nikon classic Picture Control from a mini-film profile.
///
/// Nikon `.NCP` files can only represent a 1D luminosity curve plus coarse
/// picture-control sliders. The command therefore resolves the requested profile,
/// fits the available XMP/Hald transform into those controls, writes a real NCP
/// file, and optionally writes a report describing approximation error.
pub(crate) fn run_nikon(args: NikonArgs) -> Result<()> {
    let info = inspect_profile(
        &args.profile,
        &args.profiles_root,
        &args.hald_dir,
        args.hald_level,
    )?;
    let name = args.name.unwrap_or_else(|| default_name(&info));
    let (profile, report) = match info {
        ProfileInfo::RgbTableProfile { converted, .. } => fit_nikon_picture_control(
            &name,
            &converted.table,
            &converted.adjustments,
            converted.sharpening,
        ),
        ProfileInfo::Emulation {
            recipe, converted, ..
        } => {
            let adjustments = merge_adjustments(&converted.adjustments, &recipe.adjustments);
            let sharpening = merge_sharpening(converted.sharpening, recipe.sharpening);
            fit_nikon_picture_control(&name, &converted.table, &adjustments, sharpening)
        }
        ProfileInfo::HaldPng { path } => fit_nikon_picture_control_from_hald(
            &name,
            &path,
            &ProfileAdjustments::default(),
            SharpeningSettings::default(),
        )?,
        ProfileInfo::RawTherapeePp3 { .. } => {
            bail!("Nikon Picture Control fitting needs XMP/RGBTable or Hald PNG input, not PP3")
        }
    };

    write_ncp(&args.output, &profile)?;
    if let Some(report_path) = args.report {
        write_report(&report_path, &profile, &report)?;
    }
    eprintln!(
        "wrote {} (mean luma error {:.4}, mean color error {:.4})",
        args.output.display(),
        report.mean_luma_error,
        report.mean_color_error
    );
    Ok(())
}

fn default_name(info: &ProfileInfo) -> String {
    let raw = match info {
        ProfileInfo::HaldPng { path } | ProfileInfo::RawTherapeePp3 { path } => path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("mini-film")
            .to_string(),
        ProfileInfo::RgbTableProfile {
            path, converted, ..
        } => profile_display_name(path, &converted.profile),
        ProfileInfo::Emulation { path, recipe, .. } => recipe.name.clone().unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("mini-film")
                .to_string()
        }),
    };
    raw.chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == ' ' || *ch == '-' || *ch == '_')
        .collect::<String>()
        .trim()
        .chars()
        .take(18)
        .collect::<String>()
}

fn merge_adjustments(
    base: &ProfileAdjustments,
    overlay: &ProfileAdjustments,
) -> ProfileAdjustments {
    let mut out = base.clone();
    out.exposure += overlay.exposure;
    out.contrast += overlay.contrast;
    out.highlights += overlay.highlights;
    out.shadows += overlay.shadows;
    out.whites += overlay.whites;
    out.blacks += overlay.blacks;
    out.saturation += overlay.saturation;
    out.vibrance += overlay.vibrance;
    out.clarity += overlay.clarity;
    out.parametric.shadows += overlay.parametric.shadows;
    out.parametric.darks += overlay.parametric.darks;
    out.parametric.lights += overlay.parametric.lights;
    out.parametric.highlights += overlay.parametric.highlights;
    out.parametric.shadow_split = overlay.parametric.shadow_split;
    out.parametric.midtone_split = overlay.parametric.midtone_split;
    out.parametric.highlight_split = overlay.parametric.highlight_split;
    for i in 0..8 {
        out.hsl.hue[i] += overlay.hsl.hue[i];
        out.hsl.saturation[i] += overlay.hsl.saturation[i];
        out.hsl.luminance[i] += overlay.hsl.luminance[i];
    }
    out.calibration.red_hue += overlay.calibration.red_hue;
    out.calibration.red_saturation += overlay.calibration.red_saturation;
    out.calibration.green_hue += overlay.calibration.green_hue;
    out.calibration.green_saturation += overlay.calibration.green_saturation;
    out.calibration.blue_hue += overlay.calibration.blue_hue;
    out.calibration.blue_saturation += overlay.calibration.blue_saturation;
    if !overlay.tone_curve.composite.is_empty() {
        out.tone_curve.composite = overlay.tone_curve.composite.clone();
    }
    if !overlay.tone_curve.red.is_empty() {
        out.tone_curve.red = overlay.tone_curve.red.clone();
    }
    if !overlay.tone_curve.green.is_empty() {
        out.tone_curve.green = overlay.tone_curve.green.clone();
    }
    if !overlay.tone_curve.blue.is_empty() {
        out.tone_curve.blue = overlay.tone_curve.blue.clone();
    }
    out
}

fn merge_sharpening(base: SharpeningSettings, overlay: SharpeningSettings) -> SharpeningSettings {
    if overlay.present { overlay } else { base }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_name_sanitizes_and_truncates_for_ncp_limits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Agfa Scala 200 faded plus grainy.xmp");
        std::fs::write(&path, "").unwrap();
        let name = default_name(&ProfileInfo::HaldPng { path });
        assert_eq!(name, "Agfa Scala 200 fad");
    }

    #[test]
    fn overlay_adjustments_add_scalars_and_replace_curves() {
        let mut base = ProfileAdjustments {
            exposure: 0.5,
            ..ProfileAdjustments::default()
        };
        base.tone_curve.composite = vec![(0.0, 0.0), (255.0, 255.0)];
        let overlay = ProfileAdjustments {
            exposure: 0.25,
            contrast: 10.0,
            tone_curve: mini_film::ToneCurves {
                composite: vec![(0.0, 10.0), (255.0, 240.0)],
                ..mini_film::ToneCurves::default()
            },
            ..ProfileAdjustments::default()
        };
        let merged = merge_adjustments(&base, &overlay);
        assert_eq!(merged.exposure, 0.75);
        assert_eq!(merged.contrast, 10.0);
        assert_eq!(
            merged.tone_curve.composite,
            vec![(0.0, 10.0), (255.0, 240.0)]
        );
    }
}
