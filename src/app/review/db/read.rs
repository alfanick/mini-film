use super::*;
use rusqlite::OptionalExtension;
use serde::de::DeserializeOwned;

pub(super) fn load_store_from_connection(
    connection: &rusqlite::Connection,
) -> Result<Option<ReviewStore>> {
    let settings = connection
        .query_row(
            "SELECT next_id, current_image_id, min_rating, exif_schema_version
             FROM review_settings WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()
        .context("reading review settings")?;
    let Some((next_id, current_image_id, min_rating, exif_schema_version)) = settings else {
        return Ok(None);
    };

    let profiles = load_profiles(connection)?;
    let images = load_images(connection)?;
    Ok(Some(ReviewStore {
        next_id: i64_to_u64(next_id, "review next_id")?,
        profiles,
        images,
        ui: ReviewUiState {
            current_image_id: current_image_id
                .map(|value| i64_to_u64(value, "current image id"))
                .transpose()?,
            min_rating: i64_to_u8(min_rating, "minimum rating")?,
        },
        exif_schema_version: i64_to_u32(exif_schema_version, "EXIF schema version")?,
    }))
}

fn load_profiles(connection: &rusqlite::Connection) -> Result<Vec<ReviewProfile>> {
    let mut statement = connection
        .prepare(
            "SELECT
                profile_index, position, selector, stem,
                retouch_exposure, retouch_highlights, retouch_shadows, retouch_whites,
                retouch_blacks, retouch_temperature, retouch_offset, retouch_clarity,
                metadata_present, profile_name, profile_uuid, look_name, look_uuid,
                source_profile_name, source_profile_uuid, has_camera_raw_settings,
                grain_amount, grain_size, grain_frequency, has_hald, has_pp3, pp3_name
             FROM profiles ORDER BY position",
        )
        .context("preparing review profile query")?;
    let mut rows = statement.query([]).context("querying review profiles")?;
    let mut profiles = Vec::new();
    while let Some(row) = rows.next().context("reading review profile row")? {
        let position = i64_to_usize(row.get("position")?, "profile position")?;
        require_next_position(position, profiles.len(), "profile")?;
        let profile_index = i64_to_usize(row.get("profile_index")?, "profile index")?;
        let metadata_present = i64_to_bool(row.get("metadata_present")?, "metadata_present")?;
        let grain_amount = row.get::<_, Option<i64>>("grain_amount")?;
        let grain_size = row.get::<_, Option<i64>>("grain_size")?;
        let grain_frequency = row.get::<_, Option<i64>>("grain_frequency")?;
        let grain = match (grain_amount, grain_size, grain_frequency) {
            (None, None, None) => None,
            (Some(amount), Some(size), Some(frequency)) => Some(ReviewProfileGrain {
                amount: i64_to_u8(amount, "profile grain amount")?,
                size: i64_to_u8(size, "profile grain size")?,
                frequency: i64_to_u8(frequency, "profile grain frequency")?,
            }),
            _ => bail!("profile {profile_index} has incomplete grain settings"),
        };
        let profile_name = row.get::<_, Option<String>>("profile_name")?;
        let metadata = if metadata_present {
            Some(ReviewProfileMetadata {
                profile_name: profile_name.ok_or_else(|| {
                    anyhow!("profile {profile_index} metadata has no profile name")
                })?,
                profile_uuid: row.get("profile_uuid")?,
                look_name: row.get("look_name")?,
                look_uuid: row.get("look_uuid")?,
                source_profile_name: row.get("source_profile_name")?,
                source_profile_uuid: row.get("source_profile_uuid")?,
                source_adjustments: ReviewProfileAdjustments::default(),
                source_sharpening: ReviewProfileSharpening::default(),
                emulation_adjustments: ReviewProfileAdjustments::default(),
                emulation_sharpening: ReviewProfileSharpening::default(),
                has_camera_raw_settings: i64_to_bool(
                    row.get("has_camera_raw_settings")?,
                    "has_camera_raw_settings",
                )?,
                grain,
                has_hald: i64_to_bool(row.get("has_hald")?, "has_hald")?,
                has_pp3: i64_to_bool(row.get("has_pp3")?, "has_pp3")?,
                pp3_name: row.get("pp3_name")?,
                pp3_adjustments: Vec::new(),
            })
        } else {
            if profile_name.is_some() || grain.is_some() {
                bail!("profile {profile_index} has metadata values without metadata_present");
            }
            None
        };
        profiles.push(ReviewProfile {
            index: profile_index,
            selector: row.get("selector")?,
            stem: row.get("stem")?,
            retouch_base: BasicRetouchAdjustments {
                exposure: row.get("retouch_exposure")?,
                highlights: row.get("retouch_highlights")?,
                shadows: row.get("retouch_shadows")?,
                whites: row.get("retouch_whites")?,
                blacks: row.get("retouch_blacks")?,
                temperature: row.get("retouch_temperature")?,
                offset: row.get("retouch_offset")?,
                clarity: row.get("retouch_clarity")?,
            },
            metadata,
            hald_path: None,
        });
    }
    drop(rows);
    drop(statement);

    let indexes = profiles
        .iter()
        .enumerate()
        .map(|(position, profile)| (profile.index, position))
        .collect::<HashMap<_, _>>();
    if indexes.len() != profiles.len() {
        bail!("review database contains duplicate profile indexes");
    }
    let adjustments = load_profile_adjustments(connection, &mut profiles, &indexes)?;
    let sharpening = load_profile_sharpening(connection, &mut profiles, &indexes)?;
    load_profile_hsl_values(connection, &mut profiles, &indexes)?;
    load_profile_tone_curve_points(connection, &mut profiles, &indexes)?;
    load_profile_pp3_sections(connection, &mut profiles, &indexes)?;
    load_profile_pp3_entries(connection, &mut profiles, &indexes)?;

    for profile in &profiles {
        if profile.metadata.is_some() {
            for scope in ["source", "emulation"] {
                if !adjustments.contains(&(profile.index, scope)) {
                    bail!(
                        "profile {} is missing {scope} adjustment settings",
                        profile.index
                    );
                }
                if !sharpening.contains(&(profile.index, scope)) {
                    bail!(
                        "profile {} is missing {scope} sharpening settings",
                        profile.index
                    );
                }
            }
        }
    }
    Ok(profiles)
}

fn load_profile_adjustments(
    connection: &rusqlite::Connection,
    profiles: &mut [ReviewProfile],
    indexes: &HashMap<usize, usize>,
) -> Result<HashSet<(usize, &'static str)>> {
    let mut statement = connection.prepare(
        "SELECT profile_index, scope, exposure, contrast, highlights, shadows, whites,
                blacks, saturation, vibrance, clarity, parametric_shadows,
                parametric_darks, parametric_lights, parametric_highlights,
                parametric_shadow_split, parametric_midtone_split,
                parametric_highlight_split, calibration_red_hue,
                calibration_red_saturation, calibration_green_hue,
                calibration_green_saturation, calibration_blue_hue,
                calibration_blue_saturation
         FROM profile_adjustments ORDER BY profile_index, scope",
    )?;
    let mut rows = statement.query([])?;
    let mut seen = HashSet::new();
    while let Some(row) = rows.next()? {
        let profile_index = i64_to_usize(row.get("profile_index")?, "profile index")?;
        let scope = parse_scope(&row.get::<_, String>("scope")?)?;
        if !seen.insert((profile_index, scope)) {
            bail!("profile {profile_index} has duplicate {scope} adjustments");
        }
        let metadata = profile_metadata_mut(profiles, indexes, profile_index)?;
        let adjustments = ReviewProfileAdjustments {
            exposure: row.get("exposure")?,
            contrast: row.get("contrast")?,
            highlights: row.get("highlights")?,
            shadows: row.get("shadows")?,
            whites: row.get("whites")?,
            blacks: row.get("blacks")?,
            saturation: row.get("saturation")?,
            vibrance: row.get("vibrance")?,
            clarity: row.get("clarity")?,
            parametric: ReviewProfileParametricTone {
                shadows: row.get("parametric_shadows")?,
                darks: row.get("parametric_darks")?,
                lights: row.get("parametric_lights")?,
                highlights: row.get("parametric_highlights")?,
                shadow_split: row.get("parametric_shadow_split")?,
                midtone_split: row.get("parametric_midtone_split")?,
                highlight_split: row.get("parametric_highlight_split")?,
            },
            hsl: ReviewProfileHslAdjustments::default(),
            calibration: ReviewProfileCalibration {
                red_hue: row.get("calibration_red_hue")?,
                red_saturation: row.get("calibration_red_saturation")?,
                green_hue: row.get("calibration_green_hue")?,
                green_saturation: row.get("calibration_green_saturation")?,
                blue_hue: row.get("calibration_blue_hue")?,
                blue_saturation: row.get("calibration_blue_saturation")?,
            },
            tone_curve: ReviewProfileToneCurves::default(),
        };
        match scope {
            "source" => metadata.source_adjustments = adjustments,
            "emulation" => metadata.emulation_adjustments = adjustments,
            _ => unreachable!(),
        }
    }
    Ok(seen)
}

fn load_profile_sharpening(
    connection: &rusqlite::Connection,
    profiles: &mut [ReviewProfile],
    indexes: &HashMap<usize, usize>,
) -> Result<HashSet<(usize, &'static str)>> {
    let mut statement = connection.prepare(
        "SELECT profile_index, scope, present, amount, radius, detail, masking
         FROM profile_sharpening ORDER BY profile_index, scope",
    )?;
    let mut rows = statement.query([])?;
    let mut seen = HashSet::new();
    while let Some(row) = rows.next()? {
        let profile_index = i64_to_usize(row.get("profile_index")?, "profile index")?;
        let scope = parse_scope(&row.get::<_, String>("scope")?)?;
        if !seen.insert((profile_index, scope)) {
            bail!("profile {profile_index} has duplicate {scope} sharpening settings");
        }
        let sharpening = ReviewProfileSharpening {
            present: i64_to_bool(row.get("present")?, "profile sharpening present")?,
            amount: row.get("amount")?,
            radius: row.get("radius")?,
            detail: row.get("detail")?,
            masking: row.get("masking")?,
        };
        let metadata = profile_metadata_mut(profiles, indexes, profile_index)?;
        match scope {
            "source" => metadata.source_sharpening = sharpening,
            "emulation" => metadata.emulation_sharpening = sharpening,
            _ => unreachable!(),
        }
    }
    Ok(seen)
}

fn load_profile_hsl_values(
    connection: &rusqlite::Connection,
    profiles: &mut [ReviewProfile],
    indexes: &HashMap<usize, usize>,
) -> Result<()> {
    let mut statement = connection.prepare(
        "SELECT profile_index, scope, channel, value_index, value
         FROM profile_hsl_values ORDER BY profile_index, scope, channel, value_index",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let profile_index = i64_to_usize(row.get("profile_index")?, "profile index")?;
        let scope = parse_scope(&row.get::<_, String>("scope")?)?;
        let channel = row.get::<_, String>("channel")?;
        let position = i64_to_usize(row.get("value_index")?, "HSL value position")?;
        let metadata = profile_metadata_mut(profiles, indexes, profile_index)?;
        let adjustments = profile_adjustments_mut(metadata, scope);
        let values = match channel.as_str() {
            "hue" => &mut adjustments.hsl.hue,
            "saturation" => &mut adjustments.hsl.saturation,
            "luminance" => &mut adjustments.hsl.luminance,
            _ => bail!("profile {profile_index} has unsupported HSL channel {channel:?}"),
        };
        require_next_position(position, values.len(), "profile HSL value")?;
        values.push(row.get("value")?);
    }
    Ok(())
}

fn load_profile_tone_curve_points(
    connection: &rusqlite::Connection,
    profiles: &mut [ReviewProfile],
    indexes: &HashMap<usize, usize>,
) -> Result<()> {
    let mut statement = connection.prepare(
        "SELECT profile_index, scope, channel, point_index, x, y
         FROM profile_tone_curve_points
         ORDER BY profile_index, scope, channel, point_index",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let profile_index = i64_to_usize(row.get("profile_index")?, "profile index")?;
        let scope = parse_scope(&row.get::<_, String>("scope")?)?;
        let channel = row.get::<_, String>("channel")?;
        let position = i64_to_usize(row.get("point_index")?, "tone curve point position")?;
        let metadata = profile_metadata_mut(profiles, indexes, profile_index)?;
        let adjustments = profile_adjustments_mut(metadata, scope);
        let points = match channel.as_str() {
            "composite" => &mut adjustments.tone_curve.composite,
            "red" => &mut adjustments.tone_curve.red,
            "green" => &mut adjustments.tone_curve.green,
            "blue" => &mut adjustments.tone_curve.blue,
            _ => bail!("profile {profile_index} has unsupported tone curve channel {channel:?}"),
        };
        require_next_position(position, points.len(), "tone curve point")?;
        points.push([row.get("x")?, row.get("y")?]);
    }
    Ok(())
}

fn load_profile_pp3_sections(
    connection: &rusqlite::Connection,
    profiles: &mut [ReviewProfile],
    indexes: &HashMap<usize, usize>,
) -> Result<()> {
    let mut statement = connection.prepare(
        "SELECT profile_index, section_position, source, section
         FROM profile_pp3_sections ORDER BY profile_index, section_position",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let profile_index = i64_to_usize(row.get("profile_index")?, "profile index")?;
        let position = i64_to_usize(row.get("section_position")?, "PP3 section position")?;
        let metadata = profile_metadata_mut(profiles, indexes, profile_index)?;
        require_next_position(position, metadata.pp3_adjustments.len(), "PP3 section")?;
        metadata.pp3_adjustments.push(ReviewProfilePp3Section {
            source: row.get("source")?,
            section: row.get("section")?,
            entries: Vec::new(),
        });
    }
    Ok(())
}

fn load_profile_pp3_entries(
    connection: &rusqlite::Connection,
    profiles: &mut [ReviewProfile],
    indexes: &HashMap<usize, usize>,
) -> Result<()> {
    let mut statement = connection.prepare(
        "SELECT profile_index, section_position, entry_position, key, value
         FROM profile_pp3_entries
         ORDER BY profile_index, section_position, entry_position",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let profile_index = i64_to_usize(row.get("profile_index")?, "profile index")?;
        let section_position = i64_to_usize(row.get("section_position")?, "PP3 section position")?;
        let entry_position = i64_to_usize(row.get("entry_position")?, "PP3 entry position")?;
        let metadata = profile_metadata_mut(profiles, indexes, profile_index)?;
        let section = metadata
            .pp3_adjustments
            .get_mut(section_position)
            .ok_or_else(|| {
                anyhow!(
                    "profile {profile_index} PP3 entry references missing section {section_position}"
                )
            })?;
        require_next_position(entry_position, section.entries.len(), "PP3 entry")?;
        section.entries.push(ReviewProfilePp3Entry {
            key: row.get("key")?,
            value: row.get("value")?,
        });
    }
    Ok(())
}

fn profile_metadata_mut<'a>(
    profiles: &'a mut [ReviewProfile],
    indexes: &HashMap<usize, usize>,
    profile_index: usize,
) -> Result<&'a mut ReviewProfileMetadata> {
    let position = indexes
        .get(&profile_index)
        .copied()
        .ok_or_else(|| anyhow!("settings reference missing profile {profile_index}"))?;
    profiles[position]
        .metadata
        .as_mut()
        .ok_or_else(|| anyhow!("profile {profile_index} has settings but no metadata"))
}

