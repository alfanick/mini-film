#[cfg(target_arch = "x86_64")]
use std::simd::prelude::*;
use std::sync::OnceLock;

use anyhow::{Result, anyhow};
use image::{DynamicImage, GenericImageView, ImageBuffer, Rgb, Rgba};
use rayon::prelude::*;

use crate::model::GrainSettings;

const RFGR_GAUSSIAN_SIGMA: f32 = 0.8;
const RFGR_MONTE_CARLO_SAMPLES: usize = 32;
const RFGR_RADIUS_Q999_NORMAL: f32 = 3.090_232_4;
const RFGR_MIN_RADIUS: f32 = 0.001;
const RFGR_MAX_POISSON_COUNT: u32 = 128;
const TWO_PI: f32 = std::f32::consts::TAU;
const REC_709: [f32; 3] = [0.2126, 0.7152, 0.0722];
const MONOCHROME_CHANNEL: usize = 0;
const MONOCHROME_MEAN_SPREAD_UNIT: f64 = 2.5 / 255.0;
const MONOCHROME_OUTLIER_SPREAD_UNIT: f64 = 10.0 / 255.0;
const MONOCHROME_MAX_OUTLIER_RATIO: f64 = 0.01;
const CHANNEL_STREAMS: [u64; 3] = [
    0xA1B2_C3D4_E5F6_1023,
    0x6C8E_9CF5_37A1_D42B,
    0xB529_7A4D_0F13_EE91,
];
const FAST_STREAM: u64 = 0x517C_C1B7_2722_0A95;

/// Render RFGR film grain into a 16-bit RGBA image buffer.
///
/// This is an original implementation of the pixel-wise algorithm described in
/// Newson, Faraj, Galerne, and Delon's "Realistic Film Grain Rendering": each
/// color channel is treated as an inhomogeneous Boolean model of random disks,
/// filtered with Gaussian Monte Carlo samples, then blended back toward the
/// input by Lightroom's public grain amount control.
pub(crate) fn render_grain(
    image: DynamicImage,
    grain: GrainSettings,
    seed: u64,
) -> Result<DynamicImage> {
    let (width, height) = image.dimensions();
    let source = image.to_rgba16().into_raw();
    let mut out = source.clone();
    let model = RfgrModel::from_settings(grain);

    render_rgba16_rows(&source, &mut out, width, height, seed, &model);

    let image = ImageBuffer::<Rgba<u16>, Vec<u16>>::from_raw(width, height, out)
        .ok_or_else(|| anyhow!("failed to rebuild RFGR grained image buffer"))?;
    Ok(DynamicImage::ImageRgba16(image))
}

/// Render RFGR film grain into an 8-bit RGB image buffer for JPEG-bound output.
pub(crate) fn render_grain_8(
    image: DynamicImage,
    grain: GrainSettings,
    seed: u64,
) -> Result<ImageBuffer<Rgb<u8>, Vec<u8>>> {
    let (width, height) = image.dimensions();
    let source = image.to_rgb8().into_raw();
    let mut out = source.clone();
    let model = RfgrModel::from_settings(grain);

    render_rgb8_rows(&source, &mut out, width, height, seed, &model);

    ImageBuffer::from_raw(width, height, out)
        .ok_or_else(|| anyhow!("failed to rebuild RFGR grained JPEG image buffer"))
}

/// Render a cached RFGR approximation into a 16-bit RGBA image buffer.
///
/// The exact RFGR path evaluates the random disk model at each Monte Carlo
/// sample. This faster mode caches one seeded stochastic coverage plane per
/// channel, applies the RFGR Gaussian/disk-scale blur once, then blends that
/// cached field into the image. It keeps the same public controls and seed
/// behavior while avoiding the per-pixel Monte Carlo disk walk.
pub(crate) fn render_grain_fast(
    image: DynamicImage,
    grain: GrainSettings,
    seed: u64,
) -> Result<DynamicImage> {
    let (width, height) = image.dimensions();
    let source = image.to_rgba16().into_raw();
    let mut out = source.clone();
    let model = RfgrModel::from_settings(grain);

    render_rgba16_fast_rows(&source, &mut out, width, height, seed, &model);

    let image = ImageBuffer::<Rgba<u16>, Vec<u16>>::from_raw(width, height, out)
        .ok_or_else(|| anyhow!("failed to rebuild fast RFGR grained image buffer"))?;
    Ok(DynamicImage::ImageRgba16(image))
}

/// Render a cached RFGR approximation into an 8-bit RGB image buffer.
pub(crate) fn render_grain_8_fast(
    image: DynamicImage,
    grain: GrainSettings,
    seed: u64,
) -> Result<ImageBuffer<Rgb<u8>, Vec<u8>>> {
    let (width, height) = image.dimensions();
    let source = image.to_rgb8().into_raw();
    let mut out = source.clone();
    let model = RfgrModel::from_settings(grain);

    render_rgb8_fast_rows(&source, &mut out, width, height, seed, &model);

    ImageBuffer::from_raw(width, height, out)
        .ok_or_else(|| anyhow!("failed to rebuild fast RFGR grained JPEG image buffer"))
}

fn render_rgb8_rows(
    source: &[u8],
    out: &mut [u8],
    width: u32,
    height: u32,
    seed: u64,
    model: &RfgrModel,
) {
    if is_monochrome_rgb8(source) {
        render_rgb8_monochrome_rows(source, out, width, height, seed, model);
        return;
    }

    let row_stride = width as usize * 3;
    let path = RfgrSimdPath::detect();
    let context = Rgb8Context {
        source,
        width,
        height,
        seed,
        model,
    };
    out.par_chunks_mut(row_stride)
        .enumerate()
        .for_each(|(y, row)| match path {
            #[cfg(target_arch = "x86_64")]
            RfgrSimdPath::Avx512 => unsafe { render_rgb8_row_avx512(&context, row, y) },
            #[cfg(target_arch = "x86_64")]
            RfgrSimdPath::Avx2 => unsafe { render_rgb8_row_avx2(&context, row, y) },
            RfgrSimdPath::Scalar => render_rgb8_row_scalar(&context, row, y, 0),
        });
}

fn render_rgba16_rows(
    source: &[u16],
    out: &mut [u16],
    width: u32,
    height: u32,
    seed: u64,
    model: &RfgrModel,
) {
    if is_monochrome_rgba16(source) {
        render_rgba16_monochrome_rows(source, out, width, height, seed, model);
        return;
    }

    let row_stride = width as usize * 4;
    let path = RfgrSimdPath::detect();
    let context = Rgba16Context {
        source,
        width,
        height,
        seed,
        model,
    };
    out.par_chunks_mut(row_stride)
        .enumerate()
        .for_each(|(y, row)| match path {
            #[cfg(target_arch = "x86_64")]
            RfgrSimdPath::Avx512 => unsafe { render_rgba16_row_avx512(&context, row, y) },
            #[cfg(target_arch = "x86_64")]
            RfgrSimdPath::Avx2 => unsafe { render_rgba16_row_avx2(&context, row, y) },
            RfgrSimdPath::Scalar => render_rgba16_row_scalar(&context, row, y, 0),
        });
}

