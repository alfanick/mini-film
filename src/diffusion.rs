use std::{fmt, fs, path::Path, sync::OnceLock};

use anyhow::{Context, Result, anyhow, bail};
use image::{DynamicImage, GenericImageView, ImageBuffer, ImageFormat, ImageReader, Rgb};
use serde::{Deserialize, Serialize};

const REFERENCE_MPIX: f64 = 12.0;
const PYRAMID_FILTER: [f32; 5] = [0.05, 0.25, 0.40, 0.25, 0.05];

const MIST_SIGMAS: [f32; 6] = [1.0, 2.0, 4.0, 8.0, 16.0, 32.0];
const MIST_CORE_WEIGHTS: [f32; 3] = [0.55, 0.30, 0.15];
const MIST_WING_WEIGHTS: [f32; 3] = [0.20, 0.30, 0.50];

const GLOW_SIGMAS: [f32; 6] = [1.5, 3.0, 6.0, 12.0, 24.0, 48.0];
// These are a mini-film calibration of a neutral, long-tailed glare PSF. They
// are deliberately non-negative and sum to one; they are not constants quoted
// by Spencer et al.
const GLOW_WEIGHTS: [f32; 6] = [0.34, 0.24, 0.17, 0.12, 0.08, 0.05];
// Glare is broader than the local-Laplacian detail bands, so a 12 MP energy
// proxy retains its smallest normalized lobe while bounding the six-scale
// workspace for 45-48 MP inputs.
const GLOW_MAX_WORKING_PIXELS: usize = 12_000_000;

const LLF_REFERENCE_COUNT: usize = 32;
const LLF_RANGE_SIGMA: f32 = 0.20;
const LLF_MAX_BAND: usize = 8;
// A sampled local-Laplacian pyramid has several live float planes per
// reference. Building those planes at 45-48 MP costs more than a gigabyte for
// luminance alone, even though the diffusion detail bands only need a modest
// spatial grid. Work on a 3 MP luminance proxy and scale its pyramid bands back
// to the 12 MP reference size. Equal scenes at 12 MP and 48 MP therefore use
// the same proxy geometry and band radii, while the full-resolution RGB buffer
// remains the only allocation that grows with the source image.
const LLF_MAX_WORKING_PIXELS: usize = 3_000_000;

/// Film-diffusion renderer used after profile simulation and before grain.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum DiffusionMethod {
    /// Layered optical mist with a broad, resolution-normalized tail.
    #[default]
    #[value(name = "multi-scale-mist")]
    MultiScaleMist,
    /// Edge-aware fine-detail reduction followed by neutral highlight glare.
    #[value(name = "edge-aware-glow")]
    EdgeAwareGlow,
}

impl DiffusionMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MultiScaleMist => "multi-scale-mist",
            Self::EdgeAwareGlow => "edge-aware-glow",
        }
    }
}

impl fmt::Display for DiffusionMethod {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Named diffusion strength used by the CLI and review wizard.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum DiffusionPreset {
    #[default]
    Off,
    Subtle,
    Medium,
    Strong,
}

impl DiffusionPreset {
    pub const fn amounts(self) -> (u8, u8) {
        match self {
            Self::Off => (0, 0),
            Self::Subtle => (25, 25),
            Self::Medium => (50, 50),
            Self::Strong => (75, 75),
        }
    }

    pub const fn settings(self, method: DiffusionMethod) -> DiffusionSettings {
        let (softness, highlight_glow) = self.amounts();
        DiffusionSettings {
            method,
            softness,
            highlight_glow,
        }
    }
}

/// Independent diffusion controls. Both strengths use the inclusive range
/// `0..=100`; the all-zero setting is a strict no-op.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffusionSettings {
    pub method: DiffusionMethod,
    pub softness: u8,
    pub highlight_glow: u8,
}

impl DiffusionSettings {
    pub const fn from_preset(method: DiffusionMethod, preset: DiffusionPreset) -> Self {
        preset.settings(method)
    }

    pub const fn is_enabled(self) -> bool {
        self.softness > 0 || self.highlight_glow > 0
    }

    pub fn validate(self) -> Result<()> {
        if self.softness > 100 || self.highlight_glow > 100 {
            bail!("diffusion softness and highlight glow must be between 0 and 100");
        }
        Ok(())
    }
}

/// Apply diffusion to a 16-bit TIFF-style image.
///
/// Enabled rendering is performed in linear sRGB and writes a 16-bit TIFF with
/// the same dimensions and channel layout as the input. Disabled diffusion is
/// copied byte-for-byte so an off setting cannot introduce a decode/encode
/// round trip.
pub fn apply_diffusion(input: &Path, output: &Path, settings: DiffusionSettings) -> Result<()> {
    settings.validate()?;
    if !settings.is_enabled() {
        if input != output {
            fs::copy(input, output)
                .with_context(|| format!("copying {} to {}", input.display(), output.display()))?;
        }
        return Ok(());
    }

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let mut reader =
        ImageReader::open(input).with_context(|| format!("opening {}", input.display()))?;
    reader.no_limits();
    let image = reader
        .decode()
        .with_context(|| format!("decoding {}", input.display()))?;
    let rendered = render_dynamic_16(image, settings)?;
    rendered
        .save_with_format(output, ImageFormat::Tiff)
        .with_context(|| format!("saving {}", output.display()))?;
    Ok(())
}

/// Render diffusion in-place into an RGB16 image buffer.
///
/// This is the integration entrypoint for callers that already decoded their
/// TIFF intermediate and want to avoid a second decode before grain/export.
pub fn render_diffusion_rgb16(
    image: &mut ImageBuffer<Rgb<u16>, Vec<u16>>,
    settings: DiffusionSettings,
) -> Result<()> {
    settings.validate()?;
    if !settings.is_enabled() {
        return Ok(());
    }
    let (width, height) = image.dimensions();
    render_interleaved_16(
        image.as_mut(),
        width as usize,
        height as usize,
        PixelLayout::Rgb,
        settings,
        spatial_scale(width, height)?,
    )
}

fn render_dynamic_16(image: DynamicImage, settings: DiffusionSettings) -> Result<DynamicImage> {
    let (width, height) = image.dimensions();
    let scale = spatial_scale(width, height)?;
    match image {
        DynamicImage::ImageLuma16(mut image) => {
            render_interleaved_16(
                image.as_mut(),
                width as usize,
                height as usize,
                PixelLayout::Luma,
                settings,
                scale,
            )?;
            Ok(DynamicImage::ImageLuma16(image))
        }
        DynamicImage::ImageLumaA16(mut image) => {
            render_interleaved_16(
                image.as_mut(),
                width as usize,
                height as usize,
                PixelLayout::LumaAlpha,
                settings,
                scale,
            )?;
            Ok(DynamicImage::ImageLumaA16(image))
        }
        DynamicImage::ImageRgb16(mut image) => {
            render_interleaved_16(
                image.as_mut(),
                width as usize,
                height as usize,
                PixelLayout::Rgb,
                settings,
                scale,
            )?;
            Ok(DynamicImage::ImageRgb16(image))
        }
        DynamicImage::ImageRgba16(mut image) => {
            render_interleaved_16(
                image.as_mut(),
                width as usize,
                height as usize,
                PixelLayout::Rgba,
                settings,
                scale,
            )?;
            Ok(DynamicImage::ImageRgba16(image))
        }
        other => bail!(
            "diffusion requires a 16-bit integer image, got {:?}",
            other.color()
        ),
    }
}

