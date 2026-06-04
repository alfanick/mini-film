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

    let profile = extract_rgb_table(input)
        .with_context(|| format!("reading RGBTable from {}", input.display()))?;
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

        write_hald_png(&table, options.hald_level, output)
            .with_context(|| format!("writing {}", output.display()))?;
    }

    Ok(ConvertedProfile {
        input: input.to_path_buf(),
        output: (!options.info_only).then(|| output.to_path_buf()),
        profile,
        table,
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
        "{}{}{}: dims={} divisions={} primaries={} gamma={} gamut={} amount=[{:.2},{:.2}] flags={:?}",
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
        converted.table.flags
    )
}

pub fn write_hald_png(table: &RgbTable, level: u32, path: &Path) -> Result<()> {
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
    let mut text_target = None::<TextTarget>;
    let mut inside_look = false;

    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if tag == "crs:Look" {
                    inside_look = true;
                }
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
                        rgb_table_id = Some(value);
                    } else if key == "crs:UUID" {
                        if inside_look {
                            look_uuid = Some(value);
                        } else {
                            uuid = Some(value);
                        }
                    } else if key == "crs:Name" && inside_look {
                        look_name = Some(value);
                    } else if key == "crs:GrainAmount" {
                        grain.amount = parse_u8(&value, "GrainAmount")?;
                    } else if key == "crs:GrainSize" {
                        grain.size = parse_u8(&value, "GrainSize")?;
                    } else if key == "crs:GrainFrequency" {
                        grain.frequency = parse_u8(&value, "GrainFrequency")?;
                    } else if let Some(id) = key.strip_prefix("crs:Table_") {
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
                        rgb_table_id = Some(value);
                    } else if key == "crs:UUID" {
                        if empty_inside_look {
                            look_uuid = Some(value);
                        } else {
                            uuid = Some(value);
                        }
                    } else if key == "crs:Name" && empty_inside_look {
                        look_name = Some(value);
                    } else if key == "crs:GrainAmount" {
                        grain.amount = parse_u8(&value, "GrainAmount")?;
                    } else if key == "crs:GrainSize" {
                        grain.size = parse_u8(&value, "GrainSize")?;
                    } else if key == "crs:GrainFrequency" {
                        grain.frequency = parse_u8(&value, "GrainFrequency")?;
                    } else if let Some(id) = key.strip_prefix("crs:Table_") {
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

fn render_grain(image: DynamicImage, grain: GrainSettings, seed: u64) -> Result<DynamicImage> {
    let (width, height) = image.dimensions();
    let mut out: ImageBuffer<Rgba<u16>, Vec<u16>> = image.to_rgba16();
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let normal = Normal::new(0.0, 1.0)?;
    let perlin = Perlin::new((seed & 0xffff_ffff) as u32);

    let amount = grain.amount as f32 / 100.0;
    let size = (grain.size.max(1) as f64 / 50.0).clamp(0.2, 3.0);
    let frequency = (grain.frequency.max(1) as f32 / 50.0).clamp(0.2, 2.0);
    let sigma = amount * 34.0;

    for y in 0..height {
        for x in 0..width {
            let pixel = out.get_pixel_mut(x, y);
            let [r, g, b, a] = pixel.0;
            let luma = (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 65535.0;
            let shadow_bias = 0.45 + (1.0 - luma).powf(0.7) * 0.75;
            let clump = perlin.get([x as f64 / (42.0 * size), y as f64 / (42.0 * size)]);
            let clump = 0.75 + ((clump as f32 + 1.0) * 0.5) * 0.5;
            let grain_value = normal.sample(&mut rng) as f32 * sigma * 257.0 * shadow_bias * clump;
            let color_jitter = 0.18 / frequency;

            let rr = add_grain(
                r,
                grain_value * (1.0 + normal.sample(&mut rng) as f32 * color_jitter),
            );
            let gg = add_grain(
                g,
                grain_value * (1.0 + normal.sample(&mut rng) as f32 * color_jitter),
            );
            let bb = add_grain(
                b,
                grain_value * (1.0 + normal.sample(&mut rng) as f32 * color_jitter),
            );
            *pixel = Rgba([rr, gg, bb, a]);
        }
    }

    Ok(DynamicImage::ImageRgba16(out))
}

fn add_grain(channel: u16, delta: f32) -> u16 {
    (channel as f32 + delta).round().clamp(0.0, 65535.0) as u16
}

fn parse_u8(value: &str, name: &str) -> Result<u8> {
    let parsed: u16 = value
        .parse()
        .with_context(|| format!("invalid {name} value {value:?}"))?;
    Ok(parsed.min(100) as u8)
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
