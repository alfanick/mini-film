use std::{fs, path::Path};

use anyhow::{Context, Result, anyhow};
use image::{DynamicImage, GenericImageView, ImageBuffer, ImageReader, Rgba};
use noise::{NoiseFn, Perlin};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, Normal};
use rayon::prelude::*;

use crate::model::GrainSettings;

/// Apply 16-bit grain to an image file.
///
/// This is the high-bit-depth entrypoint used for TIFF-style outputs. If grain
/// is disabled it preserves the pipeline by copying the input to the requested
/// output; otherwise it decodes without image-size limits, renders grain in
/// memory, and writes the result through the `image` crate. Keeping IO here and
/// noise synthesis in `render_grain` makes the renderer easier to optimize.
pub fn apply_grain(input: &Path, output: &Path, grain: GrainSettings, seed: u64) -> Result<()> {
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
    let grained = render_grain(image, grain, seed)?;
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
        DynamicImage::ImageRgb8(render_grain_8(image, grain, seed)?)
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
/// bright specks. Rows are processed in parallel, while row-local ChaCha RNGs
/// keep output deterministic for a given seed.
fn render_grain(image: DynamicImage, grain: GrainSettings, seed: u64) -> Result<DynamicImage> {
    let (width, height) = image.dimensions();
    let mut out = image.to_rgba16().into_raw();
    let normal = Normal::new(0.0, 1.0)?;
    let perlin = Perlin::new((seed & 0xffff_ffff) as u32);
    let shadow_bias = shadow_bias_lut_u16();
    let model = GrainModel::from_settings(grain);

    let row_stride = width as usize * 4;

    out.par_chunks_mut(row_stride)
        .enumerate()
        .for_each(|(y, row)| {
            let mut rng = ChaCha8Rng::seed_from_u64(row_seed(seed, y as u64));
            for (x, pixel) in row.chunks_exact_mut(4).enumerate() {
                let r = pixel[0];
                let g = pixel[1];
                let b = pixel[2];
                let luma = ((13933u32 * r as u32 + 46871u32 * g as u32 + 4732u32 * b as u32) >> 16)
                    as usize;
                let clump = silver_clump_strength(x as f64, y as f64, seed, &model, &perlin);
                let fine = normal.sample(&mut rng) as f32;
                let mut grain_value =
                    fine * model.sigma_16 * shadow_bias[luma] * clump * density_mask_u16(luma);
                grain_value *= density_skew(grain_value);

                pixel[0] = add_grain(
                    r,
                    grain_value * channel_dye_jitter(&mut rng, &normal, model.color_jitter),
                );
                pixel[1] = add_grain(
                    g,
                    grain_value * channel_dye_jitter(&mut rng, &normal, model.color_jitter * 0.65),
                );
                pixel[2] = add_grain(
                    b,
                    grain_value * channel_dye_jitter(&mut rng, &normal, model.color_jitter),
                );
            }
        });

    let image = ImageBuffer::<Rgba<u16>, Vec<u16>>::from_raw(width, height, out)
        .ok_or_else(|| anyhow!("failed to rebuild grained image buffer"))?;
    Ok(DynamicImage::ImageRgba16(image))
}

/// Render silver-clump grain into an 8-bit RGB image buffer.
///
/// This mirrors `render_grain` but works directly in RGB8 for JPEG-bound data.
/// The same seeded cellular clump field is used, so JPEG exports retain the
/// silver-cluster structure instead of becoming flat white noise after
/// quantization. The only scaling difference is that channel deltas are already
/// expressed in 0..255 space rather than 16-bit channel space.
fn render_grain_8(
    image: DynamicImage,
    grain: GrainSettings,
    seed: u64,
) -> Result<ImageBuffer<image::Rgb<u8>, Vec<u8>>> {
    let (width, height) = image.dimensions();
    let mut out = image.to_rgb8().into_raw();
    let normal = Normal::new(0.0, 1.0)?;
    let perlin = Perlin::new((seed & 0xffff_ffff) as u32);
    let shadow_bias = shadow_bias_lut_u8();
    let model = GrainModel::from_settings(grain);

    let row_stride = width as usize * 3;

    out.par_chunks_mut(row_stride)
        .enumerate()
        .for_each(|(y, row)| {
            let mut rng = ChaCha8Rng::seed_from_u64(row_seed(seed, y as u64));
            for (x, pixel) in row.chunks_exact_mut(3).enumerate() {
                let r = pixel[0];
                let g = pixel[1];
                let b = pixel[2];
                let luma =
                    ((54u16 * r as u16 + 183u16 * g as u16 + 19u16 * b as u16) >> 8) as usize;
                let clump = silver_clump_strength(x as f64, y as f64, seed, &model, &perlin);
                let fine = normal.sample(&mut rng) as f32;
                let mut grain_value =
                    fine * model.sigma_8 * shadow_bias[luma] * clump * density_mask_u8(luma);
                grain_value *= density_skew(grain_value);

                pixel[0] = add_grain_u8(
                    r,
                    grain_value * channel_dye_jitter(&mut rng, &normal, model.color_jitter),
                );
                pixel[1] = add_grain_u8(
                    g,
                    grain_value * channel_dye_jitter(&mut rng, &normal, model.color_jitter * 0.65),
                );
                pixel[2] = add_grain_u8(
                    b,
                    grain_value * channel_dye_jitter(&mut rng, &normal, model.color_jitter),
                );
            }
        });

    ImageBuffer::from_raw(width, height, out)
        .ok_or_else(|| anyhow!("failed to rebuild grained JPEG image buffer"))
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
/// Film grain tends to be more apparent in darker tones, so each possible luma
/// value maps to a multiplier that is strongest near black and tapers with a
/// soft power curve. Precomputing this 65,536-entry table keeps the inner
/// per-pixel loop to an array lookup.
fn shadow_bias_lut_u16() -> Vec<f32> {
    (0..=65535)
        .map(|luma| {
            let luma = luma as f32 / 65535.0;
            0.45 + (1.0 - luma).powf(0.7) * 0.75
        })
        .collect()
}

/// Build the 8-bit luma-to-grain-strength lookup table.
///
/// The shape matches the 16-bit LUT but only has 256 entries, which is enough
/// after JPEG-path quantization. This preserves the same shadow-heavy grain
/// character without paying for a larger table in the RGB8 renderer.
fn shadow_bias_lut_u8() -> Vec<f32> {
    (0..=255)
        .map(|luma| {
            let luma = luma as f32 / 255.0;
            0.45 + (1.0 - luma).powf(0.7) * 0.75
        })
        .collect()
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
}

impl GrainModel {
    fn from_settings(grain: GrainSettings) -> Self {
        let amount = grain.amount as f32 / 100.0;
        let size = (grain.size.max(1) as f64 / 50.0).clamp(0.18, 3.2);
        let frequency = (grain.frequency.max(1) as f32 / 50.0).clamp(0.25, 2.4);
        let cell_size = (4.0 + size * 7.5) / frequency as f64;
        let secondary_cell_size = cell_size * 2.4;
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
        }
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
    );
    let secondary = cellular_clumps(
        x,
        y,
        seed ^ 0xD1B5_4A32_D192_ED03,
        model.secondary_cell_size,
        model.secondary_radius,
        model.clump_density * 0.72,
    );
    let cloud = perlin.get([x / model.perlin_scale, y / model.perlin_scale]) as f32;
    let cloud = 0.88 + (cloud + 1.0) * 0.12;
    let clustered = model.fine_mix + primary * 0.52 + secondary * 0.23;
    (clustered * cloud).clamp(0.35, 2.15)
}

fn cellular_clumps(x: f64, y: f64, seed: u64, cell_size: f64, radius: f64, density: f32) -> f32 {
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
            let kernel = (-dist2 / (2.0 * radius * radius)).exp() as f32;
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

fn density_mask_u16(luma: usize) -> f32 {
    let luma = luma as f32 / 65535.0;
    (1.08 - (luma - 0.38).max(0.0).powf(1.7) * 0.9).clamp(0.32, 1.12)
}

fn density_mask_u8(luma: usize) -> f32 {
    let luma = luma as f32 / 255.0;
    (1.08 - (luma - 0.38).max(0.0).powf(1.7) * 0.9).clamp(0.32, 1.12)
}

fn density_skew(delta: f32) -> f32 {
    if delta < 0.0 { 1.16 } else { 0.88 }
}

fn channel_dye_jitter(rng: &mut ChaCha8Rng, normal: &Normal<f64>, color_jitter: f32) -> f32 {
    (1.0 + normal.sample(rng) as f32 * color_jitter).clamp(0.62, 1.38)
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

fn unit_from_hash(hash: u64) -> f32 {
    ((hash >> 40) as u32 as f32) / ((1u32 << 24) as f32)
}

fn row_seed(seed: u64, row: u64) -> u64 {
    seed ^ row.wrapping_mul(0x9E37_79B9_7F4A_7C15)
}
