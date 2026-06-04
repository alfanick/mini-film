use std::{fs::File, io::Read, path::Path};

use anyhow::{Context, Result, anyhow};
use quick_xml::{Reader, events::Event};

use crate::model::{
    GrainSettings, ProfileAdjustments, SharpeningSettings, ToneCurves, XmpFilmRecipe, XmpRgbTable,
};

pub fn extract_rgb_table(path: &Path) -> Result<XmpRgbTable> {
    extract_film_recipe(path)?
        .rgb_table
        .ok_or_else(|| anyhow!("missing crs:RGBTable"))
}

/// Extract all film-recipe data mini-film understands from one XMP file.
///
/// The parser streams quick-xml events and collects two related shapes of XMP:
/// profiles that embed a `crs:RGBTable`, and presets that reference a Look by
/// UUID/name while carrying grain and other settings. It tracks whether parsing
/// is inside `crs:Look` so linked profile metadata does not get confused with
/// preset metadata, routes tone-curve text into the active curve target, and
/// falls back to a small raw-string lookup for Look attributes that quick-xml may
/// miss when vendors encode them in compact forms.
pub fn extract_film_recipe(path: &Path) -> Result<XmpFilmRecipe> {
    let mut xml = String::new();
    File::open(path)
        .with_context(|| format!("opening {}", path.display()))?
        .read_to_string(&mut xml)
        .with_context(|| format!("reading {}", path.display()))?;

    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(true);

    let mut rgb_table_id = None::<String>;
    let mut table_value = None::<String>;
    let mut name = None::<String>;
    let mut group = None::<String>;
    let mut uuid = None::<String>;
    let mut look_uuid = None::<String>;
    let mut look_name = None::<String>;
    let mut grain = GrainSettings::default();
    let mut adjustments = ProfileAdjustments::default();
    let mut sharpening = SharpeningSettings::default();
    let mut text_target = None::<TextTarget>;
    let mut inside_look = false;
    let mut curve_target = None::<CurveTarget>;

    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if tag == "crs:Look" {
                    inside_look = true;
                }
                curve_target = match tag.as_str() {
                    "crs:ToneCurvePV2012" => Some(CurveTarget::Composite),
                    "crs:ToneCurvePV2012Red" => Some(CurveTarget::Red),
                    "crs:ToneCurvePV2012Green" => Some(CurveTarget::Green),
                    "crs:ToneCurvePV2012Blue" => Some(CurveTarget::Blue),
                    _ => curve_target,
                };

                text_target = match tag.as_str() {
                    "rdf:li" => text_target.take(),
                    "crs:Name" => Some(TextTarget::Name),
                    "crs:Group" => Some(TextTarget::Group),
                    _ => text_target.take(),
                };

                for attr in e.attributes() {
                    let attr = attr?;
                    let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                    let value = attr
                        .decode_and_unescape_value(reader.decoder())?
                        .into_owned();

                    if key == "crs:RGBTable" {
                        rgb_table_id = Some(value.clone());
                    } else if key == "crs:UUID" {
                        if inside_look {
                            look_uuid = Some(value.clone());
                        } else {
                            uuid = Some(value.clone());
                        }
                    } else if key == "crs:Name" && inside_look {
                        look_name = Some(value.clone());
                    } else if key == "crs:GrainAmount" {
                        grain.amount = parse_u8(&value, "GrainAmount")?;
                    } else if key == "crs:GrainSize" {
                        grain.size = parse_u8(&value, "GrainSize")?;
                    } else if key == "crs:GrainFrequency" {
                        grain.frequency = parse_u8(&value, "GrainFrequency")?;
                    } else if !inside_look {
                        parse_adjustment_attr(&mut adjustments, &key, &value)?;
                        parse_sharpening_attr(&mut sharpening, &key, &value)?;
                    }

                    if let Some(id) = key.strip_prefix("crs:Table_") {
                        table_value = Some(value);
                        if rgb_table_id.is_none() {
                            rgb_table_id = Some(id.to_string());
                        }
                    }
                }
            }
            Event::Empty(e) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let empty_inside_look = inside_look || tag == "crs:Look";
                for attr in e.attributes() {
                    let attr = attr?;
                    let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                    let value = attr
                        .decode_and_unescape_value(reader.decoder())?
                        .into_owned();

                    if key == "crs:RGBTable" {
                        rgb_table_id = Some(value.clone());
                    } else if key == "crs:UUID" {
                        if empty_inside_look {
                            look_uuid = Some(value.clone());
                        } else {
                            uuid = Some(value.clone());
                        }
                    } else if key == "crs:Name" && empty_inside_look {
                        look_name = Some(value.clone());
                    } else if key == "crs:GrainAmount" {
                        grain.amount = parse_u8(&value, "GrainAmount")?;
                    } else if key == "crs:GrainSize" {
                        grain.size = parse_u8(&value, "GrainSize")?;
                    } else if key == "crs:GrainFrequency" {
                        grain.frequency = parse_u8(&value, "GrainFrequency")?;
                    } else if !empty_inside_look {
                        parse_adjustment_attr(&mut adjustments, &key, &value)?;
                        parse_sharpening_attr(&mut sharpening, &key, &value)?;
                    }

                    if let Some(id) = key.strip_prefix("crs:Table_") {
                        table_value = Some(value);
                        if rgb_table_id.is_none() {
                            rgb_table_id = Some(id.to_string());
                        }
                    }
                }
            }
            Event::Text(e) => {
                let text = e.unescape()?.into_owned();
                match text_target {
                    _ if curve_target.is_some() && !text.is_empty() => {
                        if let Some(point) = parse_curve_point(&text) {
                            push_curve_point(&mut adjustments.tone_curve, curve_target, point);
                        }
                    }
                    Some(TextTarget::Name) if name.is_none() && !text.is_empty() => {
                        name = Some(text);
                    }
                    Some(TextTarget::Group) if group.is_none() && !text.is_empty() => {
                        group = Some(text);
                    }
                    _ => {}
                }
            }
            Event::End(e) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if tag == "crs:Name" || tag == "crs:Group" {
                    text_target = None;
                } else if tag == "crs:Look" {
                    inside_look = false;
                } else if tag == "crs:ToneCurvePV2012"
                    || tag == "crs:ToneCurvePV2012Red"
                    || tag == "crs:ToneCurvePV2012Green"
                    || tag == "crs:ToneCurvePV2012Blue"
                {
                    curve_target = None;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    if look_uuid.is_none() || look_name.is_none() {
        if let Some(look_block) = extract_tag_block(&xml, "crs:Look") {
            if look_uuid.is_none() {
                look_uuid = extract_attr(look_block, "crs:UUID");
            }
            if look_name.is_none() {
                look_name = extract_attr(look_block, "crs:Name");
            }
        }
    }

    let rgb_table = match (rgb_table_id, table_value) {
        (Some(table_id), Some(encoded)) => Some(XmpRgbTable {
            name: name.clone(),
            group: group.clone(),
            uuid: uuid.clone(),
            table_id,
            encoded,
        }),
        _ => None,
    };

    Ok(XmpFilmRecipe {
        name,
        group,
        uuid,
        look_uuid,
        look_name,
        rgb_table,
        grain,
        adjustments,
        sharpening,
    })
}

