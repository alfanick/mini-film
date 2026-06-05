use std::{fs, io::Write, path::Path};

use anyhow::{Context, Result, bail};
use image::{DynamicImage, GenericImageView, ImageBuffer, Rgb};

use crate::model::{ProfileAdjustments, RgbTable, SharpeningSettings};

const NCP_SIZE: usize = 638;
const NCP_NAME_OFFSET: usize = 0x10;
const NCP_NAME_LEN: usize = 20;
const NCP_LUT_OFFSET: usize = 0x78;
const NCP_LUT_LEN: usize = 257;
const NCP_LUT_MAX: u16 = 0x7fff;
const NCP_BASE_OFFSET: usize = 0x24;
const NIKON_BASE_NEUTRAL: u16 = 0x03c2;
const NIKON_BASE_MONOCHROME: u16 = 0x064d;
const MONOCHROME_CHROMA_THRESHOLD: f32 = 0.025;

const REC_709: [f32; 3] = [0.2126, 0.7152, 0.0722];

#[derive(Debug, Clone)]
pub struct NikonPictureControl {
    pub name: String,
    pub curve: [u16; NCP_LUT_LEN],
    pub brightness: i8,
    pub contrast: i8,
    pub saturation: i8,
    pub hue: i8,
    pub sharpening: u8,
    pub monochrome: bool,
}

#[derive(Debug, Clone)]
pub struct NikonReport {
    pub mean_luma_error: f32,
    pub max_luma_error: f32,
    pub mean_color_error: f32,
    pub max_color_error: f32,
    pub saturation: i8,
    pub hue: i8,
    pub sharpening: u8,
    pub monochrome: bool,
}

#[derive(Debug, Clone, Copy)]
struct Sample {
    input: [f32; 3],
    output: [f32; 3],
}

/// Fit a Nikon classic Picture Control from an RGBTable transform.
///
/// Nikon `.NCP` Picture Controls cannot store a full 3D LUT, so the optimizer
/// reduces the transform to the pieces that classic Picture Control can carry:
/// a 257-entry luma curve, coarse saturation/hue sliders, and sharpening. If the
/// sampled output is effectively grayscale, the generated NCP switches to
/// Nikon's Monochrome base profile and disables hue/saturation color fitting.
/// The luma curve is fit from neutral gray samples, while saturation/hue and the
/// diagnostic color error are estimated from a small RGB patch grid.
pub fn fit_nikon_picture_control(
    name: &str,
    table: &RgbTable,
    adjustments: &ProfileAdjustments,
    sharpening: SharpeningSettings,
) -> (NikonPictureControl, NikonReport) {
    let curve = fit_curve_from_rgb_table(table, adjustments);
    let samples = rgb_table_patch_samples(table, adjustments, 5);
    build_picture_control(name, curve, samples, adjustments, sharpening)
}

/// Fit a Nikon classic Picture Control from a Hald CLUT PNG.
///
/// This path samples the Hald directly, then performs the same reduction as the
/// RGBTable path. It accepts ordinary Hald PNG layout where the image side is
/// `level^3` and the LUT axis is `level^2`.
pub fn fit_nikon_picture_control_from_hald(
    name: &str,
    hald: &Path,
    adjustments: &ProfileAdjustments,
    sharpening: SharpeningSettings,
) -> Result<(NikonPictureControl, NikonReport)> {
    let image = image::open(hald).with_context(|| format!("reading Hald {}", hald.display()))?;
    let sampler = HaldSampler::new(image)?;
    let curve = fit_curve_from_hald(&sampler, adjustments);
    let samples = hald_patch_samples(&sampler, adjustments, 5);
    Ok(build_picture_control(
        name,
        curve,
        samples,
        adjustments,
        sharpening,
    ))
}

