use std::{fmt, path::PathBuf};

#[derive(Debug, Clone)]
pub struct XmpRgbTable {
    pub name: Option<String>,
    pub group: Option<String>,
    pub uuid: Option<String>,
    pub table_id: String,
    pub(crate) encoded: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GrainSettings {
    pub amount: u8,
    pub size: u8,
    pub frequency: u8,
}

pub const DEFAULT_GRAIN_REFERENCE_MPIX: f64 = 12.0;

impl GrainSettings {
    pub fn is_enabled(self) -> bool {
        self.amount > 0
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum GrainEngine {
    Rfgr,
    #[value(name = "rfgrfast", alias = "rfgr-fast")]
    RfgrFast,
    #[default]
    Legacy,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GrainRenderOptions {
    pub engine: GrainEngine,
    pub normalize_grain_mpix: Option<f64>,
}

impl Default for GrainRenderOptions {
    fn default() -> Self {
        Self {
            engine: GrainEngine::Legacy,
            normalize_grain_mpix: Some(DEFAULT_GRAIN_REFERENCE_MPIX),
        }
    }
}

impl GrainEngine {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rfgr => "rfgr",
            Self::RfgrFast => "rfgrfast",
            Self::Legacy => "legacy",
        }
    }
}

impl fmt::Display for GrainEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct XmpFilmRecipe {
    pub name: Option<String>,
    pub group: Option<String>,
    pub uuid: Option<String>,
    pub look_uuid: Option<String>,
    pub look_name: Option<String>,
    pub rgb_table: Option<XmpRgbTable>,
    pub grain: GrainSettings,
    pub adjustments: ProfileAdjustments,
    pub sharpening: SharpeningSettings,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SharpeningSettings {
    pub present: bool,
    pub amount: f32,
    pub radius: f32,
    pub detail: f32,
    pub masking: f32,
}

impl SharpeningSettings {
    pub fn is_enabled(self) -> bool {
        self.present && self.amount > 0.0 && self.radius > 0.0
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProfileAdjustments {
    pub exposure: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub saturation: f32,
    pub vibrance: f32,
    pub clarity: f32,
    pub parametric: ParametricTone,
    pub hsl: HslAdjustments,
    pub calibration: CalibrationAdjustments,
    pub tone_curve: ToneCurves,
}

impl ProfileAdjustments {
    pub fn is_default(&self) -> bool {
        self.exposure == 0.0
            && self.contrast == 0.0
            && self.highlights == 0.0
            && self.shadows == 0.0
            && self.whites == 0.0
            && self.blacks == 0.0
            && self.saturation == 0.0
            && self.vibrance == 0.0
            && self.clarity == 0.0
            && self.parametric.is_default()
            && self.hsl.is_default()
            && self.calibration.is_default()
            && self.tone_curve.is_default()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ParametricTone {
    pub shadows: f32,
    pub darks: f32,
    pub lights: f32,
    pub highlights: f32,
    pub shadow_split: f32,
    pub midtone_split: f32,
    pub highlight_split: f32,
}

impl Default for ParametricTone {
    fn default() -> Self {
        Self {
            shadows: 0.0,
            darks: 0.0,
            lights: 0.0,
            highlights: 0.0,
            shadow_split: 25.0,
            midtone_split: 50.0,
            highlight_split: 75.0,
        }
    }
}

impl ParametricTone {
    fn is_default(self) -> bool {
        self.shadows == 0.0
            && self.darks == 0.0
            && self.lights == 0.0
            && self.highlights == 0.0
            && self.shadow_split == 25.0
            && self.midtone_split == 50.0
            && self.highlight_split == 75.0
    }
}

#[derive(Debug, Clone, Default)]
pub struct HslAdjustments {
    pub hue: [f32; 8],
    pub saturation: [f32; 8],
    pub luminance: [f32; 8],
}

impl HslAdjustments {
    fn is_default(&self) -> bool {
        self.hue.iter().all(|v| *v == 0.0)
            && self.saturation.iter().all(|v| *v == 0.0)
            && self.luminance.iter().all(|v| *v == 0.0)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CalibrationAdjustments {
    pub red_hue: f32,
    pub red_saturation: f32,
    pub green_hue: f32,
    pub green_saturation: f32,
    pub blue_hue: f32,
    pub blue_saturation: f32,
}

impl CalibrationAdjustments {
    fn is_default(self) -> bool {
        self.red_hue == 0.0
            && self.red_saturation == 0.0
            && self.green_hue == 0.0
            && self.green_saturation == 0.0
            && self.blue_hue == 0.0
            && self.blue_saturation == 0.0
    }
}

#[derive(Debug, Clone, Default)]
pub struct ToneCurves {
    pub composite: Vec<(f32, f32)>,
    pub red: Vec<(f32, f32)>,
    pub green: Vec<(f32, f32)>,
    pub blue: Vec<(f32, f32)>,
}

impl ToneCurves {
    fn is_default(&self) -> bool {
        crate::adjustments::curve_is_identity(&self.composite)
            && crate::adjustments::curve_is_identity(&self.red)
            && crate::adjustments::curve_is_identity(&self.green)
            && crate::adjustments::curve_is_identity(&self.blue)
    }
}

#[derive(Debug, Clone)]
pub struct RgbTable {
    pub dimensions: u32,
    pub divisions: u32,
    pub(crate) samples: Vec<[u16; 3]>,
    pub primaries: u32,
    pub gamma: u32,
    pub gamut: u32,
    pub min_amount: f64,
    pub max_amount: f64,
    pub flags: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ConvertedProfile {
    pub input: PathBuf,
    pub output: Option<PathBuf>,
    pub profile: XmpRgbTable,
    pub table: RgbTable,
    pub adjustments: ProfileAdjustments,
    pub sharpening: SharpeningSettings,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct BatchSummary {
    pub converted: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct HaldOptions {
    pub hald_level: u32,
    pub overwrite: bool,
    pub info_only: bool,
}

impl Default for HaldOptions {
    fn default() -> Self {
        Self {
            hald_level: 16,
            overwrite: false,
            info_only: false,
        }
    }
}

pub fn dummy_converted_profile() -> ConvertedProfile {
    ConvertedProfile {
        input: PathBuf::from("/tmp/profile-source.xmp"),
        output: Some(PathBuf::from("/tmp/profile-output.png")),
        profile: XmpRgbTable {
            name: Some("Sample profile".to_string()),
            group: Some("Test".to_string()),
            uuid: Some("DEADBEEF".to_string()),
            table_id: "table-id".to_string(),
            encoded: String::from("dummy"),
        },
        table: RgbTable {
            dimensions: 3,
            divisions: 32,
            samples: Vec::new(),
            primaries: 1,
            gamma: 2,
            gamut: 1,
            min_amount: 0.0,
            max_amount: 1.0,
            flags: Some(42),
        },
        adjustments: ProfileAdjustments::default(),
        sharpening: SharpeningSettings::default(),
    }
}

pub fn non_default_adjustments() -> ProfileAdjustments {
    ProfileAdjustments {
        exposure: 0.2,
        contrast: 0.1,
        highlights: 0.3,
        shadows: -0.2,
        whites: 0.1,
        blacks: -0.1,
        saturation: 0.5,
        vibrance: -0.5,
        clarity: 0.4,
        parametric: ParametricTone {
            shadows: 10.0,
            darks: 11.0,
            lights: 12.0,
            highlights: 13.0,
            shadow_split: 20.0,
            midtone_split: 55.0,
            highlight_split: 80.0,
        },
        hsl: HslAdjustments {
            hue: [0.1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            saturation: [0.0; 8],
            luminance: [0.0; 8],
        },
        calibration: CalibrationAdjustments {
            red_hue: 1.0,
            red_saturation: 0.0,
            green_hue: 0.0,
            green_saturation: 0.0,
            blue_hue: 0.0,
            blue_saturation: 0.0,
        },
        tone_curve: ToneCurves {
            composite: vec![(0.0, 0.0), (1.0, 1.0)],
            red: vec![(0.0, 0.0), (1.0, 1.0)],
            green: vec![(0.0, 0.0), (1.0, 1.0)],
            blue: vec![(0.0, 0.0), (1.0, 1.0)],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grain_settings_default_is_disabled() {
        assert!(!GrainSettings::default().is_enabled());
        assert!(
            GrainSettings {
                amount: 1,
                size: 0,
                frequency: 0,
            }
            .is_enabled()
        );
    }

    #[test]
    fn grain_render_options_default_to_legacy_at_twelve_megapixels() {
        let options = GrainRenderOptions::default();
        assert_eq!(options.engine, GrainEngine::Legacy);
        assert_eq!(
            options.normalize_grain_mpix,
            Some(DEFAULT_GRAIN_REFERENCE_MPIX)
        );
    }

    #[test]
    fn sharpening_is_enabled_only_when_present_and_positive() {
        assert!(!SharpeningSettings::default().is_enabled());
        assert!(
            !SharpeningSettings {
                present: true,
                amount: 0.0,
                radius: 1.0,
                detail: 0.0,
                masking: 0.0,
            }
            .is_enabled()
        );
        assert!(
            !SharpeningSettings {
                present: true,
                amount: 1.0,
                radius: 0.0,
                detail: 0.0,
                masking: 0.0,
            }
            .is_enabled()
        );
        assert!(
            SharpeningSettings {
                present: true,
                amount: 1.0,
                radius: 1.0,
                detail: 0.0,
                masking: 0.0,
            }
            .is_enabled()
        );
    }

    #[test]
    fn profile_adjustments_default_detects_nested_changes() {
        assert!(ProfileAdjustments::default().is_default());

        let changed = ProfileAdjustments {
            exposure: 0.1,
            ..Default::default()
        };
        assert!(!changed.is_default());

        let changed = ProfileAdjustments {
            parametric: ParametricTone {
                shadows: 1.0,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(!changed.is_default());

        let changed = ProfileAdjustments {
            hsl: HslAdjustments {
                hue: [0.0; 8],
                saturation: [1.0; 8],
                luminance: [0.0; 8],
            },
            ..Default::default()
        };
        assert!(!changed.is_default());

        let changed = ProfileAdjustments {
            calibration: CalibrationAdjustments {
                red_hue: 1.0,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(!changed.is_default());

        let changed = ProfileAdjustments {
            tone_curve: ToneCurves {
                composite: vec![(0.0, 0.0), (1.0, 2.0)],
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(!changed.is_default());
    }

    #[test]
    fn hsl_adjustments_are_default_when_clear() {
        assert!(HslAdjustments::default().is_default());
        assert!(
            !HslAdjustments {
                hue: [0.1; 8],
                saturation: [0.0; 8],
                luminance: [0.0; 8],
            }
            .is_default()
        );
    }

    #[test]
    fn calibration_is_default_and_non_default() {
        assert!(CalibrationAdjustments::default().is_default());
        assert!(
            !CalibrationAdjustments {
                red_hue: 1.0,
                ..Default::default()
            }
            .is_default()
        );
    }

    #[test]
    fn tone_curves_default_detects_identity() {
        let identity = ToneCurves::default();
        assert!(identity.is_default());

        let non_identity = ToneCurves {
            composite: vec![(0.0, 0.0), (1.0, 2.0)],
            red: vec![(0.0, 0.0), (1.0, 1.0)],
            green: Vec::new(),
            blue: Vec::new(),
        };
        assert!(!non_identity.is_default());
    }

    #[test]
    fn hald_options_default_values() {
        let options = HaldOptions::default();
        assert_eq!(options.hald_level, 16);
        assert!(!options.overwrite);
        assert!(!options.info_only);
    }

    #[test]
    fn batch_summary_defaults_to_zero() {
        assert_eq!(BatchSummary::default().converted, 0);
        assert_eq!(BatchSummary::default().skipped, 0);
    }
}
