use super::{model::*, prelude::*};
use crate::app::retouch::normalize_rotation;
use image::{RgbImage, imageops::FilterType};
use std::{cmp::Ordering as CmpOrdering, io::Write};

const DETAIL_ANALYSIS_VERSION: &str = "diffusion-detail-areas-v1";
const DETAIL_ANALYSIS_LONG_EDGE: u32 = 640;
const DETAIL_CROP_FRACTION: f64 = 0.24;
const DETAIL_CROP_MIN: u32 = 256;
const DETAIL_CROP_MAX: u32 = 448;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DiffusionPreviewDetails {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) focus_source: ReviewDiffusionFocusSource,
    pub(super) detail_areas: Vec<ReviewDiffusionDetailArea>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct CachedDiffusionPreviewDetails {
    analysis_version: String,
    preview_identity: String,
    focus_signature: String,
    preview_width: u32,
    preview_height: u32,
    focus_source: ReviewDiffusionFocusSource,
    detail_areas: Vec<ReviewDiffusionDetailArea>,
}

#[derive(Clone, Copy, Debug)]
struct ScoredCandidate {
    area: ReviewDiffusionDetailArea,
    high_contrast_score: f64,
    broad_highlight_score: f64,
    frame_center_distance: f64,
}

#[derive(Clone, Copy, Debug)]
struct FloatRect {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl FloatRect {
    fn width(self) -> f64 {
        (self.right - self.left).max(0.0)
    }

    fn height(self) -> f64 {
        (self.bottom - self.top).max(0.0)
    }

    fn area(self) -> f64 {
        self.width() * self.height()
    }

    fn clipped(self, width: f64, height: f64) -> Option<Self> {
        let clipped = Self {
            left: self.left.clamp(0.0, width),
            top: self.top.clamp(0.0, height),
            right: self.right.clamp(0.0, width),
            bottom: self.bottom.clamp(0.0, height),
        };
        (clipped.right > clipped.left && clipped.bottom > clipped.top).then_some(clipped)
    }
}

#[derive(Clone, Debug)]
struct RetouchGeometry {
    source_width: f64,
    source_height: f64,
    safe_width: f64,
    safe_height: f64,
    crop_left: f64,
    crop_top: f64,
    crop_width: f64,
    crop_height: f64,
    preview_width: f64,
    preview_height: f64,
    cos: f64,
    sin: f64,
}

impl RetouchGeometry {
    fn new(
        source_width: u32,
        source_height: u32,
        preview_width: u32,
        preview_height: u32,
        retouch: &RetouchSettings,
    ) -> Self {
        let source_width = f64::from(source_width.max(1));
        let source_height = f64::from(source_height.max(1));
        let retouch = retouch.clone().normalized();
        let rotation = normalize_rotation(retouch.rotation_degrees);
        let (safe_width_u32, safe_height_u32) =
            rotated_safe_dimensions(source_width as u32, source_height as u32, rotation);
        let safe_width = f64::from(safe_width_u32);
        let safe_height = f64::from(safe_height_u32);
        let crop = retouch.crop;
        let (crop_left, crop_top, crop_width, crop_height) = if let Some(crop) = crop {
            let width = ((crop.width * safe_width_u32 as f32).round() as u32)
                .clamp(1, safe_width_u32.max(1));
            let height = ((crop.height * safe_height_u32 as f32).round() as u32)
                .clamp(1, safe_height_u32.max(1));
            let max_x = safe_width_u32.saturating_sub(width);
            let max_y = safe_height_u32.saturating_sub(height);
            let left = ((crop.x * safe_width_u32 as f32).round() as u32).min(max_x);
            let top = ((crop.y * safe_height_u32 as f32).round() as u32).min(max_y);
            (
                f64::from(left),
                f64::from(top),
                f64::from(width),
                f64::from(height),
            )
        } else {
            (0.0, 0.0, safe_width, safe_height)
        };
        let radians = f64::from(rotation).to_radians();
        Self {
            source_width,
            source_height,
            safe_width,
            safe_height,
            crop_left,
            crop_top,
            crop_width,
            crop_height,
            preview_width: f64::from(preview_width.max(1)),
            preview_height: f64::from(preview_height.max(1)),
            cos: radians.cos(),
            sin: radians.sin(),
        }
    }

    fn transform_point(&self, x: f64, y: f64) -> (f64, f64) {
        let centered_x = x - self.source_width / 2.0;
        let centered_y = y - self.source_height / 2.0;
        let safe_x = self.cos * centered_x - self.sin * centered_y + self.safe_width / 2.0;
        let safe_y = self.sin * centered_x + self.cos * centered_y + self.safe_height / 2.0;
        (
            (safe_x - self.crop_left) * self.preview_width / self.crop_width,
            (safe_y - self.crop_top) * self.preview_height / self.crop_height,
        )
    }