/// Write a classic Nikon `.NCP` file.
///
/// The file uses a neutral or monochrome base Picture Control header and
/// replaces the embedded 257-entry user-defined curve with the optimizer result.
/// Coarse slider bytes are filled with best-effort values but the luma LUT is
/// the important payload.
pub fn write_ncp(path: &Path, profile: &NikonPictureControl) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let mut bytes = neutral_ncp_template();
    if profile.monochrome {
        bytes[NCP_BASE_OFFSET..NCP_BASE_OFFSET + 2]
            .copy_from_slice(&NIKON_BASE_MONOCHROME.to_be_bytes());
    }
    write_ncp_name(&mut bytes, &profile.name);
    write_slider_bytes(&mut bytes, profile);
    for (index, value) in profile.curve.iter().enumerate() {
        let offset = NCP_LUT_OFFSET + index * 2;
        bytes[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
    }
    fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

pub fn write_report(
    path: &Path,
    profile: &NikonPictureControl,
    report: &NikonReport,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let mut file =
        fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    writeln!(file, "name: {}", profile.name)?;
    writeln!(file, "format: Nikon classic NCP")?;
    writeln!(
        file,
        "base: {}",
        if profile.monochrome {
            "Monochrome"
        } else {
            "Neutral"
        }
    )?;
    writeln!(file, "curve_points: 257")?;
    writeln!(file, "brightness: {}", profile.brightness)?;
    writeln!(file, "contrast: {}", profile.contrast)?;
    writeln!(file, "saturation: {}", profile.saturation)?;
    writeln!(file, "hue: {}", profile.hue)?;
    writeln!(file, "sharpening: {}", profile.sharpening)?;
    writeln!(file, "monochrome_detected: {}", report.monochrome)?;
    writeln!(file, "mean_luma_error: {:.5}", report.mean_luma_error)?;
    writeln!(file, "max_luma_error: {:.5}", report.max_luma_error)?;
    writeln!(file, "mean_color_error: {:.5}", report.mean_color_error)?;
    writeln!(file, "max_color_error: {:.5}", report.max_color_error)?;
    if profile.monochrome {
        writeln!(
            file,
            "note: monochrome output was detected, so the NCP uses Nikon's Monochrome base profile and the fitted luminosity curve."
        )?;
    } else {
        writeln!(
            file,
            "note: NCP stores a 1D luminosity curve plus coarse sliders, not a full 3D LUT; color-specific film behavior is approximated."
        )?;
    }
    Ok(())
}

fn build_picture_control(
    name: &str,
    curve: [u16; NCP_LUT_LEN],
    samples: Vec<Sample>,
    adjustments: &ProfileAdjustments,
    sharpening: SharpeningSettings,
) -> (NikonPictureControl, NikonReport) {
    let monochrome = detect_monochrome(&samples);
    let saturation = if monochrome {
        -3
    } else {
        estimate_saturation_slider(&samples, adjustments)
    };
    let hue = if monochrome {
        0
    } else {
        estimate_hue_slider(&samples, adjustments)
    };
    let sharpening = estimate_sharpening(sharpening);
    let contrast = (adjustments.contrast / 25.0).round().clamp(-3.0, 3.0) as i8;
    let brightness = (adjustments.exposure * 2.0 + adjustments.whites / 50.0
        - adjustments.blacks / 50.0)
        .round()
        .clamp(-3.0, 3.0) as i8;
    let report = estimate_report(&curve, &samples, saturation, hue, sharpening, monochrome);
    let profile = NikonPictureControl {
        name: ncp_safe_name(name),
        curve,
        brightness,
        contrast,
        saturation,
        hue,
        sharpening,
        monochrome,
    };
    (profile, report)
}

fn fit_curve_from_rgb_table(
    table: &RgbTable,
    adjustments: &ProfileAdjustments,
) -> [u16; NCP_LUT_LEN] {
    let mut curve = [0u16; NCP_LUT_LEN];
    for (index, value) in curve.iter_mut().enumerate() {
        let input = index as f32 / 256.0;
        let coord = (input * 256.0).round() as u32;
        let rgb = crate::rgb_table::sample_rgb_table(table, coord, coord, coord, 257);
        let output = apply_luma_adjustments(luma_u16(rgb), input, adjustments);
        *value = ncp_lut_value(output);
    }
    smooth_monotonic(&mut curve);
    curve
}

fn fit_curve_from_hald(
    sampler: &HaldSampler,
    adjustments: &ProfileAdjustments,
) -> [u16; NCP_LUT_LEN] {
    let mut curve = [0u16; NCP_LUT_LEN];
    for (index, value) in curve.iter_mut().enumerate() {
        let input = index as f32 / 256.0;
        let rgb = sampler.sample([input, input, input]);
        let output = apply_luma_adjustments(luma(rgb), input, adjustments);
        *value = ncp_lut_value(output);
    }
    smooth_monotonic(&mut curve);
    curve
}

fn rgb_table_patch_samples(
    table: &RgbTable,
    adjustments: &ProfileAdjustments,
    grid: u32,
) -> Vec<Sample> {
    let mut samples = Vec::new();
    let axis = 257;
    for r in 0..grid {
        for g in 0..grid {
            for b in 0..grid {
                let input = [
                    r as f32 / (grid - 1) as f32,
                    g as f32 / (grid - 1) as f32,
                    b as f32 / (grid - 1) as f32,
                ];
                let coord = |value: f32| (value * (axis - 1) as f32).round() as u32;
                let rgb = crate::rgb_table::sample_rgb_table(
                    table,
                    coord(input[0]),
                    coord(input[1]),
                    coord(input[2]),
                    axis,
                );
                let mut output = [
                    rgb[0] as f32 / 65535.0,
                    rgb[1] as f32 / 65535.0,
                    rgb[2] as f32 / 65535.0,
                ];
                apply_rgb_adjustments(&mut output, input, adjustments);
                samples.push(Sample { input, output });
            }
        }
    }
    samples
}

fn hald_patch_samples(
    sampler: &HaldSampler,
    adjustments: &ProfileAdjustments,
    grid: u32,
) -> Vec<Sample> {
    let mut samples = Vec::new();
    for r in 0..grid {
        for g in 0..grid {
            for b in 0..grid {
                let input = [
                    r as f32 / (grid - 1) as f32,
                    g as f32 / (grid - 1) as f32,
                    b as f32 / (grid - 1) as f32,
                ];
                let mut output = sampler.sample(input);
                apply_rgb_adjustments(&mut output, input, adjustments);
                samples.push(Sample { input, output });
            }
        }
    }
    samples
}

fn apply_rgb_adjustments(rgb: &mut [f32; 3], input: [f32; 3], adjustments: &ProfileAdjustments) {
    let in_luma = luma(input);
    let out_luma = apply_luma_adjustments(luma(*rgb), in_luma, adjustments);
    let current = luma(*rgb).max(0.0001);
    let scale = out_luma / current;
    for channel in rgb {
        *channel = (*channel * scale).clamp(0.0, 1.0);
    }
}

fn apply_luma_adjustments(mut value: f32, input: f32, adjustments: &ProfileAdjustments) -> f32 {
    value *= 2.0_f32.powf(adjustments.exposure);
    value = (value - 0.5) * (1.0 + adjustments.contrast / 100.0) + 0.5;
    value += adjustments.whites / 200.0;
    value -= adjustments.blacks / 200.0;
    if input > 0.55 {
        value += adjustments.highlights / 200.0 * ((input - 0.55) / 0.45);
    }
    if input < 0.45 {
        value += adjustments.shadows / 200.0 * ((0.45 - input) / 0.45);
    }
    value = apply_curve(&adjustments.tone_curve.composite, value);
    value.clamp(0.0, 1.0)
}

fn apply_curve(points: &[(f32, f32)], value: f32) -> f32 {
    if points.is_empty() {
        return value;
    }
    let mut points = points.to_vec();
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    let x = value * 255.0;
    if x <= points[0].0 {
        return (points[0].1 / 255.0).clamp(0.0, 1.0);
    }
    for pair in points.windows(2) {
        let (x0, y0) = pair[0];
        let (x1, y1) = pair[1];
        if x <= x1 {
            let t = if (x1 - x0).abs() < f32::EPSILON {
                0.0
            } else {
                (x - x0) / (x1 - x0)
            };
            return ((y0 + (y1 - y0) * t) / 255.0).clamp(0.0, 1.0);
        }
    }
    (points.last().unwrap().1 / 255.0).clamp(0.0, 1.0)
}

fn estimate_saturation_slider(samples: &[Sample], adjustments: &ProfileAdjustments) -> i8 {
    let mut ratios = Vec::new();
    for sample in samples {
        let input_chroma = chroma(sample.input);
        if input_chroma > 0.03 {
            ratios.push(chroma(sample.output) / input_chroma);
        }
    }
    let table_sat = if ratios.is_empty() {
        0.0
    } else {
        median(&mut ratios) - 1.0
    };
    let xmp_sat = (adjustments.saturation + adjustments.vibrance) / 100.0;
    ((table_sat + xmp_sat) * 3.0).round().clamp(-3.0, 3.0) as i8
}

fn estimate_hue_slider(samples: &[Sample], adjustments: &ProfileAdjustments) -> i8 {
    let mut shifts = Vec::new();
    for sample in samples {
        if chroma(sample.input) <= 0.05 {
            continue;
        }
        let input = hue_angle(sample.input);
        let output = hue_angle(sample.output);
        shifts.push(short_hue_delta(input, output));
    }
    let table_shift = if shifts.is_empty() {
        0.0
    } else {
        median(&mut shifts)
    };
    let xmp_shift = adjustments.hsl.hue.iter().sum::<f32>() / adjustments.hsl.hue.len() as f32;
    ((table_shift + xmp_shift) / 3.0).round().clamp(-3.0, 3.0) as i8
}

fn estimate_sharpening(sharpening: SharpeningSettings) -> u8 {
    if !sharpening.is_enabled() {
        return 3;
    }
    (sharpening.amount / 100.0 * 9.0).round().clamp(0.0, 9.0) as u8
}

fn detect_monochrome(samples: &[Sample]) -> bool {
    let mut chroma_values = Vec::new();
    for sample in samples {
        if chroma(sample.input) > 0.05 {
            chroma_values.push(chroma(sample.output));
        }
    }
    if chroma_values.is_empty() {
        return false;
    }
    median(&mut chroma_values) < MONOCHROME_CHROMA_THRESHOLD
}

fn estimate_report(
    curve: &[u16; NCP_LUT_LEN],
    samples: &[Sample],
    saturation: i8,
    hue: i8,
    sharpening: u8,
    monochrome: bool,
) -> NikonReport {
    let mut luma_total = 0.0;
    let mut luma_max = 0.0_f32;
    let mut color_total = 0.0;
    let mut color_max = 0.0_f32;
    for sample in samples {
        let approx_luma = sample_curve(curve, luma(sample.input));
        let target_luma = luma(sample.output);
        let luma_error = (target_luma - approx_luma).abs();
        luma_total += luma_error;
        luma_max = luma_max.max(luma_error);

        let approx = approximate_rgb(sample.input, curve, saturation, hue, monochrome);
        let color_error = ((sample.output[0] - approx[0]).powi(2)
            + (sample.output[1] - approx[1]).powi(2)
            + (sample.output[2] - approx[2]).powi(2))
        .sqrt();
        color_total += color_error;
        color_max = color_max.max(color_error);
    }
    let count = samples.len().max(1) as f32;
    NikonReport {
        mean_luma_error: luma_total / count,
        max_luma_error: luma_max,
        mean_color_error: color_total / count,
        max_color_error: color_max,
        saturation,
        hue,
        sharpening,
        monochrome,
    }
}

fn approximate_rgb(
    input: [f32; 3],
    curve: &[u16; NCP_LUT_LEN],
    saturation: i8,
    _hue: i8,
    monochrome: bool,
) -> [f32; 3] {
    let in_luma = luma(input);
    let out_luma = sample_curve(curve, in_luma);
    if monochrome {
        return [out_luma, out_luma, out_luma];
    }
    let sat_scale = 1.0 + saturation as f32 / 3.0 * 0.35;
    let mut out = [0.0; 3];
    for i in 0..3 {
        out[i] = (out_luma + (input[i] - in_luma) * sat_scale).clamp(0.0, 1.0);
    }
    out
}

fn sample_curve(curve: &[u16; NCP_LUT_LEN], input: f32) -> f32 {
    let pos = input.clamp(0.0, 1.0) * 256.0;
    let lo = pos.floor() as usize;
    let hi = (lo + 1).min(256);
    let t = pos - lo as f32;
    let a = curve[lo] as f32 / NCP_LUT_MAX as f32;
    let b = curve[hi] as f32 / NCP_LUT_MAX as f32;
    a + (b - a) * t
}

fn ncp_lut_value(value: f32) -> u16 {
    (value.clamp(0.0, 1.0) * NCP_LUT_MAX as f32).round() as u16
}

fn neutral_lut_value(index: usize) -> u16 {
    let value = index as u32 * 128 - ((index.saturating_sub(1)) as u32 / 128);
    value.min(NCP_LUT_MAX as u32) as u16
}

fn smooth_monotonic(curve: &mut [u16; NCP_LUT_LEN]) {
    let mut last = 0u16;
    for value in &mut *curve {
        if *value < last {
            *value = last;
        }
        last = *value;
    }
    curve[0] = 0;
    curve[NCP_LUT_LEN - 1] = NCP_LUT_MAX;
}

fn neutral_ncp_template() -> [u8; NCP_SIZE] {
    let mut bytes = [0u8; NCP_SIZE];
    bytes[0..4].copy_from_slice(b"NCP\0");
    bytes[7] = 0x01;
    bytes[11] = 0x24;
    bytes[12..16].copy_from_slice(b"0100");
    bytes[0x24..0x48].copy_from_slice(&[
        (NIKON_BASE_NEUTRAL >> 8) as u8,
        NIKON_BASE_NEUTRAL as u8,
        0x00,
        0xff,
        0x82,
        0x01,
        0x01,
        0x80,
        0x80,
        0xff,
        0xff,
        0xff,
        0x00,
        0x00,
        0x00,
        0x02,
        0x00,
        0x00,
        0x02,
        0x42,
        0x49,
        0x30,
        0x00,
        0xff,
        0x00,
        0xff,
        0x01,
        0x00,
        0x02,
        0x00,
        0x00,
        0xff,
        0xff,
        0x00,
        0x00,
        0x00,
    ]);
    for index in 0..NCP_LUT_LEN {
        let offset = NCP_LUT_OFFSET + index * 2;
        bytes[offset..offset + 2].copy_from_slice(&neutral_lut_value(index).to_be_bytes());
    }
    bytes
}

fn write_ncp_name(bytes: &mut [u8; NCP_SIZE], name: &str) {
    let mut ascii = [0u8; NCP_NAME_LEN];
    for (index, byte) in ncp_safe_name(name)
        .bytes()
        .take(NCP_NAME_LEN - 1)
        .enumerate()
    {
        ascii[index] = byte;
    }
    bytes[NCP_NAME_OFFSET..NCP_NAME_OFFSET + NCP_NAME_LEN].copy_from_slice(&ascii);
}

fn write_slider_bytes(bytes: &mut [u8; NCP_SIZE], profile: &NikonPictureControl) {
    bytes[0x28] = encode_signed_slider(profile.sharpening as i8 - 3);
    bytes[0x29] = encode_signed_slider(profile.contrast);
    bytes[0x2a] = encode_signed_slider(profile.brightness);
    bytes[0x2b] = encode_signed_slider(profile.saturation);
    bytes[0x2c] = encode_signed_slider(profile.hue);
}

fn encode_signed_slider(value: i8) -> u8 {
    (0x80_i16 + value.clamp(-9, 9) as i16).clamp(0, 255) as u8
}

fn ncp_safe_name(name: &str) -> String {
    let mut out: String = name
        .chars()
        .filter(|ch| ch.is_ascii_graphic() || *ch == ' ')
        .take(NCP_NAME_LEN - 1)
        .collect();
    if out.trim().is_empty() {
        out = "mini-film".to_string();
    }
    out
}

fn luma(rgb: [f32; 3]) -> f32 {
    rgb[0] * REC_709[0] + rgb[1] * REC_709[1] + rgb[2] * REC_709[2]
}

fn luma_u16(rgb: [u16; 3]) -> f32 {
    luma([
        rgb[0] as f32 / 65535.0,
        rgb[1] as f32 / 65535.0,
        rgb[2] as f32 / 65535.0,
    ])
}

fn chroma(rgb: [f32; 3]) -> f32 {
    let mean = (rgb[0] + rgb[1] + rgb[2]) / 3.0;
    ((rgb[0] - mean).powi(2) + (rgb[1] - mean).powi(2) + (rgb[2] - mean).powi(2)).sqrt()
}

fn hue_angle(rgb: [f32; 3]) -> f32 {
    let r = rgb[0];
    let g = rgb[1];
    let b = rgb[2];
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    if delta <= f32::EPSILON {
        return 0.0;
    }
    let hue = if max == r {
        60.0 * (((g - b) / delta) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    hue.rem_euclid(360.0)
}

fn short_hue_delta(a: f32, b: f32) -> f32 {
    (b - a + 180.0).rem_euclid(360.0) - 180.0
}

fn median(values: &mut [f32]) -> f32 {
    values.sort_by(|a, b| a.total_cmp(b));
    values[values.len() / 2]
}

struct HaldSampler {
    image: ImageBuffer<Rgb<u16>, Vec<u16>>,
    level: u32,
    axis: u32,
}

impl HaldSampler {
    fn new(image: DynamicImage) -> Result<Self> {
        let (width, height) = image.dimensions();
        if width != height {
            bail!("Hald PNG must be square, got {width}x{height}");
        }
        let level = hald_level_from_side(width)?;
        Ok(Self {
            image: image.to_rgb16(),
            level,
            axis: level * level,
        })
    }

    fn sample(&self, rgb: [f32; 3]) -> [f32; 3] {
        let r = (rgb[0].clamp(0.0, 1.0) * (self.axis - 1) as f32).round() as u32;
        let g = (rgb[1].clamp(0.0, 1.0) * (self.axis - 1) as f32).round() as u32;
        let b = (rgb[2].clamp(0.0, 1.0) * (self.axis - 1) as f32).round() as u32;
        let index = (b * self.axis + g) * self.axis + r;
        let x = index % (self.level * self.axis);
        let y = index / (self.level * self.axis);
        let pixel = self.image.get_pixel(x, y);
        [
            pixel[0] as f32 / 65535.0,
            pixel[1] as f32 / 65535.0,
            pixel[2] as f32 / 65535.0,
        ]
    }
}

fn hald_level_from_side(side: u32) -> Result<u32> {
    for level in 2..=64 {
        if level * level * level == side {
            return Ok(level);
        }
    }
    bail!("could not infer Hald level from image side {side}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RgbTable;

    fn identity_table() -> RgbTable {
        let mut samples = Vec::new();
        for r in 0..2 {
            for g in 0..2 {
                for b in 0..2 {
                    samples.push([
                        if r == 0 { 0 } else { 65535 },
                        if g == 0 { 0 } else { 65535 },
                        if b == 0 { 0 } else { 65535 },
                    ]);
                }
            }
        }
        RgbTable {
            dimensions: 3,
            divisions: 2,
            samples,
            primaries: 0,
            gamma: 0,
            gamut: 0,
            min_amount: 0.0,
            max_amount: 1.0,
            flags: None,
        }
    }

    fn monochrome_table() -> RgbTable {
        let mut samples = Vec::new();
        for r in 0..2 {
            for g in 0..2 {
                for b in 0..2 {
                    let luma = if r == 0 { 0.0 } else { REC_709[0] }
                        + if g == 0 { 0.0 } else { REC_709[1] }
                        + if b == 0 { 0.0 } else { REC_709[2] };
                    let value = (luma * 65535.0).round() as u16;
                    samples.push([value, value, value]);
                }
            }
        }
        RgbTable {
            dimensions: 3,
            divisions: 2,
            samples,
            primaries: 0,
            gamma: 0,
            gamut: 0,
            min_amount: 0.0,
            max_amount: 1.0,
            flags: None,
        }
    }

    #[test]
    fn neutral_lut_matches_nikon_integer_formula() {
        assert_eq!(neutral_lut_value(0), 0);
        assert_eq!(neutral_lut_value(128), 16_384);
        assert_eq!(neutral_lut_value(129), 16_511);
        assert_eq!(neutral_lut_value(256), 32_767);
    }

    #[test]
    fn fit_identity_table_produces_nearly_neutral_curve() {
        let (profile, report) = fit_nikon_picture_control(
            "Test Profile",
            &identity_table(),
            &ProfileAdjustments::default(),
            SharpeningSettings::default(),
        );
        assert_eq!(profile.name, "Test Profile");
        assert_eq!(profile.curve[0], 0);
        assert_eq!(profile.curve[256], NCP_LUT_MAX);
        assert!((profile.curve[128] as i32 - neutral_lut_value(128) as i32).abs() <= 1);
        assert!(report.mean_luma_error < 0.01);
        assert!(!profile.monochrome);
        assert!(!report.monochrome);
    }

    #[test]
    fn monochrome_table_switches_to_monochrome_base_profile() {
        let (profile, report) = fit_nikon_picture_control(
            "BW Profile",
            &monochrome_table(),
            &ProfileAdjustments::default(),
            SharpeningSettings::default(),
        );
        assert!(profile.monochrome);
        assert!(report.monochrome);
        assert_eq!(profile.saturation, -3);
        assert_eq!(profile.hue, 0);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bw.ncp");
        write_ncp(&path, &profile).unwrap();
        let bytes = fs::read(path).unwrap();
        assert_eq!(
            u16::from_be_bytes([bytes[NCP_BASE_OFFSET], bytes[NCP_BASE_OFFSET + 1]]),
            NIKON_BASE_MONOCHROME
        );
    }

    #[test]
    fn ncp_writer_places_name_and_big_endian_curve_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.ncp");
        let mut curve = [0u16; NCP_LUT_LEN];
        for (index, value) in curve.iter_mut().enumerate() {
            *value = neutral_lut_value(index);
        }
        let profile = NikonPictureControl {
            name: "Long Nikon Profile Name".to_string(),
            curve,
            brightness: 0,
            contrast: 0,
            saturation: 0,
            hue: 0,
            sharpening: 3,
            monochrome: false,
        };
        write_ncp(&path, &profile).unwrap();
        let bytes = fs::read(path).unwrap();
        assert_eq!(&bytes[0..4], b"NCP\0");
        assert_eq!(
            &bytes[NCP_NAME_OFFSET..NCP_NAME_OFFSET + 19],
            b"Long Nikon Profile "
        );
        assert_eq!(
            u16::from_be_bytes([bytes[NCP_LUT_OFFSET + 256], bytes[NCP_LUT_OFFSET + 257]]),
            neutral_lut_value(128)
        );
    }
}
