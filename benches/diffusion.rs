use std::{env, hint::black_box, sync::Once, time::Duration};

use criterion::{
    BatchSize, BenchmarkGroup, BenchmarkId, Criterion, SamplingMode, Throughput, criterion_group,
    measurement::WallTime,
};
use image::{ImageBuffer, Rgb};
use mini_film::{DiffusionMethod, DiffusionPreset, DiffusionSettings, render_diffusion_rgb16};
use rayon::{ThreadPool, ThreadPoolBuilder, prelude::*};

type Rgb16Image = ImageBuffer<Rgb<u16>, Vec<u16>>;

#[derive(Clone, Copy)]
struct ImageSize {
    name: &'static str,
    width: u32,
    height: u32,
}

impl ImageSize {
    const PREVIEW: Self = Self {
        name: "preview-2048x1365",
        width: 2048,
        height: 1365,
    };

    const REFERENCE: Self = Self {
        name: "reference-4000x3000",
        width: 4000,
        height: 3000,
    };

    const FULL: Self = Self {
        name: "full-8256x5504",
        width: 8256,
        height: 5504,
    };

    const fn pixels(self) -> u64 {
        self.width as u64 * self.height as u64
    }
}

struct PoolCase {
    name: String,
    pool: ThreadPool,
}

fn benchmark_config() -> Criterion {
    Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(5))
}

fn benchmark_diffusion(criterion: &mut Criterion) {
    let all_threads = std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1);
    let pools = [
        PoolCase {
            name: "single-thread".to_string(),
            pool: thread_pool(1),
        },
        PoolCase {
            name: format!("all-{all_threads}-threads"),
            pool: thread_pool(all_threads),
        },
    ];

    let mut sizes = vec![ImageSize::PREVIEW, ImageSize::REFERENCE];
    if full_resolution_enabled() {
        sizes.push(ImageSize::FULL);
    }

    for size in sizes {
        let source = synthetic_image(size);
        for pool in &pools {
            let mut group =
                criterion.benchmark_group(format!("diffusion/{}/{}", size.name, pool.name));
            group.sampling_mode(SamplingMode::Flat);
            group.throughput(Throughput::Elements(size.pixels()));
            bench_render(
                &mut group,
                "multi-scale-mist-medium",
                &source,
                &pool.pool,
                DiffusionSettings::from_preset(
                    DiffusionMethod::MultiScaleMist,
                    DiffusionPreset::Medium,
                ),
            );
            bench_render(
                &mut group,
                "edge-aware-glow-medium",
                &source,
                &pool.pool,
                DiffusionSettings::from_preset(
                    DiffusionMethod::EdgeAwareGlow,
                    DiffusionPreset::Medium,
                ),
            );

            if size.name == ImageSize::REFERENCE.name {
                let medium = DiffusionSettings::from_preset(
                    DiffusionMethod::EdgeAwareGlow,
                    DiffusionPreset::Medium,
                );
                bench_render(
                    &mut group,
                    "edge-aware-softness-only",
                    &source,
                    &pool.pool,
                    DiffusionSettings {
                        highlight_glow: 0,
                        ..medium
                    },
                );
                bench_render(
                    &mut group,
                    "edge-aware-glow-only",
                    &source,
                    &pool.pool,
                    DiffusionSettings {
                        softness: 0,
                        ..medium
                    },
                );
            }
            group.finish();
        }
    }

    bench_concurrent_previews(criterion, all_threads);
}

fn bench_render(
    group: &mut BenchmarkGroup<'_, WallTime>,
    name: &str,
    source: &Rgb16Image,
    pool: &ThreadPool,
    settings: DiffusionSettings,
) {
    let validation = Once::new();
    group.bench_with_input(
        BenchmarkId::from_parameter(name),
        &settings,
        |bencher, settings| {
            validation.call_once(|| {
                let checksum = rendered_checksum(source, pool, *settings);
                eprintln!("diffusion benchmark checksum {name}: {checksum:016x}");
            });
            bencher.iter_batched(
                || source.clone(),
                |mut image| {
                    pool.install(|| render_diffusion_rgb16(&mut image, *settings))
                        .expect("benchmark diffusion render should succeed");
                    black_box(image)
                },
                BatchSize::LargeInput,
            );
        },
    );
}