    fn transform_region(&self, region: GalleryFocusRegion) -> Option<FloatRect> {
        let left = f64::from(region.x) * self.source_width;
        let top = f64::from(region.y) * self.source_height;
        let right = f64::from(region.x + region.width) * self.source_width;
        let bottom = f64::from(region.y + region.height) * self.source_height;
        let corners = [
            self.transform_point(left, top),
            self.transform_point(right, top),
            self.transform_point(right, bottom),
            self.transform_point(left, bottom),
        ];
        let bounds = FloatRect {
            left: corners
                .iter()
                .map(|(x, _)| *x)
                .fold(f64::INFINITY, f64::min),
            top: corners
                .iter()
                .map(|(_, y)| *y)
                .fold(f64::INFINITY, f64::min),
            right: corners
                .iter()
                .map(|(x, _)| *x)
                .fold(f64::NEG_INFINITY, f64::max),
            bottom: corners
                .iter()
                .map(|(_, y)| *y)
                .fold(f64::NEG_INFINITY, f64::max),
        };
        bounds.clipped(self.preview_width, self.preview_height)
    }
}

pub(super) fn load_or_analyze_diffusion_preview_details(
    before: &Path,
    base: &Path,
    cache_dir: &Path,
    focus_regions: &[GalleryFocusRegion],
    retouch: &RetouchSettings,
) -> Result<DiffusionPreviewDetails> {
    let bytes = fs::read(before).with_context(|| format!("reading {}", before.display()))?;
    let preview_identity = digest_bytes(&bytes);
    let (preview_width, preview_height) =
        image::image_dimensions(before).with_context(|| format!("reading {}", before.display()))?;
    if preview_width == 0 || preview_height == 0 {
        bail!(
            "diffusion preview has empty dimensions: {}",
            before.display()
        );
    }
    let (base_width, base_height) =
        image::image_dimensions(base).with_context(|| format!("reading {}", base.display()))?;
    let focus_signature = focus_signature(base_width, base_height, focus_regions, retouch)?;
    let cache_path = cache_dir.join("areas-v1.json");

    if let Some(cached) = read_cached_details(
        &cache_path,
        &preview_identity,
        &focus_signature,
        preview_width,
        preview_height,
    ) {
        return Ok(DiffusionPreviewDetails {
            width: cached.preview_width,
            height: cached.preview_height,
            focus_source: cached.focus_source,
            detail_areas: cached.detail_areas,
        });
    }

    let decoded = image::load_from_memory(&bytes)
        .with_context(|| format!("decoding {}", before.display()))?
        .into_rgb8();
    let crop_side = detail_crop_side(preview_width, preview_height);
    let geometry = RetouchGeometry::new(
        base_width,
        base_height,
        preview_width,
        preview_height,
        retouch,
    );
    let (focus_area, focus_source) = select_focus_area(
        &geometry,
        focus_regions,
        crop_side,
        preview_width,
        preview_height,
    );
    let candidates = analyze_candidates(&decoded, crop_side)?;
    let high_contrast = select_candidate(&candidates, CandidateScore::HighContrast, &[focus_area])
        .unwrap_or_else(|| {
            centered_area(
                ReviewDiffusionDetailAreaKind::HighContrastHighlight,
                crop_side,
                preview_width,
                preview_height,
            )
        });
    let broad_highlight = select_candidate(
        &candidates,
        CandidateScore::BroadHighlight,
        &[focus_area, high_contrast],
    )
    .unwrap_or_else(|| {
        centered_area(
            ReviewDiffusionDetailAreaKind::BroadHighlight,
            crop_side,
            preview_width,
            preview_height,
        )
    });
    let detail_areas = vec![focus_area, high_contrast, broad_highlight];

    let cached = CachedDiffusionPreviewDetails {
        analysis_version: DETAIL_ANALYSIS_VERSION.to_string(),
        preview_identity,
        focus_signature,
        preview_width,
        preview_height,
        focus_source,
        detail_areas: detail_areas.clone(),
    };
    write_cached_details(&cache_path, &cached)?;

    Ok(DiffusionPreviewDetails {
        width: preview_width,
        height: preview_height,
        focus_source,
        detail_areas,
    })
}

fn read_cached_details(
    path: &Path,
    preview_identity: &str,
    focus_signature: &str,
    preview_width: u32,
    preview_height: u32,
) -> Option<CachedDiffusionPreviewDetails> {
    let cached =
        serde_json::from_slice::<CachedDiffusionPreviewDetails>(&fs::read(path).ok()?).ok()?;
    (cached.analysis_version == DETAIL_ANALYSIS_VERSION
        && cached.preview_identity == preview_identity
        && cached.focus_signature == focus_signature
        && cached.preview_width == preview_width
        && cached.preview_height == preview_height
        && valid_detail_areas(&cached.detail_areas, preview_width, preview_height))
    .then_some(cached)
}

fn write_cached_details(path: &Path, cached: &CachedDiffusionPreviewDetails) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("diffusion detail cache path has no parent"))?;
    let mut temp = Builder::new()
        .prefix(".areas-")
        .suffix(".json")
        .tempfile_in(parent)
        .with_context(|| format!("creating diffusion detail cache in {}", parent.display()))?;
    serde_json::to_writer(&mut temp, cached).context("serializing diffusion detail cache")?;
    temp.flush().context("flushing diffusion detail cache")?;
    temp.persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("publishing {}", path.display()))?;
    Ok(())
}