#[derive(Debug, Clone, Copy)]
enum PixelLayout {
    Luma,
    LumaAlpha,
    Rgb,
    Rgba,
}

impl PixelLayout {
    const fn stride(self) -> usize {
        match self {
            Self::Luma => 1,
            Self::LumaAlpha => 2,
            Self::Rgb => 3,
            Self::Rgba => 4,
        }
    }

    const fn color_offsets(self) -> &'static [usize] {
        match self {
            Self::Luma | Self::LumaAlpha => &[0],
            Self::Rgb | Self::Rgba => &[0, 1, 2],
        }
    }

    const fn is_color(self) -> bool {
        matches!(self, Self::Rgb | Self::Rgba)
    }
}

fn spatial_scale(width: u32, height: u32) -> Result<f32> {
    let pixels = u64::from(width) * u64::from(height);
    if pixels == 0 {
        bail!("diffusion cannot render an empty image");
    }
    let scale = (pixels as f64 / 1_000_000.0 / REFERENCE_MPIX).sqrt();
    let scale = scale as f32;
    if !scale.is_finite() || scale <= 0.0 {
        bail!("diffusion image dimensions produce an unsupported spatial scale");
    }
    Ok(scale)
}

fn render_interleaved_16(
    raw: &mut [u16],
    width: usize,
    height: usize,
    layout: PixelLayout,
    settings: DiffusionSettings,
    scale: f32,
) -> Result<()> {
    let pixels = width
        .checked_mul(height)
        .ok_or_else(|| anyhow!("diffusion image dimensions overflow"))?;
    let expected = pixels
        .checked_mul(layout.stride())
        .ok_or_else(|| anyhow!("diffusion image buffer length overflows"))?;
    if raw.len() != expected {
        bail!(
            "diffusion image buffer has {} samples; expected {expected}",
            raw.len()
        );
    }

    match settings.method {
        DiffusionMethod::MultiScaleMist => {
            render_multi_scale_mist(raw, width, height, layout, settings, scale)
        }
        DiffusionMethod::EdgeAwareGlow => {
            render_edge_aware_glow(raw, width, height, layout, settings, scale)
        }
    }
}

fn render_multi_scale_mist(
    raw: &mut [u16],
    width: usize,
    height: usize,
    layout: PixelLayout,
    settings: DiffusionSettings,
    scale: f32,
) -> Result<()> {
    let softness = settings.softness as f32 / 100.0;
    let glow = settings.highlight_glow as f32 / 100.0;
    let core_mix = 0.10 * softness;
    let wing_mix = 0.12 * glow;
    if core_mix == 0.0 && wing_mix == 0.0 {
        return Ok(());
    }

    let targets = MIST_SIGMAS.map(|sigma| sigma * scale);
    let mut weights = [0.0; 6];
    for index in 0..3 {
        weights[index] = core_mix * MIST_CORE_WEIGHTS[index];
        weights[index + 3] = wing_mix * MIST_WING_WEIGHTS[index];
    }
    let base_mix = 1.0 - core_mix - wing_mix;

    for &channel in layout.color_offsets() {
        let source = decode_channel(raw, layout.stride(), channel);
        let mut accumulated = source
            .iter()
            .map(|value| value * base_mix)
            .collect::<Vec<_>>();
        accumulate_scale_space(source, width, height, &targets, &weights, &mut accumulated);
        encode_channel(raw, layout.stride(), channel, &accumulated);
    }
    Ok(())
}

fn render_edge_aware_glow(
    raw: &mut [u16],
    width: usize,
    height: usize,
    layout: PixelLayout,
    settings: DiffusionSettings,
    scale: f32,
) -> Result<()> {
    if settings.softness > 0 {
        apply_edge_aware_softness(raw, width, height, layout, settings.softness, scale)?;
    }
    if settings.highlight_glow > 0 {
        apply_neutral_highlight_glow(raw, width, height, layout, settings.highlight_glow, scale)?;
    }
    Ok(())
}

fn apply_edge_aware_softness(
    raw: &mut [u16],
    width: usize,
    height: usize,
    layout: PixelLayout,
    amount: u8,
    scale: f32,
) -> Result<()> {
    apply_edge_aware_softness_with_limit(
        raw,
        width,
        height,
        layout,
        amount,
        scale,
        LLF_MAX_WORKING_PIXELS,
    )
}

fn apply_edge_aware_softness_with_limit(
    raw: &mut [u16],
    width: usize,
    height: usize,
    layout: PixelLayout,
    amount: u8,
    scale: f32,
    max_working_pixels: usize,
) -> Result<()> {
    let strength = 0.55 * amount as f32 / 100.0;
    let working =
        llf_working_luma_with_limit(raw, width, height, layout, scale, max_working_pixels)?;
    let mut delta = local_laplacian_soften(
        &working.data,
        working.width,
        working.height,
        working.scale,
        strength,
    )?;
    for (value, original) in delta.iter_mut().zip(&working.data) {
        *value -= original;
    }
    apply_luma_delta(
        raw,
        width,
        height,
        layout,
        &delta,
        working.width,
        working.height,
    );
    Ok(())
}

#[derive(Debug)]
struct LlfWorkingLuma {
    width: usize,
    height: usize,
    scale: f32,
    data: Vec<f32>,
}

fn llf_working_luma_with_limit(
    raw: &[u16],
    width: usize,
    height: usize,
    layout: PixelLayout,
    scale: f32,
    max_pixels: usize,
) -> Result<LlfWorkingLuma> {
    let pixels = width
        .checked_mul(height)
        .ok_or_else(|| anyhow!("diffusion image dimensions overflow"))?;
    if pixels == 0 || max_pixels == 0 {
        bail!("diffusion cannot build an empty local-Laplacian plane");
    }
    let (working_width, working_height) = bounded_working_dimensions(width, height, max_pixels)?;
    let working_pixels = working_width
        .checked_mul(working_height)
        .ok_or_else(|| anyhow!("diffusion working dimensions overflow"))?;
    let working_scale = scale * (working_pixels as f64 / pixels as f64).sqrt() as f32;
    let data = if (working_width, working_height) == (width, height) {
        decode_luma(raw, layout)
    } else {
        box_resample_luma(raw, width, height, layout, working_width, working_height)?
    };
    Ok(LlfWorkingLuma {
        width: working_width,
        height: working_height,
        scale: working_scale,
        data,
    })
}