/// Parse one Lightroom adjustment attribute into the profile adjustment model.
///
/// XMP uses many separate `crs:*` keys for basic tone, parametric curves, HSL,
/// and calibration. This function strips the namespace, dispatches known scalar
/// fields directly, and delegates HSL channel-name decoding to `hsl_attr`. It is
/// intentionally tolerant of unknown keys because presets often contain many
/// Lightroom fields that mini-film does not emulate yet.
fn parse_adjustment_attr(
    adjustments: &mut ProfileAdjustments,
    key: &str,
    value: &str,
) -> Result<()> {
    let Some(key) = key.strip_prefix("crs:") else {
        return Ok(());
    };

    match key {
        "Exposure2012" => adjustments.exposure = parse_f32(value, key)?,
        "Contrast2012" => adjustments.contrast = parse_f32(value, key)?,
        "Highlights2012" => adjustments.highlights = parse_f32(value, key)?,
        "Shadows2012" => adjustments.shadows = parse_f32(value, key)?,
        "Whites2012" => adjustments.whites = parse_f32(value, key)?,
        "Blacks2012" => adjustments.blacks = parse_f32(value, key)?,
        "Saturation" => adjustments.saturation = parse_f32(value, key)?,
        "Vibrance" => adjustments.vibrance = parse_f32(value, key)?,
        "Clarity2012" => adjustments.clarity = parse_f32(value, key)?,
        "ParametricShadows" => adjustments.parametric.shadows = parse_f32(value, key)?,
        "ParametricDarks" => adjustments.parametric.darks = parse_f32(value, key)?,
        "ParametricLights" => adjustments.parametric.lights = parse_f32(value, key)?,
        "ParametricHighlights" => adjustments.parametric.highlights = parse_f32(value, key)?,
        "ParametricShadowSplit" => adjustments.parametric.shadow_split = parse_f32(value, key)?,
        "ParametricMidtoneSplit" => adjustments.parametric.midtone_split = parse_f32(value, key)?,
        "ParametricHighlightSplit" => {
            adjustments.parametric.highlight_split = parse_f32(value, key)?
        }
        "RedHue" => adjustments.calibration.red_hue = parse_f32(value, key)?,
        "RedSaturation" => adjustments.calibration.red_saturation = parse_f32(value, key)?,
        "GreenHue" => adjustments.calibration.green_hue = parse_f32(value, key)?,
        "GreenSaturation" => adjustments.calibration.green_saturation = parse_f32(value, key)?,
        "BlueHue" => adjustments.calibration.blue_hue = parse_f32(value, key)?,
        "BlueSaturation" => adjustments.calibration.blue_saturation = parse_f32(value, key)?,
        _ => {
            if let Some((kind, index)) = hsl_attr(key) {
                let parsed = parse_f32(value, key)?;
                match kind {
                    HslAttr::Hue => adjustments.hsl.hue[index] = parsed,
                    HslAttr::Saturation => adjustments.hsl.saturation[index] = parsed,
                    HslAttr::Luminance => adjustments.hsl.luminance[index] = parsed,
                }
            }
        }
    }

    Ok(())
}