fn valid_detail_areas(areas: &[ReviewDiffusionDetailArea], width: u32, height: u32) -> bool {
    let expected = [
        ReviewDiffusionDetailAreaKind::Focus,
        ReviewDiffusionDetailAreaKind::HighContrastHighlight,
        ReviewDiffusionDetailAreaKind::BroadHighlight,
    ];
    areas.len() == expected.len()
        && areas.iter().zip(expected).all(|(area, kind)| {
            area.kind == kind
                && area.width > 0
                && area.height > 0
                && area
                    .x
                    .checked_add(area.width)
                    .is_some_and(|right| right <= width)
                && area
                    .y
                    .checked_add(area.height)
                    .is_some_and(|bottom| bottom <= height)
        })
}

fn focus_signature(
    base_width: u32,
    base_height: u32,
    focus_regions: &[GalleryFocusRegion],
    retouch: &RetouchSettings,
) -> Result<String> {
    let payload = serde_json::to_vec(&json!({
        "base_width": base_width,
        "base_height": base_height,
        "focus_regions": focus_regions,
        "retouch": retouch.clone().normalized(),
    }))
    .context("serializing diffusion focus identity")?;
    Ok(digest_bytes(&payload))
}

fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn detail_crop_side(width: u32, height: u32) -> u32 {
    let short_edge = width.min(height).max(1);
    ((f64::from(short_edge) * DETAIL_CROP_FRACTION).round() as u32)
        .clamp(DETAIL_CROP_MIN, DETAIL_CROP_MAX)
        .min(short_edge)
}

fn select_focus_area(
    geometry: &RetouchGeometry,
    focus_regions: &[GalleryFocusRegion],
    crop_side: u32,
    preview_width: u32,
    preview_height: u32,
) -> (ReviewDiffusionDetailArea, ReviewDiffusionFocusSource) {
    let mut transformed = focus_regions
        .iter()
        .filter_map(|region| {
            geometry
                .transform_region(*region)
                .map(|bounds| (*region, bounds))
        })
        .collect::<Vec<_>>();
    transformed.sort_by(|(left_region, left), (right_region, right)| {
        right_region
            .primary
            .cmp(&left_region.primary)
            .then_with(|| right.area().total_cmp(&left.area()))
            .then_with(|| left.top.total_cmp(&right.top))
            .then_with(|| left.left.total_cmp(&right.left))
    });
    if let Some((_, bounds)) = transformed.first() {
        let center_x = (bounds.left + bounds.right) / 2.0;
        let center_y = (bounds.top + bounds.bottom) / 2.0;
        return (
            area_around(
                ReviewDiffusionDetailAreaKind::Focus,
                center_x,
                center_y,
                crop_side,
                preview_width,
                preview_height,
            ),
            ReviewDiffusionFocusSource::CameraFocus,
        );
    }
    (
        centered_area(
            ReviewDiffusionDetailAreaKind::Focus,
            crop_side,
            preview_width,
            preview_height,
        ),
        ReviewDiffusionFocusSource::CenterFallback,
    )
}

fn centered_area(
    kind: ReviewDiffusionDetailAreaKind,
    side: u32,
    width: u32,
    height: u32,
) -> ReviewDiffusionDetailArea {
    area_around(
        kind,
        f64::from(width) / 2.0,
        f64::from(height) / 2.0,
        side,
        width,
        height,
    )
}

fn area_around(
    kind: ReviewDiffusionDetailAreaKind,
    center_x: f64,
    center_y: f64,
    side: u32,
    width: u32,
    height: u32,
) -> ReviewDiffusionDetailArea {
    let side = side.min(width).min(height).max(1);
    let max_x = width.saturating_sub(side);
    let max_y = height.saturating_sub(side);
    let x = (center_x - f64::from(side) / 2.0)
        .round()
        .clamp(0.0, f64::from(max_x)) as u32;
    let y = (center_y - f64::from(side) / 2.0)
        .round()
        .clamp(0.0, f64::from(max_y)) as u32;
    ReviewDiffusionDetailArea {
        kind,
        x,
        y,
        width: side,
        height: side,
    }
}