fn bounded_working_dimensions(
    width: usize,
    height: usize,
    max_pixels: usize,
) -> Result<(usize, usize)> {
    let pixels = width
        .checked_mul(height)
        .ok_or_else(|| anyhow!("diffusion image dimensions overflow"))?;
    if pixels == 0 || max_pixels == 0 {
        bail!("diffusion cannot build an empty local-Laplacian plane");
    }
    if pixels <= max_pixels {
        return Ok((width, height));
    }

    let resize = (max_pixels as f64 / pixels as f64).sqrt();
    let mut working_width = ((width as f64 * resize).floor() as usize).max(1);
    let mut working_height = ((height as f64 * resize).floor() as usize).max(1);
    if working_width.saturating_mul(working_height) > max_pixels {
        if working_width >= working_height {
            working_width = (max_pixels / working_height).max(1);
        } else {
            working_height = (max_pixels / working_width).max(1);
        }
    }
    debug_assert!(working_width <= width);
    debug_assert!(working_height <= height);
    debug_assert!(working_width.saturating_mul(working_height) <= max_pixels);
    Ok((working_width, working_height))
}

fn box_resample_luma(
    raw: &[u16],
    width: usize,
    height: usize,
    layout: PixelLayout,
    output_width: usize,
    output_height: usize,
) -> Result<Vec<f32>> {
    if output_width == 0 || output_height == 0 || output_width > width || output_height > height {
        bail!("invalid local-Laplacian luminance proxy dimensions");
    }
    let output_len = output_width
        .checked_mul(output_height)
        .ok_or_else(|| anyhow!("diffusion working dimensions overflow"))?;
    let stride = layout.stride();
    let expected = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(stride))
        .ok_or_else(|| anyhow!("diffusion image buffer length overflows"))?;
    if raw.len() != expected {
        bail!(
            "diffusion image buffer has {} samples; expected {expected}",
            raw.len()
        );
    }

    let x_bins = (0..width)
        .map(|x| ((x as u128 * output_width as u128) / width as u128) as usize)
        .collect::<Vec<_>>();
    let y_bins = (0..height)
        .map(|y| ((y as u128 * output_height as u128) / height as u128) as usize)
        .collect::<Vec<_>>();
    let mut x_counts = vec![0usize; output_width];
    let mut y_counts = vec![0usize; output_height];
    for &bin in &x_bins {
        x_counts[bin] += 1;
    }
    for &bin in &y_bins {
        y_counts[bin] += 1;
    }

    let mut output = vec![0.0f32; output_len];
    for (y, row) in raw.chunks_exact(width * stride).enumerate() {
        let output_row = y_bins[y] * output_width;
        for (x, pixel) in row.chunks_exact(stride).enumerate() {
            output[output_row + x_bins[x]] += decode_pixel_luma(pixel, layout);
        }
    }
    for y in 0..output_height {
        for x in 0..output_width {
            let samples = x_counts[x]
                .checked_mul(y_counts[y])
                .ok_or_else(|| anyhow!("diffusion proxy sample count overflow"))?;
            output[y * output_width + x] /= samples as f32;
        }
    }
    Ok(output)
}

#[derive(Debug, Clone, Copy)]
struct LinearSample {
    lower: usize,
    upper: usize,
    upper_weight: f32,
}

fn linear_samples(output_len: usize, input_len: usize) -> Vec<LinearSample> {
    (0..output_len)
        .map(|position| {
            let source = ((position as f64 + 0.5) * input_len as f64 / output_len as f64 - 0.5)
                .clamp(0.0, (input_len - 1) as f64);
            let lower = source.floor() as usize;
            LinearSample {
                lower,
                upper: (lower + 1).min(input_len - 1),
                upper_weight: (source - lower as f64) as f32,
            }
        })
        .collect()
}

fn bilinear_sample(
    plane: &[f32],
    width: usize,
    x_sample: LinearSample,
    y_sample: LinearSample,
) -> f32 {
    let top = lerp(
        plane[y_sample.lower * width + x_sample.lower],
        plane[y_sample.lower * width + x_sample.upper],
        x_sample.upper_weight,
    );
    let bottom = lerp(
        plane[y_sample.upper * width + x_sample.lower],
        plane[y_sample.upper * width + x_sample.upper],
        x_sample.upper_weight,
    );
    lerp(top, bottom, y_sample.upper_weight)
}

fn apply_luma_delta(
    raw: &mut [u16],
    width: usize,
    height: usize,
    layout: PixelLayout,
    delta: &[f32],
    delta_width: usize,
    delta_height: usize,
) {
    debug_assert_eq!(delta.len(), delta_width * delta_height);
    let x_samples = linear_samples(width, delta_width);
    let y_samples = linear_samples(height, delta_height);
    let stride = layout.stride();

    for (y, row) in raw.chunks_exact_mut(width * stride).enumerate() {
        let y_sample = y_samples[y];
        for (x, pixel) in row.chunks_exact_mut(stride).enumerate() {
            let x_sample = x_samples[x];
            let adjustment = bilinear_sample(delta, delta_width, x_sample, y_sample);

            if layout.is_color() {
                let linear = [
                    srgb_to_linear(pixel[0]),
                    srgb_to_linear(pixel[1]),
                    srgb_to_linear(pixel[2]),
                ];
                let before = 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2];
                if before > 1.0e-7 {
                    let ratio = (before + adjustment).clamp(0.0, 1.0) / before;
                    for (channel, value) in pixel.iter_mut().take(3).zip(linear) {
                        *channel = linear_to_srgb_u16(value * ratio);
                    }
                }
            } else {
                let before = srgb_to_linear(pixel[0]);
                pixel[0] = linear_to_srgb_u16(before + adjustment);
            }
        }
    }
}

fn apply_neutral_highlight_glow(
    raw: &mut [u16],
    width: usize,
    height: usize,
    layout: PixelLayout,
    amount: u8,
    scale: f32,
) -> Result<()> {
    let parameters = glow_parameters(amount);
    if parameters.strength == 0.0 {
        return Ok(());
    }
    let pixels = width
        .checked_mul(height)
        .ok_or_else(|| anyhow!("diffusion image dimensions overflow"))?;
    let (working_width, working_height) =
        bounded_working_dimensions(width, height, GLOW_MAX_WORKING_PIXELS)?;
    let working_pixels = working_width
        .checked_mul(working_height)
        .ok_or_else(|| anyhow!("diffusion glow dimensions overflow"))?;
    let working_scale = scale * (working_pixels as f64 / pixels as f64).sqrt() as f32;
    let energy_planes = resample_highlight_energy(
        raw,
        width,
        height,
        layout,
        working_width,
        working_height,
        parameters,
    )?;
    let targets = GLOW_SIGMAS.map(|sigma| sigma * working_scale);
    let mut bloom_planes = Vec::with_capacity(energy_planes.len());
    for energy in energy_planes {
        let mut bloom = vec![0.0; working_pixels];
        accumulate_scale_space(
            energy,
            working_width,
            working_height,
            &targets,
            &GLOW_WEIGHTS,
            &mut bloom,
        );
        bloom_planes.push(bloom);
    }
    apply_highlight_bloom(
        raw,
        width,
        height,
        layout,
        &bloom_planes,
        (working_width, working_height),
        parameters,
    );
    Ok(())
}

