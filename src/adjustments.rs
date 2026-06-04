use crate::model::{ParametricTone, ProfileAdjustments, ToneCurves};

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

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

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
