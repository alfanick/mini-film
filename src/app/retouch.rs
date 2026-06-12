use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use mini_film::ProfileAdjustments;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub(crate) struct BasicRetouchAdjustments {
    #[serde(default)]
    pub(crate) exposure: f32,
    #[serde(default)]
    pub(crate) highlights: f32,
    #[serde(default)]
    pub(crate) shadows: f32,
    #[serde(default)]
    pub(crate) whites: f32,
    #[serde(default)]
    pub(crate) blacks: f32,
    #[serde(default)]
    pub(crate) temperature: f32,
    #[serde(default)]
    pub(crate) offset: f32,
    #[serde(default)]
    pub(crate) clarity: f32,
}

impl BasicRetouchAdjustments {
    pub(crate) fn from_profile_adjustments(adjustments: &ProfileAdjustments) -> Self {
        Self {
            exposure: adjustments.exposure,
            highlights: adjustments.highlights,
            shadows: adjustments.shadows,
            whites: adjustments.whites,
            blacks: adjustments.blacks,
            temperature: 0.0,
            offset: 0.0,
            clarity: adjustments.clarity,
        }
    }

    pub(crate) fn add(self, other: Self) -> Self {
        Self {
            exposure: self.exposure + other.exposure,
            highlights: self.highlights + other.highlights,
            shadows: self.shadows + other.shadows,
            whites: self.whites + other.whites,
            blacks: self.blacks + other.blacks,
            temperature: self.temperature + other.temperature,
            offset: self.offset + other.offset,
            clarity: self.clarity + other.clarity,
        }
    }