fn resample_highlight_energy(
    raw: &[u16],
    width: usize,
    height: usize,
    layout: PixelLayout,
    output_width: usize,
    output_height: usize,
    parameters: GlowParameters,
) -> Result<Vec<Vec<f32>>> {
    if output_width == 0 || output_height == 0 || output_width > width || output_height > height {
        bail!("invalid highlight-glow proxy dimensions");
    }
    let output_len = output_width
        .checked_mul(output_height)
        .ok_or_else(|| anyhow!("diffusion glow dimensions overflow"))?;
    let stride = layout.stride();
    let expected = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(stride))
        .ok_or_else(|| anyhow!("diffusion image buffer length overflows"))?;
    if raw.len() != expected {
        bail!(
            "diffusion image buffer has {} samples; expected {expected}",
            raw.len()
        );
    }

    let x_bins = (0..width)
        .map(|x| ((x as u128 * output_width as u128) / width as u128) as usize)
        .collect::<Vec<_>>();
    let y_bins = (0..height)
        .map(|y| ((y as u128 * output_height as u128) / height as u128) as usize)
        .collect::<Vec<_>>();
    let mut x_counts = vec![0usize; output_width];
    let mut y_counts = vec![0usize; output_height];
    for &bin in &x_bins {
        x_counts[bin] += 1;
    }
    for &bin in &y_bins {
        y_counts[bin] += 1;
    }

    let channels = layout.color_offsets();
    let mut energies = (0..channels.len())
        .map(|_| vec![0.0f32; output_len])
        .collect::<Vec<_>>();
    for (y, row) in raw.chunks_exact(width * stride).enumerate() {
        let output_row = y_bins[y] * output_width;
        for (x, pixel) in row.chunks_exact(stride).enumerate() {
            let mask = highlight_mask(decode_pixel_luma(pixel, layout), parameters);
            let output_index = output_row + x_bins[x];
            for (energy, &channel) in energies.iter_mut().zip(channels) {
                energy[output_index] += srgb_to_linear(pixel[channel]) * mask;
            }
        }
    }
    if (output_width, output_height) != (width, height) {
        for (y, &y_count) in y_counts.iter().enumerate() {
            for (x, &x_count) in x_counts.iter().enumerate() {
                let samples = x_count
                    .checked_mul(y_count)
                    .ok_or_else(|| anyhow!("diffusion glow sample count overflow"))?;
                let reciprocal = 1.0 / samples as f32;
                let index = y * output_width + x;
                for energy in &mut energies {
                    energy[index] *= reciprocal;
                }
            }
        }
    }
    Ok(energies)
}

fn apply_highlight_bloom(
    raw: &mut [u16],
    width: usize,
    height: usize,
    layout: PixelLayout,
    bloom_planes: &[Vec<f32>],
    bloom_size: (usize, usize),
    parameters: GlowParameters,
) {
    let (bloom_width, bloom_height) = bloom_size;
    debug_assert_eq!(bloom_planes.len(), layout.color_offsets().len());
    debug_assert!(
        bloom_planes
            .iter()
            .all(|plane| plane.len() == bloom_width * bloom_height)
    );
    let x_samples = linear_samples(width, bloom_width);
    let y_samples = linear_samples(height, bloom_height);
    let stride = layout.stride();
    let channels = layout.color_offsets();
    for (y, row) in raw.chunks_exact_mut(width * stride).enumerate() {
        let y_sample = y_samples[y];
        for (x, pixel) in row.chunks_exact_mut(stride).enumerate() {
            let x_sample = x_samples[x];
            let mask = highlight_mask(decode_pixel_luma(pixel, layout), parameters);
            for (bloom, &channel) in bloom_planes.iter().zip(channels) {
                let color = srgb_to_linear(pixel[channel]);
                let scattered = bilinear_sample(bloom, bloom_width, x_sample, y_sample);
                let energy = color * mask;
                let output = color - parameters.strength * energy + parameters.strength * scattered;
                pixel[channel] = linear_to_srgb_u16(output);
            }
        }
    }
}

fn highlight_mask(luma: f32, parameters: GlowParameters) -> f32 {
    smoothstep(
        parameters.threshold - parameters.knee,
        parameters.threshold + parameters.knee,
        luma,
    )
}

#[derive(Debug, Clone, Copy)]
struct GlowParameters {
    strength: f32,
    threshold: f32,
    knee: f32,
}

fn glow_parameters(amount: u8) -> GlowParameters {
    const ANCHORS: [(u8, GlowParameters); 5] = [
        (
            0,
            GlowParameters {
                strength: 0.0,
                threshold: 0.90,
                knee: 0.06,
            },
        ),
        (
            25,
            GlowParameters {
                strength: 0.025,
                threshold: 0.85,
                knee: 0.08,
            },
        ),
        (
            50,
            GlowParameters {
                strength: 0.055,
                threshold: 0.78,
                knee: 0.12,
            },
        ),
        (
            75,
            GlowParameters {
                strength: 0.095,
                threshold: 0.70,
                knee: 0.16,
            },
        ),
        (
            100,
            GlowParameters {
                strength: 0.14,
                threshold: 0.62,
                knee: 0.20,
            },
        ),
    ];

    let amount = amount.min(100);
    for pair in ANCHORS.windows(2) {
        let (lower_amount, lower) = pair[0];
        let (upper_amount, upper) = pair[1];
        if amount <= upper_amount {
            let t = (amount - lower_amount) as f32 / (upper_amount - lower_amount) as f32;
            return GlowParameters {
                strength: lerp(lower.strength, upper.strength, t),
                threshold: lerp(lower.threshold, upper.threshold, t),
                knee: lerp(lower.knee, upper.knee, t),
            };
        }
    }
    ANCHORS[ANCHORS.len() - 1].1
}

