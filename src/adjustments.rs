use crate::model::{ParametricTone, ProfileAdjustments, ToneCurves};

/// Bake Lightroom-style profile adjustments into one RGB sample.
///
/// The Hald writer calls this for every sampled CLUT coordinate. The function
/// converts 16-bit integer RGB into normalized floats, applies exposure first
/// as a power-of-two scale, then applies tone, curves, and color controls in a
/// stable order that roughly follows Lightroom's perceptual pipeline. The
/// result is clamped and quantized back to 16-bit because the generated Hald PNG
/// is the reusable LUT consumed later by ImageMagick/GraphicsMagick.
pub(crate) fn apply_profile_adjustments(
    rgb: [u16; 3],
    adjustments: &ProfileAdjustments,
) -> [u16; 3] {
    if adjustments.is_default() {
        return rgb;
    }

    let mut color = [
        rgb[0] as f32 / 65535.0,
        rgb[1] as f32 / 65535.0,
        rgb[2] as f32 / 65535.0,
    ];

    if adjustments.exposure != 0.0 {
        let scale = 2.0f32.powf(adjustments.exposure);
        color = color.map(|v| (v * scale).clamp(0.0, 1.0));
    }

    color = apply_basic_tone(color, adjustments);
    color = apply_tone_curves(color, &adjustments.tone_curve);
    color = apply_color_adjustments(color, adjustments);

    [
        (color[0].clamp(0.0, 1.0) * 65535.0).round() as u16,
        (color[1].clamp(0.0, 1.0) * 65535.0).round() as u16,
        (color[2].clamp(0.0, 1.0) * 65535.0).round() as u16,
    ]
}

/// Apply scalar tone controls that mostly operate through luminance.
///
/// Highlights/shadows and whites/blacks are weighted by smooth luminance masks
/// so they affect the intended tonal regions instead of the whole RGB triplet.
/// Parametric curve controls add another luma delta based on split points, while
/// contrast and clarity are approximated as centered gain around mid-gray. This
/// is deliberately local and deterministic because it runs while generating the
/// LUT, not while processing each final image pixel.
fn apply_basic_tone(mut color: [f32; 3], adjustments: &ProfileAdjustments) -> [f32; 3] {
    let mut current_luma = luma(color);

    if adjustments.highlights != 0.0 || adjustments.shadows != 0.0 {
        let highlight_weight = smoothstep(0.45, 1.0, current_luma);
        let shadow_weight = 1.0 - smoothstep(0.0, 0.55, current_luma);
        let delta = adjustments.highlights / 100.0 * 0.32 * highlight_weight
            + adjustments.shadows / 100.0 * 0.32 * shadow_weight;
        color = add_luma_delta(color, delta);
        current_luma = luma(color);
    }

    if adjustments.whites != 0.0 || adjustments.blacks != 0.0 {
        let white_weight = smoothstep(0.70, 1.0, current_luma);
        let black_weight = 1.0 - smoothstep(0.0, 0.30, current_luma);
        let delta = adjustments.whites / 100.0 * 0.22 * white_weight
            + adjustments.blacks / 100.0 * 0.22 * black_weight;
        color = add_luma_delta(color, delta);
        current_luma = luma(color);
    }

    let parametric_delta = parametric_delta(current_luma, adjustments.parametric);
    if parametric_delta != 0.0 {
        color = add_luma_delta(color, parametric_delta);
    }

    if adjustments.contrast != 0.0 {
        let factor = 1.0 + adjustments.contrast / 100.0;
        color = color.map(|v| ((v - 0.5) * factor + 0.5).clamp(0.0, 1.0));
    }

    if adjustments.clarity != 0.0 {
        let factor = 1.0 + adjustments.clarity / 180.0;
        color = color.map(|v| ((v - 0.5) * factor + 0.5).clamp(0.0, 1.0));
    }

    color
}

/// Apply composite and per-channel tone curves.
///
/// Lightroom stores tone curves as 0..255 control points. The composite curve is
/// applied to all channels first, then red/green/blue curves are applied to
/// their respective channels. Identity curves are skipped so empty or no-op XMP
/// data does not add sorting/interpolation work during Hald generation.
fn apply_tone_curves(mut color: [f32; 3], curves: &ToneCurves) -> [f32; 3] {
    if !curve_is_identity(&curves.composite) {
        color = color.map(|v| apply_curve(v, &curves.composite));
    }
    if !curve_is_identity(&curves.red) {
        color[0] = apply_curve(color[0], &curves.red);
    }
    if !curve_is_identity(&curves.green) {
        color[1] = apply_curve(color[1], &curves.green);
    }
    if !curve_is_identity(&curves.blue) {
        color[2] = apply_curve(color[2], &curves.blue);
    }
    color
}