    pub(crate) fn is_default(self) -> bool {
        self.exposure == 0.0
            && self.highlights == 0.0
            && self.shadows == 0.0
            && self.whites == 0.0
            && self.blacks == 0.0
            && self.temperature == 0.0
            && self.offset == 0.0
            && self.clarity == 0.0
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct RetouchCrop {
    #[serde(default)]
    pub(crate) x: f32,
    #[serde(default)]
    pub(crate) y: f32,
    #[serde(default = "full_extent")]
    pub(crate) width: f32,
    #[serde(default = "full_extent")]
    pub(crate) height: f32,
}

fn full_extent() -> f32 {
    1.0
}

impl RetouchCrop {
    pub(crate) fn normalized(self) -> Option<Self> {
        let width = self.width.clamp(0.01, 1.0);
        let height = self.height.clamp(0.01, 1.0);
        let x = self.x.clamp(0.0, 1.0 - width);
        let y = self.y.clamp(0.0, 1.0 - height);
        let crop = Self {
            x,
            y,
            width,
            height,
        };
        (!crop.is_full_frame()).then_some(crop)
    }

    pub(crate) fn is_full_frame(self) -> bool {
        self.x.abs() <= 0.0001
            && self.y.abs() <= 0.0001
            && (self.width - 1.0).abs() <= 0.0001
            && (self.height - 1.0).abs() <= 0.0001
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub(crate) struct RetouchSettings {
    #[serde(default)]
    pub(crate) adjustments: BasicRetouchAdjustments,
    #[serde(default)]
    pub(crate) crop: Option<RetouchCrop>,
    #[serde(default)]
    pub(crate) rotation_degrees: f32,
}

impl RetouchSettings {
    pub(crate) fn normalized(mut self) -> Self {
        self.crop = self.crop.and_then(RetouchCrop::normalized);
        self.rotation_degrees = normalize_rotation(self.rotation_degrees);
        self
    }

    pub(crate) fn render_key(&self) -> String {
        let normalized = self.clone().normalized();
        let mut hasher = Sha1::new();
        hasher.update(format!("{:.4}", normalized.adjustments.exposure));
        hasher.update(format!("{:.3}", normalized.adjustments.highlights));
        hasher.update(format!("{:.3}", normalized.adjustments.shadows));
        hasher.update(format!("{:.3}", normalized.adjustments.whites));
        hasher.update(format!("{:.3}", normalized.adjustments.blacks));
        hasher.update(format!("{:.3}", normalized.adjustments.temperature));
        hasher.update(format!("{:.3}", normalized.adjustments.offset));
        hasher.update(format!("{:.3}", normalized.adjustments.clarity));
        hasher.update(format!("{:.3}", normalized.rotation_degrees));
        if let Some(crop) = normalized.crop {
            hasher.update(format!(
                "{:.5}:{:.5}:{:.5}:{:.5}",
                crop.x, crop.y, crop.width, crop.height
            ));
        }
        let digest = hasher.finalize();
        digest
            .iter()
            .take(8)
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    pub(crate) fn summary(&self) -> String {
        let normalized = self.clone().normalized();
        let crop = normalized
            .crop
            .map(|crop| {
                format!(
                    " crop={:.1}%,{:.1}%,{:.1}%,{:.1}%",
                    crop.x * 100.0,
                    crop.y * 100.0,
                    crop.width * 100.0,
                    crop.height * 100.0
                )
            })
            .unwrap_or_default();
        format!(
            "retouch exposure={:.2} highlights={:.1} shadows={:.1} whites={:.1} blacks={:.1} temperature={:.0} offset={:.1} clarity={:.1} rotation={:.1}{}",
            normalized.adjustments.exposure,
            normalized.adjustments.highlights,
            normalized.adjustments.shadows,
            normalized.adjustments.whites,
            normalized.adjustments.blacks,
            normalized.adjustments.temperature,
            normalized.adjustments.offset,
            normalized.adjustments.clarity,
            normalized.rotation_degrees,
            crop
        )
    }
}

pub(crate) fn normalize_rotation(rotation: f32) -> f32 {
    if !rotation.is_finite() {
        return 0.0;
    }
    let mut rotation = rotation % 360.0;
    if rotation > 180.0 {
        rotation -= 360.0;
    } else if rotation < -180.0 {
        rotation += 360.0;
    }
    if rotation.abs() < 0.0001 {
        0.0
    } else {
        rotation
    }
}

pub(crate) fn write_rawtherapee_retouch_profile(
    path: &Path,
    base: BasicRetouchAdjustments,
    retouch: &RetouchSettings,
) -> Result<Option<PathBuf>> {
    if retouch.adjustments.is_default() {
        return Ok(None);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let effective = base.add(retouch.adjustments);
    let mut out = String::new();
    let _ = writeln!(out, "[Exposure]");
    let _ = writeln!(out, "Auto=false");
    let _ = writeln!(out, "Compensation={}", fmt_f32(effective.exposure));
    let _ = writeln!(out, "Brightness={}", fmt_slider(effective.whites * 0.5));
    let _ = writeln!(out, "Black={}", fmt_slider(-effective.blacks));
    let _ = writeln!(
        out,
        "HighlightCompr={}",
        fmt_slider((-effective.highlights).clamp(0.0, 100.0))
    );
    let _ = writeln!(
        out,
        "ShadowCompr={}",
        fmt_slider(effective.shadows.clamp(0.0, 100.0))
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "[Luminance Curve]");
    let _ = writeln!(out, "Enabled=true");
    let _ = writeln!(out, "Contrast={}", fmt_slider(effective.clarity));
    let _ = writeln!(out);
    if effective.temperature != 0.0 || effective.offset != 0.0 {
        let _ = writeln!(out, "[White Balance]");
        let _ = writeln!(out, "Enabled=true");
        let _ = writeln!(out, "Setting=Camera");
        if effective.temperature != 0.0 {
            let _ = writeln!(out, "TemperatureBias={}", fmt_i32(effective.temperature));
        }
        if effective.offset != 0.0 {
            let _ = writeln!(
                out,
                "Green={}",
                fmt_f32(wb_green_from_offset(effective.offset))
            );
        }
        let _ = writeln!(out);
    }
    std::fs::write(path, out).with_context(|| format!("writing {}", path.display()))?;
    Ok(Some(path.to_path_buf()))
}

fn fmt_f32(value: f32) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded == 0.0 {
        "0".to_string()
    } else {
        format!("{rounded:.3}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

fn fmt_slider(value: f32) -> String {
    // RawTherapee accepts fractional values for fields such as exposure
    // compensation, but PP3 slider fields like Brightness/Black/Contrast
    // are integer-only. Decimal slider output can make rawtherapee-cli exit
    // before rendering, so retouch sliders are rounded at the PP3 boundary.
    fmt_i32(value.clamp(-100.0, 100.0))
}

fn fmt_i32(value: f32) -> String {
    (value.round() as i32).to_string()
}

fn wb_green_from_offset(offset: f32) -> f32 {
    (1.0 + offset.clamp(-100.0, 100.0) * 0.002).clamp(0.8, 1.2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_normalization_rejects_full_frame() {
        assert!(
            RetouchCrop {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            }
            .normalized()
            .is_none()
        );
        assert_eq!(
            RetouchCrop {
                x: 0.9,
                y: -1.0,
                width: 0.3,
                height: 1.2,
            }
            .normalized(),
            Some(RetouchCrop {
                x: 0.7,
                y: 0.0,
                width: 0.3,
                height: 1.0,
            })
        );
    }

    #[test]
    fn rotation_is_normalized_to_half_turn_range() {
        assert_eq!(normalize_rotation(0.0), 0.0);
        assert_eq!(normalize_rotation(450.0), 90.0);
        assert_eq!(normalize_rotation(-450.0), -90.0);
        assert_eq!(normalize_rotation(f32::NAN), 0.0);
    }

    #[test]
    fn retouch_key_changes_when_adjustments_change() {
        let mut left = RetouchSettings::default();
        let mut right = RetouchSettings::default();
        assert_eq!(left.render_key(), right.render_key());
        right.adjustments.exposure = 0.25;
        assert_ne!(left.render_key(), right.render_key());
        left.adjustments.exposure = 0.25;
        assert_eq!(left.render_key(), right.render_key());
    }

    #[test]
    fn rawtherapee_retouch_profile_writes_tone_and_temperature_overrides() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("retouch.pp3");
        let retouch = RetouchSettings {
            adjustments: BasicRetouchAdjustments {
                exposure: 0.5,
                highlights: -20.0,
                shadows: 30.0,
                whites: 10.0,
                blacks: -5.0,
                temperature: 450.0,
                offset: 35.0,
                clarity: 12.0,
            },
            crop: None,
            rotation_degrees: 0.0,
        };

        let written = write_rawtherapee_retouch_profile(
            &output,
            BasicRetouchAdjustments::default(),
            &retouch,
        )
        .unwrap();

        assert_eq!(written, Some(output.clone()));
        let text = std::fs::read_to_string(output).unwrap();
        assert!(text.contains("[Exposure]"));
        assert!(text.contains("Compensation=0.5"));
        assert!(text.contains("Brightness=5"));
        assert!(text.contains("Black=5"));
        assert!(text.contains("HighlightCompr=20"));
        assert!(text.contains("ShadowCompr=30"));
        assert!(text.contains("[White Balance]"));
        assert!(text.contains("TemperatureBias=450"));
        assert!(text.contains("Green=1.07"));
        assert!(text.contains("Contrast=12"));
    }

    #[test]
    fn rawtherapee_retouch_profile_rounds_slider_values_to_integers() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("retouch.pp3");
        let retouch = RetouchSettings {
            adjustments: BasicRetouchAdjustments {
                whites: 33.0,
                blacks: 21.4,
                highlights: -20.6,
                shadows: 30.5,
                clarity: 15.6,
                ..BasicRetouchAdjustments::default()
            },
            crop: None,
            rotation_degrees: 0.0,
        };

        write_rawtherapee_retouch_profile(&output, BasicRetouchAdjustments::default(), &retouch)
            .unwrap();

        let text = std::fs::read_to_string(output).unwrap();
        assert!(text.contains("Brightness=17\n"));
        assert!(text.contains("Black=-21\n"));
        assert!(text.contains("HighlightCompr=21\n"));
        assert!(text.contains("ShadowCompr=31\n"));
        assert!(text.contains("Contrast=16\n"));
        assert!(!text.contains("Brightness=16.5\n"));
    }

    #[test]
    fn rawtherapee_retouch_profile_rounds_fractional_base_and_retouch_adjustments() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("retouch.pp3");
        let base = BasicRetouchAdjustments {
            highlights: -10.4,
            shadows: 10.4,
            whites: 3.4,
            blacks: 1.4,
            temperature: 99.4,
            clarity: 1.4,
            ..BasicRetouchAdjustments::default()
        };
        let retouch = RetouchSettings {
            adjustments: BasicRetouchAdjustments {
                highlights: -10.2,
                shadows: 20.2,
                whites: 29.6,
                blacks: 20.0,
                temperature: 351.2,
                clarity: 14.2,
                ..BasicRetouchAdjustments::default()
            },
            crop: None,
            rotation_degrees: 0.0,
        };

        write_rawtherapee_retouch_profile(&output, base, &retouch).unwrap();

        let text = std::fs::read_to_string(output).unwrap();
        for key in [
            "Brightness",
            "Black",
            "HighlightCompr",
            "ShadowCompr",
            "Contrast",
            "TemperatureBias",
        ] {
            let line = text
                .lines()
                .find(|line| line.starts_with(&format!("{key}=")))
                .unwrap_or_else(|| panic!("missing {key} in\n{text}"));
            assert!(
                !line.contains('.'),
                "{key} should be integer-only, got {line}"
            );
        }
        assert!(text.contains("Brightness=17\n"));
        assert!(text.contains("Black=-21\n"));
        assert!(text.contains("HighlightCompr=21\n"));
        assert!(text.contains("ShadowCompr=31\n"));
        assert!(text.contains("Contrast=16\n"));
        assert!(text.contains("TemperatureBias=451\n"));
    }
}
