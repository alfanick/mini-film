use std::{
    fs::{self, File},
    io::{BufWriter, Read},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use flate2::read::ZlibDecoder;
use image::{DynamicImage, GenericImageView, ImageBuffer, ImageReader, Rgba};
use noise::{NoiseFn, Perlin};
use quick_xml::{Reader, events::Event};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, Normal};
use rayon::prelude::*;
use walkdir::WalkDir;

const BTT_RGB_TABLE: u32 = 1;
const RGB_TABLE_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct XmpRgbTable {
    pub name: Option<String>,
    pub group: Option<String>,
    pub uuid: Option<String>,
    pub table_id: String,
    encoded: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GrainSettings {
    pub amount: u8,
    pub size: u8,
    pub frequency: u8,
}

impl GrainSettings {
    pub fn is_enabled(self) -> bool {
        self.amount > 0
    }
}

#[derive(Debug, Clone)]
pub struct XmpFilmRecipe {
    pub name: Option<String>,
    pub group: Option<String>,
    pub uuid: Option<String>,
    pub look_uuid: Option<String>,
    pub look_name: Option<String>,
    pub rgb_table: Option<XmpRgbTable>,
    pub grain: GrainSettings,
    pub adjustments: ProfileAdjustments,
    pub sharpening: SharpeningSettings,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SharpeningSettings {
    pub present: bool,
    pub amount: f32,
    pub radius: f32,
    pub detail: f32,
    pub masking: f32,
}

impl SharpeningSettings {
    pub fn is_enabled(self) -> bool {
        self.present && self.amount > 0.0 && self.radius > 0.0
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProfileAdjustments {
    pub exposure: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub saturation: f32,
    pub vibrance: f32,
    pub clarity: f32,
    pub parametric: ParametricTone,
    pub hsl: HslAdjustments,
    pub calibration: CalibrationAdjustments,
    pub tone_curve: ToneCurves,
}

impl ProfileAdjustments {
    pub fn is_default(&self) -> bool {
        self.exposure == 0.0
            && self.contrast == 0.0
            && self.highlights == 0.0
            && self.shadows == 0.0
            && self.whites == 0.0
            && self.blacks == 0.0
            && self.saturation == 0.0
            && self.vibrance == 0.0
            && self.clarity == 0.0
            && self.parametric.is_default()
            && self.hsl.is_default()
            && self.calibration.is_default()
            && self.tone_curve.is_default()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ParametricTone {
    pub shadows: f32,
    pub darks: f32,
    pub lights: f32,
    pub highlights: f32,
    pub shadow_split: f32,
    pub midtone_split: f32,
    pub highlight_split: f32,
}

impl Default for ParametricTone {
    fn default() -> Self {
        Self {
            shadows: 0.0,
            darks: 0.0,
            lights: 0.0,
            highlights: 0.0,
            shadow_split: 25.0,
            midtone_split: 50.0,
            highlight_split: 75.0,
        }
    }
}

impl ParametricTone {
    fn is_default(self) -> bool {
        self.shadows == 0.0
            && self.darks == 0.0
            && self.lights == 0.0
            && self.highlights == 0.0
            && self.shadow_split == 25.0
            && self.midtone_split == 50.0
            && self.highlight_split == 75.0
    }
}

#[derive(Debug, Clone, Default)]
pub struct HslAdjustments {
    pub hue: [f32; 8],
    pub saturation: [f32; 8],
    pub luminance: [f32; 8],
}

impl HslAdjustments {
    fn is_default(&self) -> bool {
        self.hue.iter().all(|v| *v == 0.0)
            && self.saturation.iter().all(|v| *v == 0.0)
            && self.luminance.iter().all(|v| *v == 0.0)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CalibrationAdjustments {
    pub red_hue: f32,
    pub red_saturation: f32,
    pub green_hue: f32,
    pub green_saturation: f32,
    pub blue_hue: f32,
    pub blue_saturation: f32,
}

impl CalibrationAdjustments {
    fn is_default(self) -> bool {
        self.red_hue == 0.0
            && self.red_saturation == 0.0
            && self.green_hue == 0.0
            && self.green_saturation == 0.0
            && self.blue_hue == 0.0
            && self.blue_saturation == 0.0
    }
}

#[derive(Debug, Clone, Default)]
pub struct ToneCurves {
    pub composite: Vec<(f32, f32)>,
    pub red: Vec<(f32, f32)>,
    pub green: Vec<(f32, f32)>,
    pub blue: Vec<(f32, f32)>,
}

impl ToneCurves {
    fn is_default(&self) -> bool {
        curve_is_identity(&self.composite)
            && curve_is_identity(&self.red)
            && curve_is_identity(&self.green)
            && curve_is_identity(&self.blue)
    }
}

#[derive(Debug, Clone)]
pub struct RgbTable {
    pub dimensions: u32,
    pub divisions: u32,
    samples: Vec<[u16; 3]>,
    pub primaries: u32,
    pub gamma: u32,
    pub gamut: u32,
    pub min_amount: f64,
    pub max_amount: f64,
    pub flags: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ConvertedProfile {
    pub input: PathBuf,
    pub output: Option<PathBuf>,
    pub profile: XmpRgbTable,
    pub table: RgbTable,
    pub adjustments: ProfileAdjustments,
    pub sharpening: SharpeningSettings,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct BatchSummary {
    pub converted: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct HaldOptions {
    pub hald_level: u32,
    pub overwrite: bool,
    pub info_only: bool,
}

impl Default for HaldOptions {
    fn default() -> Self {
        Self {
            hald_level: 8,
            overwrite: false,
            info_only: false,
        }
    }
}

pub fn convert_path(
    input: &Path,
    output: &Path,
    options: HaldOptions,
) -> Result<Vec<ConvertedProfile>> {
    validate_hald_level(options.hald_level)?;

    if input.is_dir() {
        if !options.info_only {
            fs::create_dir_all(output).with_context(|| format!("creating {}", output.display()))?;
        }
        convert_dir(input, output, options)
    } else {
        Ok(vec![convert_xmp_to_hald(input, output, options)?])
    }
}

pub fn convert_dir(
    input_dir: &Path,
    output_dir: &Path,
    options: HaldOptions,
) -> Result<Vec<ConvertedProfile>> {
    validate_hald_level(options.hald_level)?;

    let mut converted = Vec::new();
    for entry in WalkDir::new(input_dir).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|s| s.to_str()) != Some("xmp") {
            continue;
        }

        let rel = entry.path().strip_prefix(input_dir).unwrap_or(entry.path());
        let stem = rel
            .file_stem()
            .and_then(|s| s.to_str())
            .map(sanitize_filename::sanitize)
            .unwrap_or_else(|| "profile".to_string());
        let parent = rel.parent().unwrap_or_else(|| Path::new(""));
        let out = output_dir.join(parent).join(format!("{stem}.hald.png"));

        converted.push(convert_xmp_to_hald(entry.path(), &out, options)?);
    }

    Ok(converted)
}

pub fn try_convert_dir(
    input_dir: &Path,
    output_dir: &Path,
    options: HaldOptions,
) -> Result<(Vec<ConvertedProfile>, BatchSummary)> {
    validate_hald_level(options.hald_level)?;

    let mut converted = Vec::new();
    let mut summary = BatchSummary::default();
    for entry in WalkDir::new(input_dir).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().and_then(|s| s.to_str()) != Some("xmp") {
            continue;
        }

        let rel = entry.path().strip_prefix(input_dir).unwrap_or(entry.path());
        let stem = rel
            .file_stem()
            .and_then(|s| s.to_str())
            .map(sanitize_filename::sanitize)
            .unwrap_or_else(|| "profile".to_string());
        let parent = rel.parent().unwrap_or_else(|| Path::new(""));
        let out = output_dir.join(parent).join(format!("{stem}.hald.png"));

        match convert_xmp_to_hald(entry.path(), &out, options) {
            Ok(profile) => {
                summary.converted += 1;
                converted.push(profile);
            }
            Err(err) => {
                summary.skipped += 1;
                eprintln!("skip {}: {err:#}", entry.path().display());
            }
        }
    }

    Ok((converted, summary))
}

pub fn convert_xmp_to_hald(
    input: &Path,
    output: &Path,
    options: HaldOptions,
) -> Result<ConvertedProfile> {
    validate_hald_level(options.hald_level)?;

    let recipe = extract_film_recipe(input)
        .with_context(|| format!("reading RGBTable from {}", input.display()))?;
    let profile = recipe
        .rgb_table
        .clone()
        .ok_or_else(|| anyhow!("missing crs:RGBTable"))?;
    let decoded = decode_rgb_table(&profile.encoded)
        .with_context(|| format!("decoding table {}", profile.table_id))?;
    let table = parse_rgb_table(&decoded)?;

    if !options.info_only {
        if output.exists() && !options.overwrite {
            bail!("output exists, pass --overwrite: {}", output.display());
        }

        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }

        write_hald_png_with_adjustments(&table, options.hald_level, output, &recipe.adjustments)
            .with_context(|| format!("writing {}", output.display()))?;
    }

    Ok(ConvertedProfile {
        input: input.to_path_buf(),
        output: (!options.info_only).then(|| output.to_path_buf()),
        profile,
        table,
        adjustments: recipe.adjustments,
        sharpening: recipe.sharpening,
    })
}

pub fn profile_display_name(input: &Path, profile: &XmpRgbTable) -> String {
    profile.name.clone().unwrap_or_else(|| {
        input
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown profile")
            .to_string()
    })
}

pub fn profile_info_line(converted: &ConvertedProfile) -> String {
    let display_name = profile_display_name(&converted.input, &converted.profile);
    format!(
        "{}{}{}: dims={} divisions={} primaries={} gamma={} gamut={} amount=[{:.2},{:.2}] flags={:?}{}{}",
        display_name,
        converted
            .profile
            .group
            .as_deref()
            .map(|group| format!(" [{group}]"))
            .unwrap_or_default(),
        converted
            .profile
            .uuid
            .as_deref()
            .map(|uuid| format!(" uuid={uuid}"))
            .unwrap_or_default(),
        converted.table.dimensions,
        converted.table.divisions,
        converted.table.primaries,
        converted.table.gamma,
        converted.table.gamut,
        converted.table.min_amount,
        converted.table.max_amount,
        converted.table.flags,
        if converted.adjustments.is_default() {
            ""
        } else {
            " adjustments=baked"
        },
        if converted.sharpening.is_enabled() {
            " sharpening=enabled"
        } else {
            ""
        }
    )
}

pub fn write_hald_png(table: &RgbTable, level: u32, path: &Path) -> Result<()> {
    write_hald_png_with_adjustments(table, level, path, &ProfileAdjustments::default())
}

pub fn write_hald_png_with_adjustments(
    table: &RgbTable,
    level: u32,
    path: &Path,
    adjustments: &ProfileAdjustments,
) -> Result<()> {
    validate_hald_level(level)?;

    let axis = level
        .checked_mul(level)
        .ok_or_else(|| anyhow!("hald level overflow"))?;
    let side = level
        .checked_mul(axis)
        .ok_or_else(|| anyhow!("hald side overflow"))?;
    let pixel_count = (side as usize)
        .checked_mul(side as usize)
        .ok_or_else(|| anyhow!("hald image too large"))?;

    let mut data = Vec::with_capacity(pixel_count * 6);

    for b in 0..axis {
        for g in 0..axis {
            for r in 0..axis {
                let rgb = sample_table(table, r, g, b, axis);
                let rgb = apply_profile_adjustments(rgb, adjustments);
                for channel in rgb {
                    data.extend_from_slice(&channel.to_be_bytes());
                }
            }
        }
    }

    let file = File::create(path)?;
    let writer = BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, side, side);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Sixteen);
    let mut png_writer = encoder.write_header()?;
    png_writer.write_image_data(&data)?;
    Ok(())
}

pub fn extract_rgb_table(path: &Path) -> Result<XmpRgbTable> {
    extract_film_recipe(path)?
        .rgb_table
        .ok_or_else(|| anyhow!("missing crs:RGBTable"))
}

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

pub fn decode_rgb_table(encoded: &str) -> Result<Vec<u8>> {
    let compressed = adobe_base85_decode(encoded);
    if compressed.len() < 5 {
        bail!("decoded payload too short");
    }

    let expected_len = u32::from_le_bytes(compressed[0..4].try_into().unwrap()) as usize;
    let mut decoder = ZlibDecoder::new(&compressed[4..]);
    let mut decoded = Vec::with_capacity(expected_len);
    decoder.read_to_end(&mut decoded)?;

    if decoded.len() != expected_len {
        bail!(
            "zlib length mismatch: expected {expected_len}, got {}",
            decoded.len()
        );
    }

    Ok(decoded)
}

pub fn parse_rgb_table(bytes: &[u8]) -> Result<RgbTable> {
    let mut r = LeReader::new(bytes);

    let table_type = r.u32()?;
    if table_type != BTT_RGB_TABLE {
        bail!("not an RGB table: type {table_type}");
    }

    let version = r.u32()?;
    if version != RGB_TABLE_VERSION {
        bail!("unsupported RGB table version {version}");
    }

    let dimensions = r.u32()?;
    let divisions = r.u32()?;
    if dimensions != 1 && dimensions != 3 {
        bail!("unsupported RGB table dimensions {dimensions}");
    }
    if divisions < 2 {
        bail!("invalid division count {divisions}");
    }

    let nop: Vec<u16> = (0..divisions)
        .map(|i| ((i * 0x0ffff + (divisions >> 1)) / (divisions - 1)) as u16)
        .collect();

    let sample_count = if dimensions == 1 {
        divisions as usize
    } else {
        (divisions as usize).pow(3)
    };
    let mut samples = Vec::with_capacity(sample_count);

    if dimensions == 1 {
        for i in 0..divisions as usize {
            let rr = r.u16()?.wrapping_add(nop[i]);
            let gg = r.u16()?.wrapping_add(nop[i]);
            let bb = r.u16()?.wrapping_add(nop[i]);
            samples.push([rr, gg, bb]);
        }
    } else {
        for ri in 0..divisions as usize {
            for gi in 0..divisions as usize {
                for bi in 0..divisions as usize {
                    let rr = r.u16()?.wrapping_add(nop[ri]);
                    let gg = r.u16()?.wrapping_add(nop[gi]);
                    let bb = r.u16()?.wrapping_add(nop[bi]);
                    samples.push([rr, gg, bb]);
                }
            }
        }
    }

    let primaries = r.u32()?;
    let gamma = r.u32()?;
    let gamut = r.u32()?;
    let min_amount = r.f64()?;
    let max_amount = r.f64()?;
    let flags = if r.remaining() >= 4 {
        Some(r.u32()?)
    } else {
        None
    };

    Ok(RgbTable {
        dimensions,
        divisions,
        samples,
        primaries,
        gamma,
        gamut,
        min_amount,
        max_amount,
        flags,
    })
}

fn validate_hald_level(level: u32) -> Result<()> {
    if level < 2 {
        bail!("--hald-level must be at least 2");
    }
    Ok(())
}

fn adobe_base85_decode(encoded: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity((encoded.len() + 4) / 5 * 4);
    let mut phase = 0u32;
    let mut value = 0u32;

    for byte in encoded.bytes() {
        if !(32..=127).contains(&byte) {
            continue;
        }
        let digit = match adobe_digit(byte) {
            Some(d) => d as u32,
            None => continue,
        };

        phase += 1;
        match phase {
            1 => value = digit,
            2 => value += digit * 85,
            3 => value += digit * 85 * 85,
            4 => value += digit * 85 * 85 * 85,
            5 => {
                value += digit * 85 * 85 * 85 * 85;
                out.extend_from_slice(&value.to_le_bytes());
                phase = 0;
            }
            _ => unreachable!(),
        }
    }

    if phase > 1 {
        let bytes = value.to_le_bytes();
        let count = match phase {
            2 => 1,
            3 => 2,
            4 => 3,
            _ => 0,
        };
        out.extend_from_slice(&bytes[..count]);
    }

    out
}

fn adobe_digit(byte: u8) -> Option<u8> {
    let value = match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'z' => 10 + byte - b'a',
        b'A'..=b'Z' => 36 + byte - b'A',
        b'.' => 62,
        b'-' => 63,
        b':' => 64,
        b'+' => 65,
        b'=' => 66,
        b'^' => 67,
        b'!' => 68,
        b'/' => 69,
        b'*' => 70,
        b'?' => 71,
        b'`' => 72,
        b'\'' => 73,
        b'|' => 74,
        b'(' => 75,
        b')' => 76,
        b'[' => 77,
        b']' => 78,
        b'{' => 79,
        b'}' => 80,
        b'@' => 81,
        b'%' => 82,
        b'$' => 83,
        b'#' => 84,
        _ => return None,
    };
    Some(value)
}

fn sample_table(table: &RgbTable, r: u32, g: u32, b: u32, axis: u32) -> [u16; 3] {
    if table.dimensions == 1 {
        return [
            sample_1d(table, r, axis, 0),
            sample_1d(table, g, axis, 1),
            sample_1d(table, b, axis, 2),
        ];
    }

    let d = table.divisions as usize;
    let rf = scaled_pos(r, axis, table.divisions);
    let gf = scaled_pos(g, axis, table.divisions);
    let bf = scaled_pos(b, axis, table.divisions);

    let (r0, r1, rt) = split_pos(rf, d);
    let (g0, g1, gt) = split_pos(gf, d);
    let (b0, b1, bt) = split_pos(bf, d);

    let mut out = [0u16; 3];
    for c in 0..3 {
        let c000 = table.samples[idx3(d, r0, g0, b0)][c] as f64;
        let c100 = table.samples[idx3(d, r1, g0, b0)][c] as f64;
        let c010 = table.samples[idx3(d, r0, g1, b0)][c] as f64;
        let c110 = table.samples[idx3(d, r1, g1, b0)][c] as f64;
        let c001 = table.samples[idx3(d, r0, g0, b1)][c] as f64;
        let c101 = table.samples[idx3(d, r1, g0, b1)][c] as f64;
        let c011 = table.samples[idx3(d, r0, g1, b1)][c] as f64;
        let c111 = table.samples[idx3(d, r1, g1, b1)][c] as f64;

        let c00 = lerp(c000, c100, rt);
        let c10 = lerp(c010, c110, rt);
        let c01 = lerp(c001, c101, rt);
        let c11 = lerp(c011, c111, rt);
        let c0 = lerp(c00, c10, gt);
        let c1 = lerp(c01, c11, gt);
        out[c] = lerp(c0, c1, bt).round().clamp(0.0, 65535.0) as u16;
    }

    out
}

fn apply_profile_adjustments(rgb: [u16; 3], adjustments: &ProfileAdjustments) -> [u16; 3] {
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

fn curve_is_identity(points: &[(f32, f32)]) -> bool {
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

fn sample_1d(table: &RgbTable, input: u32, axis: u32, channel: usize) -> u16 {
    let d = table.divisions as usize;
    let pos = scaled_pos(input, axis, table.divisions);
    let (i0, i1, t) = split_pos(pos, d);
    lerp(
        table.samples[i0][channel] as f64,
        table.samples[i1][channel] as f64,
        t,
    )
    .round()
    .clamp(0.0, 65535.0) as u16
}

fn scaled_pos(input: u32, axis: u32, divisions: u32) -> f64 {
    if axis <= 1 {
        return 0.0;
    }
    input as f64 * (divisions - 1) as f64 / (axis - 1) as f64
}

fn split_pos(pos: f64, divisions: usize) -> (usize, usize, f64) {
    let lo = pos.floor().clamp(0.0, (divisions - 1) as f64) as usize;
    let hi = (lo + 1).min(divisions - 1);
    (lo, hi, pos - lo as f64)
}

fn idx3(divisions: usize, r: usize, g: usize, b: usize) -> usize {
    (r * divisions + g) * divisions + b
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

#[derive(Copy, Clone)]
enum TextTarget {
    Name,
    Group,
}

struct LeReader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> LeReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.pos)
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N]> {
        if self.remaining() < N {
            bail!("truncated RGB table at byte {}", self.pos);
        }
        let mut out = [0u8; N];
        out.copy_from_slice(&self.bytes[self.pos..self.pos + N]);
        self.pos += N;
        Ok(out)
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take()?))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take()?))
    }

    fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_le_bytes(self.take()?))
    }
}
