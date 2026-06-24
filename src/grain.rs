#[cfg(target_arch = "x86_64")]
use std::simd::prelude::*;
use std::{fs, path::Path};

use anyhow::{Context, Result, anyhow};
use image::{DynamicImage, GenericImageView, ImageBuffer, ImageReader, Rgba};
use noise::{NoiseFn, Perlin};
use rayon::prelude::*;

use crate::model::{GrainEngine, GrainSettings};

const KERNEL_LUT_SIZE: usize = 1024;
const KERNEL_MAX_RADIUS2: f64 = 9.0;

/// Apply 16-bit grain to an image file.
///
/// This is the high-bit-depth entrypoint used for TIFF-style outputs. If grain
/// is disabled it preserves the pipeline by copying the input to the requested
/// output; otherwise it decodes without image-size limits, renders grain in
/// memory, and writes the result through the `image` crate. Keeping IO here and
/// noise synthesis in `render_grain` makes the renderer easier to optimize.
pub fn apply_grain(input: &Path, output: &Path, grain: GrainSettings, seed: u64) -> Result<()> {
    apply_grain_with_engine(input, output, grain, seed, GrainEngine::default())
}

pub fn apply_grain_with_engine(
    input: &Path,
    output: &Path,
    grain: GrainSettings,
    seed: u64,
    engine: GrainEngine,
) -> Result<()> {
    if !grain.is_enabled() {
        fs::copy(input, output)
            .with_context(|| format!("copying {} to {}", input.display(), output.display()))?;
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
    let grained = match engine {
        GrainEngine::Rfgr => crate::grain_rfgr::render_grain(image, grain, seed)?,
        GrainEngine::RfgrFast => crate::grain_rfgr::render_grain_fast(image, grain, seed)?,
        GrainEngine::Legacy => render_legacy_grain(image, grain, seed)?,
    };
    grained
        .save(output)
        .with_context(|| format!("saving {}", output.display()))?;
    Ok(())
}

/// Apply 8-bit grain to an image file intended for JPEG export.
///
/// JPEG output is quantized to 8-bit anyway, so this entrypoint converts to RGB8
/// before adding grain and avoids doing 16-bit work that would be discarded. It
/// still decodes the full intermediate image and preserves deterministic seeding
/// so batch runs can reproduce per-file grain when given the same seed.
pub fn apply_grain_8bit(
    input: &Path,
    output: &Path,
    grain: GrainSettings,
    seed: u64,
) -> Result<()> {
    apply_grain_8bit_with_engine(input, output, grain, seed, GrainEngine::default())
}

pub fn apply_grain_8bit_with_engine(
    input: &Path,
    output: &Path,
    grain: GrainSettings,
    seed: u64,
    engine: GrainEngine,
) -> Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let mut reader =
        ImageReader::open(input).with_context(|| format!("opening {}", input.display()))?;
    reader.no_limits();
    let image = reader
        .decode()
        .with_context(|| format!("decoding {}", input.display()))?;
    let grained = if grain.is_enabled() {
        DynamicImage::ImageRgb8(match engine {
            GrainEngine::Rfgr => crate::grain_rfgr::render_grain_8(image, grain, seed)?,
            GrainEngine::RfgrFast => crate::grain_rfgr::render_grain_8_fast(image, grain, seed)?,
            GrainEngine::Legacy => render_legacy_grain_8(image, grain, seed)?,
        })
    } else {
        DynamicImage::ImageRgb8(image.to_rgb8())
    };

    grained
        .save(output)
        .with_context(|| format!("saving {}", output.display()))?;
    Ok(())
}

