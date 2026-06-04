use std::{fs, path::Path};

use anyhow::{Context, Result, anyhow};
use image::{DynamicImage, GenericImageView, ImageBuffer, ImageReader, Rgba};
use noise::{NoiseFn, Perlin};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, Normal};
use rayon::prelude::*;

use crate::model::GrainSettings;

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

fn render_grain(image: DynamicImage, grain: GrainSettings, seed: u64) -> Result<DynamicImage> {
    let (width, height) = image.dimensions();
    let mut out = image.to_rgba16().into_raw();
    let normal = Normal::new(0.0, 1.0)?;
    let perlin = Perlin::new((seed & 0xffff_ffff) as u32);
    let shadow_bias = shadow_bias_lut_u16();

    let amount = grain.amount as f32 / 100.0;
    let size = (grain.size.max(1) as f64 / 50.0).clamp(0.2, 3.0);
    let frequency = (grain.frequency.max(1) as f32 / 50.0).clamp(0.2, 2.0);
    let sigma = amount * 34.0;
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
                let clump = perlin.get([x as f64 / (42.0 * size), y as f64 / (42.0 * size)]);
                let clump = 0.75 + ((clump as f32 + 1.0) * 0.5) * 0.5;
                let grain_value =
                    normal.sample(&mut rng) as f32 * sigma * 257.0 * shadow_bias[luma] * clump;
                let color_jitter = 0.18 / frequency;

                pixel[0] = add_grain(
                    r,
                    grain_value * (1.0 + normal.sample(&mut rng) as f32 * color_jitter),
                );
                pixel[1] = add_grain(
                    g,
                    grain_value * (1.0 + normal.sample(&mut rng) as f32 * color_jitter),
                );
                pixel[2] = add_grain(
                    b,
                    grain_value * (1.0 + normal.sample(&mut rng) as f32 * color_jitter),
                );
            }
        });

    let image = ImageBuffer::<Rgba<u16>, Vec<u16>>::from_raw(width, height, out)
        .ok_or_else(|| anyhow!("failed to rebuild grained image buffer"))?;
    Ok(DynamicImage::ImageRgba16(image))
}

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

    let amount = grain.amount as f32 / 100.0;
    let size = (grain.size.max(1) as f64 / 50.0).clamp(0.2, 3.0);
    let frequency = (grain.frequency.max(1) as f32 / 50.0).clamp(0.2, 2.0);
    let sigma = amount * 34.0;
    let row_stride = width as usize * 3;

    out.par_chunks_mut(row_stride)
        .enumerate()
        .for_each(|(y, row)| {
            let mut rng = ChaCha8Rng::seed_from_u64(row_seed(seed, y as u64));
            let color_jitter = 0.18 / frequency;
            for (x, pixel) in row.chunks_exact_mut(3).enumerate() {
                let r = pixel[0];
                let g = pixel[1];
                let b = pixel[2];
                let luma =
                    ((54u16 * r as u16 + 183u16 * g as u16 + 19u16 * b as u16) >> 8) as usize;
                let clump = perlin.get([x as f64 / (42.0 * size), y as f64 / (42.0 * size)]);
                let clump = 0.75 + ((clump as f32 + 1.0) * 0.5) * 0.5;
                let grain_value =
                    normal.sample(&mut rng) as f32 * sigma * shadow_bias[luma] * clump;

                pixel[0] = add_grain_u8(
                    r,
                    grain_value * (1.0 + normal.sample(&mut rng) as f32 * color_jitter),
                );
                pixel[1] = add_grain_u8(
                    g,
                    grain_value * (1.0 + normal.sample(&mut rng) as f32 * color_jitter),
                );
                pixel[2] = add_grain_u8(
                    b,
                    grain_value * (1.0 + normal.sample(&mut rng) as f32 * color_jitter),
                );
            }
        });

    ImageBuffer::from_raw(width, height, out)
        .ok_or_else(|| anyhow!("failed to rebuild grained JPEG image buffer"))
}

fn add_grain(channel: u16, delta: f32) -> u16 {
    (channel as f32 + delta).round().clamp(0.0, 65535.0) as u16
}

fn add_grain_u8(channel: u8, delta: f32) -> u8 {
    (channel as f32 + delta).round().clamp(0.0, 255.0) as u8
}

fn shadow_bias_lut_u16() -> Vec<f32> {
    (0..=65535)
        .map(|luma| {
            let luma = luma as f32 / 65535.0;
            0.45 + (1.0 - luma).powf(0.7) * 0.75
        })
        .collect()
}

fn shadow_bias_lut_u8() -> Vec<f32> {
    (0..=255)
        .map(|luma| {
            let luma = luma as f32 / 255.0;
            0.45 + (1.0 - luma).powf(0.7) * 0.75
        })
        .collect()
}

fn row_seed(seed: u64, row: u64) -> u64 {
    seed ^ row.wrapping_mul(0x9E37_79B9_7F4A_7C15)
}