fn profile_adjustments_mut<'a>(
    metadata: &'a mut ReviewProfileMetadata,
    scope: &str,
) -> &'a mut ReviewProfileAdjustments {
    match scope {
        "source" => &mut metadata.source_adjustments,
        "emulation" => &mut metadata.emulation_adjustments,
        _ => unreachable!(),
    }
}

fn load_images(connection: &rusqlite::Connection) -> Result<Vec<ReviewImage>> {
    let mut statement = connection.prepare(
        "SELECT
            image_id, position, raw_path, sooc_sidecar_path, relative_path, file_name,
            exif_capture_timestamp, exif_rating, exif_focal_length, exif_aperture,
            exif_shutter_speed, exif_iso, exif_camera_model, exif_lens_model,
            exif_shooting_mode, exif_exposure_compensation, exif_flash, exif_note,
            exif_active_d_lighting, source_file_size_bytes, source_width, source_height,
            preview_status, preview_path, preview_error, preview_duration_ms,
            preview_render_key, preview_updated_at, selected_profile_index, rating,
            label, notes, rating_source, tags_source, notes_source, codex_status,
            codex_flags_tags, codex_flags_note, codex_flags_rating, codex_model,
            codex_analysis_key, codex_error, codex_updated_at, retouch_exposure,
            retouch_highlights, retouch_shadows, retouch_whites, retouch_blacks,
            retouch_temperature, retouch_offset, retouch_clarity, retouch_crop_x,
            retouch_crop_y, retouch_crop_width, retouch_crop_height,
            retouch_rotation_degrees, publish_profiles_default, updated_at
         FROM images ORDER BY position",
    )?;
    let mut rows = statement.query([])?;
    let mut images = Vec::new();
    while let Some(row) = rows.next()? {
        let position = i64_to_usize(row.get("position")?, "image position")?;
        require_next_position(position, images.len(), "image")?;
        let image_id = i64_to_u64(row.get("image_id")?, "image id")?;
        let crop_values = (
            row.get::<_, Option<f32>>("retouch_crop_x")?,
            row.get::<_, Option<f32>>("retouch_crop_y")?,
            row.get::<_, Option<f32>>("retouch_crop_width")?,
            row.get::<_, Option<f32>>("retouch_crop_height")?,
        );
        let crop = match crop_values {
            (None, None, None, None) => None,
            (Some(x), Some(y), Some(width), Some(height)) => {
                Some(crate::app::retouch::RetouchCrop {
                    x,
                    y,
                    width,
                    height,
                })
            }
            _ => bail!("image {image_id} has incomplete crop settings"),
        };
        let publish_profiles_default = i64_to_bool(
            row.get("publish_profiles_default")?,
            "publish_profiles_default",
        )?;
        images.push(ReviewImage {
            id: image_id,
            raw_path: PathBuf::from(row.get::<_, String>("raw_path")?),
            sooc_sidecar_path: row
                .get::<_, Option<String>>("sooc_sidecar_path")?
                .map(PathBuf::from),
            relative_path: row.get("relative_path")?,
            file_name: row.get("file_name")?,
            exif: GalleryExifData {
                capture_timestamp: row.get("exif_capture_timestamp")?,
                rating: row
                    .get::<_, Option<i64>>("exif_rating")?
                    .map(|value| i64_to_u8(value, "EXIF rating"))
                    .transpose()?,
                file_size_bytes: row
                    .get::<_, Option<i64>>("source_file_size_bytes")?
                    .map(|value| i64_to_u64(value, "source file size"))
                    .transpose()?,
                image_width: row
                    .get::<_, Option<i64>>("source_width")?
                    .map(|value| i64_to_u32(value, "source width"))
                    .transpose()?,
                image_height: row
                    .get::<_, Option<i64>>("source_height")?
                    .map(|value| i64_to_u32(value, "source height"))
                    .transpose()?,
                focal_length: row.get("exif_focal_length")?,
                aperture: row.get("exif_aperture")?,
                shutter_speed: row.get("exif_shutter_speed")?,
                iso: row.get("exif_iso")?,
                camera_model: row.get("exif_camera_model")?,
                lens_model: row.get("exif_lens_model")?,
                shooting_mode: row.get("exif_shooting_mode")?,
                exposure_compensation: row.get("exif_exposure_compensation")?,
                flash: row.get("exif_flash")?,
                active_d_lighting: row.get("exif_active_d_lighting")?,
                tags: Vec::new(),
                note: row.get("exif_note")?,
            },
            preview: ReviewPreview {
                status: parse_enum(&row.get::<_, String>("preview_status")?, "preview status")?,
                path: row
                    .get::<_, Option<String>>("preview_path")?
                    .map(PathBuf::from),
                error: row.get("preview_error")?,
                duration_ms: row
                    .get::<_, Option<i64>>("preview_duration_ms")?
                    .map(|value| i64_to_u64(value, "preview duration"))
                    .transpose()?,
                render_key: row.get("preview_render_key")?,
                updated_at: row.get("preview_updated_at")?,
            },
            selected_profile_index: i64_to_usize(
                row.get("selected_profile_index")?,
                "selected profile index",
            )?,
            rating: i64_to_u8(row.get("rating")?, "image rating")?,
            label: parse_enum(&row.get::<_, String>("label")?, "image label")?,
            labels: Vec::new(),
            tags: Vec::new(),
            notes: row.get("notes")?,
            rating_source: parse_enum(&row.get::<_, String>("rating_source")?, "rating source")?,
            tags_source: parse_enum(&row.get::<_, String>("tags_source")?, "tags source")?,
            notes_source: parse_enum(&row.get::<_, String>("notes_source")?, "notes source")?,
            codex: ReviewCodexAnalysis {
                status: parse_enum(&row.get::<_, String>("codex_status")?, "Codex status")?,
                flags: CodexAnalysisFlags {
                    tags: i64_to_bool(row.get("codex_flags_tags")?, "Codex tags flag")?,
                    note: i64_to_bool(row.get("codex_flags_note")?, "Codex note flag")?,
                    rating: i64_to_bool(row.get("codex_flags_rating")?, "Codex rating flag")?,
                },
                model: row.get("codex_model")?,
                analysis_key: row.get("codex_analysis_key")?,
                error: row.get("codex_error")?,
                updated_at: row.get("codex_updated_at")?,
            },
            retouch: RetouchSettings {
                adjustments: BasicRetouchAdjustments {
                    exposure: row.get("retouch_exposure")?,
                    highlights: row.get("retouch_highlights")?,
                    shadows: row.get("retouch_shadows")?,
                    whites: row.get("retouch_whites")?,
                    blacks: row.get("retouch_blacks")?,
                    temperature: row.get("retouch_temperature")?,
                    offset: row.get("retouch_offset")?,
                    clarity: row.get("retouch_clarity")?,
                },
                crop,
                rotation_degrees: row.get("retouch_rotation_degrees")?,
            },
            publish_profile_indexes: (!publish_profiles_default).then(Vec::new),
            profile_bw_filters: Vec::new(),
            profiles: Vec::new(),
            updated_at: row.get("updated_at")?,
        });
    }
    drop(rows);
    drop(statement);

    let indexes = images
        .iter()
        .enumerate()
        .map(|(position, image)| (image.id, position))
        .collect::<HashMap<_, _>>();
    if indexes.len() != images.len() {
        bail!("review database contains duplicate image ids");
    }
    load_image_exif_tags(connection, &mut images, &indexes)?;
    load_image_tags(connection, &mut images, &indexes)?;
    load_image_labels(connection, &mut images, &indexes)?;
    load_image_publish_profiles(connection, &mut images, &indexes)?;
    load_image_profile_bw_filters(connection, &mut images, &indexes)?;
    load_image_profile_renders(connection, &mut images, &indexes)?;
    Ok(images)
}

