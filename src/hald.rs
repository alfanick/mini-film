use std::{
    fs::{self, File},
    io::BufWriter,
    path::Path,
};

use anyhow::{Context, Result, anyhow, bail};
use walkdir::WalkDir;

use crate::model::{
    BatchSummary, ConvertedProfile, HaldOptions, ProfileAdjustments, RgbTable, XmpRgbTable,
};

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

    let recipe = crate::xmp::extract_film_recipe(input)
        .with_context(|| format!("reading RGBTable from {}", input.display()))?;
    let profile = recipe
        .rgb_table
        .clone()
        .ok_or_else(|| anyhow!("missing crs:RGBTable"))?;
    let decoded = crate::rgb_table::decode_rgb_table(&profile.encoded)
        .with_context(|| format!("decoding table {}", profile.table_id))?;
    let table = crate::rgb_table::parse_rgb_table(&decoded)?;

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
                let rgb = crate::rgb_table::sample_table(table, r, g, b, axis);
                let rgb = crate::adjustments::apply_profile_adjustments(rgb, adjustments);
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

pub(crate) fn validate_hald_level(level: u32) -> Result<()> {
    if level < 2 {
        bail!("--hald-level must be at least 2");
    }
    Ok(())
}