fn render_rgb8_fast_rows(
    source: &[u8],
    out: &mut [u8],
    width: u32,
    height: u32,
    seed: u64,
    model: &RfgrModel,
) {
    if is_monochrome_rgb8(source) {
        render_rgb8_fast_monochrome_rows(source, out, width, height, seed, model);
        return;
    }

    let blur_sigma = model.fast_filter_sigma();
    let path = RfgrSimdPath::detect();
    for channel in 0..3 {
        let mut plane = fast_coverage_plane_rgb8(source, width, height, seed, channel);
        gaussian_blur_in_place(&mut plane, width, height, blur_sigma);
        apply_cached_rgb8_channel(out, &plane, width, channel, model.alpha, path);
    }
}

fn render_rgba16_fast_rows(
    source: &[u16],
    out: &mut [u16],
    width: u32,
    height: u32,
    seed: u64,
    model: &RfgrModel,
) {
    if is_monochrome_rgba16(source) {
        render_rgba16_fast_monochrome_rows(source, out, width, height, seed, model);
        return;
    }

    let blur_sigma = model.fast_filter_sigma();
    let path = RfgrSimdPath::detect();
    for channel in 0..3 {
        let mut plane = fast_coverage_plane_rgba16(source, width, height, seed, channel);
        gaussian_blur_in_place(&mut plane, width, height, blur_sigma);
        apply_cached_rgba16_channel(out, &plane, width, channel, model.alpha, path);
    }
}

fn render_rgb8_monochrome_rows(
    source: &[u8],
    out: &mut [u8],
    width: u32,
    height: u32,
    seed: u64,
    model: &RfgrModel,
) {
    let row_stride = width as usize * 3;
    let path = RfgrSimdPath::detect();
    let context = Rgb8Context {
        source,
        width,
        height,
        seed,
        model,
    };
    out.par_chunks_mut(row_stride)
        .enumerate()
        .for_each(|(y, row)| match path {
            #[cfg(target_arch = "x86_64")]
            RfgrSimdPath::Avx512 => unsafe { render_rgb8_monochrome_row_avx512(&context, row, y) },
            #[cfg(target_arch = "x86_64")]
            RfgrSimdPath::Avx2 => unsafe { render_rgb8_monochrome_row_avx2(&context, row, y) },
            RfgrSimdPath::Scalar => render_rgb8_monochrome_row_scalar(&context, row, y, 0),
        });
}

fn render_rgba16_monochrome_rows(
    source: &[u16],
    out: &mut [u16],
    width: u32,
    height: u32,
    seed: u64,
    model: &RfgrModel,
) {
    let row_stride = width as usize * 4;
    let path = RfgrSimdPath::detect();
    let context = Rgba16Context {
        source,
        width,
        height,
        seed,
        model,
    };
    out.par_chunks_mut(row_stride)
        .enumerate()
        .for_each(|(y, row)| match path {
            #[cfg(target_arch = "x86_64")]
            RfgrSimdPath::Avx512 => unsafe {
                render_rgba16_monochrome_row_avx512(&context, row, y)
            },
            #[cfg(target_arch = "x86_64")]
            RfgrSimdPath::Avx2 => unsafe { render_rgba16_monochrome_row_avx2(&context, row, y) },
            RfgrSimdPath::Scalar => render_rgba16_monochrome_row_scalar(&context, row, y, 0),
        });
}

fn render_rgb8_fast_monochrome_rows(
    source: &[u8],
    out: &mut [u8],
    width: u32,
    height: u32,
    seed: u64,
    model: &RfgrModel,
) {
    let blur_sigma = model.fast_filter_sigma();
    let path = RfgrSimdPath::detect();
    let mut plane = fast_monochrome_coverage_plane_rgb8(source, width, height, seed);
    gaussian_blur_in_place(&mut plane, width, height, blur_sigma);
    for channel in 0..3 {
        apply_cached_rgb8_channel(out, &plane, width, channel, model.alpha, path);
    }
}

fn render_rgba16_fast_monochrome_rows(
    source: &[u16],
    out: &mut [u16],
    width: u32,
    height: u32,
    seed: u64,
    model: &RfgrModel,
) {
    let blur_sigma = model.fast_filter_sigma();
    let path = RfgrSimdPath::detect();
    let mut plane = fast_monochrome_coverage_plane_rgba16(source, width, height, seed);
    gaussian_blur_in_place(&mut plane, width, height, blur_sigma);
    for channel in 0..3 {
        apply_cached_rgba16_channel(out, &plane, width, channel, model.alpha, path);
    }
}

fn fast_coverage_plane_rgb8(
    source: &[u8],
    width: u32,
    height: u32,
    seed: u64,
    channel: usize,
) -> Vec<f32> {
    let width = width as usize;
    let height = height as usize;
    let row_stride = width * 3;
    let channel_seed = fast_channel_seed(seed, channel);
    let mut plane = vec![0.0f32; width * height];

    plane
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row)| {
            let source_row = &source[y * row_stride..(y + 1) * row_stride];
            for (x, value) in row.iter_mut().enumerate() {
                let input = srgb8_to_linear(source_row[x * 3 + channel]);
                let sample = unit_from_hash(hash_sample(channel_seed, x, y, 0));
                *value = if sample < input { 1.0 } else { 0.0 };
            }
        });

    plane
}

fn fast_monochrome_coverage_plane_rgb8(
    source: &[u8],
    width: u32,
    height: u32,
    seed: u64,
) -> Vec<f32> {
    let width = width as usize;
    let height = height as usize;
    let row_stride = width * 3;
    let channel_seed = fast_channel_seed(seed, MONOCHROME_CHANNEL);
    let mut plane = vec![0.0f32; width * height];

    plane
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row)| {
            let source_row = &source[y * row_stride..(y + 1) * row_stride];
            for (x, value) in row.iter_mut().enumerate() {
                let offset = x * 3;
                let input = linear_luma_rgb8(
                    source_row[offset],
                    source_row[offset + 1],
                    source_row[offset + 2],
                );
                let sample = unit_from_hash(hash_sample(channel_seed, x, y, 0));
                *value = if sample < input { 1.0 } else { 0.0 };
            }
        });

    plane
}

fn fast_coverage_plane_rgba16(
    source: &[u16],
    width: u32,
    height: u32,
    seed: u64,
    channel: usize,
) -> Vec<f32> {
    let width = width as usize;
    let height = height as usize;
    let row_stride = width * 4;
    let channel_seed = fast_channel_seed(seed, channel);
    let mut plane = vec![0.0f32; width * height];

    plane
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row)| {
            let source_row = &source[y * row_stride..(y + 1) * row_stride];
            for (x, value) in row.iter_mut().enumerate() {
                let input = srgb16_to_linear(source_row[x * 4 + channel]);
                let sample = unit_from_hash(hash_sample(channel_seed, x, y, 0));
                *value = if sample < input { 1.0 } else { 0.0 };
            }
        });

    plane
}