fn load_image_exif_tags(
    connection: &rusqlite::Connection,
    images: &mut [ReviewImage],
    indexes: &HashMap<u64, usize>,
) -> Result<()> {
    let mut statement = connection.prepare(
        "SELECT image_id, position, tag
         FROM image_exif_tags ORDER BY image_id, position",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let image_id = i64_to_u64(row.get("image_id")?, "image id")?;
        let position = i64_to_usize(row.get("position")?, "EXIF tag position")?;
        let image = image_mut(images, indexes, image_id)?;
        require_next_position(position, image.exif.tags.len(), "EXIF tag")?;
        image.exif.tags.push(row.get("tag")?);
    }
    Ok(())
}

fn load_image_tags(
    connection: &rusqlite::Connection,
    images: &mut [ReviewImage],
    indexes: &HashMap<u64, usize>,
) -> Result<()> {
    let mut statement = connection.prepare(
        "SELECT image_tags.image_id, image_tags.position, tags.tag
         FROM image_tags
         JOIN tags ON tags.tag_id = image_tags.tag_id
         ORDER BY image_tags.image_id, image_tags.position",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let image_id = i64_to_u64(row.get("image_id")?, "image id")?;
        let position = i64_to_usize(row.get("position")?, "image tag position")?;
        let image = image_mut(images, indexes, image_id)?;
        require_next_position(position, image.tags.len(), "image tag")?;
        image.tags.push(row.get("tag")?);
    }
    Ok(())
}

