use std::{fmt, fs, path::Path, sync::OnceLock};

use anyhow::{Context, Result, anyhow, bail};
use image::{DynamicImage, GenericImageView, ImageBuffer, ImageFormat, ImageReader, Rgb};
use pulp::{Arch, Simd, WithSimd};
use rayon::prelude::*;
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
const PARALLEL_MIN_SAMPLES: usize = 262_144;

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
        let (softness_radius_percent, glow_radius_percent, intensity_percent, highlight_reach) =
            match self {
                Self::Off => (100, 100, 100, 50),
                Self::Subtle => (100, 150, 150, 50),
                Self::Medium => (
                    150,
                    225,
                    225,
                    if matches!(method, DiffusionMethod::EdgeAwareGlow) {
                        60
                    } else {
                        50
                    },
                ),
                Self::Strong => (
                    200,
                    300,
                    300,
                    if matches!(method, DiffusionMethod::EdgeAwareGlow) {
                        70
                    } else {
                        50
                    },
                ),
            };
        DiffusionSettings {
            method,
            softness,
            highlight_glow,
            softness_radius_percent,
            glow_radius_percent,
            intensity_percent,
            highlight_reach,
        }
    }
}

/// Independent diffusion controls. The all-zero strength setting is a strict
/// no-op; neutral advanced controls reproduce the original diffusion renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiffusionSettings {
    pub method: DiffusionMethod,
    pub softness: u8,
    pub highlight_glow: u8,
    pub softness_radius_percent: u16,
    pub glow_radius_percent: u16,
    pub intensity_percent: u16,
    pub highlight_reach: u8,
}

impl Default for DiffusionSettings {
    fn default() -> Self {
        Self {
            method: DiffusionMethod::default(),
            softness: 0,
            highlight_glow: 0,
            softness_radius_percent: 100,
            glow_radius_percent: 100,
            intensity_percent: 100,
            highlight_reach: 50,
        }
    }
}

impl DiffusionSettings {
    pub const fn from_preset(method: DiffusionMethod, preset: DiffusionPreset) -> Self {
        preset.settings(method)
    }

    pub const fn is_enabled(self) -> bool {
        self.softness > 0 || self.highlight_glow > 0
    }

    pub const fn has_neutral_advanced_controls(self) -> bool {
        self.softness_radius_percent == 100
            && self.glow_radius_percent == 100
            && self.intensity_percent == 100
            && self.highlight_reach == 50
    }

    /// Normalize settings that cannot affect pixels so render identities and
    /// equality checks do not invalidate otherwise reusable output.
    pub const fn canonical_render_settings(mut self) -> Self {
        if !self.is_enabled() {
            return Self {
                method: DiffusionMethod::MultiScaleMist,
                softness: 0,
                highlight_glow: 0,
                softness_radius_percent: 100,
                glow_radius_percent: 100,
                intensity_percent: 100,
                highlight_reach: 50,
            };
        }
        if self.softness == 0 {
            self.softness_radius_percent = 100;
        }
        if self.highlight_glow == 0 {
            self.glow_radius_percent = 100;
            self.highlight_reach = 50;
        } else if matches!(self.method, DiffusionMethod::MultiScaleMist) {
            self.highlight_reach = 50;
        }
        self
    }

    pub fn render_equivalent(self, other: Self) -> bool {
        self.canonical_render_settings() == other.canonical_render_settings()
    }

    pub fn render_identity(self) -> Option<String> {
        let settings = self.canonical_render_settings();
        if !settings.is_enabled() {
            return None;
        }
        if settings.has_neutral_advanced_controls() {
            return Some(format!(
                "diffusion-v1={}/{}/{}",
                settings.method.as_str(),
                settings.softness,
                settings.highlight_glow,
            ));
        }
        Some(format!(
            "diffusion-v2={}/{}/{}/{}/{}/{}/{}",
            settings.method.as_str(),
            settings.softness,
            settings.highlight_glow,
            settings.softness_radius_percent,
            settings.glow_radius_percent,
            settings.intensity_percent,
            settings.highlight_reach,
        ))
    }

    pub fn validate(self) -> Result<()> {
        if self.softness > 100 || self.highlight_glow > 100 {
            bail!("diffusion softness and highlight glow must be between 0 and 100");
        }
        if !(50..=400).contains(&self.softness_radius_percent)
            || !(50..=400).contains(&self.glow_radius_percent)
        {
            bail!("diffusion softness and glow radii must be between 50 and 400 percent");
        }
        if !(25..=300).contains(&self.intensity_percent) {
            bail!("diffusion intensity must be between 25 and 300 percent");
        }
        if self.highlight_reach > 100 {
            bail!("diffusion highlight reach must be between 0 and 100");
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
    let settings = settings.canonical_render_settings();

    Arch::new().dispatch(RenderInterleaved {
        raw,
        width,
        height,
        layout,
        settings,
        scale,
    })
}

struct RenderInterleaved<'a> {
    raw: &'a mut [u16],
    width: usize,
    height: usize,
    layout: PixelLayout,
    settings: DiffusionSettings,
    scale: f32,
}

impl WithSimd for RenderInterleaved<'_> {
    type Output = Result<()>;

    #[inline(always)]
    fn with_simd<S: Simd>(self, simd: S) -> Self::Output {
        render_interleaved_16_with_simd(
            self.raw,
            self.width,
            self.height,
            self.layout,
            self.settings,
            self.scale,
            DiffusionKernels { simd },
        )
    }
}

#[derive(Debug, Clone, Copy)]
struct DiffusionKernels<S: Simd> {
    simd: S,
}

impl<S: Simd> DiffusionKernels<S> {
    fn scale(self, input: &[f32], scale: f32) -> Vec<f32> {
        let mut output = vec![0.0; input.len()];
        if input.len() >= PARALLEL_MIN_SAMPLES {
            output
                .par_chunks_mut(PARALLEL_MIN_SAMPLES)
                .zip(input.par_chunks(PARALLEL_MIN_SAMPLES))
                .for_each(|(output, input)| self.scale_slice(output, input, scale));
        } else {
            self.scale_slice(&mut output, input, scale);
        }
        output
    }

    #[inline(always)]
    fn scale_slice(self, output: &mut [f32], input: &[f32], scale: f32) {
        debug_assert_eq!(output.len(), input.len());
        let (output_simd, output_tail) = S::as_mut_simd_f32s(output);
        let (input_simd, input_tail) = S::as_simd_f32s(input);
        debug_assert_eq!(output_simd.len(), input_simd.len());
        let scale_simd = self.simd.splat_f32s(scale);
        for (output, &input) in output_simd.iter_mut().zip(input_simd) {
            *output = self.simd.mul_f32s(input, scale_simd);
        }
        for (output, input) in output_tail.iter_mut().zip(input_tail) {
            *output = *input * scale;
        }
    }

    fn add_scaled(self, output: &mut [f32], input: &[f32], scale: f32) {
        debug_assert_eq!(output.len(), input.len());
        if output.len() >= PARALLEL_MIN_SAMPLES {
            output
                .par_chunks_mut(PARALLEL_MIN_SAMPLES)
                .zip(input.par_chunks(PARALLEL_MIN_SAMPLES))
                .for_each(|(output, input)| self.add_scaled_slice(output, input, scale));
        } else {
            self.add_scaled_slice(output, input, scale);
        }
    }

    #[inline(always)]
    fn add_scaled_slice(self, output: &mut [f32], input: &[f32], scale: f32) {
        let (output_simd, output_tail) = S::as_mut_simd_f32s(output);
        let (input_simd, input_tail) = S::as_simd_f32s(input);
        debug_assert_eq!(output_simd.len(), input_simd.len());
        let scale_simd = self.simd.splat_f32s(scale);
        for (output, &input) in output_simd.iter_mut().zip(input_simd) {
            let scaled = self.simd.mul_f32s(scale_simd, input);
            *output = self.simd.add_f32s(*output, scaled);
        }
        for (output, input) in output_tail.iter_mut().zip(input_tail) {
            *output += scale * *input;
        }
    }