fn accumulate_scale_space(
    mut current: Vec<f32>,
    width: usize,
    height: usize,
    targets: &[f32],
    weights: &[f32],
    accumulated: &mut [f32],
) {
    debug_assert_eq!(current.len(), width * height);
    debug_assert_eq!(current.len(), accumulated.len());
    debug_assert_eq!(targets.len(), weights.len());

    let mut scratch = vec![0.0; current.len()];
    let mut variance = 0.0f32;
    let max_dilation = width.max(height).saturating_mul(2).max(1);

    for (&target, &weight) in targets.iter().zip(weights) {
        let target_variance = target.max(0.0).powi(2);
        if target_variance > variance + f32::EPSILON {
            let incremental_sigma = (target_variance - variance).sqrt();
            let base_variance = 0.9f32;
            let ideal_dilation = incremental_sigma / base_variance.sqrt();
            let (dilation, mix) = if ideal_dilation < 1.0 {
                (1usize, ideal_dilation * ideal_dilation)
            } else {
                (
                    (ideal_dilation.round() as usize).clamp(1, max_dilation),
                    1.0,
                )
            };
            atrous_blur_in_place(&mut current, &mut scratch, width, height, dilation, mix);
            variance += base_variance * (dilation * dilation) as f32 * mix;
        }
        if weight != 0.0 {
            for (output, value) in accumulated.iter_mut().zip(&current) {
                *output += weight * value;
            }
        }
    }
}

fn atrous_blur_in_place(
    current: &mut [f32],
    scratch: &mut [f32],
    width: usize,
    height: usize,
    dilation: usize,
    mix: f32,
) {
    if current.is_empty() || mix <= 0.0 {
        return;
    }
    let dilation = dilation as isize;
    for y in 0..height {
        let row = y * width;
        for x in 0..width {
            let mut value = 0.0;
            for (kernel_index, weight) in PYRAMID_FILTER.iter().enumerate() {
                let offset = (kernel_index as isize - 2) * dilation;
                let sample_x = reflect_index(x as isize + offset, width);
                value += weight * current[row + sample_x];
            }
            scratch[row + x] = value;
        }
    }
    for y in 0..height {
        for x in 0..width {
            let mut blurred = 0.0;
            for (kernel_index, weight) in PYRAMID_FILTER.iter().enumerate() {
                let offset = (kernel_index as isize - 2) * dilation;
                let sample_y = reflect_index(y as isize + offset, height);
                blurred += weight * scratch[sample_y * width + x];
            }
            let index = y * width + x;
            current[index] = lerp(current[index], blurred, mix);
        }
    }
}

fn local_laplacian_soften(
    input: &[f32],
    width: usize,
    height: usize,
    scale: f32,
    strength: f32,
) -> Result<Vec<f32>> {
    if strength <= 0.0 || input.is_empty() {
        return Ok(input.to_vec());
    }
    let band_weights = normalized_band_weights(width, height, scale);
    let Some(max_band) = band_weights.iter().rposition(|weight| *weight > 0.0) else {
        return Ok(input.to_vec());
    };

    let mut gaussian = Vec::with_capacity(max_band + 2);
    gaussian.push(Plane::new(width, height, input.to_vec())?);
    for level in 0..=max_band {
        gaussian.push(downsample_plane(&gaussian[level])?);
    }
    let mut output_bands = gaussian[..=max_band]
        .iter()
        .map(|plane| Plane::zeros(plane.width, plane.height))
        .collect::<Result<Vec<_>>>()?;

    let reference_step = 1.0 / (LLF_REFERENCE_COUNT - 1) as f32;
    let range_denom = 2.0 * LLF_RANGE_SIGMA * LLF_RANGE_SIGMA;
    for reference_index in 0..LLF_REFERENCE_COUNT {
        let reference = reference_index as f32 * reference_step;
        let remapped = input
            .iter()
            .map(|value| {
                let delta = *value - reference;
                -strength * delta * (-delta * delta / range_denom).exp()
            })
            .collect::<Vec<_>>();
        let mut current = Plane::new(width, height, remapped)?;

        for level in 0..=max_band {
            let next = downsample_plane(&current)?;
            let expanded = upsample_plane(&next, current.width, current.height)?;
            let band_weight = band_weights[level];
            if band_weight > 0.0 {
                for index in 0..current.data.len() {
                    let interpolation =
                        1.0 - ((gaussian[level].data[index] - reference).abs() / reference_step);
                    if interpolation > 0.0 {
                        output_bands[level].data[index] += band_weight
                            * interpolation
                            * (current.data[index] - expanded.data[index]);
                    }
                }
            }
            current = next;
        }
    }

    let residual = &gaussian[max_band + 1];
    let mut reconstructed = Plane::zeros(residual.width, residual.height)?;
    for level in (0..=max_band).rev() {
        let mut expanded = upsample_plane(
            &reconstructed,
            output_bands[level].width,
            output_bands[level].height,
        )?;
        for (value, band) in expanded.data.iter_mut().zip(&output_bands[level].data) {
            *value += band;
        }
        reconstructed = expanded;
    }

    let mut output = input
        .iter()
        .zip(reconstructed.data)
        .map(|(value, delta)| (value + delta).clamp(0.0, 1.0))
        .collect::<Vec<_>>();
    limit_to_local_range(input, &mut output, width, height);
    Ok(output)
}

/// The sampled fast LLF can overshoot by a few code values when only a subset
/// of pyramid bands is reconstructed. Constraining each result to its original
/// 3x3 range removes those approximation rims without restoring the fine
/// texture that the filter intentionally attenuated.
fn limit_to_local_range(input: &[f32], output: &mut [f32], width: usize, height: usize) {
    for y in 0..height {
        for x in 0..width {
            let mut minimum = f32::INFINITY;
            let mut maximum = f32::NEG_INFINITY;
            for offset_y in -1..=1 {
                let sample_y = reflect_index(y as isize + offset_y, height);
                for offset_x in -1..=1 {
                    let sample_x = reflect_index(x as isize + offset_x, width);
                    let value = input[sample_y * width + sample_x];
                    minimum = minimum.min(value);
                    maximum = maximum.max(value);
                }
            }
            let index = y * width + x;
            output[index] = output[index].clamp(minimum, maximum);
        }
    }
}

fn normalized_band_weights(width: usize, height: usize, scale: f32) -> Vec<f32> {
    let max_available = max_pyramid_band(width, height).min(LLF_MAX_BAND);
    let mut weights = vec![0.0; max_available + 1];
    let shift = scale.max(f32::MIN_POSITIVE).log2();
    for base_band in 0..3 {
        let target = base_band as f32 + shift;
        let lower = target.floor();
        let fraction = target - lower;
        for (band, weight) in [
            (lower as isize, 1.0 - fraction),
            (lower as isize + 1, fraction),
        ] {
            if band >= 0 && (band as usize) <= max_available {
                weights[band as usize] += weight;
            }
        }
    }
    for weight in &mut weights {
        *weight = weight.min(1.0);
    }
    weights
}

fn max_pyramid_band(mut width: usize, mut height: usize) -> usize {
    let mut bands = 0;
    while width > 1 || height > 1 {
        bands += 1;
        width = width.div_ceil(2);
        height = height.div_ceil(2);
        if bands > LLF_MAX_BAND {
            break;
        }
    }
    bands.saturating_sub(1)
}

#[derive(Debug, Clone)]
struct Plane {
    width: usize,
    height: usize,
    data: Vec<f32>,
}