fn load_image_labels(
    connection: &rusqlite::Connection,
    images: &mut [ReviewImage],
    indexes: &HashMap<u64, usize>,
) -> Result<()> {
    let mut statement = connection.prepare(
        "SELECT image_id, position, label FROM image_labels ORDER BY image_id, position",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let image_id = i64_to_u64(row.get("image_id")?, "image id")?;
        let position = i64_to_usize(row.get("position")?, "image label position")?;
        let image = image_mut(images, indexes, image_id)?;
        require_next_position(position, image.labels.len(), "image label")?;
        image
            .labels
            .push(parse_enum(&row.get::<_, String>("label")?, "image label")?);
    }
    Ok(())
}

fn load_image_publish_profiles(
    connection: &rusqlite::Connection,
    images: &mut [ReviewImage],
    indexes: &HashMap<u64, usize>,
) -> Result<()> {
    let mut statement = connection.prepare(
        "SELECT image_id, position, profile_index
         FROM image_publish_profiles ORDER BY image_id, position",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let image_id = i64_to_u64(row.get("image_id")?, "image id")?;
        let position = i64_to_usize(row.get("position")?, "publish profile position")?;
        let profile_index = i64_to_usize(row.get("profile_index")?, "publish profile index")?;
        let image = image_mut(images, indexes, image_id)?;
        let profiles = image.publish_profile_indexes.as_mut().ok_or_else(|| {
            anyhow!("image {image_id} has publish profiles while configured to use defaults")
        })?;
        require_next_position(position, profiles.len(), "publish profile")?;
        profiles.push(profile_index);
    }
    Ok(())
}