fn analyze_candidates(image: &RgbImage, crop_side: u32) -> Result<Vec<ScoredCandidate>> {
    let (width, height) = image.dimensions();
    let scale = (f64::from(DETAIL_ANALYSIS_LONG_EDGE) / f64::from(width.max(height))).min(1.0);
    let proxy_width = (f64::from(width) * scale).round().max(1.0) as u32;
    let proxy_height = (f64::from(height) * scale).round().max(1.0) as u32;
    let proxy = image::imageops::resize(image, proxy_width, proxy_height, FilterType::Triangle);
    let proxy_width_usize = proxy_width as usize;
    let proxy_height_usize = proxy_height as usize;
    let luma = proxy
        .pixels()
        .map(|pixel| {
            (0.2126 * f64::from(pixel[0])
                + 0.7152 * f64::from(pixel[1])
                + 0.0722 * f64::from(pixel[2]))
                / 255.0
        })
        .collect::<Vec<_>>();
    let blurred = binomial_blur(&luma, proxy_width_usize, proxy_height_usize);
    let (p10, p90, p99) = luminance_percentiles(&blurred);
    let highlight_threshold = p90 + 0.35 * (p99 - p90);
    let highlight_span = (p99 - highlight_threshold).max(0.025);
    let highlight = blurred
        .iter()
        .map(|value| ((*value - highlight_threshold) / highlight_span).clamp(0.0, 1.0))
        .collect::<Vec<_>>();
    let highlight_nearby = dilate_three_by_three(&highlight, proxy_width_usize, proxy_height_usize);
    let edge = scharr_edges(&blurred, proxy_width_usize, proxy_height_usize);
    let highlight_edge = edge
        .iter()
        .zip(&highlight_nearby)
        .map(|(edge, highlight)| edge * highlight)
        .collect::<Vec<_>>();
    let luma_squared = blurred
        .iter()
        .map(|value| value * value)
        .collect::<Vec<_>>();
    let luma_integral = IntegralImage::new(&blurred, proxy_width_usize, proxy_height_usize);
    let luma_squared_integral =
        IntegralImage::new(&luma_squared, proxy_width_usize, proxy_height_usize);
    let highlight_integral = IntegralImage::new(&highlight, proxy_width_usize, proxy_height_usize);
    let edge_integral = IntegralImage::new(&edge, proxy_width_usize, proxy_height_usize);
    let highlight_edge_integral =
        IntegralImage::new(&highlight_edge, proxy_width_usize, proxy_height_usize);
    let proxy_scale_x = f64::from(proxy_width) / f64::from(width);
    let proxy_scale_y = f64::from(proxy_height) / f64::from(height);
    let proxy_side = (f64::from(crop_side) * proxy_scale_x.min(proxy_scale_y))
        .round()
        .max(1.0) as usize;
    let proxy_side = proxy_side.min(proxy_width_usize).min(proxy_height_usize);
    let max_x = proxy_width_usize - proxy_side;
    let max_y = proxy_height_usize - proxy_side;
    let stride = (proxy_side / 6).max(3);
    let x_positions = axis_positions(max_x, stride);
    let y_positions = axis_positions(max_y, stride);
    let low_key_or_flat = p99 < 0.25 || p99 - p90 < 0.025 || p99 - p10 < 0.08;
    let mut candidates = Vec::with_capacity(x_positions.len() * y_positions.len());
    for y in y_positions {
        for &x in &x_positions {
            let pixel_count = (proxy_side * proxy_side) as f64;
            let mean = luma_integral.sum(x, y, proxy_side, proxy_side) / pixel_count;
            let mean_squared =
                luma_squared_integral.sum(x, y, proxy_side, proxy_side) / pixel_count;
            let deviation = (mean_squared - mean * mean).max(0.0).sqrt();
            let highlight_coverage =
                highlight_integral.sum(x, y, proxy_side, proxy_side) / pixel_count;
            let edge_mean = edge_integral.sum(x, y, proxy_side, proxy_side) / pixel_count;
            let highlight_edge_mean =
                highlight_edge_integral.sum(x, y, proxy_side, proxy_side) / pixel_count;
            let high_contrast_score = if low_key_or_flat {
                deviation
            } else {
                0.58 * highlight_edge_mean + 0.27 * deviation + 0.15 * highlight_coverage
            };
            let broad_highlight_score = 0.58 * highlight_coverage + 0.35 * mean - 0.22 * edge_mean;
            let center_x = (x as f64 + proxy_side as f64 / 2.0) / proxy_scale_x;
            let center_y = (y as f64 + proxy_side as f64 / 2.0) / proxy_scale_y;
            let area = area_around(
                ReviewDiffusionDetailAreaKind::HighContrastHighlight,
                center_x,
                center_y,
                crop_side,
                width,
                height,
            );
            candidates.push(ScoredCandidate {
                area,
                high_contrast_score,
                broad_highlight_score,
                frame_center_distance: center_proximity_to_frame(area, width, height),
            });
        }
    }
    if candidates.is_empty() {
        bail!("diffusion detail analysis produced no candidates");
    }
    Ok(candidates)
}