    fn subtract_assign(self, output: &mut [f32], input: &[f32]) {
        debug_assert_eq!(output.len(), input.len());
        if output.len() >= PARALLEL_MIN_SAMPLES {
            output
                .par_chunks_mut(PARALLEL_MIN_SAMPLES)
                .zip(input.par_chunks(PARALLEL_MIN_SAMPLES))
                .for_each(|(output, input)| self.subtract_assign_slice(output, input));
        } else {
            self.subtract_assign_slice(output, input);
        }
    }

    #[inline(always)]
    fn subtract_assign_slice(self, output: &mut [f32], input: &[f32]) {
        let (output_simd, output_tail) = S::as_mut_simd_f32s(output);
        let (input_simd, input_tail) = S::as_simd_f32s(input);
        debug_assert_eq!(output_simd.len(), input_simd.len());
        for (output, &input) in output_simd.iter_mut().zip(input_simd) {
            *output = self.simd.sub_f32s(*output, input);
        }
        for (output, input) in output_tail.iter_mut().zip(input_tail) {
            *output -= input;
        }
    }

    fn add_assign(self, output: &mut [f32], input: &[f32]) {
        debug_assert_eq!(output.len(), input.len());
        if output.len() >= PARALLEL_MIN_SAMPLES {
            output
                .par_chunks_mut(PARALLEL_MIN_SAMPLES)
                .zip(input.par_chunks(PARALLEL_MIN_SAMPLES))
                .for_each(|(output, input)| self.add_assign_slice(output, input));
        } else {
            self.add_assign_slice(output, input);
        }
    }

    #[inline(always)]
    fn add_assign_slice(self, output: &mut [f32], input: &[f32]) {
        let (output_simd, output_tail) = S::as_mut_simd_f32s(output);
        let (input_simd, input_tail) = S::as_simd_f32s(input);
        debug_assert_eq!(output_simd.len(), input_simd.len());
        for (output, &input) in output_simd.iter_mut().zip(input_simd) {
            *output = self.simd.add_f32s(*output, input);
        }
        for (output, input) in output_tail.iter_mut().zip(input_tail) {
            *output += input;
        }
    }

    fn add_clamped(self, input: &[f32], delta: &mut [f32]) {
        debug_assert_eq!(input.len(), delta.len());
        if delta.len() >= PARALLEL_MIN_SAMPLES {
            delta
                .par_chunks_mut(PARALLEL_MIN_SAMPLES)
                .zip(input.par_chunks(PARALLEL_MIN_SAMPLES))
                .for_each(|(delta, input)| self.add_clamped_slice(input, delta));
        } else {
            self.add_clamped_slice(input, delta);
        }
    }