fn fast_monochrome_coverage_plane_rgba16(
    source: &[u16],
    width: u32,
    height: u32,
    seed: u64,
) -> Vec<f32> {
    let width = width as usize;
    let height = height as usize;
    let row_stride = width * 4;
    let channel_seed = fast_channel_seed(seed, MONOCHROME_CHANNEL);
    let mut plane = vec![0.0f32; width * height];

    plane
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row)| {
            let source_row = &source[y * row_stride..(y + 1) * row_stride];
            for (x, value) in row.iter_mut().enumerate() {
                let offset = x * 4;
                let input = linear_luma_rgba16(
                    source_row[offset],
                    source_row[offset + 1],
                    source_row[offset + 2],
                );
                let sample = unit_from_hash(hash_sample(channel_seed, x, y, 0));
                *value = if sample < input { 1.0 } else { 0.0 };
            }
        });

    plane
}

fn fast_channel_seed(seed: u64, channel: usize) -> u64 {
    seed ^ CHANNEL_STREAMS[channel] ^ FAST_STREAM
}

fn is_monochrome_rgb8(source: &[u8]) -> bool {
    let pixels = source.len() / 3;
    if pixels == 0 {
        return false;
    }

    let mut spread_total = 0.0f64;
    let mut outliers = 0usize;
    for pixel in source.as_chunks::<3>().0 {
        let max = pixel[0].max(pixel[1]).max(pixel[2]);
        let min = pixel[0].min(pixel[1]).min(pixel[2]);
        let spread = f64::from(max - min) / 255.0;
        spread_total += spread;
        if spread > MONOCHROME_OUTLIER_SPREAD_UNIT {
            outliers += 1;
        }
    }

    monochrome_spread_is_within_limits(spread_total, outliers, pixels)
}

fn is_monochrome_rgba16(source: &[u16]) -> bool {
    let pixels = source.len() / 4;
    if pixels == 0 {
        return false;
    }

    let mut spread_total = 0.0f64;
    let mut outliers = 0usize;
    for pixel in source.as_chunks::<4>().0 {
        let max = pixel[0].max(pixel[1]).max(pixel[2]);
        let min = pixel[0].min(pixel[1]).min(pixel[2]);
        let spread = f64::from(max - min) / 65535.0;
        spread_total += spread;
        if spread > MONOCHROME_OUTLIER_SPREAD_UNIT {
            outliers += 1;
        }
    }

    monochrome_spread_is_within_limits(spread_total, outliers, pixels)
}

fn monochrome_spread_is_within_limits(spread_total: f64, outliers: usize, pixels: usize) -> bool {
    spread_total / pixels as f64 <= MONOCHROME_MEAN_SPREAD_UNIT
        && outliers as f64 / pixels as f64 <= MONOCHROME_MAX_OUTLIER_RATIO
}

fn gaussian_blur_in_place(field: &mut [f32], width: u32, height: u32, sigma: f32) {
    if field.is_empty() {
        return;
    }

    let width = width as usize;
    let height = height as usize;
    let kernel = gaussian_kernel(sigma);
    let radius = kernel.len() / 2;
    let mut scratch = vec![0.0f32; field.len()];

    scratch
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row)| {
            let source_row = &field[y * width..(y + 1) * width];
            for (x, value) in row.iter_mut().enumerate() {
                let mut sum = 0.0f32;
                for (k, weight) in kernel.iter().enumerate() {
                    let sx = clamped_kernel_index(x, k, radius, width);
                    sum += source_row[sx] * weight;
                }
                *value = sum;
            }
        });

    field
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, value) in row.iter_mut().enumerate() {
                let mut sum = 0.0f32;
                for (k, weight) in kernel.iter().enumerate() {
                    let sy = clamped_kernel_index(y, k, radius, height);
                    sum += scratch[sy * width + x] * weight;
                }
                *value = sum;
            }
        });
}

fn gaussian_kernel(sigma: f32) -> Vec<f32> {
    let sigma = sigma.max(0.01);
    let radius = (sigma * 3.0).ceil().max(1.0) as usize;
    let mut kernel = Vec::with_capacity(radius * 2 + 1);
    let mut sum = 0.0f32;
    for index in 0..=(radius * 2) {
        let x = index as f32 - radius as f32;
        let value = (-0.5 * (x / sigma).powi(2)).exp();
        kernel.push(value);
        sum += value;
    }
    for value in &mut kernel {
        *value /= sum;
    }
    kernel
}

fn clamped_kernel_index(
    position: usize,
    kernel_index: usize,
    radius: usize,
    limit: usize,
) -> usize {
    if kernel_index < radius {
        position.saturating_sub(radius - kernel_index)
    } else {
        position
            .saturating_add(kernel_index - radius)
            .min(limit - 1)
    }
}

fn apply_cached_rgb8_channel(
    out: &mut [u8],
    plane: &[f32],
    width: u32,
    channel: usize,
    alpha: f32,
    path: RfgrSimdPath,
) {
    let width = width as usize;
    let row_stride = width * 3;
    out.par_chunks_mut(row_stride)
        .zip(plane.par_chunks(width))
        .for_each(|(row, grain_row)| match path {
            #[cfg(target_arch = "x86_64")]
            RfgrSimdPath::Avx512 => unsafe {
                apply_cached_rgb8_row_avx512(row, grain_row, channel, alpha)
            },
            #[cfg(target_arch = "x86_64")]
            RfgrSimdPath::Avx2 => unsafe {
                apply_cached_rgb8_row_avx2(row, grain_row, channel, alpha)
            },
            RfgrSimdPath::Scalar => apply_cached_rgb8_row_scalar(row, grain_row, channel, alpha),
        });
}