fn load_image_profile_bw_filters(
    connection: &rusqlite::Connection,
    images: &mut [ReviewImage],
    indexes: &HashMap<u64, usize>,
) -> Result<()> {
    let mut statement = connection.prepare(
        "SELECT image_id, position, profile_index, bw_filter
         FROM image_profile_bw_filters ORDER BY image_id, position",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let image_id = i64_to_u64(row.get("image_id")?, "image id")?;
        let position = i64_to_usize(row.get("position")?, "profile BW filter position")?;
        let image = image_mut(images, indexes, image_id)?;
        require_next_position(
            position,
            image.profile_bw_filters.len(),
            "profile BW filter",
        )?;
        image.profile_bw_filters.push(ReviewProfileBwFilter {
            profile_index: i64_to_usize(row.get("profile_index")?, "profile BW filter index")?,
            filter: parse_enum(&row.get::<_, String>("bw_filter")?, "BW filter")?,
        });
    }
    Ok(())
}

fn load_image_profile_renders(
    connection: &rusqlite::Connection,
    images: &mut [ReviewImage],
    indexes: &HashMap<u64, usize>,
) -> Result<()> {
    let mut statement = connection.prepare(
        "SELECT image_id, position, profile_index, profile_stem, display_name, status,
                output_path, error, duration_ms, render_key, processing_key, width,
                height, updated_at
         FROM image_profile_renders ORDER BY image_id, position",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let image_id = i64_to_u64(row.get("image_id")?, "image id")?;
        let position = i64_to_usize(row.get("position")?, "profile render position")?;
        let image = image_mut(images, indexes, image_id)?;
        require_next_position(position, image.profiles.len(), "profile render")?;
        image.profiles.push(ReviewProfileRender {
            profile_index: i64_to_usize(row.get("profile_index")?, "profile render index")?,
            profile_stem: row.get("profile_stem")?,
            display_name: row.get("display_name")?,
            status: parse_enum(&row.get::<_, String>("status")?, "profile render status")?,
            output_path: row
                .get::<_, Option<String>>("output_path")?
                .map(PathBuf::from),
            error: row.get("error")?,
            duration_ms: row
                .get::<_, Option<i64>>("duration_ms")?
                .map(|value| i64_to_u64(value, "profile render duration"))
                .transpose()?,
            render_key: row.get("render_key")?,
            processing_key: row.get("processing_key")?,
            width: row
                .get::<_, Option<i64>>("width")?
                .map(|value| i64_to_u32(value, "profile render width"))
                .transpose()?,
            height: row
                .get::<_, Option<i64>>("height")?
                .map(|value| i64_to_u32(value, "profile render height"))
                .transpose()?,
            updated_at: row.get("updated_at")?,
        });
    }
    Ok(())
}