    #[inline(always)]
    fn add_clamped_slice(self, input: &[f32], delta: &mut [f32]) {
        let (input_simd, input_tail) = S::as_simd_f32s(input);
        let (delta_simd, delta_tail) = S::as_mut_simd_f32s(delta);
        debug_assert_eq!(input_simd.len(), delta_simd.len());
        let zero = self.simd.splat_f32s(0.0);
        let one = self.simd.splat_f32s(1.0);
        for (&input, delta) in input_simd.iter().zip(delta_simd.iter_mut()) {
            let value = self.simd.add_f32s(input, *delta);
            let value = self
                .simd
                .select_f32s(self.simd.less_than_f32s(value, zero), zero, value);
            *delta = self
                .simd
                .select_f32s(self.simd.greater_than_f32s(value, one), one, value);
        }
        for (input, delta) in input_tail.iter().zip(delta_tail) {
            *delta = (*input + *delta).clamp(0.0, 1.0);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn accumulate_laplacian_band(
        self,
        output: &mut [f32],
        gaussian: &[f32],
        current: &[f32],
        expanded: &[f32],
        reference: f32,
        reference_step: f32,
        band_weight: f32,
    ) {
        debug_assert_eq!(output.len(), gaussian.len());
        debug_assert_eq!(output.len(), current.len());
        debug_assert_eq!(output.len(), expanded.len());
        if output.len() >= PARALLEL_MIN_SAMPLES {
            output
                .par_chunks_mut(PARALLEL_MIN_SAMPLES)
                .zip(gaussian.par_chunks(PARALLEL_MIN_SAMPLES))
                .zip(current.par_chunks(PARALLEL_MIN_SAMPLES))
                .zip(expanded.par_chunks(PARALLEL_MIN_SAMPLES))
                .for_each(|(((output, gaussian), current), expanded)| {
                    self.accumulate_laplacian_band_slice(
                        output,
                        gaussian,
                        current,
                        expanded,
                        reference,
                        reference_step,
                        band_weight,
                    );
                });
        } else {
            self.accumulate_laplacian_band_slice(
                output,
                gaussian,
                current,
                expanded,
                reference,
                reference_step,
                band_weight,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    fn accumulate_laplacian_band_slice(
        self,
        output: &mut [f32],
        gaussian: &[f32],
        current: &[f32],
        expanded: &[f32],
        reference: f32,
        reference_step: f32,
        band_weight: f32,
    ) {
        let (output_simd, output_tail) = S::as_mut_simd_f32s(output);
        let (gaussian_simd, gaussian_tail) = S::as_simd_f32s(gaussian);
        let (current_simd, current_tail) = S::as_simd_f32s(current);
        let (expanded_simd, expanded_tail) = S::as_simd_f32s(expanded);
        debug_assert_eq!(output_simd.len(), gaussian_simd.len());
        debug_assert_eq!(output_simd.len(), current_simd.len());
        debug_assert_eq!(output_simd.len(), expanded_simd.len());
        let zero = self.simd.splat_f32s(0.0);
        let one = self.simd.splat_f32s(1.0);
        let reference_simd = self.simd.splat_f32s(reference);
        let reference_step_simd = self.simd.splat_f32s(reference_step);
        let band_weight_simd = self.simd.splat_f32s(band_weight);
        for (((output, &gaussian), &current), &expanded) in output_simd
            .iter_mut()
            .zip(gaussian_simd)
            .zip(current_simd)
            .zip(expanded_simd)
        {
            let distance = self
                .simd
                .abs_f32s(self.simd.sub_f32s(gaussian, reference_simd));
            let interpolation = self
                .simd
                .sub_f32s(one, self.simd.div_f32s(distance, reference_step_simd));
            let detail = self.simd.sub_f32s(current, expanded);
            let contribution = self.simd.mul_f32s(band_weight_simd, interpolation);
            let contribution = self.simd.mul_f32s(contribution, detail);
            let updated = self.simd.add_f32s(*output, contribution);
            *output = self.simd.select_f32s(
                self.simd.greater_than_f32s(interpolation, zero),
                updated,
                *output,
            );
        }
        for (((output, gaussian), current), expanded) in output_tail
            .iter_mut()
            .zip(gaussian_tail)
            .zip(current_tail)
            .zip(expanded_tail)
        {
            let interpolation = 1.0 - ((*gaussian - reference).abs() / reference_step);
            if interpolation > 0.0 {
                *output += band_weight * interpolation * (*current - *expanded);
            }
        }
    }
}

impl<S: Simd> DiffusionKernels<S> {
    fn atrous_horizontal_row(self, input: &[f32], output: &mut [f32], dilation: usize) {
        debug_assert_eq!(input.len(), output.len());
        let width = input.len();
        let border = dilation.saturating_mul(2).min(width);
        let interior_end = width.saturating_sub(border);

        for (x, value) in output.iter_mut().enumerate().take(border.min(width)) {
            *value = atrous_horizontal_sample(input, x, dilation);
        }
        if border < interior_end {
            self.atrous_horizontal_interior(
                &mut output[border..interior_end],
                &input[border - 2 * dilation..interior_end - 2 * dilation],
                &input[border - dilation..interior_end - dilation],
                &input[border..interior_end],
                &input[border + dilation..interior_end + dilation],
                &input[border + 2 * dilation..interior_end + 2 * dilation],
            );
        }
        for (x, value) in output.iter_mut().enumerate().skip(interior_end.max(border)) {
            *value = atrous_horizontal_sample(input, x, dilation);
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    fn atrous_horizontal_interior(
        self,
        output: &mut [f32],
        sample_0: &[f32],
        sample_1: &[f32],
        sample_2: &[f32],
        sample_3: &[f32],
        sample_4: &[f32],
    ) {
        let (output_simd, output_tail) = S::as_mut_simd_f32s(output);
        let (sample_0_simd, sample_0_tail) = S::as_simd_f32s(sample_0);
        let (sample_1_simd, sample_1_tail) = S::as_simd_f32s(sample_1);
        let (sample_2_simd, sample_2_tail) = S::as_simd_f32s(sample_2);
        let (sample_3_simd, sample_3_tail) = S::as_simd_f32s(sample_3);
        let (sample_4_simd, sample_4_tail) = S::as_simd_f32s(sample_4);
        let weights = PYRAMID_FILTER.map(|weight| self.simd.splat_f32s(weight));
        for (((((output, &sample_0), &sample_1), &sample_2), &sample_3), &sample_4) in output_simd
            .iter_mut()
            .zip(sample_0_simd)
            .zip(sample_1_simd)
            .zip(sample_2_simd)
            .zip(sample_3_simd)
            .zip(sample_4_simd)
        {
            let mut value = self.simd.splat_f32s(0.0);
            value = self
                .simd
                .add_f32s(value, self.simd.mul_f32s(weights[0], sample_0));
            value = self
                .simd
                .add_f32s(value, self.simd.mul_f32s(weights[1], sample_1));
            value = self
                .simd
                .add_f32s(value, self.simd.mul_f32s(weights[2], sample_2));
            value = self
                .simd
                .add_f32s(value, self.simd.mul_f32s(weights[3], sample_3));
            *output = self
                .simd
                .add_f32s(value, self.simd.mul_f32s(weights[4], sample_4));
        }
        for (((((output, sample_0), sample_1), sample_2), sample_3), sample_4) in output_tail
            .iter_mut()
            .zip(sample_0_tail)
            .zip(sample_1_tail)
            .zip(sample_2_tail)
            .zip(sample_3_tail)
            .zip(sample_4_tail)
        {
            let mut value = 0.0;
            value += PYRAMID_FILTER[0] * *sample_0;
            value += PYRAMID_FILTER[1] * *sample_1;
            value += PYRAMID_FILTER[2] * *sample_2;
            value += PYRAMID_FILTER[3] * *sample_3;
            value += PYRAMID_FILTER[4] * *sample_4;
            *output = value;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn atrous_vertical_row(
        self,
        output: &mut [f32],
        scratch: &[f32],
        width: usize,
        height: usize,
        y: usize,
        dilation: usize,
        mix: f32,
    ) {
        let dilation = dilation as isize;
        let sample_rows = std::array::from_fn::<_, 5, _>(|kernel_index| {
            let offset = (kernel_index as isize - 2) * dilation;
            reflect_index(y as isize + offset, height)
        });
        let row = |sample_y| &scratch[sample_y * width..(sample_y + 1) * width];
        self.atrous_vertical_slices(
            output,
            row(sample_rows[0]),
            row(sample_rows[1]),
            row(sample_rows[2]),
            row(sample_rows[3]),
            row(sample_rows[4]),
            mix,
        );
    }

    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    fn atrous_vertical_slices(
        self,
        output: &mut [f32],
        sample_0: &[f32],
        sample_1: &[f32],
        sample_2: &[f32],
        sample_3: &[f32],
        sample_4: &[f32],
        mix: f32,
    ) {
        let (output_simd, output_tail) = S::as_mut_simd_f32s(output);
        let (sample_0_simd, sample_0_tail) = S::as_simd_f32s(sample_0);
        let (sample_1_simd, sample_1_tail) = S::as_simd_f32s(sample_1);
        let (sample_2_simd, sample_2_tail) = S::as_simd_f32s(sample_2);
        let (sample_3_simd, sample_3_tail) = S::as_simd_f32s(sample_3);
        let (sample_4_simd, sample_4_tail) = S::as_simd_f32s(sample_4);
        let weights = PYRAMID_FILTER.map(|weight| self.simd.splat_f32s(weight));
        let mix_simd = self.simd.splat_f32s(mix);
        for (((((output, &sample_0), &sample_1), &sample_2), &sample_3), &sample_4) in output_simd
            .iter_mut()
            .zip(sample_0_simd)
            .zip(sample_1_simd)
            .zip(sample_2_simd)
            .zip(sample_3_simd)
            .zip(sample_4_simd)
        {
            let mut blurred = self.simd.splat_f32s(0.0);
            blurred = self
                .simd
                .add_f32s(blurred, self.simd.mul_f32s(weights[0], sample_0));
            blurred = self
                .simd
                .add_f32s(blurred, self.simd.mul_f32s(weights[1], sample_1));
            blurred = self
                .simd
                .add_f32s(blurred, self.simd.mul_f32s(weights[2], sample_2));
            blurred = self
                .simd
                .add_f32s(blurred, self.simd.mul_f32s(weights[3], sample_3));
            blurred = self
                .simd
                .add_f32s(blurred, self.simd.mul_f32s(weights[4], sample_4));
            let delta = self.simd.sub_f32s(blurred, *output);
            let delta = self.simd.mul_f32s(delta, mix_simd);
            *output = self.simd.add_f32s(*output, delta);
        }
        for (((((output, sample_0), sample_1), sample_2), sample_3), sample_4) in output_tail
            .iter_mut()
            .zip(sample_0_tail)
            .zip(sample_1_tail)
            .zip(sample_2_tail)
            .zip(sample_3_tail)
            .zip(sample_4_tail)
        {
            let mut blurred = 0.0;
            blurred += PYRAMID_FILTER[0] * *sample_0;
            blurred += PYRAMID_FILTER[1] * *sample_1;
            blurred += PYRAMID_FILTER[2] * *sample_2;
            blurred += PYRAMID_FILTER[3] * *sample_3;
            blurred += PYRAMID_FILTER[4] * *sample_4;
            *output = lerp(*output, blurred, mix);
        }
    }

    fn upsample_vertical_row(
        self,
        output: &mut [f32],
        horizontal: &[f32],
        width: usize,
        input_height: usize,
        y: usize,
    ) {
        let coarse = y / 2;
        let center_y = coarse.min(input_height - 1);
        let next_y = reflect_index(coarse as isize + 1, input_height);
        let center = &horizontal[center_y * width..(center_y + 1) * width];
        let next = &horizontal[next_y * width..(next_y + 1) * width];
        if y.is_multiple_of(2) {
            let previous_y = reflect_index(coarse as isize - 1, input_height);
            let previous = &horizontal[previous_y * width..(previous_y + 1) * width];
            self.upsample_even_vertical_slice(output, previous, center, next);
        } else {
            self.upsample_odd_vertical_slice(output, center, next);
        }
    }

    #[inline(always)]
    fn upsample_even_vertical_slice(
        self,
        output: &mut [f32],
        previous: &[f32],
        center: &[f32],
        next: &[f32],
    ) {
        let (output_simd, output_tail) = S::as_mut_simd_f32s(output);
        let (previous_simd, previous_tail) = S::as_simd_f32s(previous);
        let (center_simd, center_tail) = S::as_simd_f32s(center);
        let (next_simd, next_tail) = S::as_simd_f32s(next);
        let tenth = self.simd.splat_f32s(0.1);
        let eight_tenths = self.simd.splat_f32s(0.8);
        for (((output, &previous), &center), &next) in output_simd
            .iter_mut()
            .zip(previous_simd)
            .zip(center_simd)
            .zip(next_simd)
        {
            let previous = self.simd.mul_f32s(tenth, previous);
            let center = self.simd.mul_f32s(eight_tenths, center);
            let value = self.simd.add_f32s(previous, center);
            *output = self.simd.add_f32s(value, self.simd.mul_f32s(tenth, next));
        }
        for (((output, previous), center), next) in output_tail
            .iter_mut()
            .zip(previous_tail)
            .zip(center_tail)
            .zip(next_tail)
        {
            *output = 0.1 * *previous + 0.8 * *center + 0.1 * *next;
        }
    }

    #[inline(always)]
    fn upsample_odd_vertical_slice(self, output: &mut [f32], center: &[f32], next: &[f32]) {
        let (output_simd, output_tail) = S::as_mut_simd_f32s(output);
        let (center_simd, center_tail) = S::as_simd_f32s(center);
        let (next_simd, next_tail) = S::as_simd_f32s(next);
        let half = self.simd.splat_f32s(0.5);
        for ((output, &center), &next) in output_simd.iter_mut().zip(center_simd).zip(next_simd) {
            let center = self.simd.mul_f32s(half, center);
            let next = self.simd.mul_f32s(half, next);
            *output = self.simd.add_f32s(center, next);
        }
        for ((output, center), next) in output_tail.iter_mut().zip(center_tail).zip(next_tail) {
            *output = 0.5 * *center + 0.5 * *next;
        }
    }

    fn limit_local_range_row(
        self,
        input: &[f32],
        output: &mut [f32],
        width: usize,
        height: usize,
        y: usize,
    ) {
        if width <= 2 {
            for (x, value) in output.iter_mut().enumerate() {
                *value = clamp_to_local_range(input, *value, width, height, x, y);
            }
            return;
        }

        output[0] = clamp_to_local_range(input, output[0], width, height, 0, y);
        output[width - 1] =
            clamp_to_local_range(input, output[width - 1], width, height, width - 1, y);
        let previous_y = reflect_index(y as isize - 1, height);
        let next_y = reflect_index(y as isize + 1, height);
        let row = |sample_y, start, end| &input[sample_y * width + start..sample_y * width + end];
        self.limit_local_range_interior(
            &mut output[1..width - 1],
            [
                row(previous_y, 0, width - 2),
                row(previous_y, 1, width - 1),
                row(previous_y, 2, width),
                row(y, 0, width - 2),
                row(y, 1, width - 1),
                row(y, 2, width),
                row(next_y, 0, width - 2),
                row(next_y, 1, width - 1),
                row(next_y, 2, width),
            ],
        );
    }

    fn limit_local_range_interior(self, output: &mut [f32], samples: [&[f32]; 9]) {
        let (output_simd, output_tail) = S::as_mut_simd_f32s(output);
        let simd_samples = samples.map(S::as_simd_f32s);
        let infinity = self.simd.splat_f32s(f32::INFINITY);
        let negative_infinity = self.simd.splat_f32s(f32::NEG_INFINITY);
        for index in 0..output_simd.len() {
            let mut minimum = infinity;
            let mut maximum = negative_infinity;
            for (sample, _) in &simd_samples {
                minimum = self.simd.min_f32s(minimum, sample[index]);
                maximum = self.simd.max_f32s(maximum, sample[index]);
            }
            let value = output_simd[index];
            let value =
                self.simd
                    .select_f32s(self.simd.less_than_f32s(value, minimum), minimum, value);
            output_simd[index] =
                self.simd
                    .select_f32s(self.simd.greater_than_f32s(value, maximum), maximum, value);
        }
        for index in 0..output_tail.len() {
            let mut minimum = f32::INFINITY;
            let mut maximum = f32::NEG_INFINITY;
            for (_, sample) in &simd_samples {
                minimum = minimum.min(sample[index]);
                maximum = maximum.max(sample[index]);
            }
            output_tail[index] = output_tail[index].clamp(minimum, maximum);
        }
    }
}

fn atrous_horizontal_sample(input: &[f32], x: usize, dilation: usize) -> f32 {
    let mut value = 0.0;
    let dilation = dilation as isize;
    for (kernel_index, weight) in PYRAMID_FILTER.iter().enumerate() {
        let offset = (kernel_index as isize - 2) * dilation;
        let sample_x = reflect_index(x as isize + offset, input.len());
        value += weight * input[sample_x];
    }
    value
}

fn clamp_to_local_range(
    input: &[f32],
    value: f32,
    width: usize,
    height: usize,
    x: usize,
    y: usize,
) -> f32 {
    let mut minimum = f32::INFINITY;
    let mut maximum = f32::NEG_INFINITY;
    for offset_y in -1..=1 {
        let sample_y = reflect_index(y as isize + offset_y, height);
        for offset_x in -1..=1 {
            let sample_x = reflect_index(x as isize + offset_x, width);
            let sample = input[sample_y * width + sample_x];
            minimum = minimum.min(sample);
            maximum = maximum.max(sample);
        }
    }
    value.clamp(minimum, maximum)
}

fn render_interleaved_16_with_simd<S: Simd>(
    raw: &mut [u16],
    width: usize,
    height: usize,
    layout: PixelLayout,
    settings: DiffusionSettings,
    scale: f32,
    kernels: DiffusionKernels<S>,
) -> Result<()> {
    match settings.method {
        DiffusionMethod::MultiScaleMist => {
            render_multi_scale_mist(raw, width, height, layout, settings, scale, kernels)
        }
        DiffusionMethod::EdgeAwareGlow => {
            render_edge_aware_glow(raw, width, height, layout, settings, scale, kernels)
        }
    }
}

fn render_multi_scale_mist<S: Simd>(
    raw: &mut [u16],
    width: usize,
    height: usize,
    layout: PixelLayout,
    settings: DiffusionSettings,
    scale: f32,
    kernels: DiffusionKernels<S>,
) -> Result<()> {
    let softness = settings.softness as f32 / 100.0;
    let glow = settings.highlight_glow as f32 / 100.0;
    let intensity = settings.intensity_percent as f32 / 100.0;
    let mut core_mix = 0.10 * softness * intensity;
    let mut wing_mix = 0.12 * glow * intensity;
    if core_mix == 0.0 && wing_mix == 0.0 {
        return Ok(());
    }
    const MAX_COMBINED_MIX: f32 = 0.85;
    let combined_mix = core_mix + wing_mix;
    if combined_mix > MAX_COMBINED_MIX {
        let normalization = MAX_COMBINED_MIX / combined_mix;
        core_mix *= normalization;
        wing_mix *= normalization;
    }

    let softness_radius = settings.softness_radius_percent as f32 / 100.0;
    let glow_radius = settings.glow_radius_percent as f32 / 100.0;
    let mut scales = [(0.0, 0.0); 6];
    for index in 0..3 {
        scales[index] = (
            MIST_SIGMAS[index] * scale * softness_radius,
            core_mix * MIST_CORE_WEIGHTS[index],
        );
        scales[index + 3] = (
            MIST_SIGMAS[index + 3] * scale * glow_radius,
            wing_mix * MIST_WING_WEIGHTS[index],
        );
    }
    scales.sort_by(|left, right| left.0.total_cmp(&right.0));
    let targets = scales.map(|(target, _)| target);
    let weights = scales.map(|(_, weight)| weight);
    let base_mix = 1.0 - core_mix - wing_mix;

    for &channel in layout.color_offsets() {
        let source = decode_channel(raw, layout.stride(), channel);
        let mut accumulated = kernels.scale(&source, base_mix);
        accumulate_scale_space(
            source,
            width,
            height,
            &targets,
            &weights,
            &mut accumulated,
            kernels,
        );
        encode_channel(raw, layout.stride(), channel, &accumulated);
    }
    Ok(())
}

fn render_edge_aware_glow<S: Simd>(
    raw: &mut [u16],
    width: usize,
    height: usize,
    layout: PixelLayout,
    settings: DiffusionSettings,
    scale: f32,
    kernels: DiffusionKernels<S>,
) -> Result<()> {
    let intensity = settings.intensity_percent as f32 / 100.0;
    if settings.softness > 0 {
        apply_edge_aware_softness(
            raw,
            width,
            height,
            layout,
            settings.softness,
            intensity,
            scale * settings.softness_radius_percent as f32 / 100.0,
            kernels,
        )?;
    }
    if settings.highlight_glow > 0 {
        apply_neutral_highlight_glow(
            raw,
            width,
            height,
            layout,
            adjusted_glow_parameters(settings.highlight_glow, intensity, settings.highlight_reach),
            scale * settings.glow_radius_percent as f32 / 100.0,
            kernels,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_edge_aware_softness<S: Simd>(
    raw: &mut [u16],
    width: usize,
    height: usize,
    layout: PixelLayout,
    amount: u8,
    intensity: f32,
    scale: f32,
    kernels: DiffusionKernels<S>,
) -> Result<()> {
    let strength = edge_softness_strength(amount, intensity);
    apply_edge_aware_softness_with_limit(
        raw,
        width,
        height,
        layout,
        strength,
        scale,
        LLF_MAX_WORKING_PIXELS,
        kernels,
    )
}

fn edge_softness_strength(amount: u8, intensity: f32) -> f32 {
    (0.55 * amount as f32 / 100.0 * intensity).min(1.0)
}

#[allow(clippy::too_many_arguments)]
fn apply_edge_aware_softness_with_limit<S: Simd>(
    raw: &mut [u16],
    width: usize,
    height: usize,
    layout: PixelLayout,
    strength: f32,
    scale: f32,
    max_working_pixels: usize,
    kernels: DiffusionKernels<S>,
) -> Result<()> {
    let working =
        llf_working_luma_with_limit(raw, width, height, layout, scale, max_working_pixels)?;
    let mut delta = local_laplacian_soften(
        &working.data,
        working.width,
        working.height,
        working.scale,
        strength,
        kernels,
    )?;
    kernels.subtract_assign(&mut delta, &working.data);
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

    let x_ranges = resample_bin_ranges(width, output_width);
    let y_ranges = resample_bin_ranges(height, output_height);
    let normalize = (output_width, output_height) != (width, height);
    let mut output = vec![0.0f32; output_len];
    let fill_row = |(output_y, output): (usize, &mut [f32])| {
        let y_range = y_ranges[output_y].clone();
        for (output_x, value) in output.iter_mut().enumerate() {
            let x_range = x_ranges[output_x].clone();
            let mut sum = 0.0;
            for source_y in y_range.clone() {
                let row = &raw[source_y * width * stride..(source_y + 1) * width * stride];
                for source_x in x_range.clone() {
                    let pixel = &row[source_x * stride..(source_x + 1) * stride];
                    sum += decode_pixel_luma(pixel, layout);
                }
            }
            if normalize {
                sum /= (x_range.len() * y_range.len()) as f32;
            }
            *value = sum;
        }
    };
    if width * height >= PARALLEL_MIN_SAMPLES {
        output
            .par_chunks_mut(output_width)
            .enumerate()
            .for_each(fill_row);
    } else {
        output
            .chunks_exact_mut(output_width)
            .enumerate()
            .for_each(fill_row);
    }
    Ok(output)
}

fn resample_bin_ranges(input_len: usize, output_len: usize) -> Vec<std::ops::Range<usize>> {
    (0..output_len)
        .map(|bin| {
            let start_numerator = bin as u128 * input_len as u128;
            let end_numerator = (bin + 1) as u128 * input_len as u128;
            let denominator = output_len as u128;
            let start = start_numerator.div_ceil(denominator) as usize;
            let end = end_numerator.div_ceil(denominator) as usize;
            start..end
        })
        .collect()
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

    let apply_row = |(y, row): (usize, &mut [u16])| {
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
    };
    if width * height >= PARALLEL_MIN_SAMPLES {
        raw.par_chunks_mut(width * stride)
            .enumerate()
            .for_each(apply_row);
    } else {
        raw.chunks_exact_mut(width * stride)
            .enumerate()
            .for_each(apply_row);
    }
}

fn apply_neutral_highlight_glow<S: Simd>(
    raw: &mut [u16],
    width: usize,
    height: usize,
    layout: PixelLayout,
    parameters: GlowParameters,
    scale: f32,
    kernels: DiffusionKernels<S>,
) -> Result<()> {
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
            kernels,
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

    let x_ranges = resample_bin_ranges(width, output_width);
    let y_ranges = resample_bin_ranges(height, output_height);
    let channels = layout.color_offsets();
    let mut energies = (0..channels.len())
        .map(|_| vec![0.0f32; output_len])
        .collect::<Vec<_>>();
    let normalize = (output_width, output_height) != (width, height);
    match energies.as_mut_slice() {
        [energy] => {
            let channel = channels[0];
            let fill_row = |(output_y, output): (usize, &mut [f32])| {
                fill_highlight_energy_row_luma(
                    output, output_y, raw, width, stride, layout, &x_ranges, &y_ranges, channel,
                    parameters, normalize,
                );
            };
            if width * height >= PARALLEL_MIN_SAMPLES {
                energy
                    .par_chunks_mut(output_width)
                    .enumerate()
                    .for_each(fill_row);
            } else {
                energy
                    .chunks_exact_mut(output_width)
                    .enumerate()
                    .for_each(fill_row);
            }
        }
        [red, green, blue] => {
            type EnergyRows<'a> = (((&'a mut [f32], &'a mut [f32]), &'a mut [f32]), usize);
            let fill_row = |(((red, green), blue), output_y): EnergyRows<'_>| {
                fill_highlight_energy_row_rgb(
                    red,
                    green,
                    blue,
                    output_y,
                    raw,
                    width,
                    stride,
                    layout,
                    &x_ranges,
                    &y_ranges,
                    [channels[0], channels[1], channels[2]],
                    parameters,
                    normalize,
                );
            };
            if width * height >= PARALLEL_MIN_SAMPLES {
                red.par_chunks_mut(output_width)
                    .zip(green.par_chunks_mut(output_width))
                    .zip(blue.par_chunks_mut(output_width))
                    .zip(0..output_height)
                    .for_each(fill_row);
            } else {
                red.chunks_exact_mut(output_width)
                    .zip(green.chunks_exact_mut(output_width))
                    .zip(blue.chunks_exact_mut(output_width))
                    .zip(0..output_height)
                    .for_each(fill_row);
            }
        }
        _ => unreachable!("diffusion supports one or three color channels"),
    }
    Ok(energies)
}

#[allow(clippy::too_many_arguments)]
fn fill_highlight_energy_row_luma(
    output: &mut [f32],
    output_y: usize,
    raw: &[u16],
    width: usize,
    stride: usize,
    layout: PixelLayout,
    x_ranges: &[std::ops::Range<usize>],
    y_ranges: &[std::ops::Range<usize>],
    channel: usize,
    parameters: GlowParameters,
    normalize: bool,
) {
    let y_range = y_ranges[output_y].clone();
    for (output_x, energy) in output.iter_mut().enumerate() {
        let x_range = x_ranges[output_x].clone();
        let mut sum = 0.0;
        for source_y in y_range.clone() {
            let row = &raw[source_y * width * stride..(source_y + 1) * width * stride];
            for source_x in x_range.clone() {
                let pixel = &row[source_x * stride..(source_x + 1) * stride];
                let mask = highlight_mask(decode_pixel_luma(pixel, layout), parameters);
                sum += srgb_to_linear(pixel[channel]) * mask;
            }
        }
        if normalize {
            sum *= 1.0 / (x_range.len() * y_range.len()) as f32;
        }
        *energy = sum;
    }
}

#[allow(clippy::too_many_arguments)]
fn fill_highlight_energy_row_rgb(
    red: &mut [f32],
    green: &mut [f32],
    blue: &mut [f32],
    output_y: usize,
    raw: &[u16],
    width: usize,
    stride: usize,
    layout: PixelLayout,
    x_ranges: &[std::ops::Range<usize>],
    y_ranges: &[std::ops::Range<usize>],
    channels: [usize; 3],
    parameters: GlowParameters,
    normalize: bool,
) {
    let y_range = y_ranges[output_y].clone();
    for output_x in 0..red.len() {
        let x_range = x_ranges[output_x].clone();
        let mut sums = [0.0; 3];
        for source_y in y_range.clone() {
            let row = &raw[source_y * width * stride..(source_y + 1) * width * stride];
            for source_x in x_range.clone() {
                let pixel = &row[source_x * stride..(source_x + 1) * stride];
                let mask = highlight_mask(decode_pixel_luma(pixel, layout), parameters);
                for (sum, channel) in sums.iter_mut().zip(channels) {
                    *sum += srgb_to_linear(pixel[channel]) * mask;
                }
            }
        }
        if normalize {
            let reciprocal = 1.0 / (x_range.len() * y_range.len()) as f32;
            for sum in &mut sums {
                *sum *= reciprocal;
            }
        }
        red[output_x] = sums[0];
        green[output_x] = sums[1];
        blue[output_x] = sums[2];
    }
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
    let apply_row = |(y, row): (usize, &mut [u16])| {
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
    };
    if width * height >= PARALLEL_MIN_SAMPLES {
        raw.par_chunks_mut(width * stride)
            .enumerate()
            .for_each(apply_row);
    } else {
        raw.chunks_exact_mut(width * stride)
            .enumerate()
            .for_each(apply_row);
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

fn adjusted_glow_parameters(amount: u8, intensity: f32, highlight_reach: u8) -> GlowParameters {
    let mut parameters = glow_parameters(amount);
    parameters.strength = (parameters.strength * intensity).min(0.5);
    parameters.threshold = adjusted_highlight_threshold(parameters.threshold, highlight_reach);
    parameters
}

fn adjusted_highlight_threshold(threshold: f32, highlight_reach: u8) -> f32 {
    let reach_offset = (50_i16 - i16::from(highlight_reach)) as f32 * 0.005;
    (threshold + reach_offset).clamp(0.30, 0.98)
}

fn accumulate_scale_space<S: Simd>(
    mut current: Vec<f32>,
    width: usize,
    height: usize,
    targets: &[f32],
    weights: &[f32],
    accumulated: &mut [f32],
    kernels: DiffusionKernels<S>,
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
            atrous_blur_in_place(
                &mut current,
                &mut scratch,
                width,
                height,
                dilation,
                mix,
                kernels,
            );
            variance += base_variance * (dilation * dilation) as f32 * mix;
        }
        if weight != 0.0 {
            kernels.add_scaled(accumulated, &current, weight);
        }
    }
}

fn atrous_blur_in_place<S: Simd>(
    current: &mut [f32],
    scratch: &mut [f32],
    width: usize,
    height: usize,
    dilation: usize,
    mix: f32,
    kernels: DiffusionKernels<S>,
) {
    if current.is_empty() || mix <= 0.0 {
        return;
    }
    if current.len() >= PARALLEL_MIN_SAMPLES {
        scratch
            .par_chunks_mut(width)
            .enumerate()
            .for_each(|(y, output)| {
                kernels.atrous_horizontal_row(
                    &current[y * width..(y + 1) * width],
                    output,
                    dilation,
                );
            });
    } else {
        for (input, output) in current
            .chunks_exact(width)
            .zip(scratch.chunks_exact_mut(width))
        {
            kernels.atrous_horizontal_row(input, output, dilation);
        }
    }

    if current.len() >= PARALLEL_MIN_SAMPLES {
        current
            .par_chunks_mut(width)
            .enumerate()
            .for_each(|(y, output)| {
                kernels.atrous_vertical_row(output, scratch, width, height, y, dilation, mix);
            });
    } else {
        for (y, output) in current.chunks_exact_mut(width).enumerate() {
            kernels.atrous_vertical_row(output, scratch, width, height, y, dilation, mix);
        }
    }
}

fn local_laplacian_soften<S: Simd>(
    input: &[f32],
    width: usize,
    height: usize,
    scale: f32,
    strength: f32,
    kernels: DiffusionKernels<S>,
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
        gaussian.push(downsample_plane(&gaussian[level], kernels)?);
    }
    let mut output_bands = gaussian[..=max_band]
        .iter()
        .map(|plane| Plane::zeros(plane.width, plane.height))
        .collect::<Result<Vec<_>>>()?;

    let reference_step = 1.0 / (LLF_REFERENCE_COUNT - 1) as f32;
    let range_denom = 2.0 * LLF_RANGE_SIGMA * LLF_RANGE_SIGMA;
    for reference_index in 0..LLF_REFERENCE_COUNT {
        let reference = reference_index as f32 * reference_step;
        let remap = |value: &f32| {
            let delta = *value - reference;
            -strength * delta * (-delta * delta / range_denom).exp()
        };
        let remapped = if input.len() >= PARALLEL_MIN_SAMPLES {
            input.par_iter().map(remap).collect::<Vec<_>>()
        } else {
            input.iter().map(remap).collect::<Vec<_>>()
        };
        let mut current = Plane::new(width, height, remapped)?;

        for level in 0..=max_band {
            let next = downsample_plane(&current, kernels)?;
            let expanded = upsample_plane(&next, current.width, current.height, kernels)?;
            let band_weight = band_weights[level];
            if band_weight > 0.0 {
                kernels.accumulate_laplacian_band(
                    &mut output_bands[level].data,
                    &gaussian[level].data,
                    &current.data,
                    &expanded.data,
                    reference,
                    reference_step,
                    band_weight,
                );
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
            kernels,
        )?;
        kernels.add_assign(&mut expanded.data, &output_bands[level].data);
        reconstructed = expanded;
    }

    let mut output = reconstructed.data;
    kernels.add_clamped(input, &mut output);
    limit_to_local_range(input, &mut output, width, height, kernels);
    Ok(output)
}

/// The sampled fast LLF can overshoot by a few code values when only a subset
/// of pyramid bands is reconstructed. Constraining each result to its original
/// 3x3 range removes those approximation rims without restoring the fine
/// texture that the filter intentionally attenuated.
fn limit_to_local_range<S: Simd>(
    input: &[f32],
    output: &mut [f32],
    width: usize,
    height: usize,
    kernels: DiffusionKernels<S>,
) {
    let clamp_row = |(y, output): (usize, &mut [f32])| {
        kernels.limit_local_range_row(input, output, width, height, y);
    };
    if input.len() >= PARALLEL_MIN_SAMPLES {
        output.par_chunks_mut(width).enumerate().for_each(clamp_row);
    } else {
        output
            .chunks_exact_mut(width)
            .enumerate()
            .for_each(clamp_row);
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

fn downsample_plane<S: Simd>(input: &Plane, _kernels: DiffusionKernels<S>) -> Result<Plane> {
    let width = input.width.div_ceil(2);
    let height = input.height.div_ceil(2);
    let mut output = Plane::zeros(width, height)?;
    let fill_row = |(y, output): (usize, &mut [f32])| {
        for (x, output) in output.iter_mut().enumerate() {
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
            *output = value;
        }
    };
    if input.data.len() >= PARALLEL_MIN_SAMPLES {
        output
            .data
            .par_chunks_mut(width)
            .enumerate()
            .for_each(fill_row);
    } else {
        output
            .data
            .chunks_exact_mut(width)
            .enumerate()
            .for_each(fill_row);
    }
    Ok(output)
}

fn upsample_plane<S: Simd>(
    input: &Plane,
    width: usize,
    height: usize,
    kernels: DiffusionKernels<S>,
) -> Result<Plane> {
    let mut horizontal = Plane::zeros(width, input.height)?;
    let fill_horizontal_row = |(y, output): (usize, &mut [f32])| {
        for (x, output) in output.iter_mut().enumerate() {
            let coarse = x / 2;
            *output = if x.is_multiple_of(2) {
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
    };
    if horizontal.data.len() >= PARALLEL_MIN_SAMPLES {
        horizontal
            .data
            .par_chunks_mut(width)
            .enumerate()
            .for_each(fill_horizontal_row);
    } else {
        horizontal
            .data
            .chunks_exact_mut(width)
            .enumerate()
            .for_each(fill_horizontal_row);
    }

    let mut output = Plane::zeros(width, height)?;
    let fill_vertical_row = |(y, output): (usize, &mut [f32])| {
        kernels.upsample_vertical_row(output, &horizontal.data, width, input.height, y);
    };
    if output.data.len() >= PARALLEL_MIN_SAMPLES {
        output
            .data
            .par_chunks_mut(width)
            .enumerate()
            .for_each(fill_vertical_row);
    } else {
        output
            .data
            .chunks_exact_mut(width)
            .enumerate()
            .for_each(fill_vertical_row);
    }
    Ok(output)
}

fn decode_luma(raw: &[u16], layout: PixelLayout) -> Vec<f32> {
    if layout.is_color() {
        if raw.len() / layout.stride() >= PARALLEL_MIN_SAMPLES {
            raw.par_chunks(layout.stride())
                .map(|pixel| decode_pixel_luma(pixel, layout))
                .collect()
        } else {
            raw.chunks_exact(layout.stride())
                .map(|pixel| decode_pixel_luma(pixel, layout))
                .collect()
        }
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
    if raw.len() / stride >= PARALLEL_MIN_SAMPLES {
        raw.par_chunks(stride)
            .map(|pixel| srgb_to_linear(pixel[channel]))
            .collect()
    } else {
        raw.chunks_exact(stride)
            .map(|pixel| srgb_to_linear(pixel[channel]))
            .collect()
    }
}

fn encode_channel(raw: &mut [u16], stride: usize, channel: usize, linear: &[f32]) {
    if linear.len() >= PARALLEL_MIN_SAMPLES {
        raw.par_chunks_mut(stride)
            .zip(linear.par_iter())
            .for_each(|(pixel, value)| pixel[channel] = linear_to_srgb_u16(*value));
    } else {
        for (pixel, value) in raw.chunks_exact_mut(stride).zip(linear) {
            pixel[channel] = linear_to_srgb_u16(*value);
        }
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

    fn scalar_kernels() -> DiffusionKernels<pulp::Scalar> {
        DiffusionKernels { simd: pulp::Scalar }
    }

    fn settings(method: DiffusionMethod, softness: u8, highlight_glow: u8) -> DiffusionSettings {
        DiffusionSettings {
            method,
            softness,
            highlight_glow,
            ..DiffusionSettings::default()
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

        assert_eq!(
            DiffusionPreset::Subtle.settings(DiffusionMethod::MultiScaleMist),
            DiffusionSettings {
                method: DiffusionMethod::MultiScaleMist,
                softness: 25,
                highlight_glow: 25,
                softness_radius_percent: 100,
                glow_radius_percent: 150,
                intensity_percent: 150,
                highlight_reach: 50,
            }
        );
        assert_eq!(
            DiffusionPreset::Medium.settings(DiffusionMethod::EdgeAwareGlow),
            DiffusionSettings {
                method: DiffusionMethod::EdgeAwareGlow,
                softness: 50,
                highlight_glow: 50,
                softness_radius_percent: 150,
                glow_radius_percent: 225,
                intensity_percent: 225,
                highlight_reach: 60,
            }
        );
        assert_eq!(
            DiffusionPreset::Strong.settings(DiffusionMethod::EdgeAwareGlow),
            DiffusionSettings {
                method: DiffusionMethod::EdgeAwareGlow,
                softness: 75,
                highlight_glow: 75,
                softness_radius_percent: 200,
                glow_radius_percent: 300,
                intensity_percent: 300,
                highlight_reach: 70,
            }
        );
    }

    #[test]
    fn old_serialized_settings_receive_neutral_advanced_controls() {
        let settings: DiffusionSettings = serde_json::from_str(
            r#"{"method":"edge-aware-glow","softness":40,"highlight_glow":30}"#,
        )
        .unwrap();
        assert_eq!(
            settings,
            DiffusionSettings {
                method: DiffusionMethod::EdgeAwareGlow,
                softness: 40,
                highlight_glow: 30,
                ..DiffusionSettings::default()
            }
        );
        assert!(settings.has_neutral_advanced_controls());
    }

    #[test]
    fn neutral_advanced_controls_preserve_legacy_pixels() {
        let width = 96usize;
        let height = 64usize;
        let scale = spatial_scale(width as u32, height as u32).unwrap();

        let mut expected_mist = patterned_image(width as u32, height as u32);
        let mut actual_mist = expected_mist.clone();
        let core_mix = 0.10 * 0.75;
        let wing_mix = 0.12 * 0.75;
        let targets = MIST_SIGMAS.map(|sigma| sigma * scale);
        let mut weights = [0.0; 6];
        for index in 0..3 {
            weights[index] = core_mix * MIST_CORE_WEIGHTS[index];
            weights[index + 3] = wing_mix * MIST_WING_WEIGHTS[index];
        }
        for &channel in PixelLayout::Rgb.color_offsets() {
            let source = decode_channel(expected_mist.as_raw(), PixelLayout::Rgb.stride(), channel);
            let mut accumulated = source
                .iter()
                .map(|value| value * (1.0 - core_mix - wing_mix))
                .collect::<Vec<_>>();
            accumulate_scale_space(
                source,
                width,
                height,
                &targets,
                &weights,
                &mut accumulated,
                scalar_kernels(),
            );
            encode_channel(
                expected_mist.as_mut(),
                PixelLayout::Rgb.stride(),
                channel,
                &accumulated,
            );
        }
        render_interleaved_16(
            actual_mist.as_mut(),
            width,
            height,
            PixelLayout::Rgb,
            settings(DiffusionMethod::MultiScaleMist, 75, 75),
            scale,
        )
        .unwrap();
        assert_eq!(actual_mist.as_raw(), expected_mist.as_raw());

        let mut expected_edge = patterned_image(width as u32, height as u32);
        let mut actual_edge = expected_edge.clone();
        apply_edge_aware_softness(
            expected_edge.as_mut(),
            width,
            height,
            PixelLayout::Rgb,
            75,
            1.0,
            scale,
            scalar_kernels(),
        )
        .unwrap();
        apply_neutral_highlight_glow(
            expected_edge.as_mut(),
            width,
            height,
            PixelLayout::Rgb,
            glow_parameters(75),
            scale,
            scalar_kernels(),
        )
        .unwrap();
        render_interleaved_16(
            actual_edge.as_mut(),
            width,
            height,
            PixelLayout::Rgb,
            settings(DiffusionMethod::EdgeAwareGlow, 75, 75),
            scale,
        )
        .unwrap();
        assert_eq!(actual_edge.as_raw(), expected_edge.as_raw());
    }

    #[test]
    fn render_identity_ignores_parameters_that_cannot_affect_pixels() {
        let disabled = DiffusionSettings {
            method: DiffusionMethod::EdgeAwareGlow,
            softness_radius_percent: 400,
            glow_radius_percent: 50,
            intensity_percent: 300,
            highlight_reach: 100,
            ..DiffusionSettings::default()
        };
        assert!(disabled.render_equivalent(DiffusionSettings::default()));
        assert_eq!(disabled.render_identity(), None);

        let neutral = settings(DiffusionMethod::EdgeAwareGlow, 25, 30);
        assert_eq!(
            neutral.render_identity().as_deref(),
            Some("diffusion-v1=edge-aware-glow/25/30")
        );

        let mist = DiffusionSettings {
            method: DiffusionMethod::MultiScaleMist,
            softness: 50,
            highlight_reach: 100,
            ..DiffusionSettings::default()
        };
        assert_eq!(
            mist.render_identity().as_deref(),
            Some("diffusion-v1=multi-scale-mist/50/0")
        );

        let advanced = DiffusionSettings {
            method: DiffusionMethod::EdgeAwareGlow,
            highlight_glow: 50,
            glow_radius_percent: 225,
            intensity_percent: 150,
            highlight_reach: 70,
            ..DiffusionSettings::default()
        };
        assert_eq!(
            advanced.render_identity().as_deref(),
            Some("diffusion-v2=edge-aware-glow/0/50/100/225/150/70")
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
        accumulate_scale_space(
            source,
            width,
            height,
            &targets,
            &weights,
            &mut output,
            scalar_kernels(),
        );
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
        let output =
            local_laplacian_soften(&input, width, height, 1.0, 0.45, scalar_kernels()).unwrap();
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
        let output =
            local_laplacian_soften(&input, width, height, 1.0, 0.55, scalar_kernels()).unwrap();
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
            scalar_kernels(),
        )
        .unwrap();
        let high_softened = local_laplacian_soften(
            &high_proxy.data,
            high_proxy.width,
            high_proxy.height,
            high_proxy.scale,
            0.45,
            scalar_kernels(),
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
            edge_softness_strength(80, 1.0),
            1.0,
            1_200,
            scalar_kernels(),
        )
        .unwrap();
        apply_edge_aware_softness_with_limit(
            high_rendered.as_mut(),
            low_width * 2,
            low_height * 2,
            PixelLayout::Rgb,
            edge_softness_strength(80, 1.0),
            2.0,
            1_200,
            scalar_kernels(),
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
    fn highlight_reach_only_changes_the_edge_aware_threshold() {
        let legacy = glow_parameters(75);
        let neutral = adjusted_glow_parameters(75, 1.0, 50);
        let narrow = adjusted_glow_parameters(75, 1.0, 0);
        let broad = adjusted_glow_parameters(75, 1.0, 100);
        assert_eq!(neutral.strength, legacy.strength);
        assert_eq!(neutral.threshold, legacy.threshold);
        assert_eq!(neutral.knee, legacy.knee);
        assert!(narrow.threshold > neutral.threshold);
        assert!(broad.threshold < neutral.threshold);
        assert_eq!(narrow.strength, broad.strength);
        assert_eq!(narrow.knee, broad.knee);
        assert_eq!(adjusted_glow_parameters(100, 10.0, 50).strength, 0.5);
        assert_eq!(adjusted_highlight_threshold(0.95, 0), 0.98);
        assert_eq!(adjusted_highlight_threshold(0.10, 100), 0.30);
    }

    #[test]
    fn stronger_intensity_and_separate_radii_change_mist_output() {
        let source = patterned_image(96, 64);
        let mut neutral = source.clone();
        let mut strong = source.clone();
        let mut crossed_radii = source.clone();
        render_diffusion_rgb16(
            &mut neutral,
            DiffusionSettings {
                softness: 75,
                highlight_glow: 75,
                ..DiffusionSettings::default()
            },
        )
        .unwrap();
        render_diffusion_rgb16(
            &mut strong,
            DiffusionSettings {
                softness: 75,
                highlight_glow: 75,
                intensity_percent: 300,
                ..DiffusionSettings::default()
            },
        )
        .unwrap();
        render_diffusion_rgb16(
            &mut crossed_radii,
            DiffusionSettings {
                softness: 75,
                highlight_glow: 75,
                softness_radius_percent: 400,
                glow_radius_percent: 50,
                ..DiffusionSettings::default()
            },
        )
        .unwrap();

        let difference = |image: &ImageBuffer<Rgb<u16>, Vec<u16>>| {
            image
                .as_raw()
                .iter()
                .zip(source.as_raw())
                .map(|(rendered, original)| u64::from(rendered.abs_diff(*original)))
                .sum::<u64>()
        };
        assert!(difference(&strong) > difference(&neutral));
        assert_ne!(crossed_radii.as_raw(), neutral.as_raw());
    }

    #[test]
    fn inactive_advanced_controls_do_not_change_pixels() {
        let source = patterned_image(64, 48);
        let mut first = source.clone();
        let mut second = source.clone();
        render_diffusion_rgb16(
            &mut first,
            DiffusionSettings {
                softness: 60,
                glow_radius_percent: 50,
                highlight_reach: 0,
                ..DiffusionSettings::default()
            },
        )
        .unwrap();
        render_diffusion_rgb16(
            &mut second,
            DiffusionSettings {
                softness: 60,
                glow_radius_percent: 400,
                highlight_reach: 100,
                ..DiffusionSettings::default()
            },
        )
        .unwrap();
        assert_eq!(first, second);

        first = source.clone();
        second = source;
        render_diffusion_rgb16(
            &mut first,
            DiffusionSettings {
                method: DiffusionMethod::EdgeAwareGlow,
                highlight_glow: 60,
                softness_radius_percent: 50,
                ..DiffusionSettings::default()
            },
        )
        .unwrap();
        render_diffusion_rgb16(
            &mut second,
            DiffusionSettings {
                method: DiffusionMethod::EdgeAwareGlow,
                highlight_glow: 60,
                softness_radius_percent: 400,
                ..DiffusionSettings::default()
            },
        )
        .unwrap();
        assert_eq!(first, second);
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

        apply_edge_aware_softness(
            image.as_mut(),
            width,
            height,
            PixelLayout::Rgb,
            100,
            1.0,
            1.0,
            scalar_kernels(),
        )
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
    fn auto_simd_matches_forced_scalar_for_all_layouts_and_tails() {
        for (width, height) in [(1usize, 1usize), (2, 3), (9, 2), (37, 23)] {
            for layout in [
                PixelLayout::Luma,
                PixelLayout::LumaAlpha,
                PixelLayout::Rgb,
                PixelLayout::Rgba,
            ] {
                let stride = layout.stride();
                let source = (0..width * height)
                    .flat_map(|pixel| {
                        (0..stride).map(move |channel| {
                            if channel >= layout.color_offsets().len() && !layout.is_color() {
                                10_000u16.wrapping_add(pixel as u16 * 37)
                            } else if matches!(layout, PixelLayout::Rgba) && channel == 3 {
                                20_000u16.wrapping_add(pixel as u16 * 53)
                            } else {
                                (pixel as u16)
                                    .wrapping_mul(977)
                                    .wrapping_add(channel as u16 * 11_003)
                            }
                        })
                    })
                    .collect::<Vec<_>>();

                for method in [
                    DiffusionMethod::MultiScaleMist,
                    DiffusionMethod::EdgeAwareGlow,
                ] {
                    let settings = DiffusionSettings {
                        method,
                        softness: 72,
                        highlight_glow: 81,
                        softness_radius_percent: 175,
                        glow_radius_percent: 275,
                        intensity_percent: 240,
                        highlight_reach: 68,
                    };
                    let scale = spatial_scale(width as u32, height as u32).unwrap();
                    let mut scalar = source.clone();
                    render_interleaved_16_with_simd(
                        &mut scalar,
                        width,
                        height,
                        layout,
                        settings.canonical_render_settings(),
                        scale,
                        scalar_kernels(),
                    )
                    .unwrap();
                    let mut automatic = source.clone();
                    render_interleaved_16(&mut automatic, width, height, layout, settings, scale)
                        .unwrap();
                    assert_eq!(automatic, scalar, "{method:?} {layout:?} {width}x{height}");

                    if matches!(layout, PixelLayout::LumaAlpha | PixelLayout::Rgba) {
                        let alpha = stride - 1;
                        for (rendered, original) in automatic
                            .chunks_exact(stride)
                            .zip(source.chunks_exact(stride))
                        {
                            assert_eq!(rendered[alpha], original[alpha]);
                        }
                    }
                }
            }
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
        for invalid in [49, 401] {
            assert!(
                DiffusionSettings {
                    softness_radius_percent: invalid,
                    ..DiffusionSettings::default()
                }
                .validate()
                .is_err()
            );
            assert!(
                DiffusionSettings {
                    glow_radius_percent: invalid,
                    ..DiffusionSettings::default()
                }
                .validate()
                .is_err()
            );
        }
        for invalid in [24, 301] {
            assert!(
                DiffusionSettings {
                    intensity_percent: invalid,
                    ..DiffusionSettings::default()
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            DiffusionSettings {
                highlight_reach: 101,
                ..DiffusionSettings::default()
            }
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
