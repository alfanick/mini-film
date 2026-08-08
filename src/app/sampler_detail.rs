use std::cmp::Ordering;

use image::{RgbImage, imageops::FilterType};
use serde::{Deserialize, Serialize};

use crate::app::timestamps::GalleryFocusRegion;

pub(crate) const SAMPLER_DETAIL_ANALYSIS_VERSION: &str = "sampler-detail-areas-v2";

pub(crate) const ANALYSIS_LONG_EDGE: u32 = 640;
const WINDOW_FRACTION: f64 = 0.24;
const MIN_WINDOW_SIDE: u32 = 24;
const MAX_WINDOW_SIDE: u32 = 192;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SamplerDetailKind {
    Focus,
    Highlights,
    Shadows,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct SamplerDetailArea {
    pub(crate) kind: SamplerDetailKind,
    pub(crate) center_x: f32,
    pub(crate) center_y: f32,
}

impl SamplerDetailArea {
    fn is_valid(self) -> bool {
        self.center_x.is_finite()
            && self.center_y.is_finite()
            && (0.0..=1.0).contains(&self.center_x)
            && (0.0..=1.0).contains(&self.center_y)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct SamplerDetailAnalysis {
    pub(crate) areas: Vec<SamplerDetailArea>,
}

impl SamplerDetailAnalysis {
    pub(crate) fn is_valid(&self) -> bool {
        let expected = [
            SamplerDetailKind::Focus,
            SamplerDetailKind::Highlights,
            SamplerDetailKind::Shadows,
        ];
        self.areas.len() == expected.len()
            && self
                .areas
                .iter()
                .zip(expected)
                .all(|(area, kind)| area.kind == kind && area.is_valid())
    }
}

#[derive(Clone, Copy, Debug)]
struct Candidate {
    area: PixelArea,
    highlights_score: f64,
    shadows_score: f64,
    frame_center_distance: f64,
}

#[derive(Clone, Copy, Debug)]
struct PixelArea {
    x: u32,
    y: u32,
    side: u32,
}

impl PixelArea {
    fn center(self, width: u32, height: u32, kind: SamplerDetailKind) -> SamplerDetailArea {
        SamplerDetailArea {
            kind,
            center_x: ((f64::from(self.x) + f64::from(self.side) / 2.0) / f64::from(width.max(1)))
                .clamp(0.0, 1.0) as f32,
            center_y: ((f64::from(self.y) + f64::from(self.side) / 2.0) / f64::from(height.max(1)))
                .clamp(0.0, 1.0) as f32,
        }
    }
}

#[derive(Clone, Copy)]
enum CandidateScore {
    Highlights,
    Shadows,
}

/// Locates representative focus, highlight, and shadow detail in a rendered
/// neutral image. Returned centers are normalized and can be reused across
/// differently sized renderings of the same framing.
pub(crate) fn analyze_sampler_detail_areas(
    image: &RgbImage,
    focus_regions: &[GalleryFocusRegion],
) -> SamplerDetailAnalysis {
    let (source_width, source_height) = image.dimensions();
    if source_width == 0 || source_height == 0 {
        return centered_analysis();
    }

    let scale =
        (f64::from(ANALYSIS_LONG_EDGE) / f64::from(source_width.max(source_height))).min(1.0);
    let width = (f64::from(source_width) * scale).round().max(1.0) as u32;
    let height = (f64::from(source_height) * scale).round().max(1.0) as u32;
    let proxy = if (width, height) == (source_width, source_height) {
        image.clone()
    } else {
        image::imageops::resize(image, width, height, FilterType::Triangle)
    };
    let side = analysis_window_side(width, height);
    let (focus, focus_center) = focus_area(focus_regions, width, height, side);
    let candidates = analyze_candidates(&proxy, side);
    let highlights = select_candidate(&candidates, CandidateScore::Highlights, &[focus])
        .unwrap_or_else(|| centered_pixel_area(width, height, side));
    let shadows = select_candidate(&candidates, CandidateScore::Shadows, &[focus, highlights])
        .unwrap_or_else(|| centered_pixel_area(width, height, side));

    let analysis = SamplerDetailAnalysis {
        areas: vec![
            focus_center,
            highlights.center(width, height, SamplerDetailKind::Highlights),
            shadows.center(width, height, SamplerDetailKind::Shadows),
        ],
    };
    debug_assert!(analysis.is_valid());
    analysis
}

fn centered_analysis() -> SamplerDetailAnalysis {
    SamplerDetailAnalysis {
        areas: vec![
            normalized_center(SamplerDetailKind::Focus),
            normalized_center(SamplerDetailKind::Highlights),
            normalized_center(SamplerDetailKind::Shadows),
        ],
    }
}

fn normalized_center(kind: SamplerDetailKind) -> SamplerDetailArea {
    SamplerDetailArea {
        kind,
        center_x: 0.5,
        center_y: 0.5,
    }
}

fn analysis_window_side(width: u32, height: u32) -> u32 {
    let short_edge = width.min(height).max(1);
    ((f64::from(short_edge) * WINDOW_FRACTION).round() as u32)
        .clamp(MIN_WINDOW_SIDE, MAX_WINDOW_SIDE)
        .min(short_edge)
}

fn focus_area(
    focus_regions: &[GalleryFocusRegion],
    width: u32,
    height: u32,
    side: u32,
) -> (PixelArea, SamplerDetailArea) {
    let selected = focus_regions
        .iter()
        .filter_map(|region| normalized_focus_bounds(*region))
        .max_by(compare_focus_regions);
    let (center_x, center_y) = selected
        .map(|region| {
            (
                (region.left + region.right) / 2.0,
                (region.top + region.bottom) / 2.0,
            )
        })
        .unwrap_or((0.5, 0.5));
    (
        pixel_area_around(
            center_x * f64::from(width),
            center_y * f64::from(height),
            width,
            height,
            side,
        ),
        SamplerDetailArea {
            kind: SamplerDetailKind::Focus,
            center_x: center_x as f32,
            center_y: center_y as f32,
        },
    )
}

#[derive(Clone, Copy, Debug)]
struct FocusBounds {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
    primary: bool,
}

impl FocusBounds {
    fn area(self) -> f64 {
        (self.right - self.left) * (self.bottom - self.top)
    }
}

fn normalized_focus_bounds(region: GalleryFocusRegion) -> Option<FocusBounds> {
    let values = [region.x, region.y, region.width, region.height];
    if values.iter().any(|value| !value.is_finite()) || region.width <= 0.0 || region.height <= 0.0
    {
        return None;
    }
    let left = f64::from(region.x).clamp(0.0, 1.0);
    let top = f64::from(region.y).clamp(0.0, 1.0);
    let right = f64::from(region.x + region.width).clamp(0.0, 1.0);
    let bottom = f64::from(region.y + region.height).clamp(0.0, 1.0);
    (right > left && bottom > top).then_some(FocusBounds {
        left,
        top,
        right,
        bottom,
        primary: region.primary,
    })
}

fn compare_focus_regions(left: &FocusBounds, right: &FocusBounds) -> Ordering {
    left.primary
        .cmp(&right.primary)
        .then_with(|| left.area().total_cmp(&right.area()))
        .then_with(|| right.top.total_cmp(&left.top))
        .then_with(|| right.left.total_cmp(&left.left))
}

fn analyze_candidates(image: &RgbImage, side: u32) -> Vec<Candidate> {
    let (width, height) = image.dimensions();
    let width_usize = width as usize;
    let height_usize = height as usize;
    let side_usize = side as usize;
    let luma = image
        .pixels()
        .map(|pixel| relative_luminance(pixel.0))
        .collect::<Vec<_>>();
    let tone = binomial_blur(&luma, width_usize, height_usize);
    let edges = scharr_edges(&tone, width_usize, height_usize);
    let (p05, p20, p80, p95) = luminance_percentiles(&tone);
    let low_span = (p20 - p05).max(0.02);
    let high_span = (p95 - p80).max(0.02);
    let tonal_span = (p95 - p05).max(0.04);
    let highlight_membership = tone
        .iter()
        .map(|value| ((*value - p80) / high_span).clamp(0.0, 1.0))
        .collect::<Vec<_>>();
    let shadow_membership = tone
        .iter()
        .map(|value| ((p20 - *value) / low_span).clamp(0.0, 1.0))
        .collect::<Vec<_>>();
    let clipped_highlights = tone
        .iter()
        .map(|value| if *value >= 0.985 { 1.0 } else { 0.0 })
        .collect::<Vec<_>>();
    let clipped_shadows = tone
        .iter()
        .map(|value| if *value <= 0.0015 { 1.0 } else { 0.0 })
        .collect::<Vec<_>>();
    let squared = tone.iter().map(|value| value * value).collect::<Vec<_>>();
    let tone_integral = IntegralImage::new(&tone, width_usize, height_usize);
    let squared_integral = IntegralImage::new(&squared, width_usize, height_usize);
    let edge_integral = IntegralImage::new(&edges, width_usize, height_usize);
    let highlight_integral = IntegralImage::new(&highlight_membership, width_usize, height_usize);
    let shadow_integral = IntegralImage::new(&shadow_membership, width_usize, height_usize);
    let clipped_highlight_integral =
        IntegralImage::new(&clipped_highlights, width_usize, height_usize);
    let clipped_shadow_integral = IntegralImage::new(&clipped_shadows, width_usize, height_usize);
    let maximum_x = width_usize.saturating_sub(side_usize);
    let maximum_y = height_usize.saturating_sub(side_usize);
    let stride = (side_usize / 5).max(1);
    let x_positions = axis_positions(maximum_x, stride);
    let y_positions = axis_positions(maximum_y, stride);
    let pixel_count = (side_usize * side_usize).max(1) as f64;
    let mut candidates = Vec::with_capacity(x_positions.len() * y_positions.len());

    for y in y_positions {
        for &x in &x_positions {
            let mean = tone_integral.sum(x, y, side_usize, side_usize) / pixel_count;
            let mean_squared = squared_integral.sum(x, y, side_usize, side_usize) / pixel_count;
            let deviation = (mean_squared - mean * mean).max(0.0).sqrt();
            let edge = edge_integral.sum(x, y, side_usize, side_usize) / pixel_count;
            let highlight_coverage =
                highlight_integral.sum(x, y, side_usize, side_usize) / pixel_count;
            let shadow_coverage = shadow_integral.sum(x, y, side_usize, side_usize) / pixel_count;
            let clipped_highlight_coverage =
                clipped_highlight_integral.sum(x, y, side_usize, side_usize) / pixel_count;
            let clipped_shadow_coverage =
                clipped_shadow_integral.sum(x, y, side_usize, side_usize) / pixel_count;
            let brightness = ((mean - p05) / tonal_span).clamp(0.0, 1.0);
            let darkness = ((p95 - mean) / tonal_span).clamp(0.0, 1.0);
            let texture = (deviation / tonal_span).clamp(0.0, 1.0);
            let edge_detail = (edge / tonal_span).clamp(0.0, 1.0);
            let detail_strength =
                ((deviation + edge) / (tonal_span * 0.20).max(0.01)).clamp(0.0, 1.0);
            let flat_highlight_penalty = highlight_coverage * (1.0 - detail_strength);
            let flat_shadow_penalty = shadow_coverage * (1.0 - detail_strength);
            let area = PixelArea {
                x: x as u32,
                y: y as u32,
                side,
            };
            candidates.push(Candidate {
                area,
                highlights_score: 0.62 * highlight_coverage
                    + 0.24 * brightness
                    + 0.09 * texture
                    + 0.05 * edge_detail
                    - 0.65 * clipped_highlight_coverage
                    - 0.18 * flat_highlight_penalty,
                shadows_score: 0.62 * shadow_coverage
                    + 0.24 * darkness
                    + 0.09 * texture
                    + 0.05 * edge_detail
                    - 0.65 * clipped_shadow_coverage
                    - 0.18 * flat_shadow_penalty,
                frame_center_distance: center_distance_from_frame(area, width, height),
            });
        }
    }
    candidates
}

fn select_candidate(
    candidates: &[Candidate],
    score: CandidateScore,
    selected: &[PixelArea],
) -> Option<PixelArea> {
    for (maximum_overlap, minimum_distance) in [(0.10, 0.8), (0.25, 0.6), (1.0, 0.0)] {
        if let Some(candidate) = candidates
            .iter()
            .filter(|candidate| {
                selected.iter().all(|selected| {
                    overlap_fraction(candidate.area, *selected) <= maximum_overlap
                        && center_distance(candidate.area, *selected)
                            >= minimum_distance * f64::from(candidate.area.side)
                })
            })
            .max_by(|left, right| compare_candidates(left, right, score, selected))
        {
            return Some(candidate.area);
        }
    }
    None
}

fn compare_candidates(
    left: &Candidate,
    right: &Candidate,
    score: CandidateScore,
    selected: &[PixelArea],
) -> Ordering {
    let left_score = match score {
        CandidateScore::Highlights => left.highlights_score,
        CandidateScore::Shadows => left.shadows_score,
    };
    let right_score = match score {
        CandidateScore::Highlights => right.highlights_score,
        CandidateScore::Shadows => right.shadows_score,
    };
    score_bucket(left_score)
        .cmp(&score_bucket(right_score))
        .then_with(|| {
            minimum_distance(left.area, selected).total_cmp(&minimum_distance(right.area, selected))
        })
        .then_with(|| {
            right
                .frame_center_distance
                .total_cmp(&left.frame_center_distance)
        })
        .then_with(|| right.area.y.cmp(&left.area.y))
        .then_with(|| right.area.x.cmp(&left.area.x))
}

fn score_bucket(score: f64) -> i64 {
    (score * 1_000_000.0).round() as i64
}

fn minimum_distance(area: PixelArea, selected: &[PixelArea]) -> f64 {
    selected
        .iter()
        .map(|selected| center_distance(area, *selected))
        .fold(f64::INFINITY, f64::min)
}

fn center_distance(left: PixelArea, right: PixelArea) -> f64 {
    let left_x = f64::from(left.x) + f64::from(left.side) / 2.0;
    let left_y = f64::from(left.y) + f64::from(left.side) / 2.0;
    let right_x = f64::from(right.x) + f64::from(right.side) / 2.0;
    let right_y = f64::from(right.y) + f64::from(right.side) / 2.0;
    (left_x - right_x).hypot(left_y - right_y)
}

fn center_distance_from_frame(area: PixelArea, width: u32, height: u32) -> f64 {
    let center_x = f64::from(area.x) + f64::from(area.side) / 2.0;
    let center_y = f64::from(area.y) + f64::from(area.side) / 2.0;
    (center_x - f64::from(width) / 2.0).hypot(center_y - f64::from(height) / 2.0)
}

fn overlap_fraction(left: PixelArea, right: PixelArea) -> f64 {
    let intersection_width = left
        .x
        .saturating_add(left.side)
        .min(right.x.saturating_add(right.side))
        .saturating_sub(left.x.max(right.x));
    let intersection_height = left
        .y
        .saturating_add(left.side)
        .min(right.y.saturating_add(right.side))
        .saturating_sub(left.y.max(right.y));
    f64::from(intersection_width) * f64::from(intersection_height)
        / f64::from(left.side.max(1)).powi(2)
}

fn centered_pixel_area(width: u32, height: u32, side: u32) -> PixelArea {
    pixel_area_around(
        f64::from(width) / 2.0,
        f64::from(height) / 2.0,
        width,
        height,
        side,
    )
}

fn pixel_area_around(
    center_x: f64,
    center_y: f64,
    width: u32,
    height: u32,
    side: u32,
) -> PixelArea {
    let side = side.min(width).min(height).max(1);
    let maximum_x = width.saturating_sub(side);
    let maximum_y = height.saturating_sub(side);
    PixelArea {
        x: (center_x - f64::from(side) / 2.0)
            .round()
            .clamp(0.0, f64::from(maximum_x)) as u32,
        y: (center_y - f64::from(side) / 2.0)
            .round()
            .clamp(0.0, f64::from(maximum_y)) as u32,
        side,
    }
}

fn axis_positions(maximum: usize, stride: usize) -> Vec<usize> {
    let mut positions = (0..=maximum).step_by(stride.max(1)).collect::<Vec<_>>();
    if positions.last().copied() != Some(maximum) {
        positions.push(maximum);
    }
    positions
}

fn relative_luminance(rgb: [u8; 3]) -> f64 {
    fn linear(channel: u8) -> f64 {
        let value = f64::from(channel) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * linear(rgb[0]) + 0.7152 * linear(rgb[1]) + 0.0722 * linear(rgb[2])
}

fn luminance_percentiles(values: &[f64]) -> (f64, f64, f64, f64) {
    if values.is_empty() {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    (
        percentile(&sorted, 0.05),
        percentile(&sorted, 0.20),
        percentile(&sorted, 0.80),
        percentile(&sorted, 0.95),
    )
}

fn percentile(sorted: &[f64], percentile: f64) -> f64 {
    let index = ((sorted.len().saturating_sub(1)) as f64 * percentile).round() as usize;
    sorted[index.min(sorted.len().saturating_sub(1))]
}

fn binomial_blur(values: &[f64], width: usize, height: usize) -> Vec<f64> {
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let weights = [1.0, 4.0, 6.0, 4.0, 1.0];
    let mut horizontal = vec![0.0; values.len()];
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0;
            for (offset, weight) in (-2isize..=2).zip(weights) {
                let sample_x = (x as isize + offset).clamp(0, width as isize - 1) as usize;
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
                let sample_y = (y as isize + offset).clamp(0, height as isize - 1) as usize;
                sum += horizontal[sample_y * width + x] * weight;
            }
            vertical[y * width + x] = sum / 16.0;
        }
    }
    vertical
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
            let gradient_x = 3.0 * (values[top * width + right] - values[top * width + left])
                + 10.0 * (values[y * width + right] - values[y * width + left])
                + 3.0 * (values[bottom * width + right] - values[bottom * width + left]);
            let gradient_y = 3.0 * (values[bottom * width + left] - values[top * width + left])
                + 10.0 * (values[bottom * width + x] - values[top * width + x])
                + 3.0 * (values[bottom * width + right] - values[top * width + right]);
            edges[y * width + x] = gradient_x.hypot(gradient_y) / 16.0;
        }
    }
    edges
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
    use image::Rgb;

    fn region(x: f32, y: f32, width: f32, height: f32, primary: bool) -> GalleryFocusRegion {
        GalleryFocusRegion {
            x,
            y,
            width,
            height,
            primary,
        }
    }

    fn area(analysis: &SamplerDetailAnalysis, kind: SamplerDetailKind) -> SamplerDetailArea {
        *analysis
            .areas
            .iter()
            .find(|area| area.kind == kind)
            .unwrap()
    }

    #[test]
    fn primary_focus_wins_then_largest_primary_region_is_used() {
        let image = RgbImage::from_pixel(400, 200, Rgb([128, 128, 128]));
        let analysis = analyze_sampler_detail_areas(
            &image,
            &[
                region(0.70, 0.10, 0.25, 0.50, false),
                region(0.75, 0.70, 0.05, 0.05, true),
                region(0.10, 0.20, 0.20, 0.20, true),
            ],
        );
        let focus = area(&analysis, SamplerDetailKind::Focus);
        assert!((focus.center_x - 0.20).abs() < 0.01);
        assert!((focus.center_y - 0.30).abs() < 0.01);
    }

    #[test]
    fn invalid_or_missing_focus_falls_back_to_exact_frame_center() {
        let image = RgbImage::from_pixel(301, 199, Rgb([128, 128, 128]));
        for regions in [
            Vec::new(),
            vec![region(f32::NAN, 0.1, 0.2, 0.2, true)],
            vec![region(0.2, 0.2, 0.0, 0.2, true)],
            vec![region(2.0, 2.0, 0.2, 0.2, true)],
        ] {
            let analysis = analyze_sampler_detail_areas(&image, &regions);
            let focus = area(&analysis, SamplerDetailKind::Focus);
            assert_eq!((focus.center_x, focus.center_y), (0.5, 0.5));
        }
    }

    #[test]
    fn bright_and_dark_textured_regions_are_selected() {
        let mut image = RgbImage::from_pixel(320, 200, Rgb([110, 110, 110]));
        for y in 16..88 {
            for x in 18..106 {
                let value = if (x + y) % 9 == 0 { 210 } else { 245 };
                image.put_pixel(x, y, Rgb([value, value, value]));
            }
        }
        for y in 112..190 {
            for x in 216..310 {
                let value = if (x + y) % 8 == 0 { 42 } else { 12 };
                image.put_pixel(x, y, Rgb([value, value, value]));
            }
        }
        let analysis = analyze_sampler_detail_areas(&image, &[]);
        let highlights = area(&analysis, SamplerDetailKind::Highlights);
        let shadows = area(&analysis, SamplerDetailKind::Shadows);
        assert!(highlights.center_x < 0.4 && highlights.center_y < 0.55);
        assert!(shadows.center_x > 0.6 && shadows.center_y > 0.5);
    }

    #[test]
    fn textured_highlights_beat_a_flat_clipped_white_patch() {
        let mut image = RgbImage::from_pixel(420, 220, Rgb([105, 105, 105]));
        for y in 20..120 {
            for x in 15..135 {
                image.put_pixel(x, y, Rgb([255, 255, 255]));
            }
        }
        for y in 20..120 {
            for x in 285..405 {
                let value = if (x / 7 + y / 7) % 2 == 0 { 245 } else { 205 };
                image.put_pixel(x, y, Rgb([value, value, value]));
            }
        }

        let first = analyze_sampler_detail_areas(&image, &[]);
        let second = analyze_sampler_detail_areas(&image, &[]);
        let highlights = area(&first, SamplerDetailKind::Highlights);
        assert_eq!(first, second);
        assert!(highlights.center_x > 0.6, "selected {highlights:?}");
        assert!(highlights.center_y < 0.65, "selected {highlights:?}");
    }

    #[test]
    fn textured_shadows_beat_a_flat_clipped_black_patch() {
        let mut image = RgbImage::from_pixel(420, 220, Rgb([150, 150, 150]));
        for y in 100..205 {
            for x in 15..135 {
                image.put_pixel(x, y, Rgb([0, 0, 0]));
            }
        }
        for y in 100..205 {
            for x in 285..405 {
                let value = if (x / 7 + y / 7) % 2 == 0 { 45 } else { 10 };
                image.put_pixel(x, y, Rgb([value, value, value]));
            }
        }

        let first = analyze_sampler_detail_areas(&image, &[]);
        let second = analyze_sampler_detail_areas(&image, &[]);
        let shadows = area(&first, SamplerDetailKind::Shadows);
        assert_eq!(first, second);
        assert!(shadows.center_x > 0.6, "selected {shadows:?}");
        assert!(shadows.center_y > 0.35, "selected {shadows:?}");
    }

    #[test]
    fn normal_images_produce_distinct_bounded_serializable_areas() {
        let mut image = RgbImage::new(400, 260);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            let value = ((x * 3 + y * 5) % 256) as u8;
            *pixel = Rgb([value, value.saturating_add((x % 31) as u8), value / 2]);
        }
        let analysis =
            analyze_sampler_detail_areas(&image, &[region(0.42, 0.42, 0.08, 0.08, true)]);
        assert!(analysis.is_valid());
        for (index, left) in analysis.areas.iter().enumerate() {
            for right in analysis.areas.iter().skip(index + 1) {
                let distance = f64::from(left.center_x - right.center_x)
                    .hypot(f64::from(left.center_y - right.center_y));
                assert!(distance > 0.08);
            }
        }
        let decoded: SamplerDetailAnalysis =
            serde_json::from_slice(&serde_json::to_vec(&analysis).unwrap()).unwrap();
        assert_eq!(decoded, analysis);
        assert!(decoded.is_valid());
    }

    #[test]
    fn portrait_analysis_is_bounded_and_deterministic() {
        let mut image = RgbImage::new(90, 300);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            let value = ((x * 7 + y * 2) % 256) as u8;
            *pixel = Rgb([value, value, value]);
        }
        let first = analyze_sampler_detail_areas(&image, &[]);
        let second = analyze_sampler_detail_areas(&image, &[]);
        assert_eq!(first, second);
        assert!(first.is_valid());
    }

    #[test]
    fn tiny_and_uniform_images_are_deterministic() {
        for (width, height) in [(1, 1), (2, 3), (37, 19), (200, 120)] {
            let image = RgbImage::from_pixel(width, height, Rgb([77, 77, 77]));
            let first = analyze_sampler_detail_areas(&image, &[]);
            let second = analyze_sampler_detail_areas(&image, &[]);
            assert_eq!(first, second);
            assert!(first.is_valid());
        }
    }

    #[test]
    fn invalid_deserialized_analysis_is_rejected() {
        let invalid = SamplerDetailAnalysis {
            areas: vec![SamplerDetailArea {
                kind: SamplerDetailKind::Focus,
                center_x: 1.1,
                center_y: 0.5,
            }],
        };
        assert!(!invalid.is_valid());
    }
}