fn image_mut<'a>(
    images: &'a mut [ReviewImage],
    indexes: &HashMap<u64, usize>,
    image_id: u64,
) -> Result<&'a mut ReviewImage> {
    let position = indexes
        .get(&image_id)
        .copied()
        .ok_or_else(|| anyhow!("settings reference missing image {image_id}"))?;
    Ok(&mut images[position])
}

fn parse_scope(scope: &str) -> Result<&'static str> {
    match scope {
        "source" => Ok("source"),
        "emulation" => Ok("emulation"),
        _ => bail!("unsupported profile adjustment scope {scope:?}"),
    }
}

fn parse_enum<T: DeserializeOwned>(text: &str, name: &str) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(text.to_string()))
        .with_context(|| format!("parsing {name} {text:?}"))
}

fn require_next_position(position: usize, expected: usize, name: &str) -> Result<()> {
    if position == expected {
        Ok(())
    } else {
        bail!("{name} position {position} is not the expected position {expected}")
    }
}

fn i64_to_bool(value: i64, name: &str) -> Result<bool> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => bail!("{name} has invalid sqlite boolean value {value}"),
    }
}

fn i64_to_u8(value: i64, name: &str) -> Result<u8> {
    u8::try_from(value).with_context(|| format!("{name} {value} does not fit u8"))
}

fn i64_to_u32(value: i64, name: &str) -> Result<u32> {
    u32::try_from(value).with_context(|| format!("{name} {value} does not fit u32"))
}

fn i64_to_u64(value: i64, name: &str) -> Result<u64> {
    u64::try_from(value).with_context(|| format!("{name} {value} does not fit u64"))
}

fn i64_to_usize(value: i64, name: &str) -> Result<usize> {
    usize::try_from(value).with_context(|| format!("{name} {value} does not fit usize"))
}