/// Parse one Lightroom sharpening attribute into sharpening settings.
///
/// Sharpening is not baked into the Hald because it is spatial, and it is not
/// applied by ImageMagick because RawTherapee has a native sharpening stage.
/// The parser marks sharpening as present when any relevant field appears, then
/// stores amount/radius/detail/masking so the generated `.pp3` can pass an
/// approximation to RawTherapee.
fn parse_sharpening_attr(
    sharpening: &mut SharpeningSettings,
    key: &str,
    value: &str,
) -> Result<()> {
    let Some(key) = key.strip_prefix("crs:") else {
        return Ok(());
    };

    match key {
        "Sharpness" => {
            sharpening.present = true;
            sharpening.amount = parse_f32(value, key)?;
        }
        "SharpenRadius" => {
            sharpening.present = true;
            sharpening.radius = parse_f32(value, key)?;
        }
        "SharpenDetail" => {
            sharpening.present = true;
            sharpening.detail = parse_f32(value, key)?;
        }
        "SharpenEdgeMasking" => {
            sharpening.present = true;
            sharpening.masking = parse_f32(value, key)?;
        }
        _ => {}
    }

    Ok(())
}

fn parse_f32(value: &str, name: &str) -> Result<f32> {
    value
        .parse()
        .with_context(|| format!("invalid {name} value {value:?}"))
}

fn parse_u8(value: &str, name: &str) -> Result<u8> {
    let parsed: u16 = value
        .parse()
        .with_context(|| format!("invalid {name} value {value:?}"))?;
    Ok(parsed.min(100) as u8)
}

fn parse_curve_point(value: &str) -> Option<(f32, f32)> {
    let (x, y) = value.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// Append a parsed curve point to the active tone-curve channel.
///
/// Tone curve text nodes are interpreted according to the most recent enclosing
/// `crs:ToneCurvePV2012*` tag. The target enum keeps that state explicit, so
/// composite, red, green, and blue curves can all be collected from the same XMP
/// event stream without duplicating parser code.
fn push_curve_point(curves: &mut ToneCurves, target: Option<CurveTarget>, point: (f32, f32)) {
    match target {
        Some(CurveTarget::Composite) => curves.composite.push(point),
        Some(CurveTarget::Red) => curves.red.push(point),
        Some(CurveTarget::Green) => curves.green.push(point),
        Some(CurveTarget::Blue) => curves.blue.push(point),
        None => {}
    }
}

#[derive(Clone, Copy)]
enum CurveTarget {
    Composite,
    Red,
    Green,
    Blue,
}

enum HslAttr {
    Hue,
    Saturation,
    Luminance,
}

/// Map Lightroom HSL adjustment names to array slots.
///
/// Lightroom encodes HSL sliders as names such as `HueAdjustmentOrange` and
/// `LuminanceAdjustmentBlue`. This helper splits the control family from the
/// color suffix and returns both the destination array and the fixed hue-channel
/// index used by `ProfileAdjustments`.
fn hsl_attr(key: &str) -> Option<(HslAttr, usize)> {
    let (prefix, suffix) = if let Some(suffix) = key.strip_prefix("HueAdjustment") {
        (HslAttr::Hue, suffix)
    } else if let Some(suffix) = key.strip_prefix("SaturationAdjustment") {
        (HslAttr::Saturation, suffix)
    } else if let Some(suffix) = key.strip_prefix("LuminanceAdjustment") {
        (HslAttr::Luminance, suffix)
    } else {
        return None;
    };

    let index = match suffix {
        "Red" => 0,
        "Orange" => 1,
        "Yellow" => 2,
        "Green" => 3,
        "Aqua" => 4,
        "Blue" => 5,
        "Purple" => 6,
        "Magenta" => 7,
        _ => return None,
    };
    Some((prefix, index))
}

fn extract_tag_block<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let start = xml.find(&format!("<{tag}"))?;
    let end_tag = format!("</{tag}>");
    let end = xml[start..].find(&end_tag)? + start + end_tag.len();
    Some(&xml[start..end])
}

fn extract_attr(block: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=\"");
    let start = block.find(&needle)? + needle.len();
    let end = block[start..].find('"')? + start;
    Some(block[start..end].to_string())
}

#[derive(Copy, Clone)]
enum TextTarget {
    Name,
    Group,
}