#[derive(Clone, Copy)]
enum CandidateScore {
    HighContrast,
    BroadHighlight,
}

fn select_candidate(
    candidates: &[ScoredCandidate],
    score: CandidateScore,
    selected: &[ReviewDiffusionDetailArea],
) -> Option<ReviewDiffusionDetailArea> {
    let constraints = [(0.10, 0.8), (0.25, 0.6), (1.0, 0.0)];
    for (max_overlap, min_distance) in constraints {
        let best = candidates
            .iter()
            .filter(|candidate| {
                selected.iter().all(|area| {
                    overlap_fraction(candidate.area, *area) <= max_overlap
                        && center_distance(candidate.area, *area)
                            >= min_distance * f64::from(candidate.area.width)
                })
            })
            .max_by(|left, right| compare_candidates(left, right, score, selected));
        if let Some(best) = best {
            let mut area = best.area;
            area.kind = match score {
                CandidateScore::HighContrast => {
                    ReviewDiffusionDetailAreaKind::HighContrastHighlight
                }
                CandidateScore::BroadHighlight => ReviewDiffusionDetailAreaKind::BroadHighlight,
            };
            return Some(area);
        }
    }
    None
}

fn compare_candidates(
    left: &ScoredCandidate,
    right: &ScoredCandidate,
    score: CandidateScore,
    selected: &[ReviewDiffusionDetailArea],
) -> CmpOrdering {
    let left_score = match score {
        CandidateScore::HighContrast => left.high_contrast_score,
        CandidateScore::BroadHighlight => left.broad_highlight_score,
    };
    let right_score = match score {
        CandidateScore::HighContrast => right.high_contrast_score,
        CandidateScore::BroadHighlight => right.broad_highlight_score,
    };
    score_bucket(left_score)
        .cmp(&score_bucket(right_score))
        .then_with(|| {
            minimum_selected_distance(left.area, selected)
                .total_cmp(&minimum_selected_distance(right.area, selected))
        })
        .then_with(|| {
            right
                .frame_center_distance
                .total_cmp(&left.frame_center_distance)
        })
        .then_with(|| right.area.y.cmp(&left.area.y))
        .then_with(|| right.area.x.cmp(&left.area.x))
}

fn score_bucket(value: f64) -> i64 {
    (value * 1_000_000.0).round() as i64
}

fn minimum_selected_distance(
    area: ReviewDiffusionDetailArea,
    selected: &[ReviewDiffusionDetailArea],
) -> f64 {
    selected
        .iter()
        .map(|selected| center_distance(area, *selected))
        .fold(f64::INFINITY, f64::min)
}

fn overlap_fraction(left: ReviewDiffusionDetailArea, right: ReviewDiffusionDetailArea) -> f64 {
    let intersection_width = left
        .x
        .saturating_add(left.width)
        .min(right.x.saturating_add(right.width))
        .saturating_sub(left.x.max(right.x));
    let intersection_height = left
        .y
        .saturating_add(left.height)
        .min(right.y.saturating_add(right.height))
        .saturating_sub(left.y.max(right.y));
    let intersection = f64::from(intersection_width) * f64::from(intersection_height);
    let denominator = f64::from(left.width) * f64::from(left.height);
    if denominator == 0.0 {
        1.0
    } else {
        intersection / denominator
    }
}

fn center_distance(left: ReviewDiffusionDetailArea, right: ReviewDiffusionDetailArea) -> f64 {
    let left_x = f64::from(left.x) + f64::from(left.width) / 2.0;
    let left_y = f64::from(left.y) + f64::from(left.height) / 2.0;
    let right_x = f64::from(right.x) + f64::from(right.width) / 2.0;
    let right_y = f64::from(right.y) + f64::from(right.height) / 2.0;
    (left_x - right_x).hypot(left_y - right_y)
}