impl Plane {
    fn new(width: usize, height: usize, data: Vec<f32>) -> Result<Self> {
        let expected = width
            .checked_mul(height)
            .ok_or_else(|| anyhow!("diffusion plane dimensions overflow"))?;
        if data.len() != expected {
            bail!(
                "diffusion plane has {} samples; expected {expected}",
                data.len()
            );
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    fn zeros(width: usize, height: usize) -> Result<Self> {
        let len = width
            .checked_mul(height)
            .ok_or_else(|| anyhow!("diffusion plane dimensions overflow"))?;
        Ok(Self {
            width,
            height,
            data: vec![0.0; len],
        })
    }
}

fn downsample_plane(input: &Plane) -> Result<Plane> {
    let width = input.width.div_ceil(2);
    let height = input.height.div_ceil(2);
    let mut output = Plane::zeros(width, height)?;
    for y in 0..height {
        for x in 0..width {
            let center_x = (x * 2) as isize;
            let center_y = (y * 2) as isize;
            let mut value = 0.0;
            for (kernel_y, weight_y) in PYRAMID_FILTER.iter().enumerate() {
                let source_y = reflect_index(center_y + kernel_y as isize - 2, input.height);
                for (kernel_x, weight_x) in PYRAMID_FILTER.iter().enumerate() {
                    let source_x = reflect_index(center_x + kernel_x as isize - 2, input.width);
                    value += weight_y * weight_x * input.data[source_y * input.width + source_x];
                }
            }
            output.data[y * width + x] = value;
        }
    }
    Ok(output)
}

fn upsample_plane(input: &Plane, width: usize, height: usize) -> Result<Plane> {
    let mut horizontal = Plane::zeros(width, input.height)?;
    for y in 0..input.height {
        for x in 0..width {
            let coarse = x / 2;
            horizontal.data[y * width + x] = if x % 2 == 0 {
                0.1 * input.data[y * input.width + reflect_index(coarse as isize - 1, input.width)]
                    + 0.8 * input.data[y * input.width + coarse.min(input.width - 1)]
                    + 0.1
                        * input.data
                            [y * input.width + reflect_index(coarse as isize + 1, input.width)]
            } else {
                0.5 * input.data[y * input.width + coarse.min(input.width - 1)]
                    + 0.5
                        * input.data
                            [y * input.width + reflect_index(coarse as isize + 1, input.width)]
            };
        }
    }

    let mut output = Plane::zeros(width, height)?;
    for y in 0..height {
        let coarse = y / 2;
        for x in 0..width {
            output.data[y * width + x] = if y % 2 == 0 {
                0.1 * horizontal.data[reflect_index(coarse as isize - 1, input.height) * width + x]
                    + 0.8 * horizontal.data[coarse.min(input.height - 1) * width + x]
                    + 0.1
                        * horizontal.data
                            [reflect_index(coarse as isize + 1, input.height) * width + x]
            } else {
                0.5 * horizontal.data[coarse.min(input.height - 1) * width + x]
                    + 0.5
                        * horizontal.data
                            [reflect_index(coarse as isize + 1, input.height) * width + x]
            };
        }
    }
    Ok(output)
}

fn decode_luma(raw: &[u16], layout: PixelLayout) -> Vec<f32> {
    if layout.is_color() {
        raw.chunks_exact(layout.stride())
            .map(|pixel| decode_pixel_luma(pixel, layout))
            .collect()
    } else {
        decode_channel(raw, layout.stride(), 0)
    }
}

fn decode_pixel_luma(pixel: &[u16], layout: PixelLayout) -> f32 {
    if layout.is_color() {
        0.2126 * srgb_to_linear(pixel[0])
            + 0.7152 * srgb_to_linear(pixel[1])
            + 0.0722 * srgb_to_linear(pixel[2])
    } else {
        srgb_to_linear(pixel[0])
    }
}

fn decode_channel(raw: &[u16], stride: usize, channel: usize) -> Vec<f32> {
    raw.chunks_exact(stride)
        .map(|pixel| srgb_to_linear(pixel[channel]))
        .collect()
}

fn encode_channel(raw: &mut [u16], stride: usize, channel: usize, linear: &[f32]) {
    for (pixel, value) in raw.chunks_exact_mut(stride).zip(linear) {
        pixel[channel] = linear_to_srgb_u16(*value);
    }
}

fn srgb_to_linear(value: u16) -> f32 {
    static SRGB_TO_LINEAR: OnceLock<Vec<f32>> = OnceLock::new();
    SRGB_TO_LINEAR.get_or_init(|| {
        (0..=u16::MAX)
            .map(|value| {
                let encoded = value as f32 / 65535.0;
                if encoded <= 0.04045 {
                    encoded / 12.92
                } else {
                    ((encoded + 0.055) / 1.055).powf(2.4)
                }
            })
            .collect()
    })[value as usize]
}

#[cfg(test)]
fn srgb_to_linear_direct(value: u16) -> f32 {
    let encoded = value as f32 / 65535.0;
    if encoded <= 0.04045 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb_u16(value: f32) -> u16 {
    let value = value.clamp(0.0, 1.0);
    let encoded = if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 65535.0).round().clamp(0.0, 65535.0) as u16
}

fn reflect_index(index: isize, len: usize) -> usize {
    if len <= 1 {
        return 0;
    }
    let period = (2 * (len - 1)) as isize;
    let reflected = index.rem_euclid(period);
    if reflected < len as isize {
        reflected as usize
    } else {
        (period - reflected) as usize
    }
}

fn smoothstep(lower: f32, upper: f32, value: f32) -> f32 {
    if upper <= lower {
        return (value >= upper) as u8 as f32;
    }
    let t = ((value - lower) / (upper - lower)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(lower: f32, upper: f32, t: f32) -> f32 {
    lower + (upper - lower) * t
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, ImageReader, Rgb};

    fn settings(method: DiffusionMethod, softness: u8, highlight_glow: u8) -> DiffusionSettings {
        DiffusionSettings {
            method,
            softness,
            highlight_glow,
        }
    }

    fn patterned_image(width: u32, height: u32) -> ImageBuffer<Rgb<u16>, Vec<u16>> {
        ImageBuffer::from_fn(width, height, |x, y| {
            Rgb([
                ((x * 811 + y * 97) % 65536) as u16,
                ((x * 193 + y * 617 + 8000) % 65536) as u16,
                ((x * 419 + y * 251 + 16000) % 65536) as u16,
            ])
        })
    }

    #[test]
    fn presets_have_the_documented_strength_pairs() {
        assert_eq!(DiffusionPreset::Off.amounts(), (0, 0));
        assert_eq!(DiffusionPreset::Subtle.amounts(), (25, 25));
        assert_eq!(DiffusionPreset::Medium.amounts(), (50, 50));
        assert_eq!(DiffusionPreset::Strong.amounts(), (75, 75));
        assert!(!DiffusionSettings::default().is_enabled());
        assert_eq!(
            DiffusionMethod::EdgeAwareGlow.to_string(),
            "edge-aware-glow"
        );
        assert_eq!(
            serde_json::to_string(&DiffusionMethod::MultiScaleMist).unwrap(),
            "\"multi-scale-mist\""
        );
        assert_eq!(
            serde_json::from_str::<DiffusionMethod>("\"edge-aware-glow\"").unwrap(),
            DiffusionMethod::EdgeAwareGlow
        );
    }

    #[test]
    fn disabled_file_path_is_byte_identical() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.tif");
        let output = directory.path().join("output.tif");
        patterned_image(8, 6)
            .save_with_format(&input, ImageFormat::Tiff)
            .unwrap();

        apply_diffusion(&input, &output, DiffusionSettings::default()).unwrap();

        assert_eq!(fs::read(input).unwrap(), fs::read(output).unwrap());
    }

    #[test]
    fn constant_images_remain_constant_for_both_methods() {
        for method in [
            DiffusionMethod::MultiScaleMist,
            DiffusionMethod::EdgeAwareGlow,
        ] {
            let mut image = ImageBuffer::from_pixel(24, 18, Rgb([48000u16, 36000, 22000]));
            let expected = image.clone().into_raw();
            render_interleaved_16(
                image.as_mut(),
                24,
                18,
                PixelLayout::Rgb,
                settings(method, 75, 75),
                1.0,
            )
            .unwrap();
            for (actual, expected) in image.into_raw().into_iter().zip(expected) {
                assert!(
                    actual.abs_diff(expected) <= 1,
                    "{method:?}: {actual} != {expected}"
                );
            }
        }
    }

    #[test]
    fn normalized_scale_space_preserves_impulse_energy() {
        let width = 257usize;
        let height = 257usize;
        let mut source = vec![0.0; width * height];
        source[(height / 2) * width + width / 2] = 1.0;
        let targets = MIST_SIGMAS.map(|sigma| sigma * 0.25);
        let weights = [0.20, 0.18, 0.16, 0.15, 0.14, 0.17];
        let mut output = vec![0.0; source.len()];
        accumulate_scale_space(source, width, height, &targets, &weights, &mut output);
        let energy: f32 = output.iter().sum();
        assert!((energy - 1.0).abs() < 1.0e-4, "energy was {energy}");
    }

    #[test]
    fn edge_aware_softness_does_not_reverse_a_step_edge() {
        let width = 64usize;
        let height = 16usize;
        let input = (0..width * height)
            .map(|index| if index % width < width / 2 { 0.1 } else { 0.9 })
            .collect::<Vec<_>>();
        let output = local_laplacian_soften(&input, width, height, 1.0, 0.45).unwrap();
        for row in output.chunks_exact(width) {
            assert!(
                row.windows(2).all(|pair| pair[0] <= pair[1] + 1.0e-6),
                "{row:?}"
            );
            assert!((row[8] - 0.1).abs() < 1.0e-5);
            assert!((row[width - 9] - 0.9).abs() < 1.0e-5);
        }
    }

    #[test]
    fn edge_aware_softness_does_not_reverse_a_smooth_gradient() {
        let width = 257usize;
        let height = 9usize;
        let input = (0..width * height)
            .map(|index| {
                let x = (index % width) as f32 / (width - 1) as f32;
                0.06 + 0.88 * x.powf(1.35)
            })
            .collect::<Vec<_>>();
        let output = local_laplacian_soften(&input, width, height, 1.0, 0.55).unwrap();
        for row in output.chunks_exact(width) {
            assert!(
                row.windows(2).all(|pair| pair[0] <= pair[1] + 1.0e-6),
                "local-Laplacian softness introduced a gradient reversal"
            );
        }
    }

    #[test]
    fn capped_luminance_proxy_is_resolution_equivalent() {
        let low_width = 80usize;
        let low_height = 60usize;
        let low = ImageBuffer::from_fn(low_width as u32, low_height as u32, |x, y| {
            let linear = 0.08
                + 0.72 * x as f32 / (low_width - 1) as f32
                + if (x / 5 + y / 7) % 2 == 0 { 0.025 } else { 0.0 };
            let encoded = linear_to_srgb_u16(linear.min(0.95));
            Rgb([encoded, encoded, encoded])
        });
        let high = ImageBuffer::from_fn((low_width * 2) as u32, (low_height * 2) as u32, |x, y| {
            *low.get_pixel(x / 2, y / 2)
        });

        let low_proxy = llf_working_luma_with_limit(
            low.as_raw(),
            low_width,
            low_height,
            PixelLayout::Rgb,
            1.0,
            1_200,
        )
        .unwrap();
        let high_proxy = llf_working_luma_with_limit(
            high.as_raw(),
            low_width * 2,
            low_height * 2,
            PixelLayout::Rgb,
            2.0,
            1_200,
        )
        .unwrap();

        assert_eq!((low_proxy.width, low_proxy.height), (40, 30));
        assert_eq!(
            (low_proxy.width, low_proxy.height),
            (high_proxy.width, high_proxy.height)
        );
        assert!((low_proxy.scale - 0.5).abs() < 1.0e-6);
        assert!((low_proxy.scale - high_proxy.scale).abs() < 1.0e-6);
        for (low, high) in low_proxy.data.iter().zip(&high_proxy.data) {
            assert!((low - high).abs() < 2.0e-6, "{low} != {high}");
        }

        let low_softened = local_laplacian_soften(
            &low_proxy.data,
            low_proxy.width,
            low_proxy.height,
            low_proxy.scale,
            0.45,
        )
        .unwrap();
        let high_softened = local_laplacian_soften(
            &high_proxy.data,
            high_proxy.width,
            high_proxy.height,
            high_proxy.scale,
            0.45,
        )
        .unwrap();
        for (low, high) in low_softened.iter().zip(high_softened) {
            assert!((low - high).abs() < 3.0e-6, "{low} != {high}");
        }

        let mut low_rendered = low;
        let mut high_rendered = high;
        apply_edge_aware_softness_with_limit(
            low_rendered.as_mut(),
            low_width,
            low_height,
            PixelLayout::Rgb,
            80,
            1.0,
            1_200,
        )
        .unwrap();
        apply_edge_aware_softness_with_limit(
            high_rendered.as_mut(),
            low_width * 2,
            low_height * 2,
            PixelLayout::Rgb,
            80,
            2.0,
            1_200,
        )
        .unwrap();
        let mut maximum_error = 0.0f32;
        let mut total_error = 0.0f64;
        for y in 0..low_height {
            for x in 0..low_width {
                let low_pixel = low_rendered.get_pixel(x as u32, y as u32);
                for channel in 0..3 {
                    let low_value = srgb_to_linear(low_pixel[channel]);
                    let mut high_value = 0.0;
                    for offset_y in 0..2 {
                        for offset_x in 0..2 {
                            high_value += srgb_to_linear(
                                high_rendered.get_pixel(
                                    (x * 2 + offset_x) as u32,
                                    (y * 2 + offset_y) as u32,
                                )[channel],
                            );
                        }
                    }
                    high_value *= 0.25;
                    let error = (low_value - high_value).abs();
                    maximum_error = maximum_error.max(error);
                    total_error += f64::from(error);
                }
            }
        }
        let mean_error = total_error / (low_width * low_height * 3) as f64;
        assert!(mean_error < 2.0e-4, "mean error was {mean_error}");
        assert!(maximum_error < 0.004, "maximum error was {maximum_error}");
    }

    #[test]
    fn capped_glow_energy_is_resolution_equivalent() {
        let low_width = 80usize;
        let low_height = 60usize;
        let low = ImageBuffer::from_fn(low_width as u32, low_height as u32, |x, y| {
            let level = 0.38
                + 0.56 * x as f32 / (low_width - 1) as f32
                + if (x / 5 + y / 7) % 2 == 0 { 0.035 } else { 0.0 };
            Rgb([
                linear_to_srgb_u16((level * 0.98).min(1.0)),
                linear_to_srgb_u16((level * 0.82).min(1.0)),
                linear_to_srgb_u16((level * 0.61).min(1.0)),
            ])
        });
        let high = ImageBuffer::from_fn((low_width * 2) as u32, (low_height * 2) as u32, |x, y| {
            *low.get_pixel(x / 2, y / 2)
        });
        let parameters = glow_parameters(75);
        let low_energy = resample_highlight_energy(
            low.as_raw(),
            low_width,
            low_height,
            PixelLayout::Rgb,
            40,
            30,
            parameters,
        )
        .unwrap();
        let high_energy = resample_highlight_energy(
            high.as_raw(),
            low_width * 2,
            low_height * 2,
            PixelLayout::Rgb,
            40,
            30,
            parameters,
        )
        .unwrap();
        for (low_plane, high_plane) in low_energy.iter().zip(high_energy) {
            for (low, high) in low_plane.iter().zip(high_plane) {
                assert!((low - high).abs() < 2.0e-6, "{low} != {high}");
            }
        }
    }

    #[test]
    fn edge_aware_softness_preserves_hue() {
        let width = 96usize;
        let height = 64usize;
        let mut image = ImageBuffer::from_fn(width as u32, height as u32, |x, y| {
            let texture = if (x + y) % 2 == 0 { 0.035 } else { 0.0 };
            let level = 0.38 + 0.22 * x as f32 / (width - 1) as f32 + texture;
            Rgb([
                linear_to_srgb_u16(level * 0.92),
                linear_to_srgb_u16(level * 0.57),
                linear_to_srgb_u16(level * 0.28),
            ])
        });
        let original = image.clone();

        apply_edge_aware_softness(image.as_mut(), width, height, PixelLayout::Rgb, 100, 1.0)
            .unwrap();

        assert_ne!(image.as_raw(), original.as_raw());
        for (before, after) in original.pixels().zip(image.pixels()) {
            let before = before.0.map(srgb_to_linear);
            let after = after.0.map(srgb_to_linear);
            let before_sum = before.iter().sum::<f32>();
            let after_sum = after.iter().sum::<f32>();
            for channel in 0..3 {
                let before_chroma = before[channel] / before_sum;
                let after_chroma = after[channel] / after_sum;
                assert!(
                    (before_chroma - after_chroma).abs() < 2.5e-4,
                    "channel {channel}: {before_chroma} != {after_chroma}"
                );
            }
        }
    }

    #[test]
    fn srgb_decode_table_matches_direct_transfer_function() {
        for value in (0..=u16::MAX).step_by(257) {
            assert_eq!(srgb_to_linear(value), srgb_to_linear_direct(value));
        }
    }

    #[test]
    fn renderers_are_deterministic() {
        for method in [
            DiffusionMethod::MultiScaleMist,
            DiffusionMethod::EdgeAwareGlow,
        ] {
            let mut first = patterned_image(32, 24);
            let mut second = first.clone();
            let original = first.clone().into_raw();
            render_interleaved_16(
                first.as_mut(),
                32,
                24,
                PixelLayout::Rgb,
                settings(method, 50, 75),
                1.0,
            )
            .unwrap();
            render_interleaved_16(
                second.as_mut(),
                32,
                24,
                PixelLayout::Rgb,
                settings(method, 50, 75),
                1.0,
            )
            .unwrap();
            let first = first.into_raw();
            assert_ne!(first, original, "{method:?} did not alter the image");
            assert_eq!(first, second.into_raw(), "{method:?}");
        }
    }

    #[test]
    fn enabled_tiff_preserves_dimensions_and_rgb16_type() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.tif");
        let output = directory.path().join("output.tif");
        patterned_image(18, 12)
            .save_with_format(&input, ImageFormat::Tiff)
            .unwrap();

        apply_diffusion(
            &input,
            &output,
            settings(DiffusionMethod::MultiScaleMist, 50, 50),
        )
        .unwrap();

        let decoded = ImageReader::open(output).unwrap().decode().unwrap();
        assert_eq!(decoded.dimensions(), (18, 12));
        assert!(matches!(decoded, DynamicImage::ImageRgb16(_)));
    }

    #[test]
    fn invalid_strength_is_rejected() {
        assert!(
            settings(DiffusionMethod::MultiScaleMist, 101, 0)
                .validate()
                .is_err()
        );
        assert!(
            settings(DiffusionMethod::EdgeAwareGlow, 0, 101)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn spatial_normalization_uses_twelve_megapixels() {
        assert!((spatial_scale(4000, 3000).unwrap() - 1.0).abs() < 1.0e-6);
        assert!((spatial_scale(8000, 6000).unwrap() - 2.0).abs() < 1.0e-6);
        assert!((spatial_scale(2000, 1500).unwrap() - 0.5).abs() < 1.0e-6);
        assert_eq!(
            bounded_working_dimensions(4000, 3000, LLF_MAX_WORKING_PIXELS).unwrap(),
            (2000, 1500)
        );
        assert_eq!(
            bounded_working_dimensions(8000, 6000, LLF_MAX_WORKING_PIXELS).unwrap(),
            (2000, 1500)
        );
        let (glow_width, glow_height) =
            bounded_working_dimensions(8256, 5504, GLOW_MAX_WORKING_PIXELS).unwrap();
        let glow_pixels = glow_width * glow_height;
        let source_pixels = 8256usize * 5504usize;
        let glow_scale = spatial_scale(8256, 5504).unwrap()
            * (glow_pixels as f64 / source_pixels as f64).sqrt() as f32;
        assert!(glow_pixels <= GLOW_MAX_WORKING_PIXELS);
        assert!((glow_scale - 1.0).abs() < 5.0e-4, "{glow_scale}");
    }
}
