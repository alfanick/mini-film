use std::{
    fs::{self, File},
    io::BufWriter,
    path::Path,
};

use anyhow::{Context, Result, anyhow, bail};
use walkdir::WalkDir;

use crate::model::{BatchSummary, ConvertedProfile, HaldOptions, RgbTable, XmpRgbTable};

/// Convert one XMP file or a directory tree of XMP files to Hald PNGs.
///
/// The function validates the requested Hald level once, creates the output
/// directory for batch conversion when needed, and dispatches to either the
/// directory walker or the single-file converter. It is the public library
/// entrypoint used by the CLI `hald` command.
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

/// Convert every `.xmp` file under a directory and fail on the first error.
///
/// Each input path is mapped to a matching relative output path, with the file
/// stem sanitized and `.hald.png` appended. This mode is useful when callers
/// want strict conversion semantics and prefer a single error over a partial
/// success report.
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

/// Convert every `.xmp` file under a directory while collecting failures.
///
/// This mirrors `convert_dir` but records skipped files and continues after
/// individual conversion errors. The CLI uses it for directory conversion so one
/// malformed preset does not prevent the usable RGBTable profiles from being
/// generated.
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

/// Convert one RGBTable-bearing XMP profile into a generated Hald PNG.
///
/// The converter parses the XMP recipe, extracts the embedded RGBTable, decodes
/// the Adobe base85/zlib payload, parses the binary table, and optionally writes
/// a Hald image. The generated Hald contains only the RGBTable lookup; parsed
/// tone/color/sharpening metadata is returned so callers can hand it to
/// RawTherapee through a generated `.pp3` profile.
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

        write_hald_png(&table, options.hald_level, output)
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

/// Format a detailed metadata line for a converted profile.
///
/// The output includes human-readable profile identity plus table dimensions,
/// primaries/gamma/gamut, amount range, flags, and markers for adjustments or
/// sharpening that should be forwarded to RawTherapee through generated `.pp3`
/// files. Keeping this centralized makes `hald --info-only` and conversion logs
/// report the same facts.
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
            " adjustments=pp3"
        },
        if converted.sharpening.is_enabled() {
            " sharpening=pp3"
        } else {
            ""
        }
    )
}

pub fn write_hald_png(table: &RgbTable, level: u32, path: &Path) -> Result<()> {
    write_hald_png_with_adjustments(
        table,
        level,
        path,
        &crate::model::ProfileAdjustments::default(),
    )
}

/// Write a 16-bit RGB Hald CLUT PNG from an RGBTable.
///
/// A Hald level defines `axis = level * level` samples per channel and an image
/// side of `level * axis`. The nested b/g/r loops emit pixels in Hald order,
/// sample the RGBTable at each coordinate, and append big-endian 16-bit RGB
/// channels for the PNG encoder. Overflow checks keep impossible levels from
/// allocating invalid buffers. The adjustment argument is retained for API
/// compatibility but intentionally ignored: tone/color settings now belong in
/// generated RawTherapee `.pp3` profiles, not in the Hald.
pub fn write_hald_png_with_adjustments(
    table: &RgbTable,
    level: u32,
    path: &Path,
    _adjustments: &crate::model::ProfileAdjustments,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ProfileAdjustments, SharpeningSettings};

    fn identity_table(divisions: u32) -> RgbTable {
        let mut samples = Vec::new();
        for r in 0..divisions {
            for g in 0..divisions {
                for b in 0..divisions {
                    let scale =
                        |value: u32| ((value * 65535 + (divisions >> 1)) / (divisions - 1)) as u16;
                    samples.push([scale(r), scale(g), scale(b)]);
                }
            }
        }
        RgbTable {
            dimensions: 3,
            divisions,
            samples,
            primaries: 1,
            gamma: 2,
            gamut: 3,
            min_amount: 0.0,
            max_amount: 2.0,
            flags: Some(7),
        }
    }

    #[test]
    fn validate_hald_level_rejects_degenerate_levels() {
        assert!(validate_hald_level(1).is_err());
        assert!(validate_hald_level(2).is_ok());
    }

    #[test]
    fn profile_display_name_prefers_xmp_name_and_falls_back_to_path() {
        let named = XmpRgbTable {
            name: Some("Named Profile".to_string()),
            group: None,
            uuid: None,
            table_id: "table".to_string(),
            encoded: String::new(),
        };
        assert_eq!(
            profile_display_name(Path::new("/tmp/fallback.xmp"), &named),
            "Named Profile"
        );

        let unnamed = XmpRgbTable {
            name: None,
            group: None,
            uuid: None,
            table_id: "table".to_string(),
            encoded: String::new(),
        };
        assert_eq!(
            profile_display_name(Path::new("/tmp/fallback profile.xmp"), &unnamed),
            "fallback profile"
        );
    }

    #[test]
    fn profile_info_line_marks_adjustments_and_sharpening() {
        let converted = ConvertedProfile {
            input: Path::new("/tmp/profile.xmp").to_path_buf(),
            output: None,
            profile: XmpRgbTable {
                name: Some("Test".to_string()),
                group: Some("Group".to_string()),
                uuid: Some("uuid".to_string()),
                table_id: "table".to_string(),
                encoded: String::new(),
            },
            table: identity_table(2),
            adjustments: ProfileAdjustments {
                exposure: 0.5,
                ..ProfileAdjustments::default()
            },
            sharpening: SharpeningSettings {
                present: true,
                amount: 40.0,
                radius: 1.0,
                detail: 25.0,
                masking: 0.0,
            },
        };

        let line = profile_info_line(&converted);
        assert!(line.contains("Test [Group] uuid=uuid"));
        assert!(line.contains("dims=3 divisions=2"));
        assert!(line.contains("adjustments=pp3"));
        assert!(line.contains("sharpening=pp3"));
    }

    #[test]
    fn write_hald_png_creates_expected_16bit_rgb_dimensions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hald.png");
        write_hald_png(&identity_table(2), 2, &path).unwrap();

        let decoder = png::Decoder::new(File::open(path).unwrap());
        let reader = decoder.read_info().unwrap();
        let info = reader.info();
        assert_eq!(info.width, 8);
        assert_eq!(info.height, 8);
        assert_eq!(info.color_type, png::ColorType::Rgb);
        assert_eq!(info.bit_depth, png::BitDepth::Sixteen);
    }
}