/// Apply saturation, vibrance, HSL, and camera calibration adjustments.
///
/// The algorithm moves into HSL because these controls are hue-relative rather
/// than simple per-channel gains. Global saturation is linear, vibrance is
/// stronger on less-saturated colors, HSL sliders are blended around Lightroom's
/// eight hue centers, and calibration shifts use broader primary-centered hue
/// weights. The final HSL value is normalized and converted back to RGB.
fn apply_color_adjustments(color: [f32; 3], adjustments: &ProfileAdjustments) -> [f32; 3] {
    let (mut hue, mut saturation, mut lightness) = rgb_to_hsl(color);

    if adjustments.saturation != 0.0 {
        saturation *= 1.0 + adjustments.saturation / 100.0;
    }
    if adjustments.vibrance != 0.0 {
        saturation *= 1.0 + adjustments.vibrance / 100.0 * (1.0 - saturation);
    }

    let hsl_centers = [0.0, 30.0, 60.0, 120.0, 180.0, 240.0, 280.0, 320.0];
    for (index, center) in hsl_centers.iter().enumerate() {
        let weight = hue_weight(hue, *center, 45.0);
        if weight == 0.0 {
            continue;
        }
        hue += adjustments.hsl.hue[index] * 0.30 * weight;
        saturation *= 1.0 + adjustments.hsl.saturation[index] / 100.0 * weight;
        lightness += adjustments.hsl.luminance[index] / 100.0 * 0.22 * weight;
    }

    let calibration = [
        (
            0.0,
            adjustments.calibration.red_hue,
            adjustments.calibration.red_saturation,
        ),
        (
            120.0,
            adjustments.calibration.green_hue,
            adjustments.calibration.green_saturation,
        ),
        (
            240.0,
            adjustments.calibration.blue_hue,
            adjustments.calibration.blue_saturation,
        ),
    ];
    for (center, hue_shift, saturation_shift) in calibration {
        let weight = hue_weight(hue, center, 65.0);
        if weight == 0.0 {
            continue;
        }
        hue += hue_shift * 0.30 * weight;
        saturation *= 1.0 + saturation_shift / 100.0 * weight;
    }

    hsl_to_rgb(
        normalize_hue(hue),
        saturation.clamp(0.0, 1.0),
        lightness.clamp(0.0, 1.0),
    )
}

/// Compute the luma delta for Lightroom's parametric tone curve.
///
/// The three split points divide the luminance range into shadows, darks,
/// lights, and highlights. Edge regions use smoothstep ramps and middle regions
/// use triangular weights so adjacent controls blend rather than creating hard
/// discontinuities in the generated LUT.
fn parametric_delta(luma: f32, tone: ParametricTone) -> f32 {
    let s1 = (tone.shadow_split / 100.0).clamp(0.01, 0.99);
    let s2 = (tone.midtone_split / 100.0).clamp(s1 + 0.01, 0.99);
    let s3 = (tone.highlight_split / 100.0).clamp(s2 + 0.01, 0.99);
    let shadow = (1.0 - smoothstep(0.0, s1, luma)) * tone.shadows;
    let dark = triangle_weight(luma, s1 * 0.5, s2, s1) * tone.darks;
    let light = triangle_weight(luma, s1, s3, s2) * tone.lights;
    let highlight = smoothstep(s3, 1.0, luma) * tone.highlights;
    (shadow + dark + light + highlight) / 100.0 * 0.22
}

/// Interpolate a Lightroom tone curve at one normalized channel value.
///
/// XMP curve points are stored in 0..255 space, so the input is scaled to that
/// domain, the points are sorted by x, and the surrounding segment is linearly
/// interpolated. Values outside the first/last point are pinned to the nearest
/// endpoint, matching the usual behavior of point curves.
fn apply_curve(value: f32, points: &[(f32, f32)]) -> f32 {
    if points.is_empty() {
        return value;
    }

    let x = (value * 255.0).clamp(0.0, 255.0);
    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| a.0.total_cmp(&b.0));

    if x <= sorted[0].0 {
        return (sorted[0].1 / 255.0).clamp(0.0, 1.0);
    }
    for pair in sorted.windows(2) {
        let (x0, y0) = pair[0];
        let (x1, y1) = pair[1];
        if x <= x1 {
            let t = if x1 == x0 { 0.0 } else { (x - x0) / (x1 - x0) };
            return ((y0 + (y1 - y0) * t) / 255.0).clamp(0.0, 1.0);
        }
    }
    (sorted.last().unwrap().1 / 255.0).clamp(0.0, 1.0)
}