fn center_proximity_to_frame(area: ReviewDiffusionDetailArea, width: u32, height: u32) -> f64 {
    let center_x = f64::from(area.x) + f64::from(area.width) / 2.0;
    let center_y = f64::from(area.y) + f64::from(area.height) / 2.0;
    (center_x - f64::from(width) / 2.0).hypot(center_y - f64::from(height) / 2.0)
}

fn rotated_safe_dimensions(width: u32, height: u32, rotation: f32) -> (u32, u32) {
    let rotation = normalize_rotation(rotation);
    let absolute = rotation.abs();
    if absolute < 0.001 || (absolute - 180.0).abs() < 0.001 {
        return (width.max(1), height.max(1));
    }
    if (absolute - 90.0).abs() < 0.001 {
        return (height.max(1), width.max(1));
    }
    let width_f64 = f64::from(width.max(1));
    let height_f64 = f64::from(height.max(1));
    let radians = f64::from(absolute).to_radians();
    let sin = radians.sin().abs();
    let cos = radians.cos().abs();
    let (long_side, short_side) = if width_f64 >= height_f64 {
        (width_f64, height_f64)
    } else {
        (height_f64, width_f64)
    };
    let (safe_width, safe_height) =
        if short_side <= 2.0 * sin * cos * long_side || (sin - cos).abs() < f64::EPSILON {
            let side = 0.5 * short_side;
            if width_f64 >= height_f64 {
                (side / sin, side / cos)
            } else {
                (side / cos, side / sin)
            }
        } else {
            let cos_2a = cos * cos - sin * sin;
            (
                (width_f64 * cos - height_f64 * sin) / cos_2a,
                (height_f64 * cos - width_f64 * sin) / cos_2a,
            )
        };
    (
        safe_width.floor().max(1.0).min(width_f64).round() as u32,
        safe_height.floor().max(1.0).min(height_f64).round() as u32,
    )
}

fn axis_positions(maximum: usize, stride: usize) -> Vec<usize> {
    let mut positions = (0..=maximum).step_by(stride.max(1)).collect::<Vec<_>>();
    if positions.last().copied() != Some(maximum) {
        positions.push(maximum);
    }
    positions
}

fn binomial_blur(values: &[f64], width: usize, height: usize) -> Vec<f64> {
    let weights = [1.0, 4.0, 6.0, 4.0, 1.0];
    let mut horizontal = vec![0.0; values.len()];
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0;
            for (offset, weight) in (-2isize..=2).zip(weights) {
                let sample_x =
                    (x as isize + offset).clamp(0, width.saturating_sub(1) as isize) as usize;
                sum += values[y * width + sample_x] * weight;
            }
            horizontal[y * width + x] = sum / 16.0;
        }
    }
    let mut vertical = vec![0.0; values.len()];
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0;
            for (offset, weight) in (-2isize..=2).zip(weights) {
                let sample_y =
                    (y as isize + offset).clamp(0, height.saturating_sub(1) as isize) as usize;
                sum += horizontal[sample_y * width + x] * weight;
            }
            vertical[y * width + x] = sum / 16.0;
        }
    }
    vertical
}

fn dilate_three_by_three(values: &[f64], width: usize, height: usize) -> Vec<f64> {
    let mut dilated = vec![0.0; values.len()];
    for y in 0..height {
        for x in 0..width {
            let mut maximum: f64 = 0.0;
            for sample_y in y.saturating_sub(1)..=(y + 1).min(height.saturating_sub(1)) {
                for sample_x in x.saturating_sub(1)..=(x + 1).min(width.saturating_sub(1)) {
                    maximum = maximum.max(values[sample_y * width + sample_x]);
                }
            }
            dilated[y * width + x] = maximum;
        }
    }
    dilated
}

fn scharr_edges(values: &[f64], width: usize, height: usize) -> Vec<f64> {
    let mut edges = vec![0.0; values.len()];
    if width < 2 || height < 2 {
        return edges;
    }
    for y in 0..height {
        let top = y.saturating_sub(1);
        let bottom = (y + 1).min(height - 1);
        for x in 0..width {
            let left = x.saturating_sub(1);
            let right = (x + 1).min(width - 1);
            let top_left = values[top * width + left];
            let top_right = values[top * width + right];
            let left_value = values[y * width + left];
            let right_value = values[y * width + right];
            let bottom_left = values[bottom * width + left];
            let bottom_right = values[bottom * width + right];
            let top_value = values[top * width + x];
            let bottom_value = values[bottom * width + x];
            let gradient_x = 3.0 * (top_right - top_left)
                + 10.0 * (right_value - left_value)
                + 3.0 * (bottom_right - bottom_left);
            let gradient_y = 3.0 * (bottom_left - top_left)
                + 10.0 * (bottom_value - top_value)
                + 3.0 * (bottom_right - top_right);
            edges[y * width + x] = gradient_x.hypot(gradient_y) / 16.0;
        }
    }
    edges
}