/// Render silver-clump grain into a 16-bit RGBA image buffer.
///
/// Real film grain is not independent Gaussian noise at each pixel. In black
/// and white stocks it comes from developed silver grains, and in color stocks
/// the visible pattern still behaves more like density clouds than perfectly
/// uncorrelated RGB noise. This renderer therefore builds one signed density
/// value per pixel from three components: a fine Gaussian sample for microscopic
/// randomness, a seeded cellular field for discrete clumps, and low-frequency
/// Perlin modulation for uneven emulsion/cloud structure. The combined value is
/// mostly monochrome, lightly decorrelated per channel, stronger in shadows and
/// lower midtones, and skewed so dark specks carry a little more weight than
/// bright specks. The expensive procedural field is precomputed once into a
/// deterministic texture so row rendering only has to combine luma weighting,
/// channel jitter, and clamped channel writes.
fn render_legacy_grain(
    image: DynamicImage,
    grain: GrainSettings,
    seed: u64,
) -> Result<DynamicImage> {
    let (width, height) = image.dimensions();
    let mut out = image.to_rgba16().into_raw();
    let luma_weight = luma_weight_lut_u16();
    let model = GrainModel::from_settings(grain);
    let texture = GrainTexture::build(width, height, seed, &model);

    render_grain_16_rows(&mut out, width, seed, &luma_weight, &model, &texture);

    let image = ImageBuffer::<Rgba<u16>, Vec<u16>>::from_raw(width, height, out)
        .ok_or_else(|| anyhow!("failed to rebuild grained image buffer"))?;
    Ok(DynamicImage::ImageRgba16(image))
}