/// Detect whether a curve can be skipped.
///
/// Empty curves and curves whose points all lie on y=x leave values unchanged.
/// This matters because adjustment baking calls the curve path for every Hald
/// sample, and avoiding unnecessary interpolation keeps generation predictable.
pub(crate) fn curve_is_identity(points: &[(f32, f32)]) -> bool {
    points.is_empty()
        || points
            .iter()
            .all(|(input, output)| (*input - *output).abs() <= f32::EPSILON)
}

fn add_luma_delta(color: [f32; 3], delta: f32) -> [f32; 3] {
    color.map(|v| (v + delta).clamp(0.0, 1.0))
}

fn luma(color: [f32; 3]) -> f32 {
    0.2126 * color[0] + 0.7152 * color[1] + 0.0722 * color[2]
}

/// Smoothly ramp from zero to one between two edges.
///
/// The cubic Hermite shape avoids hard transitions at tonal-mask boundaries by
/// giving the ramp zero slope at both ends. It is used for highlight/shadow masks
/// and parametric tone regions.
fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Compute a triangular region weight around a center point.
///
/// Values outside the left/right bounds receive no weight. Values inside ramp up
/// toward the center and down after it, which makes darks/lights parametric
/// controls overlap smoothly with neighboring tonal regions.
fn triangle_weight(value: f32, left: f32, right: f32, center: f32) -> f32 {
    if value <= left || value >= right {
        return 0.0;
    }
    if value <= center {
        ((value - left) / (center - left)).clamp(0.0, 1.0)
    } else {
        ((right - value) / (right - center)).clamp(0.0, 1.0)
    }
}

/// Convert normalized RGB to HSL.
///
/// The conversion computes lightness from the min/max RGB channels, derives
/// saturation from chroma relative to lightness, and selects the hue sector from
/// whichever channel is dominant. Hue is normalized into degrees so HSL sliders
/// and calibration weights can operate on a circular color wheel.
fn rgb_to_hsl(rgb: [f32; 3]) -> (f32, f32, f32) {
    let r = rgb[0];
    let g = rgb[1];
    let b = rgb[2];
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let lightness = (max + min) * 0.5;
    let delta = max - min;

    if delta == 0.0 {
        return (0.0, 0.0, lightness);
    }

    let saturation = if lightness > 0.5 {
        delta / (2.0 - max - min)
    } else {
        delta / (max + min)
    };
    let hue = if max == r {
        60.0 * ((g - b) / delta + if g < b { 6.0 } else { 0.0 })
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };

    (normalize_hue(hue), saturation, lightness)
}

/// Convert normalized HSL back to RGB.
///
/// The implementation uses the standard p/q helper formulation. Saturation zero
/// is a grayscale fast path; otherwise each RGB channel samples the hue wheel at
/// offsets one third turn apart and returns normalized channel values.
fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> [f32; 3] {
    if saturation == 0.0 {
        return [lightness; 3];
    }

    let q = if lightness < 0.5 {
        lightness * (1.0 + saturation)
    } else {
        lightness + saturation - lightness * saturation
    };
    let p = 2.0 * lightness - q;
    [
        hue_to_rgb(p, q, hue / 360.0 + 1.0 / 3.0),
        hue_to_rgb(p, q, hue / 360.0),
        hue_to_rgb(p, q, hue / 360.0 - 1.0 / 3.0),
    ]
}

/// Resolve one channel from the HSL hue helper curve.
///
/// The input `t` is wrapped into 0..1 and then evaluated across the three linear
/// hue segments used by the standard HSL-to-RGB conversion. This is small but
/// mathematically sensitive because off-by-one wrapping would cause hue seams.
fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 1.0 / 2.0 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

fn hue_weight(hue: f32, center: f32, width: f32) -> f32 {
    let distance = hue_distance(hue, center);
    (1.0 - distance / width).clamp(0.0, 1.0)
}

fn hue_distance(a: f32, b: f32) -> f32 {
    let diff = (normalize_hue(a) - normalize_hue(b)).abs();
    diff.min(360.0 - diff)
}

fn normalize_hue(hue: f32) -> f32 {
    hue.rem_euclid(360.0)
}
