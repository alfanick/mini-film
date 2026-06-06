use std::borrow::Cow;
use std::{fs::File, io::Read, path::Path};

use anyhow::{Context, Result, anyhow};
use quick_xml::{Reader, events::Event, XmlVersion};

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
                    let value = attr.normalized_value(XmlVersion::Implicit1_0)?.into_owned();

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
                    let value = attr.normalized_value(XmlVersion::Implicit1_0)?.into_owned();

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
                let text = e.decode().map(Cow::into_owned)?;
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

    if (look_uuid.is_none() || look_name.is_none())
        && let Some(look_block) = extract_tag_block(&xml, "crs:Look")
    {
        if look_uuid.is_none() {
            look_uuid = extract_attr(look_block, "crs:UUID");
        }
        if look_name.is_none() {
            look_name = extract_attr(look_block, "crs:Name");
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
    let (kind, suffix) = key
        .strip_prefix("HueAdjustment")
        .map(|suffix| (HslAttr::Hue, suffix))
        .or_else(|| key.strip_prefix("SaturationAdjustment").map(|suffix| (HslAttr::Saturation, suffix)))
        .or_else(|| key.strip_prefix("LuminanceAdjustment").map(|suffix| (HslAttr::Luminance, suffix)))?;

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

    Some((kind, index))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Write};

    #[test]
    fn parse_helpers_accept_valid_numbers_and_reject_bad_values() {
        assert_eq!(parse_f32("1.25", "Exposure").unwrap(), 1.25);
        assert!(parse_f32("bright", "Exposure").is_err());
        assert_eq!(parse_u8("30", "GrainAmount").unwrap(), 30);
        assert_eq!(parse_u8("150", "GrainAmount").unwrap(), 100);
        assert!(parse_u8("large", "GrainAmount").is_err());
        assert_eq!(parse_curve_point(" 12.5, 99 ").unwrap(), (12.5, 99.0));
        assert!(parse_curve_point("12.5").is_none());
    }

    #[test]
    fn hsl_attr_maps_lightroom_names_to_channel_arrays() {
        let (kind, index) = hsl_attr("HueAdjustmentOrange").unwrap();
        assert!(matches!(kind, HslAttr::Hue));
        assert_eq!(index, 1);

        let (kind, index) = hsl_attr("SaturationAdjustmentBlue").unwrap();
        assert!(matches!(kind, HslAttr::Saturation));
        assert_eq!(index, 5);

        let (kind, index) = hsl_attr("LuminanceAdjustmentMagenta").unwrap();
        assert!(matches!(kind, HslAttr::Luminance));
        assert_eq!(index, 7);

        assert!(hsl_attr("HueAdjustmentTeal").is_none());
        assert!(hsl_attr("Temperature").is_none());
    }

    #[test]
    fn adjustment_and_sharpening_attrs_populate_supported_fields() {
        let mut adjustments = ProfileAdjustments::default();
        parse_adjustment_attr(&mut adjustments, "crs:Exposure2012", "0.5").unwrap();
        parse_adjustment_attr(&mut adjustments, "crs:ParametricShadowSplit", "20").unwrap();
        parse_adjustment_attr(&mut adjustments, "crs:HueAdjustmentBlue", "-12").unwrap();
        parse_adjustment_attr(&mut adjustments, "crs:RedSaturation", "9").unwrap();
        parse_adjustment_attr(&mut adjustments, "xmp:Ignored", "bad").unwrap();
        parse_adjustment_attr(&mut adjustments, "crs:Unknown", "bad").unwrap();

        assert_eq!(adjustments.exposure, 0.5);
        assert_eq!(adjustments.parametric.shadow_split, 20.0);
        assert_eq!(adjustments.hsl.hue[5], -12.0);
        assert_eq!(adjustments.calibration.red_saturation, 9.0);

        let mut sharpening = SharpeningSettings::default();
        parse_sharpening_attr(&mut sharpening, "crs:Sharpness", "40").unwrap();
        parse_sharpening_attr(&mut sharpening, "crs:SharpenRadius", "1.2").unwrap();
        parse_sharpening_attr(&mut sharpening, "crs:SharpenDetail", "25").unwrap();
        parse_sharpening_attr(&mut sharpening, "crs:SharpenEdgeMasking", "10").unwrap();

        assert!(sharpening.present);
        assert_eq!(sharpening.amount, 40.0);
        assert_eq!(sharpening.radius, 1.2);
        assert_eq!(sharpening.detail, 25.0);
        assert_eq!(sharpening.masking, 10.0);
    }

    #[test]
    fn curve_points_are_routed_to_the_active_color_channel() {
        let mut curves = ToneCurves::default();
        push_curve_point(&mut curves, Some(CurveTarget::Composite), (0.0, 0.0));
        push_curve_point(&mut curves, Some(CurveTarget::Red), (1.0, 2.0));
        push_curve_point(&mut curves, Some(CurveTarget::Green), (3.0, 4.0));
        push_curve_point(&mut curves, Some(CurveTarget::Blue), (5.0, 6.0));
        push_curve_point(&mut curves, None, (7.0, 8.0));

        assert_eq!(curves.composite, vec![(0.0, 0.0)]);
        assert_eq!(curves.red, vec![(1.0, 2.0)]);
        assert_eq!(curves.green, vec![(3.0, 4.0)]);
        assert_eq!(curves.blue, vec![(5.0, 6.0)]);
    }

    #[test]
    fn extract_film_recipe_collects_profile_grain_adjustments_and_linked_look() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preset.xmp");
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(
            br#"
<x:xmpmeta xmlns:x="adobe:ns:meta/" xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
  <rdf:RDF>
    <rdf:Description
      crs:UUID="preset-uuid"
      crs:RGBTable="abc"
      crs:Table_abc="encoded-table"
      crs:GrainAmount="150"
      crs:GrainSize="40"
      crs:GrainFrequency="30"
      crs:Exposure2012="0.5"
      crs:Contrast2012="20"
      crs:HueAdjustmentBlue="-12"
      crs:Sharpness="40"
      crs:SharpenRadius="1.2">
      <crs:Name>Preset Name</crs:Name>
      <crs:Group>Preset Group</crs:Group>
      <crs:Look>
        <rdf:Description crs:Name="Linked Look" crs:UUID="look-uuid"/>
      </crs:Look>
      <crs:ToneCurvePV2012>
        <rdf:Seq>
          <rdf:li>0, 0</rdf:li>
          <rdf:li>255, 220</rdf:li>
        </rdf:Seq>
      </crs:ToneCurvePV2012>
      <crs:ToneCurvePV2012Blue>
        <rdf:Seq>
          <rdf:li>0, 10</rdf:li>
        </rdf:Seq>
      </crs:ToneCurvePV2012Blue>
    </rdf:Description>
  </rdf:RDF>
</x:xmpmeta>
"#,
        )
        .unwrap();

        let recipe = extract_film_recipe(&path).unwrap();
        assert_eq!(recipe.name.as_deref(), Some("Preset Name"));
        assert_eq!(recipe.group.as_deref(), Some("Preset Group"));
        assert_eq!(recipe.uuid.as_deref(), Some("preset-uuid"));
        assert_eq!(recipe.look_uuid.as_deref(), Some("look-uuid"));
        assert_eq!(recipe.look_name.as_deref(), Some("Linked Look"));
        assert_eq!(recipe.grain.amount, 100);
        assert_eq!(recipe.grain.size, 40);
        assert_eq!(recipe.adjustments.exposure, 0.5);
        assert_eq!(recipe.adjustments.contrast, 20.0);
        assert_eq!(recipe.adjustments.hsl.hue[5], -12.0);
        assert_eq!(
            recipe.adjustments.tone_curve.composite,
            vec![(0.0, 0.0), (255.0, 220.0)]
        );
        assert_eq!(recipe.adjustments.tone_curve.blue, vec![(0.0, 10.0)]);
        assert!(recipe.sharpening.present);
        assert_eq!(recipe.sharpening.amount, 40.0);
        assert_eq!(recipe.sharpening.radius, 1.2);

        let table = recipe.rgb_table.unwrap();
        assert_eq!(table.name.as_deref(), Some("Preset Name"));
        assert_eq!(table.group.as_deref(), Some("Preset Group"));
        assert_eq!(table.uuid.as_deref(), Some("preset-uuid"));
        assert_eq!(table.table_id, "abc");
        assert_eq!(table.encoded, "encoded-table");
    }

    #[test]
    fn raw_attribute_fallback_extracts_compact_look_blocks() {
        let block = r#"<crs:Look crs:Name="Fallback Look" crs:UUID="fallback-uuid"></crs:Look>"#;
        assert_eq!(extract_tag_block(block, "crs:Look").unwrap(), block);
        assert_eq!(
            extract_attr(block, "crs:Name").as_deref(),
            Some("Fallback Look")
        );
        assert_eq!(
            extract_attr(block, "crs:UUID").as_deref(),
            Some("fallback-uuid")
        );
    }
}