fn luminance_percentiles(values: &[f64]) -> (f64, f64, f64) {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    (
        percentile(&sorted, 0.10),
        percentile(&sorted, 0.90),
        percentile(&sorted, 0.99),
    )
}

fn percentile(sorted: &[f64], percentile: f64) -> f64 {
    let index = ((sorted.len().saturating_sub(1)) as f64 * percentile).round() as usize;
    sorted[index.min(sorted.len().saturating_sub(1))]
}

struct IntegralImage {
    width: usize,
    values: Vec<f64>,
}

impl IntegralImage {
    fn new(source: &[f64], width: usize, height: usize) -> Self {
        let integral_width = width + 1;
        let mut values = vec![0.0; integral_width * (height + 1)];
        for y in 0..height {
            let mut row_sum = 0.0;
            for x in 0..width {
                row_sum += source[y * width + x];
                values[(y + 1) * integral_width + x + 1] =
                    values[y * integral_width + x + 1] + row_sum;
            }
        }
        Self {
            width: integral_width,
            values,
        }
    }

    fn sum(&self, x: usize, y: usize, width: usize, height: usize) -> f64 {
        let right = x + width;
        let bottom = y + height;
        self.values[bottom * self.width + right]
            - self.values[y * self.width + right]
            - self.values[bottom * self.width + x]
            + self.values[y * self.width + x]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::retouch::RetouchCrop;
    use mini_film::DiffusionSettings;

    fn region(x: f32, y: f32, width: f32, height: f32, primary: bool) -> GalleryFocusRegion {
        GalleryFocusRegion {
            x,
            y,
            width,
            height,
            primary,
        }
    }

    #[test]
    fn camera_focus_is_transformed_through_quarter_turn_and_crop() {
        let retouch = RetouchSettings {
            crop: Some(RetouchCrop {
                x: 0.0,
                y: 0.0,
                width: 0.5,
                height: 1.0,
            }),
            rotation_degrees: 90.0,
            ..RetouchSettings::default()
        };
        let geometry = RetouchGeometry::new(1200, 800, 400, 600, &retouch);
        let (area, source) = select_focus_area(
            &geometry,
            &[region(0.45, 0.65, 0.10, 0.10, true)],
            120,
            400,
            600,
        );
        assert_eq!(source, ReviewDiffusionFocusSource::CameraFocus);
        assert_eq!((area.x, area.y), (180, 240));
    }

    #[test]
    fn invisible_camera_focus_uses_center_fallback() {
        let retouch = RetouchSettings {
            crop: Some(RetouchCrop {
                x: 0.5,
                y: 0.0,
                width: 0.5,
                height: 1.0,
            }),
            ..RetouchSettings::default()
        };
        let geometry = RetouchGeometry::new(1000, 800, 500, 800, &retouch);
        let (area, source) = select_focus_area(
            &geometry,
            &[region(0.1, 0.4, 0.05, 0.05, true)],
            200,
            500,
            800,
        );
        assert_eq!(source, ReviewDiffusionFocusSource::CenterFallback);
        assert_eq!((area.x, area.y), (150, 300));
    }

    #[test]
    fn visible_primary_focus_wins_then_largest_visible_region_is_used() {
        let geometry = RetouchGeometry::new(1000, 800, 1000, 800, &RetouchSettings::default());
        let regions = [
            region(0.15, 0.45, 0.05, 0.05, true),
            region(0.65, 0.35, 0.20, 0.20, false),
        ];
        let (primary, source) = select_focus_area(&geometry, &regions, 200, 1000, 800);
        assert_eq!(source, ReviewDiffusionFocusSource::CameraFocus);
        assert!(primary.x < 200, "primary={primary:?}");

        let retouch = RetouchSettings {
            crop: Some(RetouchCrop {
                x: 0.5,
                y: 0.0,
                width: 0.5,
                height: 1.0,
            }),
            ..RetouchSettings::default()
        };
        let cropped_geometry = RetouchGeometry::new(1000, 800, 500, 800, &retouch);
        let (remaining, source) = select_focus_area(&cropped_geometry, &regions, 200, 500, 800);
        assert_eq!(source, ReviewDiffusionFocusSource::CameraFocus);
        assert!(remaining.x > 100, "remaining={remaining:?}");
    }

    #[test]
    fn camera_focus_stays_aligned_through_arbitrary_rotation_safe_crop() {
        let retouch = RetouchSettings {
            crop: Some(RetouchCrop {
                x: 0.25,
                y: 0.25,
                width: 0.5,
                height: 0.5,
            }),
            rotation_degrees: 17.0,
            ..RetouchSettings::default()
        };
        let geometry = RetouchGeometry::new(1000, 800, 400, 320, &retouch);
        let (area, source) = select_focus_area(
            &geometry,
            &[region(0.475, 0.475, 0.05, 0.05, true)],
            100,
            400,
            320,
        );
        assert_eq!(source, ReviewDiffusionFocusSource::CameraFocus);
        assert_eq!((area.x, area.y), (150, 109));
    }

    #[test]
    fn high_contrast_and_broad_highlights_select_distinct_regions() {
        let mut image = RgbImage::from_pixel(1200, 800, image::Rgb([18, 18, 18]));
        for y in 80..360 {
            for x in 80..360 {
                image.put_pixel(x, y, image::Rgb([235, 235, 235]));
            }
        }
        for y in 220..620 {
            for x in 760..1160 {
                image.put_pixel(x, y, image::Rgb([210, 210, 210]));
            }
        }
        for y in 80..360 {
            for x in (80..360).step_by(12) {
                image.put_pixel(x, y, image::Rgb([12, 12, 12]));
            }
        }
        let candidates = analyze_candidates(&image, 256).unwrap();
        let focus = area_around(
            ReviewDiffusionDetailAreaKind::Focus,
            600.0,
            400.0,
            256,
            1200,
            800,
        );
        let contrast =
            select_candidate(&candidates, CandidateScore::HighContrast, &[focus]).unwrap();
        let broad = select_candidate(
            &candidates,
            CandidateScore::BroadHighlight,
            &[focus, contrast],
        )
        .unwrap();
        assert!(contrast.x < 500, "contrast={contrast:?}");
        assert!(broad.x > 600, "broad={broad:?}");
        assert!(overlap_fraction(contrast, broad) <= 0.25);
    }

    #[test]
    fn uniform_image_produces_bounded_deterministic_areas() {
        let image = RgbImage::from_pixel(900, 600, image::Rgb([96, 96, 96]));
        let first = analyze_candidates(&image, detail_crop_side(900, 600)).unwrap();
        let second = analyze_candidates(&image, detail_crop_side(900, 600)).unwrap();
        let focus = centered_area(
            ReviewDiffusionDetailAreaKind::Focus,
            detail_crop_side(900, 600),
            900,
            600,
        );
        let first_area = select_candidate(&first, CandidateScore::HighContrast, &[focus]).unwrap();
        let second_area =
            select_candidate(&second, CandidateScore::HighContrast, &[focus]).unwrap();
        assert_eq!(first_area, second_area);
        assert!(first_area.x + first_area.width <= 900);
        assert!(first_area.y + first_area.height <= 600);
    }

    #[test]
    fn cache_identity_rejects_changed_focus_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let before = temp.path().join("before.png");
        let base = temp.path().join("base.png");
        let source = RgbImage::from_pixel(800, 600, image::Rgb([100, 100, 100]));
        source.save(&before).unwrap();
        source.save(&base).unwrap();
        let first_regions = [region(0.1, 0.1, 0.05, 0.05, true)];
        let second_regions = [region(0.8, 0.8, 0.05, 0.05, true)];
        let first = load_or_analyze_diffusion_preview_details(
            &before,
            &base,
            temp.path(),
            &first_regions,
            &RetouchSettings::default(),
        )
        .unwrap();
        let second = load_or_analyze_diffusion_preview_details(
            &before,
            &base,
            temp.path(),
            &second_regions,
            &RetouchSettings::default(),
        )
        .unwrap();
        assert_ne!(first.detail_areas[0], second.detail_areas[0]);
    }

    #[test]
    fn detail_area_json_uses_frontend_contract_names() {
        let job = ReviewDiffusionJob {
            id: 1,
            status: ReviewDiffusionJobStatus::Processing,
            image_id: 2,
            profile_index: 3,
            settings: DiffusionSettings::default(),
            before_url: Some("diffusion-preview/1/before".to_string()),
            after_url: None,
            preview_width: Some(1200),
            preview_height: Some(800),
            focus_source: Some(ReviewDiffusionFocusSource::CameraFocus),
            detail_areas: vec![ReviewDiffusionDetailArea {
                kind: ReviewDiffusionDetailAreaKind::HighContrastHighlight,
                x: 10,
                y: 20,
                width: 256,
                height: 256,
            }],
            error: None,
            before_path: None,
            after_path: None,
        };
        let json = serde_json::to_value(job).unwrap();
        assert_eq!(json["focus_source"], "camera-focus");
        assert_eq!(json["detail_areas"][0]["kind"], "high-contrast-highlight");
        assert!(json.get("before_path").is_none());
    }
}