fn bench_concurrent_previews(criterion: &mut Criterion, all_threads: usize) {
    let job_count = (all_threads / 2).max(1);
    let pool = thread_pool(all_threads);
    let source = synthetic_image(ImageSize::PREVIEW);
    let mut group = criterion.benchmark_group(format!(
        "diffusion/concurrent-preview/{job_count}-jobs-{all_threads}-threads"
    ));
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(
        ImageSize::PREVIEW.pixels() * job_count as u64,
    ));

    for method in [
        DiffusionMethod::MultiScaleMist,
        DiffusionMethod::EdgeAwareGlow,
    ] {
        let settings = DiffusionSettings::from_preset(method, DiffusionPreset::Medium);
        let validation = Once::new();
        group.bench_function(method.as_str(), |bencher| {
            validation.call_once(|| {
                let checksum = concurrent_rendered_checksum(&source, &pool, job_count, settings);
                eprintln!(
                    "diffusion concurrent benchmark checksum {}: {checksum:016x}",
                    method.as_str()
                );
            });
            bencher.iter_batched(
                || vec![source.clone(); job_count],
                |mut images| {
                    pool.install(|| {
                        images.par_iter_mut().for_each(|image| {
                            render_diffusion_rgb16(image, settings)
                                .expect("concurrent benchmark diffusion render should succeed");
                        });
                    });
                    black_box(images)
                },
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

fn thread_pool(threads: usize) -> ThreadPool {
    ThreadPoolBuilder::new()
        .num_threads(threads.max(1))
        .thread_name(|index| format!("diffusion-bench-{index}"))
        .build()
        .expect("benchmark Rayon pool should build")
}

fn full_resolution_enabled() -> bool {
    env::var("MINI_FILM_DIFFUSION_BENCH_FULL")
        .is_ok_and(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}

fn synthetic_image(size: ImageSize) -> Rgb16Image {
    let width = u64::from(size.width);
    let height = u64::from(size.height);
    let highlight_x = width * 3 / 4;
    let highlight_y = height / 3;
    let highlight_radius = width.min(height).div_ceil(7).max(1);
    let highlight_radius_squared = highlight_radius * highlight_radius;
    let mut raw = Vec::with_capacity(size.pixels() as usize * 3);

    for y in 0..height {
        for x in 0..width {
            let gradient_x = x * 38_000 / width.saturating_sub(1).max(1);
            let gradient_y = y * 17_000 / height.saturating_sub(1).max(1);
            let checker = if ((x / 32) + (y / 32)) % 2 == 0 {
                3_200_i64
            } else {
                -3_200_i64
            };
            let texture = (pixel_hash(x, y) & 2047) as i64 - 1024;
            let dx = x.abs_diff(highlight_x);
            let dy = y.abs_diff(highlight_y);
            let distance_squared = dx * dx + dy * dy;
            let highlight = highlight_radius_squared
                .saturating_sub(distance_squared)
                .saturating_mul(28_000)
                / highlight_radius_squared;
            let base = 4_000_i64
                + gradient_x as i64
                + gradient_y as i64
                + checker
                + texture
                + highlight as i64;

            raw.push(to_u16(base + 2_000));
            raw.push(to_u16(base));
            raw.push(to_u16(base - 2_000));
        }
    }

    ImageBuffer::from_raw(size.width, size.height, raw)
        .expect("synthetic RGB16 buffer dimensions should match")
}

fn pixel_hash(x: u64, y: u64) -> u64 {
    let mut value = x
        .wrapping_mul(0x9e37_79b9_7f4a_7c15)
        .wrapping_add(y.wrapping_mul(0xbf58_476d_1ce4_e5b9));
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value.wrapping_mul(0x94d0_49bb_1331_11eb) ^ (value >> 31)
}

fn to_u16(value: i64) -> u16 {
    value.clamp(0, i64::from(u16::MAX)) as u16
}

fn rendered_checksum(source: &Rgb16Image, pool: &ThreadPool, settings: DiffusionSettings) -> u64 {
    let mut image = source.clone();
    pool.install(|| render_diffusion_rgb16(&mut image, settings))
        .expect("checksum diffusion render should succeed");
    full_checksum(&image)
}

fn concurrent_rendered_checksum(
    source: &Rgb16Image,
    pool: &ThreadPool,
    job_count: usize,
    settings: DiffusionSettings,
) -> u64 {
    let mut images = vec![source.clone(); job_count];
    pool.install(|| {
        images.par_iter_mut().for_each(|image| {
            render_diffusion_rgb16(image, settings)
                .expect("concurrent checksum diffusion render should succeed");
        });
    });
    images.iter().fold(0_u64, |checksum, image| {
        checksum.rotate_left(1) ^ full_checksum(image)
    })
}

fn full_checksum(image: &Rgb16Image) -> u64 {
    image
        .as_raw()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |checksum, sample| {
            (checksum ^ u64::from(*sample)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

criterion_group! {
    name = diffusion_benches;
    config = benchmark_config();
    targets = benchmark_diffusion
}

fn main() {
    // `cargo test --all-targets` executes harness-free benchmark binaries with
    // libtest arguments. Criterion does not accept those arguments, and test
    // runs should not execute the benchmark matrix.
    if env::args_os().any(|argument| argument.to_string_lossy().starts_with("--test-threads")) {
        return;
    }

    diffusion_benches();
    Criterion::default().configure_from_args().final_summary();
}