fn render_grain_16_rows(
    out: &mut [u16],
    width: u32,
    seed: u64,
    luma_weight: &[f32],
    model: &GrainModel,
    texture: &GrainTexture,
) {
    let row_stride = width as usize * 4;
    let path = GrainSimdPath::detect();
    out.par_chunks_mut(row_stride)
        .enumerate()
        .for_each(|(y, row)| {
            let texture_row = texture.row(y);
            match path {
                #[cfg(target_arch = "x86_64")]
                GrainSimdPath::Avx512 => unsafe {
                    render_grain_16_row_avx512(row, y, seed, luma_weight, model, texture_row)
                },
                #[cfg(target_arch = "x86_64")]
                GrainSimdPath::Avx2 => unsafe {
                    render_grain_16_row_avx2(row, y, seed, luma_weight, model, texture_row)
                },
                GrainSimdPath::Scalar => {
                    render_grain_16_row_scalar(row, y, seed, luma_weight, model, texture_row)
                }
            }
        });
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw,avx512vl")]
unsafe fn render_grain_16_row_avx512(
    row: &mut [u16],
    y: usize,
    seed: u64,
    luma_weight: &[f32],
    model: &GrainModel,
    texture: &[f32],
) {
    render_grain_16_row_simd::<16>(row, y, seed, luma_weight, model, texture);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn render_grain_16_row_avx2(
    row: &mut [u16],
    y: usize,
    seed: u64,
    luma_weight: &[f32],
    model: &GrainModel,
    texture: &[f32],
) {
    render_grain_16_row_simd::<8>(row, y, seed, luma_weight, model, texture);
}

fn render_grain_16_row_scalar(
    row: &mut [u16],
    y: usize,
    seed: u64,
    luma_weight: &[f32],
    model: &GrainModel,
    texture: &[f32],
) {
    for (x, pixel) in row.chunks_exact_mut(4).enumerate() {
        let luma = luma_u16(pixel[0], pixel[1], pixel[2]);
        let grain_value = texture[x] * model.sigma_16 * luma_weight[luma];
        add_grain_pixel_u16(pixel, x, y, seed, grain_value, model.color_jitter);
    }
}

#[cfg(target_arch = "x86_64")]
fn render_grain_16_row_simd<const LANES: usize>(
    row: &mut [u16],
    y: usize,
    seed: u64,
    luma_weight: &[f32],
    model: &GrainModel,
    texture: &[f32],
) {
    let mut x = 0usize;
    let pixels = row.len() / 4;
    while x + LANES <= pixels {
        let mut r = [0.0f32; LANES];
        let mut g = [0.0f32; LANES];
        let mut b = [0.0f32; LANES];
        let mut dr = [0.0f32; LANES];
        let mut dg = [0.0f32; LANES];
        let mut db = [0.0f32; LANES];

        for lane in 0..LANES {
            let offset = (x + lane) * 4;
            let pr = row[offset];
            let pg = row[offset + 1];
            let pb = row[offset + 2];
            let luma = luma_u16(pr, pg, pb);
            let grain_value = texture[x + lane] * model.sigma_16 * luma_weight[luma];

            r[lane] = pr as f32;
            g[lane] = pg as f32;
            b[lane] = pb as f32;
            let (jr, jg, jb) = channel_dye_jitters(seed, x + lane, y, model.color_jitter);
            dr[lane] = grain_value * jr;
            dg[lane] = grain_value * jg;
            db[lane] = grain_value * jb;
        }

        let rr = (Simd::from_array(r) + Simd::from_array(dr))
            .simd_clamp(Simd::splat(0.0), Simd::splat(65535.0));
        let gg = (Simd::from_array(g) + Simd::from_array(dg))
            .simd_clamp(Simd::splat(0.0), Simd::splat(65535.0));
        let bb = (Simd::from_array(b) + Simd::from_array(db))
            .simd_clamp(Simd::splat(0.0), Simd::splat(65535.0));
        let rr = rr.to_array();
        let gg = gg.to_array();
        let bb = bb.to_array();

        for lane in 0..LANES {
            let offset = (x + lane) * 4;
            row[offset] = rr[lane].round() as u16;
            row[offset + 1] = gg[lane].round() as u16;
            row[offset + 2] = bb[lane].round() as u16;
        }
        x += LANES;
    }

    if x < pixels {
        render_grain_16_row_scalar(
            &mut row[(x * 4)..],
            y,
            seed,
            luma_weight,
            model,
            &texture[x..],
        );
    }
}

/// Render silver-clump grain into an 8-bit RGB image buffer.
///
/// This mirrors `render_grain` but works directly in RGB8 for JPEG-bound data.
/// The same seeded cellular clump field is used, so JPEG exports retain the
/// silver-cluster structure instead of becoming flat white noise after
/// quantization. The only scaling difference is that channel deltas are already
/// expressed in 0..255 space rather than 16-bit channel space.
fn render_legacy_grain_8(
    image: DynamicImage,
    grain: GrainSettings,
    seed: u64,
) -> Result<ImageBuffer<image::Rgb<u8>, Vec<u8>>> {
    let (width, height) = image.dimensions();
    let mut out = image.to_rgb8().into_raw();
    let luma_weight = luma_weight_lut_u8();
    let model = GrainModel::from_settings(grain);
    let texture = GrainTexture::build(width, height, seed, &model);

    render_grain_8_rows(&mut out, width, seed, &luma_weight, &model, &texture);

    ImageBuffer::from_raw(width, height, out)
        .ok_or_else(|| anyhow!("failed to rebuild grained JPEG image buffer"))
}

fn render_grain_8_rows(
    out: &mut [u8],
    width: u32,
    seed: u64,
    luma_weight: &[f32],
    model: &GrainModel,
    texture: &GrainTexture,
) {
    let row_stride = width as usize * 3;
    let path = GrainSimdPath::detect();
    out.par_chunks_mut(row_stride)
        .enumerate()
        .for_each(|(y, row)| {
            let texture_row = texture.row(y);
            match path {
                #[cfg(target_arch = "x86_64")]
                GrainSimdPath::Avx512 => unsafe {
                    render_grain_8_row_avx512(row, y, seed, luma_weight, model, texture_row)
                },
                #[cfg(target_arch = "x86_64")]
                GrainSimdPath::Avx2 => unsafe {
                    render_grain_8_row_avx2(row, y, seed, luma_weight, model, texture_row)
                },
                GrainSimdPath::Scalar => {
                    render_grain_8_row_scalar(row, y, seed, luma_weight, model, texture_row)
                }
            }
        });
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw,avx512vl")]
unsafe fn render_grain_8_row_avx512(
    row: &mut [u8],
    y: usize,
    seed: u64,
    luma_weight: &[f32],
    model: &GrainModel,
    texture: &[f32],
) {
    render_grain_8_row_simd::<16>(row, y, seed, luma_weight, model, texture);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn render_grain_8_row_avx2(
    row: &mut [u8],
    y: usize,
    seed: u64,
    luma_weight: &[f32],
    model: &GrainModel,
    texture: &[f32],
) {
    render_grain_8_row_simd::<8>(row, y, seed, luma_weight, model, texture);
}

fn render_grain_8_row_scalar(
    row: &mut [u8],
    y: usize,
    seed: u64,
    luma_weight: &[f32],
    model: &GrainModel,
    texture: &[f32],
) {
    for (x, pixel) in row.chunks_exact_mut(3).enumerate() {
        let luma = luma_u8(pixel[0], pixel[1], pixel[2]);
        let grain_value = texture[x] * model.sigma_8 * luma_weight[luma];
        add_grain_pixel_u8(pixel, x, y, seed, grain_value, model.color_jitter);
    }
}

#[cfg(target_arch = "x86_64")]
fn render_grain_8_row_simd<const LANES: usize>(
    row: &mut [u8],
    y: usize,
    seed: u64,
    luma_weight: &[f32],
    model: &GrainModel,
    texture: &[f32],
) {
    let mut x = 0usize;
    let pixels = row.len() / 3;
    while x + LANES <= pixels {
        let mut r = [0.0f32; LANES];
        let mut g = [0.0f32; LANES];
        let mut b = [0.0f32; LANES];
        let mut dr = [0.0f32; LANES];
        let mut dg = [0.0f32; LANES];
        let mut db = [0.0f32; LANES];

        for lane in 0..LANES {
            let offset = (x + lane) * 3;
            let pr = row[offset];
            let pg = row[offset + 1];
            let pb = row[offset + 2];
            let luma = luma_u8(pr, pg, pb);
            let grain_value = texture[x + lane] * model.sigma_8 * luma_weight[luma];

            r[lane] = pr as f32;
            g[lane] = pg as f32;
            b[lane] = pb as f32;
            let (jr, jg, jb) = channel_dye_jitters(seed, x + lane, y, model.color_jitter);
            dr[lane] = grain_value * jr;
            dg[lane] = grain_value * jg;
            db[lane] = grain_value * jb;
        }

        let rr = (Simd::from_array(r) + Simd::from_array(dr))
            .simd_clamp(Simd::splat(0.0), Simd::splat(255.0));
        let gg = (Simd::from_array(g) + Simd::from_array(dg))
            .simd_clamp(Simd::splat(0.0), Simd::splat(255.0));
        let bb = (Simd::from_array(b) + Simd::from_array(db))
            .simd_clamp(Simd::splat(0.0), Simd::splat(255.0));
        let rr = rr.to_array();
        let gg = gg.to_array();
        let bb = bb.to_array();

        for lane in 0..LANES {
            let offset = (x + lane) * 3;
            row[offset] = rr[lane].round() as u8;
            row[offset + 1] = gg[lane].round() as u8;
            row[offset + 2] = bb[lane].round() as u8;
        }
        x += LANES;
    }

    if x < pixels {
        render_grain_8_row_scalar(
            &mut row[(x * 3)..],
            y,
            seed,
            luma_weight,
            model,
            &texture[x..],
        );
    }
}

#[derive(Clone, Copy)]
enum GrainSimdPath {
    #[cfg(target_arch = "x86_64")]
    Avx512,
    #[cfg(target_arch = "x86_64")]
    Avx2,
    Scalar,
}

impl GrainSimdPath {
    fn detect() -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            if std::is_x86_feature_detected!("avx512f")
                && std::is_x86_feature_detected!("avx512bw")
                && std::is_x86_feature_detected!("avx512vl")
            {
                return Self::Avx512;
            }
            if std::is_x86_feature_detected!("avx2") {
                return Self::Avx2;
            }
        }
        Self::Scalar
    }
}

fn add_grain_pixel_u16(
    pixel: &mut [u16],
    x: usize,
    y: usize,
    seed: u64,
    grain_value: f32,
    color_jitter: f32,
) {
    let (jr, jg, jb) = channel_dye_jitters(seed, x, y, color_jitter);
    pixel[0] = add_grain(pixel[0], grain_value * jr);
    pixel[1] = add_grain(pixel[1], grain_value * jg);
    pixel[2] = add_grain(pixel[2], grain_value * jb);
}

fn add_grain_pixel_u8(
    pixel: &mut [u8],
    x: usize,
    y: usize,
    seed: u64,
    grain_value: f32,
    color_jitter: f32,
) {
    let (jr, jg, jb) = channel_dye_jitters(seed, x, y, color_jitter);
    pixel[0] = add_grain_u8(pixel[0], grain_value * jr);
    pixel[1] = add_grain_u8(pixel[1], grain_value * jg);
    pixel[2] = add_grain_u8(pixel[2], grain_value * jb);
}

fn luma_u8(r: u8, g: u8, b: u8) -> usize {
    ((54u16 * r as u16 + 183u16 * g as u16 + 19u16 * b as u16) >> 8) as usize
}

fn luma_u16(r: u16, g: u16, b: u16) -> usize {
    ((13933u32 * r as u32 + 46871u32 * g as u32 + 4732u32 * b as u32) >> 16) as usize
}

/// Add a signed grain delta to one 16-bit channel.
///
/// Grain synthesis is performed in float so Gaussian samples and luma weighting
/// can combine naturally. The final channel is rounded and saturated to the
/// valid 16-bit range so noise never wraps around at black or white.
fn add_grain(channel: u16, delta: f32) -> u16 {
    (channel as f32 + delta).round().clamp(0.0, 65535.0) as u16
}

/// Add a signed grain delta to one 8-bit channel.
///
/// This is the JPEG-path equivalent of `add_grain`: accumulate in float, round
/// to the nearest integer, and clamp to the 8-bit range to avoid overflow while
/// keeping the output encoder's expected channel depth.
fn add_grain_u8(channel: u8, delta: f32) -> u8 {
    (channel as f32 + delta).round().clamp(0.0, 255.0) as u8
}

/// Build the 16-bit luma-to-grain-strength lookup table.
///
/// Film grain tends to be more apparent in darker tones and lower midtones.
/// Earlier versions multiplied a shadow-bias table by a separate density mask
/// in the pixel loop; this precombines both curves so each pixel only performs
/// one luma-indexed lookup before scaling the signed grain texture.
fn luma_weight_lut_u16() -> Vec<f32> {
    (0..=65535)
        .map(|luma| {
            let luma = luma as f32 / 65535.0;
            shadow_bias(luma) * density_mask(luma)
        })
        .collect()
}

/// Build the 8-bit luma-to-grain-strength lookup table.
///
/// The shape matches the 16-bit LUT but only has 256 entries, which is enough
/// after JPEG-path quantization. This preserves the same shadow-heavy grain
/// character without paying for a larger table in the RGB8 renderer.
fn luma_weight_lut_u8() -> Vec<f32> {
    (0..=255)
        .map(|luma| {
            let luma = luma as f32 / 255.0;
            shadow_bias(luma) * density_mask(luma)
        })
        .collect()
}

fn shadow_bias(luma: f32) -> f32 {
    0.45 + (1.0 - luma).powf(0.7) * 0.75
}

fn density_mask(luma: f32) -> f32 {
    (1.08 - (luma - 0.38).max(0.0).powf(1.7) * 0.9).clamp(0.32, 1.12)
}

struct GrainModel {
    sigma_8: f32,
    sigma_16: f32,
    cell_size: f64,
    secondary_cell_size: f64,
    clump_radius: f64,
    secondary_radius: f64,
    clump_density: f32,
    fine_mix: f32,
    color_jitter: f32,
    perlin_scale: f64,
    clump_grid_step: usize,
    kernel_lut: Vec<f32>,
}

impl GrainModel {
    fn from_settings(grain: GrainSettings) -> Self {
        let amount = grain.amount as f32 / 100.0;
        let size = (grain.size.max(1) as f64 / 50.0).clamp(0.18, 3.2);
        let frequency = (grain.frequency.max(1) as f32 / 50.0).clamp(0.25, 2.4);
        let cell_size = (4.0 + size * 7.5) / frequency as f64;
        let secondary_cell_size = cell_size * 2.4;
        let clump_grid_step = adaptive_clump_grid_step(cell_size);
        Self {
            sigma_8: amount * 31.0,
            sigma_16: amount * 31.0 * 257.0,
            cell_size,
            secondary_cell_size,
            clump_radius: (cell_size * 0.72).max(1.5),
            secondary_radius: (secondary_cell_size * 0.85).max(3.0),
            clump_density: (0.55 + amount * 0.38).clamp(0.35, 0.92),
            fine_mix: (0.46 + 0.16 * frequency).clamp(0.42, 0.82),
            color_jitter: (0.08 + 0.10 / frequency).clamp(0.08, 0.32),
            perlin_scale: 65.0 * size,
            clump_grid_step,
            kernel_lut: build_kernel_lut(),
        }
    }
}

fn adaptive_clump_grid_step(cell_size: f64) -> usize {
    if cell_size >= 18.0 {
        8
    } else if cell_size >= 10.0 {
        4
    } else {
        2
    }
}

fn build_kernel_lut() -> Vec<f32> {
    (0..=KERNEL_LUT_SIZE)
        .map(|index| {
            let radius2 = index as f64 * KERNEL_MAX_RADIUS2 / KERNEL_LUT_SIZE as f64;
            (-0.5 * radius2).exp() as f32
        })
        .collect()
}

struct GrainTexture {
    width: usize,
    values: Vec<f32>,
}

impl GrainTexture {
    fn build(width: u32, height: u32, seed: u64, model: &GrainModel) -> Self {
        let width = width as usize;
        let height = height as usize;
        let clumps = ClumpGrid::build(width, height, seed, model);
        let mut values = vec![0.0; width * height];

        values
            .par_chunks_mut(width)
            .enumerate()
            .for_each(|(y, row)| {
                for (x, value) in row.iter_mut().enumerate() {
                    let fine = approx_normal_from_hash(hash_pixel(seed, x, y, 0));
                    let grain = fine * clumps.sample(x, y);
                    *value = grain * density_skew(grain);
                }
            });

        Self { width, values }
    }

    fn row(&self, y: usize) -> &[f32] {
        let start = y * self.width;
        &self.values[start..start + self.width]
    }
}

struct ClumpGrid {
    width: usize,
    step: usize,
    values: Vec<f32>,
}

impl ClumpGrid {
    fn build(width: usize, height: usize, seed: u64, model: &GrainModel) -> Self {
        let step = model.clump_grid_step;
        let grid_width = width.div_ceil(step) + 1;
        let grid_height = height.div_ceil(step) + 1;
        let perlin = Perlin::new((seed & 0xffff_ffff) as u32);
        let mut values = vec![0.0; grid_width * grid_height];

        values
            .par_chunks_mut(grid_width)
            .enumerate()
            .for_each(|(gy, row)| {
                let y = (gy * step).min(height.saturating_sub(1)) as f64;
                for (gx, value) in row.iter_mut().enumerate() {
                    let x = (gx * step).min(width.saturating_sub(1)) as f64;
                    *value = silver_clump_strength(x, y, seed, model, &perlin);
                }
            });

        Self {
            width: grid_width,
            step,
            values,
        }
    }

    fn sample(&self, x: usize, y: usize) -> f32 {
        let shift = self.step.trailing_zeros() as usize;
        let mask = self.step - 1;
        let gx = x >> shift;
        let gy = y >> shift;
        let tx = (x & mask) as f32 / self.step as f32;
        let ty = (y & mask) as f32 / self.step as f32;
        let row0 = gy * self.width;
        let row1 = row0 + self.width;
        let c00 = self.values[row0 + gx];
        let c10 = self.values[row0 + gx + 1];
        let c01 = self.values[row1 + gx];
        let c11 = self.values[row1 + gx + 1];
        let top = c00 + (c10 - c00) * tx;
        let bottom = c01 + (c11 - c01) * tx;
        top + (bottom - top) * ty
    }
}

/// Evaluate the silver-clump multiplier at one image coordinate.
///
/// The cellular part places deterministic pseudo-random grain centers in a grid
/// around the pixel and accumulates soft radial kernels from nearby centers.
/// That gives the output discrete clump boundaries and partial overlaps, which
/// reads closer to scanned silver than smooth Perlin modulation alone. A second
/// coarser cellular octave adds irregular groupings, while Perlin only supplies
/// broad emulsion unevenness. The result is normalized to a moderate multiplier
/// range so Lightroom's amount/size/frequency fields remain predictable.
fn silver_clump_strength(x: f64, y: f64, seed: u64, model: &GrainModel, perlin: &Perlin) -> f32 {
    let primary = cellular_clumps(
        x,
        y,
        seed,
        model.cell_size,
        model.clump_radius,
        model.clump_density,
        &model.kernel_lut,
    );
    let secondary = cellular_clumps(
        x,
        y,
        seed ^ 0xD1B5_4A32_D192_ED03,
        model.secondary_cell_size,
        model.secondary_radius,
        model.clump_density * 0.72,
        &model.kernel_lut,
    );
    let cloud = perlin.get([x / model.perlin_scale, y / model.perlin_scale]) as f32;
    let cloud = 0.88 + (cloud + 1.0) * 0.12;
    let clustered = model.fine_mix + primary * 0.52 + secondary * 0.23;
    (clustered * cloud).clamp(0.35, 2.15)
}

fn cellular_clumps(
    x: f64,
    y: f64,
    seed: u64,
    cell_size: f64,
    radius: f64,
    density: f32,
    kernel_lut: &[f32],
) -> f32 {
    let gx = (x / cell_size).floor() as i64;
    let gy = (y / cell_size).floor() as i64;
    let mut energy = 0.0f32;
    let mut norm = 0.0f32;

    for cy in (gy - 1)..=(gy + 1) {
        for cx in (gx - 1)..=(gx + 1) {
            let h = hash_cell(seed, cx, cy);
            let presence = unit_from_hash(h);
            if presence > density {
                continue;
            }
            let ox = unit_from_hash(h.rotate_left(17)) as f64;
            let oy = unit_from_hash(h.rotate_left(37)) as f64;
            let strength = 0.55 + unit_from_hash(h.rotate_left(53)) * 0.7;
            let center_x = (cx as f64 + ox) * cell_size;
            let center_y = (cy as f64 + oy) * cell_size;
            let dist2 = (x - center_x).powi(2) + (y - center_y).powi(2);
            let kernel = radial_kernel(dist2 / (radius * radius), kernel_lut);
            energy += kernel * strength;
            norm += kernel;
        }
    }

    if norm <= f32::EPSILON {
        0.0
    } else {
        (energy / norm).clamp(0.0, 1.4)
    }
}

fn radial_kernel(radius2: f64, kernel_lut: &[f32]) -> f32 {
    if radius2 >= KERNEL_MAX_RADIUS2 {
        return 0.0;
    }
    let position = radius2 * KERNEL_LUT_SIZE as f64 / KERNEL_MAX_RADIUS2;
    let index = position as usize;
    let frac = (position - index as f64) as f32;
    let a = kernel_lut[index];
    let b = kernel_lut[index + 1];
    a + (b - a) * frac
}

fn density_skew(delta: f32) -> f32 {
    if delta < 0.0 { 1.16 } else { 0.88 }
}

fn channel_dye_jitters(seed: u64, x: usize, y: usize, color_jitter: f32) -> (f32, f32, f32) {
    let hash = hash_pixel(seed, x, y, 11);
    (
        channel_jitter_from_hash(hash, color_jitter),
        channel_jitter_from_hash(hash.rotate_left(23), color_jitter * 0.65),
        channel_jitter_from_hash(hash.rotate_left(47), color_jitter),
    )
}

fn channel_jitter_from_hash(hash: u64, color_jitter: f32) -> f32 {
    (1.0 + approx_normal_from_hash(hash) * color_jitter).clamp(0.62, 1.38)
}

fn hash_cell(seed: u64, x: i64, y: i64) -> u64 {
    let mut h = seed
        ^ (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 30;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^ (h >> 31)
}

fn hash_pixel(seed: u64, x: usize, y: usize, stream: u64) -> u64 {
    let mut h = seed
        ^ stream.wrapping_mul(0xA076_1D64_78BD_642F)
        ^ (x as u64).wrapping_mul(0xE703_7ED1_A0B4_28DB)
        ^ (y as u64).wrapping_mul(0x8EBC_6AF0_9C88_C6E3);
    h ^= h >> 32;
    h = h.wrapping_mul(0xBEA2_25F9_EB34_556D);
    h ^= h >> 29;
    h = h.wrapping_mul(0xD2B7_4407_B1CE_6E93);
    h ^ (h >> 32)
}

fn unit_from_hash(hash: u64) -> f32 {
    ((hash >> 40) as u32 as f32) / ((1u32 << 24) as f32)
}

fn approx_normal_from_hash(hash: u64) -> f32 {
    let a = unit_from_hash(hash);
    let b = unit_from_hash(hash.rotate_left(17));
    let c = unit_from_hash(hash.rotate_left(37));
    let d = unit_from_hash(hash.rotate_left(53));
    (a + b + c + d - 2.0) * 0.866_025_4
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgb};

    #[test]
    fn grain_channel_addition_rounds_and_clamps() {
        assert_eq!(add_grain(10, 2.4), 12);
        assert_eq!(add_grain(10, -20.0), 0);
        assert_eq!(add_grain(65530, 20.0), 65535);
        assert_eq!(add_grain_u8(10, 2.6), 13);
        assert_eq!(add_grain_u8(10, -20.0), 0);
        assert_eq!(add_grain_u8(250, 20.0), 255);
    }

    #[test]
    fn luma_helpers_weight_channels_and_scale_to_lut_indexes() {
        assert_eq!(luma_u8(255, 255, 255), 255);
        assert_eq!(luma_u8(0, 0, 0), 0);
        assert_eq!(luma_u16(65535, 65535, 65535), 65535);
        assert_eq!(luma_u16(0, 0, 0), 0);
    }

    #[test]
    fn luma_weight_luts_are_shadow_weighted_and_bounded() {
        let lut8 = luma_weight_lut_u8();
        let lut16 = luma_weight_lut_u16();
        assert_eq!(lut8.len(), 256);
        assert_eq!(lut16.len(), 65536);
        assert!(lut8[0] > lut8[255]);
        assert!(lut16[0] > lut16[65535]);
        assert!(lut8.iter().all(|value| *value >= 0.0 && *value <= 2.0));
    }

    #[test]
    fn grain_model_scales_amount_size_and_frequency_into_positive_parameters() {
        let low = GrainModel::from_settings(GrainSettings {
            amount: 10,
            size: 10,
            frequency: 10,
        });
        let high = GrainModel::from_settings(GrainSettings {
            amount: 80,
            size: 80,
            frequency: 80,
        });
        assert!(high.sigma_8 > low.sigma_8);
        assert!(high.sigma_16 > low.sigma_16);
        assert!(low.cell_size > 0.0);
        assert!(high.clump_radius > 0.0);
        assert!(matches!(adaptive_clump_grid_step(5.0), 2));
        assert!(matches!(adaptive_clump_grid_step(12.0), 4));
        assert!(matches!(adaptive_clump_grid_step(20.0), 8));
    }

    #[test]
    fn kernel_and_hash_functions_are_deterministic_and_bounded() {
        let lut = build_kernel_lut();
        assert_eq!(lut.len(), KERNEL_LUT_SIZE + 1);
        assert!((radial_kernel(0.0, &lut) - 1.0).abs() < 1e-6);
        assert_eq!(radial_kernel(KERNEL_MAX_RADIUS2, &lut), 0.0);

        let hash = hash_pixel(1, 2, 3, 4);
        assert_eq!(hash, hash_pixel(1, 2, 3, 4));
        assert_ne!(hash, hash_pixel(1, 2, 3, 5));
        assert!((0.0..=1.0).contains(&unit_from_hash(hash)));
        assert!((-2.0..=2.0).contains(&approx_normal_from_hash(hash)));
    }

    #[test]
    fn grain_texture_is_deterministic_for_same_seed_and_changes_for_different_seed() {
        let model = GrainModel::from_settings(GrainSettings {
            amount: 30,
            size: 40,
            frequency: 50,
        });
        let a = GrainTexture::build(8, 6, 123, &model);
        let b = GrainTexture::build(8, 6, 123, &model);
        let c = GrainTexture::build(8, 6, 124, &model);
        assert_eq!(a.values, b.values);
        assert_ne!(a.values, c.values);
        assert_eq!(a.row(0).len(), 8);
        assert_eq!(a.row(5).len(), 8);
    }

    #[test]
    fn channel_jitter_stays_within_expected_range() {
        let (r, g, b) = channel_dye_jitters(1, 2, 3, 0.25);
        for value in [r, g, b] {
            assert!((0.62..=1.38).contains(&value));
        }
    }

    #[test]
    fn disabled_16bit_grain_copies_input_without_rendering() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.tif");
        let output = dir.path().join("output.tif");
        let image = ImageBuffer::from_fn(2, 2, |x, y| {
            Rgba([(1000 + x * 100) as u16, (2000 + y * 100) as u16, 3000, 4000])
        });
        DynamicImage::ImageRgba16(image)
            .save_with_format(&input, ImageFormat::Tiff)
            .unwrap();

        apply_grain_with_engine(
            &input,
            &output,
            GrainSettings::default(),
            7,
            GrainEngine::Rfgr,
        )
        .unwrap();

        assert_eq!(fs::read(input).unwrap(), fs::read(output).unwrap());
    }

    #[test]
    fn legacy_engine_dispatch_matches_legacy_renderer() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.png");
        let output = dir.path().join("output.png");
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_fn(5, 4, |x, y| {
            Rgb([(40 + x * 5) as u8, (70 + y * 7) as u8, 120])
        }));
        image.save(&input).unwrap();
        let grain = GrainSettings {
            amount: 25,
            size: 45,
            frequency: 50,
        };
        let expected = render_legacy_grain_8(image, grain, 42).unwrap().into_raw();

        apply_grain_8bit_with_engine(&input, &output, grain, 42, GrainEngine::Legacy).unwrap();
        let actual = ImageReader::open(output)
            .unwrap()
            .decode()
            .unwrap()
            .to_rgb8()
            .into_raw();

        assert_eq!(actual, expected);
    }
}