fn apply_cached_rgba16_channel(
    out: &mut [u16],
    plane: &[f32],
    width: u32,
    channel: usize,
    alpha: f32,
    path: RfgrSimdPath,
) {
    let width = width as usize;
    let row_stride = width * 4;
    out.par_chunks_mut(row_stride)
        .zip(plane.par_chunks(width))
        .for_each(|(row, grain_row)| match path {
            #[cfg(target_arch = "x86_64")]
            RfgrSimdPath::Avx512 => unsafe {
                apply_cached_rgba16_row_avx512(row, grain_row, channel, alpha)
            },
            #[cfg(target_arch = "x86_64")]
            RfgrSimdPath::Avx2 => unsafe {
                apply_cached_rgba16_row_avx2(row, grain_row, channel, alpha)
            },
            RfgrSimdPath::Scalar => apply_cached_rgba16_row_scalar(row, grain_row, channel, alpha),
        });
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw,avx512vl")]
unsafe fn apply_cached_rgb8_row_avx512(
    row: &mut [u8],
    grain_row: &[f32],
    channel: usize,
    alpha: f32,
) {
    apply_cached_rgb8_row_simd::<16>(row, grain_row, channel, alpha);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn apply_cached_rgb8_row_avx2(
    row: &mut [u8],
    grain_row: &[f32],
    channel: usize,
    alpha: f32,
) {
    apply_cached_rgb8_row_simd::<8>(row, grain_row, channel, alpha);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw,avx512vl")]
unsafe fn apply_cached_rgba16_row_avx512(
    row: &mut [u16],
    grain_row: &[f32],
    channel: usize,
    alpha: f32,
) {
    apply_cached_rgba16_row_simd::<16>(row, grain_row, channel, alpha);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn apply_cached_rgba16_row_avx2(
    row: &mut [u16],
    grain_row: &[f32],
    channel: usize,
    alpha: f32,
) {
    apply_cached_rgba16_row_simd::<8>(row, grain_row, channel, alpha);
}

fn apply_cached_rgb8_row_scalar(row: &mut [u8], grain_row: &[f32], channel: usize, alpha: f32) {
    for (pixel, grain_value) in row
        .as_chunks_mut::<3>()
        .0
        .iter_mut()
        .zip(grain_row.iter().copied())
    {
        let input = srgb8_to_linear(pixel[channel]);
        pixel[channel] = linear_to_srgb8(blend_grain(input, grain_value, alpha));
    }
}

fn apply_cached_rgba16_row_scalar(row: &mut [u16], grain_row: &[f32], channel: usize, alpha: f32) {
    for (pixel, grain_value) in row
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(grain_row.iter().copied())
    {
        let input = srgb16_to_linear(pixel[channel]);
        pixel[channel] = linear_to_srgb16(blend_grain(input, grain_value, alpha));
    }
}

#[cfg(target_arch = "x86_64")]
fn apply_cached_rgb8_row_simd<const LANES: usize>(
    row: &mut [u8],
    grain_row: &[f32],
    channel: usize,
    alpha: f32,
) {
    let pixels = grain_row.len();
    let mut x = 0usize;
    while x + LANES <= pixels {
        let mut input = [0.0f32; LANES];
        let mut grain = [0.0f32; LANES];
        for lane in 0..LANES {
            input[lane] = srgb8_to_linear(row[(x + lane) * 3 + channel]);
            grain[lane] = grain_row[x + lane];
        }
        let blended = blend_grain_simd(input, grain, alpha);
        for lane in 0..LANES {
            row[(x + lane) * 3 + channel] = linear_to_srgb8(blended[lane]);
        }
        x += LANES;
    }
    if x < pixels {
        apply_cached_rgb8_row_scalar(&mut row[(x * 3)..], &grain_row[x..], channel, alpha);
    }
}

#[cfg(target_arch = "x86_64")]
fn apply_cached_rgba16_row_simd<const LANES: usize>(
    row: &mut [u16],
    grain_row: &[f32],
    channel: usize,
    alpha: f32,
) {
    let pixels = grain_row.len();
    let mut x = 0usize;
    while x + LANES <= pixels {
        let mut input = [0.0f32; LANES];
        let mut grain = [0.0f32; LANES];
        for lane in 0..LANES {
            input[lane] = srgb16_to_linear(row[(x + lane) * 4 + channel]);
            grain[lane] = grain_row[x + lane];
        }
        let blended = blend_grain_simd(input, grain, alpha);
        for lane in 0..LANES {
            row[(x + lane) * 4 + channel] = linear_to_srgb16(blended[lane]);
        }
        x += LANES;
    }
    if x < pixels {
        apply_cached_rgba16_row_scalar(&mut row[(x * 4)..], &grain_row[x..], channel, alpha);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw,avx512vl")]
unsafe fn render_rgb8_monochrome_row_avx512(context: &Rgb8Context<'_>, row: &mut [u8], y: usize) {
    render_rgb8_monochrome_row_simd::<16>(context, row, y);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn render_rgb8_monochrome_row_avx2(context: &Rgb8Context<'_>, row: &mut [u8], y: usize) {
    render_rgb8_monochrome_row_simd::<8>(context, row, y);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw,avx512vl")]
unsafe fn render_rgba16_monochrome_row_avx512(
    context: &Rgba16Context<'_>,
    row: &mut [u16],
    y: usize,
) {
    render_rgba16_monochrome_row_simd::<16>(context, row, y);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn render_rgba16_monochrome_row_avx2(
    context: &Rgba16Context<'_>,
    row: &mut [u16],
    y: usize,
) {
    render_rgba16_monochrome_row_simd::<8>(context, row, y);
}

fn render_rgb8_monochrome_row_scalar(
    context: &Rgb8Context<'_>,
    row: &mut [u8],
    y: usize,
    x_offset: usize,
) {
    for (x, pixel) in row.as_chunks_mut::<3>().0.iter_mut().enumerate() {
        let x = x + x_offset;
        let grain_value = context.pixel_value_monochrome(x, y);
        for channel_value in pixel.iter_mut().take(3) {
            let input = srgb8_to_linear(*channel_value);
            *channel_value = linear_to_srgb8(blend_grain(input, grain_value, context.model.alpha));
        }
    }
}

fn render_rgba16_monochrome_row_scalar(
    context: &Rgba16Context<'_>,
    row: &mut [u16],
    y: usize,
    x_offset: usize,
) {
    for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let x = x + x_offset;
        let grain_value = context.pixel_value_monochrome(x, y);
        for channel_value in pixel.iter_mut().take(3) {
            let input = srgb16_to_linear(*channel_value);
            *channel_value = linear_to_srgb16(blend_grain(input, grain_value, context.model.alpha));
        }
    }
}

#[cfg(target_arch = "x86_64")]
fn render_rgb8_monochrome_row_simd<const LANES: usize>(
    context: &Rgb8Context<'_>,
    row: &mut [u8],
    y: usize,
) {
    let pixels = row.len() / 3;
    let mut x = 0usize;
    while x + LANES <= pixels {
        let mut input = [[0.0f32; LANES]; 3];
        let mut grain = [0.0f32; LANES];
        for lane in 0..LANES {
            let px = x + lane;
            let offset = px * 3;
            for channel in 0..3 {
                input[channel][lane] = srgb8_to_linear(row[offset + channel]);
            }
            grain[lane] = context.pixel_value_monochrome(px, y);
        }
        for channel in 0..3 {
            let blended = blend_grain_simd(input[channel], grain, context.model.alpha);
            for lane in 0..LANES {
                row[(x + lane) * 3 + channel] = linear_to_srgb8(blended[lane]);
            }
        }
        x += LANES;
    }
    if x < pixels {
        render_rgb8_monochrome_row_scalar(context, &mut row[(x * 3)..], y, x);
    }
}

#[cfg(target_arch = "x86_64")]
fn render_rgba16_monochrome_row_simd<const LANES: usize>(
    context: &Rgba16Context<'_>,
    row: &mut [u16],
    y: usize,
) {
    let pixels = row.len() / 4;
    let mut x = 0usize;
    while x + LANES <= pixels {
        let mut input = [[0.0f32; LANES]; 3];
        let mut grain = [0.0f32; LANES];
        for lane in 0..LANES {
            let px = x + lane;
            let offset = px * 4;
            for channel in 0..3 {
                input[channel][lane] = srgb16_to_linear(row[offset + channel]);
            }
            grain[lane] = context.pixel_value_monochrome(px, y);
        }
        for channel in 0..3 {
            let blended = blend_grain_simd(input[channel], grain, context.model.alpha);
            for lane in 0..LANES {
                row[(x + lane) * 4 + channel] = linear_to_srgb16(blended[lane]);
            }
        }
        x += LANES;
    }
    if x < pixels {
        render_rgba16_monochrome_row_scalar(context, &mut row[(x * 4)..], y, x);
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw,avx512vl")]
unsafe fn render_rgb8_row_avx512(context: &Rgb8Context<'_>, row: &mut [u8], y: usize) {
    render_rgb8_row_simd::<16>(context, row, y);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn render_rgb8_row_avx2(context: &Rgb8Context<'_>, row: &mut [u8], y: usize) {
    render_rgb8_row_simd::<8>(context, row, y);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw,avx512vl")]
unsafe fn render_rgba16_row_avx512(context: &Rgba16Context<'_>, row: &mut [u16], y: usize) {
    render_rgba16_row_simd::<16>(context, row, y);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn render_rgba16_row_avx2(context: &Rgba16Context<'_>, row: &mut [u16], y: usize) {
    render_rgba16_row_simd::<8>(context, row, y);
}

fn render_rgb8_row_scalar(context: &Rgb8Context<'_>, row: &mut [u8], y: usize, x_offset: usize) {
    for (x, pixel) in row.as_chunks_mut::<3>().0.iter_mut().enumerate() {
        let x = x + x_offset;
        for (channel, channel_value) in pixel.iter_mut().enumerate().take(3) {
            let input = srgb8_to_linear(*channel_value);
            let grain_value = context.pixel_value(x, y, channel);
            *channel_value = linear_to_srgb8(blend_grain(input, grain_value, context.model.alpha));
        }
    }
}

fn render_rgba16_row_scalar(
    context: &Rgba16Context<'_>,
    row: &mut [u16],
    y: usize,
    x_offset: usize,
) {
    for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let x = x + x_offset;
        for (channel, channel_value) in pixel.iter_mut().enumerate().take(3) {
            let input = srgb16_to_linear(*channel_value);
            let grain_value = context.pixel_value(x, y, channel);
            *channel_value = linear_to_srgb16(blend_grain(input, grain_value, context.model.alpha));
        }
    }
}

#[cfg(target_arch = "x86_64")]
fn render_rgb8_row_simd<const LANES: usize>(context: &Rgb8Context<'_>, row: &mut [u8], y: usize) {
    let pixels = row.len() / 3;
    let mut x = 0usize;
    while x + LANES <= pixels {
        let mut input = [[0.0f32; LANES]; 3];
        let mut grain = [[0.0f32; LANES]; 3];
        for lane in 0..LANES {
            let px = x + lane;
            let offset = px * 3;
            for channel in 0..3 {
                input[channel][lane] = srgb8_to_linear(row[offset + channel]);
                grain[channel][lane] = context.pixel_value(px, y, channel);
            }
        }
        for channel in 0..3 {
            let blended = blend_grain_simd(input[channel], grain[channel], context.model.alpha);
            for lane in 0..LANES {
                row[(x + lane) * 3 + channel] = linear_to_srgb8(blended[lane]);
            }
        }
        x += LANES;
    }
    if x < pixels {
        render_rgb8_row_scalar(context, &mut row[(x * 3)..], y, x);
    }
}

#[cfg(target_arch = "x86_64")]
fn render_rgba16_row_simd<const LANES: usize>(
    context: &Rgba16Context<'_>,
    row: &mut [u16],
    y: usize,
) {
    let pixels = row.len() / 4;
    let mut x = 0usize;
    while x + LANES <= pixels {
        let mut input = [[0.0f32; LANES]; 3];
        let mut grain = [[0.0f32; LANES]; 3];
        for lane in 0..LANES {
            let px = x + lane;
            let offset = px * 4;
            for channel in 0..3 {
                input[channel][lane] = srgb16_to_linear(row[offset + channel]);
                grain[channel][lane] = context.pixel_value(px, y, channel);
            }
        }
        for channel in 0..3 {
            let blended = blend_grain_simd(input[channel], grain[channel], context.model.alpha);
            for lane in 0..LANES {
                row[(x + lane) * 4 + channel] = linear_to_srgb16(blended[lane]);
            }
        }
        x += LANES;
    }
    if x < pixels {
        render_rgba16_row_scalar(context, &mut row[(x * 4)..], y, x);
    }
}

#[cfg(target_arch = "x86_64")]
fn blend_grain_simd<const LANES: usize>(
    input: [f32; LANES],
    grain: [f32; LANES],
    alpha: f32,
) -> [f32; LANES] {
    let input = Simd::from_array(input);
    let grain = Simd::from_array(grain);
    (input + Simd::splat(alpha) * (grain - input))
        .simd_clamp(Simd::splat(0.0), Simd::splat(1.0))
        .to_array()
}

#[derive(Clone, Copy)]
enum RfgrSimdPath {
    #[cfg(target_arch = "x86_64")]
    Avx512,
    #[cfg(target_arch = "x86_64")]
    Avx2,
    Scalar,
}

impl RfgrSimdPath {
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

#[derive(Debug)]
pub(crate) struct RfgrModel {
    alpha: f32,
    mu_r: f32,
    log_mu: f32,
    log_sigma: f32,
    radius_second_moment: f32,
    max_radius: f32,
    cell_size: f32,
    variable_radius: bool,
}

impl RfgrModel {
    pub(crate) fn from_settings(grain: GrainSettings) -> Self {
        let amount = (grain.amount as f32 / 100.0).clamp(0.0, 1.0);
        let alpha = amount.powf(0.85).clamp(0.0, 1.0);
        let size_n = (grain.size.clamp(1, 100) as f32 / 100.0).clamp(0.01, 1.0);
        let freq_n = (grain.frequency.clamp(1, 100) as f32 / 100.0).clamp(0.01, 1.0);
        let mu_r = (0.018 + 0.105 * size_n.powf(1.25) / (0.75 + 0.65 * freq_n)).clamp(0.015, 0.18);
        let sigma_ratio = (0.12 + 0.55 * (1.0 - freq_n).powf(0.8)).clamp(0.08, 0.75);
        let sigma_r = mu_r * sigma_ratio;
        let variable_radius = sigma_r > 1.0e-6;
        let (log_mu, log_sigma, max_radius, radius_second_moment) = if variable_radius {
            let variance_ratio = (sigma_r / mu_r).powi(2);
            let log_sigma2 = (1.0 + variance_ratio).ln();
            let log_sigma = log_sigma2.sqrt();
            let log_mu = mu_r.ln() - 0.5 * log_sigma2;
            let max_radius = (log_mu + log_sigma * RFGR_RADIUS_Q999_NORMAL)
                .exp()
                .clamp(RFGR_MIN_RADIUS, 0.75);
            let radius_second_moment = (2.0 * log_mu + 2.0 * log_sigma2).exp();
            (log_mu, log_sigma, max_radius, radius_second_moment)
        } else {
            (mu_r.ln(), 0.0, mu_r.max(RFGR_MIN_RADIUS), mu_r * mu_r)
        };
        let cell_size = max_radius.max(RFGR_MIN_RADIUS);

        Self {
            alpha,
            mu_r,
            log_mu,
            log_sigma,
            radius_second_moment,
            max_radius,
            cell_size,
            variable_radius,
        }
    }

    fn poisson_mean_for_intensity(&self, intensity: f32) -> f32 {
        let intensity = intensity.clamp(0.0, 0.999_999);
        if intensity <= f32::EPSILON {
            return 0.0;
        }
        let lambda = -(1.0 - intensity).ln()
            / (std::f32::consts::PI * self.radius_second_moment.max(1.0e-8));
        lambda * self.cell_size * self.cell_size
    }

    fn sample_radius(&self, hash: u64) -> f32 {
        if !self.variable_radius {
            return self.mu_r.max(RFGR_MIN_RADIUS);
        }
        let z = normal_from_hash(hash);
        (self.log_mu + self.log_sigma * z)
            .exp()
            .clamp(RFGR_MIN_RADIUS, self.max_radius)
    }

    fn fast_filter_sigma(&self) -> f32 {
        let disk_sigma = (self.max_radius * 16.0).clamp(0.25, 4.0);
        (RFGR_GAUSSIAN_SIGMA.powi(2) + disk_sigma.powi(2))
            .sqrt()
            .clamp(RFGR_GAUSSIAN_SIGMA, 4.25)
    }
}

struct Rgb8Context<'a> {
    source: &'a [u8],
    width: u32,
    height: u32,
    seed: u64,
    model: &'a RfgrModel,
}

struct Rgba16Context<'a> {
    source: &'a [u16],
    width: u32,
    height: u32,
    seed: u64,
    model: &'a RfgrModel,
}

impl Rgb8Context<'_> {
    fn pixel_value(&self, x: usize, y: usize, channel: usize) -> f32 {
        let mut hits = 0u32;
        for sample in 0..RFGR_MONTE_CARLO_SAMPLES {
            let (dx, dy) = gaussian_sample(self.seed, x, y, channel, sample);
            let sx = x as f32 + 0.5 + dx * RFGR_GAUSSIAN_SIGMA;
            let sy = y as f32 + 0.5 + dy * RFGR_GAUSSIAN_SIGMA;
            if boolean_hit(sx, sy, channel, self.seed, self.model, |cx, cy| {
                self.sample_linear(cx, cy, channel)
            }) {
                hits += 1;
            }
        }
        hits as f32 / RFGR_MONTE_CARLO_SAMPLES as f32
    }

    fn pixel_value_monochrome(&self, x: usize, y: usize) -> f32 {
        let mut hits = 0u32;
        for sample in 0..RFGR_MONTE_CARLO_SAMPLES {
            let (dx, dy) = gaussian_sample(self.seed, x, y, MONOCHROME_CHANNEL, sample);
            let sx = x as f32 + 0.5 + dx * RFGR_GAUSSIAN_SIGMA;
            let sy = y as f32 + 0.5 + dy * RFGR_GAUSSIAN_SIGMA;
            if boolean_hit(
                sx,
                sy,
                MONOCHROME_CHANNEL,
                self.seed,
                self.model,
                |cx, cy| self.sample_luminance_linear(cx, cy),
            ) {
                hits += 1;
            }
        }
        hits as f32 / RFGR_MONTE_CARLO_SAMPLES as f32
    }

    fn sample_linear(&self, x: f32, y: f32, channel: usize) -> f32 {
        let width = self.width as usize;
        let height = self.height as usize;
        let (x0, x1, tx) = sample_axis(x - 0.5, width);
        let (y0, y1, ty) = sample_axis(y - 0.5, height);
        let stride = width * 3;
        let c00 = srgb8_to_linear(self.source[y0 * stride + x0 * 3 + channel]);
        let c10 = srgb8_to_linear(self.source[y0 * stride + x1 * 3 + channel]);
        let c01 = srgb8_to_linear(self.source[y1 * stride + x0 * 3 + channel]);
        let c11 = srgb8_to_linear(self.source[y1 * stride + x1 * 3 + channel]);
        bilinear(c00, c10, c01, c11, tx, ty)
    }

    fn sample_luminance_linear(&self, x: f32, y: f32) -> f32 {
        let width = self.width as usize;
        let height = self.height as usize;
        let (x0, x1, tx) = sample_axis(x - 0.5, width);
        let (y0, y1, ty) = sample_axis(y - 0.5, height);
        let stride = width * 3;
        let c00 = linear_luma_rgb8_at(self.source, y0 * stride + x0 * 3);
        let c10 = linear_luma_rgb8_at(self.source, y0 * stride + x1 * 3);
        let c01 = linear_luma_rgb8_at(self.source, y1 * stride + x0 * 3);
        let c11 = linear_luma_rgb8_at(self.source, y1 * stride + x1 * 3);
        bilinear(c00, c10, c01, c11, tx, ty)
    }
}

impl Rgba16Context<'_> {
    fn pixel_value(&self, x: usize, y: usize, channel: usize) -> f32 {
        let mut hits = 0u32;
        for sample in 0..RFGR_MONTE_CARLO_SAMPLES {
            let (dx, dy) = gaussian_sample(self.seed, x, y, channel, sample);
            let sx = x as f32 + 0.5 + dx * RFGR_GAUSSIAN_SIGMA;
            let sy = y as f32 + 0.5 + dy * RFGR_GAUSSIAN_SIGMA;
            if boolean_hit(sx, sy, channel, self.seed, self.model, |cx, cy| {
                self.sample_linear(cx, cy, channel)
            }) {
                hits += 1;
            }
        }
        hits as f32 / RFGR_MONTE_CARLO_SAMPLES as f32
    }

    fn pixel_value_monochrome(&self, x: usize, y: usize) -> f32 {
        let mut hits = 0u32;
        for sample in 0..RFGR_MONTE_CARLO_SAMPLES {
            let (dx, dy) = gaussian_sample(self.seed, x, y, MONOCHROME_CHANNEL, sample);
            let sx = x as f32 + 0.5 + dx * RFGR_GAUSSIAN_SIGMA;
            let sy = y as f32 + 0.5 + dy * RFGR_GAUSSIAN_SIGMA;
            if boolean_hit(
                sx,
                sy,
                MONOCHROME_CHANNEL,
                self.seed,
                self.model,
                |cx, cy| self.sample_luminance_linear(cx, cy),
            ) {
                hits += 1;
            }
        }
        hits as f32 / RFGR_MONTE_CARLO_SAMPLES as f32
    }

    fn sample_linear(&self, x: f32, y: f32, channel: usize) -> f32 {
        let width = self.width as usize;
        let height = self.height as usize;
        let (x0, x1, tx) = sample_axis(x - 0.5, width);
        let (y0, y1, ty) = sample_axis(y - 0.5, height);
        let stride = width * 4;
        let c00 = srgb16_to_linear(self.source[y0 * stride + x0 * 4 + channel]);
        let c10 = srgb16_to_linear(self.source[y0 * stride + x1 * 4 + channel]);
        let c01 = srgb16_to_linear(self.source[y1 * stride + x0 * 4 + channel]);
        let c11 = srgb16_to_linear(self.source[y1 * stride + x1 * 4 + channel]);
        bilinear(c00, c10, c01, c11, tx, ty)
    }

    fn sample_luminance_linear(&self, x: f32, y: f32) -> f32 {
        let width = self.width as usize;
        let height = self.height as usize;
        let (x0, x1, tx) = sample_axis(x - 0.5, width);
        let (y0, y1, ty) = sample_axis(y - 0.5, height);
        let stride = width * 4;
        let c00 = linear_luma_rgba16_at(self.source, y0 * stride + x0 * 4);
        let c10 = linear_luma_rgba16_at(self.source, y0 * stride + x1 * 4);
        let c01 = linear_luma_rgba16_at(self.source, y1 * stride + x0 * 4);
        let c11 = linear_luma_rgba16_at(self.source, y1 * stride + x1 * 4);
        bilinear(c00, c10, c01, c11, tx, ty)
    }
}

fn boolean_hit<F>(
    sx: f32,
    sy: f32,
    channel: usize,
    seed: u64,
    model: &RfgrModel,
    mut intensity_at: F,
) -> bool
where
    F: FnMut(f32, f32) -> f32,
{
    let cell = model.cell_size;
    let gx = (sx / cell).floor() as i64;
    let gy = (sy / cell).floor() as i64;
    let cell_radius = (model.max_radius / cell).ceil() as i64 + 1;
    let channel_seed = seed ^ CHANNEL_STREAMS[channel];

    for cy in (gy - cell_radius)..=(gy + cell_radius) {
        for cx in (gx - cell_radius)..=(gx + cell_radius) {
            let cell_center_x = (cx as f32 + 0.5) * cell;
            let cell_center_y = (cy as f32 + 0.5) * cell;
            let mean = model.poisson_mean_for_intensity(intensity_at(cell_center_x, cell_center_y));
            let cell_hash = hash_cell(channel_seed, cx, cy);
            let count = poisson_count(cell_hash, mean);
            for index in 0..count {
                let grain_hash = hash_grain(cell_hash, index);
                let center_x = (cx as f32 + unit_from_hash(grain_hash)) * cell;
                let center_y = (cy as f32 + unit_from_hash(grain_hash.rotate_left(17))) * cell;
                let radius = model.sample_radius(grain_hash.rotate_left(37));
                let dx = sx - center_x;
                let dy = sy - center_y;
                if dx * dx + dy * dy <= radius * radius {
                    return true;
                }
            }
        }
    }

    false
}

fn gaussian_sample(seed: u64, x: usize, y: usize, channel: usize, sample: usize) -> (f32, f32) {
    let hash = hash_sample(seed ^ CHANNEL_STREAMS[channel], x, y, sample as u64);
    box_muller(hash)
}

fn poisson_count(hash: u64, mean: f32) -> u32 {
    if mean <= 0.0 {
        return 0;
    }
    if mean < 24.0 {
        let limit = (-mean).exp();
        let mut product = 1.0f32;
        let mut count = 0u32;
        loop {
            product *= unit_open_from_hash(hash_sample(hash, count as usize, 0, 19));
            if product <= limit {
                return count.min(RFGR_MAX_POISSON_COUNT);
            }
            count += 1;
            if count >= RFGR_MAX_POISSON_COUNT {
                return RFGR_MAX_POISSON_COUNT;
            }
        }
    }

    let z = normal_from_hash(hash.rotate_left(29));
    (mean + z * mean.sqrt())
        .round()
        .clamp(0.0, RFGR_MAX_POISSON_COUNT as f32) as u32
}

fn sample_axis(value: f32, limit: usize) -> (usize, usize, f32) {
    if limit <= 1 {
        return (0, 0, 0.0);
    }
    let value = value.clamp(0.0, (limit - 1) as f32);
    let low = value.floor() as usize;
    let high = (low + 1).min(limit - 1);
    (low, high, value - low as f32)
}

fn bilinear(c00: f32, c10: f32, c01: f32, c11: f32, tx: f32, ty: f32) -> f32 {
    let top = c00 + (c10 - c00) * tx;
    let bottom = c01 + (c11 - c01) * tx;
    top + (bottom - top) * ty
}

fn blend_grain(input: f32, grain: f32, alpha: f32) -> f32 {
    (input + alpha * (grain - input)).clamp(0.0, 1.0)
}

fn linear_luma_rgb8_at(source: &[u8], offset: usize) -> f32 {
    linear_luma_rgb8(source[offset], source[offset + 1], source[offset + 2])
}

fn linear_luma_rgba16_at(source: &[u16], offset: usize) -> f32 {
    linear_luma_rgba16(source[offset], source[offset + 1], source[offset + 2])
}

fn linear_luma_rgb8(r: u8, g: u8, b: u8) -> f32 {
    srgb8_to_linear(r) * REC_709[0]
        + srgb8_to_linear(g) * REC_709[1]
        + srgb8_to_linear(b) * REC_709[2]
}

fn linear_luma_rgba16(r: u16, g: u16, b: u16) -> f32 {
    srgb16_to_linear(r) * REC_709[0]
        + srgb16_to_linear(g) * REC_709[1]
        + srgb16_to_linear(b) * REC_709[2]
}

fn srgb8_to_linear(value: u8) -> f32 {
    srgb8_lut()[value as usize]
}

fn srgb16_to_linear(value: u16) -> f32 {
    srgb16_lut()[value as usize]
}

fn srgb8_lut() -> &'static [f32] {
    static LUT: OnceLock<Vec<f32>> = OnceLock::new();
    LUT.get_or_init(|| {
        (0..=255)
            .map(|value| srgb_to_linear_unit(value as f32 / 255.0))
            .collect()
    })
}

fn srgb16_lut() -> &'static [f32] {
    static LUT: OnceLock<Vec<f32>> = OnceLock::new();
    LUT.get_or_init(|| {
        (0..=65535)
            .map(|value| srgb_to_linear_unit(value as f32 / 65535.0))
            .collect()
    })
}

fn srgb_to_linear_unit(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb8(value: f32) -> u8 {
    (linear_to_srgb_unit(value) * 255.0)
        .round()
        .clamp(0.0, 255.0) as u8
}

fn linear_to_srgb16(value: f32) -> u16 {
    (linear_to_srgb_unit(value) * 65535.0)
        .round()
        .clamp(0.0, 65535.0) as u16
}

fn linear_to_srgb_unit(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

fn hash_cell(seed: u64, x: i64, y: i64) -> u64 {
    let mut h = seed
        ^ (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    mix_hash(&mut h);
    h
}

fn hash_sample(seed: u64, x: usize, y: usize, stream: u64) -> u64 {
    let mut h = seed
        ^ stream.wrapping_mul(0xA076_1D64_78BD_642F)
        ^ (x as u64).wrapping_mul(0xE703_7ED1_A0B4_28DB)
        ^ (y as u64).wrapping_mul(0x8EBC_6AF0_9C88_C6E3);
    mix_hash(&mut h);
    h
}

fn hash_grain(seed: u64, index: u32) -> u64 {
    let mut h = seed ^ (index as u64).wrapping_mul(0xD2B7_4407_B1CE_6E93);
    mix_hash(&mut h);
    h
}

fn mix_hash(hash: &mut u64) {
    *hash ^= *hash >> 30;
    *hash = (*hash).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    *hash ^= *hash >> 27;
    *hash = (*hash).wrapping_mul(0x94D0_49BB_1331_11EB);
    *hash ^= *hash >> 31;
}

fn unit_from_hash(hash: u64) -> f32 {
    ((hash >> 40) as u32 as f32) / ((1u32 << 24) as f32)
}

fn unit_open_from_hash(hash: u64) -> f32 {
    (unit_from_hash(hash) * (1.0 - 2.0e-7) + 1.0e-7).clamp(1.0e-7, 0.999_999_9)
}

fn normal_from_hash(hash: u64) -> f32 {
    box_muller(hash).0
}

fn box_muller(hash: u64) -> (f32, f32) {
    let u1 = unit_open_from_hash(hash);
    let u2 = unit_open_from_hash(hash.rotate_left(32));
    let radius = (-2.0 * u1.ln()).sqrt();
    let theta = TWO_PI * u2;
    (radius * theta.cos(), radius * theta.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(grain: GrainSettings) -> RfgrModel {
        RfgrModel::from_settings(grain)
    }

    #[test]
    fn rfgr_mapping_scales_amount_size_and_frequency() {
        let light = model(GrainSettings {
            amount: 18,
            size: 35,
            frequency: 40,
        });
        let medium = model(GrainSettings {
            amount: 30,
            size: 45,
            frequency: 45,
        });
        let heavy = model(GrainSettings {
            amount: 45,
            size: 60,
            frequency: 55,
        });
        assert!(medium.alpha > light.alpha);
        assert!(heavy.alpha > medium.alpha);
        assert!(heavy.mu_r > light.mu_r);

        let large_low_freq = model(GrainSettings {
            amount: 30,
            size: 90,
            frequency: 10,
        });
        let large_high_freq = model(GrainSettings {
            amount: 30,
            size: 90,
            frequency: 90,
        });
        assert!(large_low_freq.mu_r > large_high_freq.mu_r);
        assert!(large_low_freq.log_sigma > large_high_freq.log_sigma);
        assert!(large_high_freq.max_radius > 0.0);
        assert!(large_low_freq.fast_filter_sigma() > large_high_freq.fast_filter_sigma());
    }

    #[test]
    fn rfgr_poisson_mean_matches_boolean_coverage_mapping() {
        let model = model(GrainSettings {
            amount: 30,
            size: 45,
            frequency: 45,
        });
        assert_eq!(model.poisson_mean_for_intensity(0.0), 0.0);
        assert!(model.poisson_mean_for_intensity(0.8) > model.poisson_mean_for_intensity(0.2));
    }

    #[test]
    fn rfgr_hash_sampling_is_deterministic_and_bounded() {
        let hash = hash_sample(1, 2, 3, 4);
        assert_eq!(hash, hash_sample(1, 2, 3, 4));
        assert_ne!(hash, hash_sample(1, 2, 3, 5));
        assert!((0.0..1.0).contains(&unit_from_hash(hash)));

        let model = model(GrainSettings {
            amount: 30,
            size: 45,
            frequency: 45,
        });
        let radius = model.sample_radius(hash);
        assert!((RFGR_MIN_RADIUS..=model.max_radius).contains(&radius));
    }

    #[test]
    fn rfgr_monochrome_detection_accepts_neutral_and_rejects_color() {
        let neutral = ImageBuffer::<Rgb<u8>, Vec<u8>>::from_fn(8, 6, |x, y| {
            let value = (32 + x * 3 + y * 5) as u8;
            Rgb([value, value.saturating_add((x % 2) as u8), value])
        })
        .into_raw();
        assert!(is_monochrome_rgb8(&neutral));

        let color = ImageBuffer::<Rgb<u8>, Vec<u8>>::from_fn(8, 6, |x, y| {
            Rgb([
                (32 + x * 4) as u8,
                (80 + y * 5) as u8,
                (128 + x * 2 + y) as u8,
            ])
        })
        .into_raw();
        assert!(!is_monochrome_rgb8(&color));
    }

    #[test]
    fn rfgr_8bit_monochrome_render_uses_shared_grain() {
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_fn(6, 5, |x, y| {
            let value = (48 + x * 9 + y * 7) as u8;
            Rgb([value, value, value])
        }));
        let grain = GrainSettings {
            amount: 45,
            size: 50,
            frequency: 45,
        };

        let out = render_grain_8(image, grain, 123).unwrap().into_raw();
        for pixel in out.as_chunks::<3>().0 {
            assert_eq!(pixel[0], pixel[1]);
            assert_eq!(pixel[1], pixel[2]);
        }
    }

    #[test]
    fn rfgr_fast_8bit_monochrome_render_uses_shared_grain() {
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_fn(16, 12, |x, y| {
            let value = (24 + x * 4 + y * 3) as u8;
            Rgb([value, value, value])
        }));
        let grain = GrainSettings {
            amount: 45,
            size: 65,
            frequency: 35,
        };

        let out = render_grain_8_fast(image, grain, 123).unwrap().into_raw();
        for pixel in out.as_chunks::<3>().0 {
            assert_eq!(pixel[0], pixel[1]);
            assert_eq!(pixel[1], pixel[2]);
        }
    }

    #[test]
    fn rfgr_8bit_render_is_deterministic_and_seeded() {
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_fn(6, 5, |x, y| {
            Rgb([
                (32 + x * 7) as u8,
                (80 + y * 9) as u8,
                (128 + x * 3 + y * 2) as u8,
            ])
        }));
        let grain = GrainSettings {
            amount: 40,
            size: 50,
            frequency: 45,
        };

        let a = render_grain_8(image.clone(), grain, 123)
            .unwrap()
            .into_raw();
        let b = render_grain_8(image.clone(), grain, 123)
            .unwrap()
            .into_raw();
        let c = render_grain_8(image, grain, 124).unwrap().into_raw();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn rfgr_fast_8bit_render_is_deterministic_and_seeded() {
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_fn(16, 12, |x, y| {
            Rgb([
                (24 + x * 5) as u8,
                (72 + y * 7) as u8,
                (96 + x * 2 + y * 3) as u8,
            ])
        }));
        let grain = GrainSettings {
            amount: 45,
            size: 65,
            frequency: 35,
        };

        let a = render_grain_8_fast(image.clone(), grain, 123)
            .unwrap()
            .into_raw();
        let b = render_grain_8_fast(image.clone(), grain, 123)
            .unwrap()
            .into_raw();
        let c = render_grain_8_fast(image, grain, 124).unwrap().into_raw();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn rfgr_16bit_render_preserves_alpha_channel() {
        let image = DynamicImage::ImageRgba16(ImageBuffer::from_fn(4, 3, |x, y| {
            Rgba([
                (10_000 + x * 1000) as u16,
                (20_000 + y * 1000) as u16,
                30_000,
                (40_000 + x * 10 + y) as u16,
            ])
        }));
        let grain = GrainSettings {
            amount: 35,
            size: 40,
            frequency: 50,
        };

        let out = render_grain(image.clone(), grain, 5).unwrap().to_rgba16();
        let input = image.to_rgba16();
        for (before, after) in input.pixels().zip(out.pixels()) {
            assert_eq!(before[3], after[3]);
        }
    }

    #[test]
    fn rfgr_fast_16bit_render_preserves_alpha_channel() {
        let image = DynamicImage::ImageRgba16(ImageBuffer::from_fn(8, 6, |x, y| {
            Rgba([
                (10_000 + x * 1000) as u16,
                (20_000 + y * 1000) as u16,
                30_000,
                (40_000 + x * 10 + y) as u16,
            ])
        }));
        let grain = GrainSettings {
            amount: 35,
            size: 40,
            frequency: 50,
        };

        let out = render_grain_fast(image.clone(), grain, 5)
            .unwrap()
            .to_rgba16();
        let input = image.to_rgba16();
        for (before, after) in input.pixels().zip(out.pixels()) {
            assert_eq!(before[3], after[3]);
        }
    }
}
