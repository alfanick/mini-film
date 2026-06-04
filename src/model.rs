use std::path::PathBuf;

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

impl GrainSettings {
    pub fn is_enabled(self) -> bool {
        self.amount > 0
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
            hald_level: 8,
            overwrite: false,
            info_only: false,
        }
    }
}
