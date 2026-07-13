use super::*;
use rusqlite::{Transaction, named_params, params};

pub(super) fn replace_store(
    connection: &mut rusqlite::Connection,
    store: &ReviewStore,
) -> Result<()> {
    let tx = connection
        .transaction()
        .context("starting review state transaction")?;
    replace_store_in_transaction(&tx, store)?;
    tx.commit().context("committing review state transaction")
}

pub(super) fn replace_store_in_transaction(
    tx: &Transaction<'_>,
    store: &ReviewStore,
) -> Result<()> {
    clear_store_in_transaction(tx)?;

    for (position, profile) in store.profiles.iter().enumerate() {
        insert_profile(tx, position, profile)?;
    }
    for (position, image) in store.images.iter().enumerate() {
        insert_image(tx, position, image)?;
    }
    tx.execute(
        "INSERT INTO review_settings(
            id, next_id, current_image_id, min_rating, exif_schema_version
         ) VALUES (1, ?1, ?2, ?3, ?4)",
        params![
            u64_to_i64(store.next_id, "review next_id")?,
            optional_u64_to_i64(store.ui.current_image_id, "current image id")?,
            u8_to_i64(store.ui.min_rating),
            u32_to_i64(store.exif_schema_version),
        ],
    )
    .context("writing review settings")?;
    Ok(())
}

pub(super) fn clear_store_in_transaction(tx: &Transaction<'_>) -> Result<()> {
    tx.execute_batch(
        "DELETE FROM review_settings;
         DELETE FROM image_profile_renders;
         DELETE FROM image_profile_bw_filters;
         DELETE FROM image_publish_profiles;
         DELETE FROM image_labels;
         DELETE FROM image_tags;
         DELETE FROM tags;
         DELETE FROM image_exif_tags;
         DELETE FROM images;
         DELETE FROM profile_pp3_entries;
         DELETE FROM profile_pp3_sections;
         DELETE FROM profile_tone_curve_points;
         DELETE FROM profile_hsl_values;
         DELETE FROM profile_sharpening;
         DELETE FROM profile_adjustments;
         DELETE FROM profiles;",
    )
    .context("clearing previous relational review state")?;
    Ok(())
}

fn insert_profile(tx: &Transaction<'_>, position: usize, profile: &ReviewProfile) -> Result<()> {
    let metadata = profile.metadata.as_ref();
    let profile_name = metadata.map(|metadata| metadata.profile_name.as_str());
    let profile_uuid = metadata.and_then(|metadata| metadata.profile_uuid.as_deref());
    let look_name = metadata.and_then(|metadata| metadata.look_name.as_deref());
    let look_uuid = metadata.and_then(|metadata| metadata.look_uuid.as_deref());
    let source_profile_name = metadata.and_then(|metadata| metadata.source_profile_name.as_deref());
    let source_profile_uuid = metadata.and_then(|metadata| metadata.source_profile_uuid.as_deref());
    let grain = metadata.and_then(|metadata| metadata.grain.as_ref());
    tx.execute(
        "INSERT INTO profiles(
            profile_index, position, selector, stem,
            retouch_exposure, retouch_highlights, retouch_shadows, retouch_whites,
            retouch_blacks, retouch_temperature, retouch_offset, retouch_clarity,
            metadata_present, profile_name, profile_uuid, look_name, look_uuid,
            source_profile_name, source_profile_uuid, has_camera_raw_settings,
            grain_amount, grain_size, grain_frequency, has_hald, has_pp3, pp3_name
        ) VALUES (
            ?1, ?2, ?3, ?4,
            ?5, ?6, ?7, ?8,
            ?9, ?10, ?11, ?12,
            ?13, ?14, ?15, ?16, ?17,
            ?18, ?19, ?20,
            ?21, ?22, ?23, ?24, ?25, ?26
        )",
        params![
            usize_to_i64(profile.index, "profile index")?,
            usize_to_i64(position, "profile position")?,
            profile.selector,
            profile.stem,
            profile.retouch_base.exposure,
            profile.retouch_base.highlights,
            profile.retouch_base.shadows,
            profile.retouch_base.whites,
            profile.retouch_base.blacks,
            profile.retouch_base.temperature,
            profile.retouch_base.offset,
            profile.retouch_base.clarity,
            bool_to_i64(metadata.is_some()),
            profile_name,
            profile_uuid,
            look_name,
            look_uuid,
            source_profile_name,
            source_profile_uuid,
            bool_to_i64(
                metadata
                    .map(|metadata| metadata.has_camera_raw_settings)
                    .unwrap_or(false)
            ),
            grain.map(|grain| u8_to_i64(grain.amount)),
            grain.map(|grain| u8_to_i64(grain.size)),
            grain.map(|grain| u8_to_i64(grain.frequency)),
            bool_to_i64(metadata.map(|metadata| metadata.has_hald).unwrap_or(false)),
            bool_to_i64(metadata.map(|metadata| metadata.has_pp3).unwrap_or(false)),
            metadata.and_then(|metadata| metadata.pp3_name.as_deref()),
        ],
    )
    .context("writing review profile")?;

    if let Some(metadata) = metadata {
        insert_profile_metadata(tx, profile.index, metadata)?;
    }
    Ok(())
}

fn insert_profile_metadata(
    tx: &Transaction<'_>,
    profile_index: usize,
    metadata: &ReviewProfileMetadata,
) -> Result<()> {
    insert_profile_adjustments(
        tx,
        profile_index,
        "source",
        &metadata.source_adjustments,
        &metadata.source_sharpening,
    )?;
    insert_profile_adjustments(
        tx,
        profile_index,
        "emulation",
        &metadata.emulation_adjustments,
        &metadata.emulation_sharpening,
    )?;
    insert_profile_pp3_adjustments(tx, profile_index, &metadata.pp3_adjustments)
}

fn insert_profile_pp3_adjustments(
    tx: &Transaction<'_>,
    profile_index: usize,
    sections: &[ReviewProfilePp3Section],
) -> Result<()> {
    for (section_position, section) in sections.iter().enumerate() {
        tx.execute(
            "INSERT INTO profile_pp3_sections(
                profile_index, section_position, source, section
             ) VALUES (?1, ?2, ?3, ?4)",
            params![
                usize_to_i64(profile_index, "profile index")?,
                usize_to_i64(section_position, "pp3 section position")?,
                section.source,
                section.section,
            ],
        )
        .context("writing profile pp3 section")?;
        for (entry_position, entry) in section.entries.iter().enumerate() {
            tx.execute(
                "INSERT INTO profile_pp3_entries(
                    profile_index, section_position, entry_position, key, value
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    usize_to_i64(profile_index, "profile index")?,
                    usize_to_i64(section_position, "pp3 section position")?,
                    usize_to_i64(entry_position, "pp3 entry position")?,
                    entry.key,
                    entry.value,
                ],
            )
            .context("writing profile pp3 entry")?;
        }
    }
    Ok(())
}

fn insert_profile_adjustments(
    tx: &Transaction<'_>,
    profile_index: usize,
    scope: &str,
    adjustments: &ReviewProfileAdjustments,
    sharpening: &ReviewProfileSharpening,
) -> Result<()> {
    tx.execute(
        "INSERT INTO profile_adjustments(
            profile_index, scope,
            exposure, contrast, highlights, shadows, whites, blacks, saturation,
            vibrance, clarity, parametric_shadows, parametric_darks,
            parametric_lights, parametric_highlights, parametric_shadow_split,
            parametric_midtone_split, parametric_highlight_split,
            calibration_red_hue, calibration_red_saturation, calibration_green_hue,
            calibration_green_saturation, calibration_blue_hue, calibration_blue_saturation
        ) VALUES (
            ?1, ?2,
            ?3, ?4, ?5, ?6, ?7, ?8, ?9,
            ?10, ?11, ?12, ?13,
            ?14, ?15, ?16,
            ?17, ?18,
            ?19, ?20, ?21,
            ?22, ?23, ?24
        )",
        params![
            usize_to_i64(profile_index, "profile index")?,
            scope,
            adjustments.exposure,
            adjustments.contrast,
            adjustments.highlights,
            adjustments.shadows,
            adjustments.whites,
            adjustments.blacks,
            adjustments.saturation,
            adjustments.vibrance,
            adjustments.clarity,
            adjustments.parametric.shadows,
            adjustments.parametric.darks,
            adjustments.parametric.lights,
            adjustments.parametric.highlights,
            adjustments.parametric.shadow_split,
            adjustments.parametric.midtone_split,
            adjustments.parametric.highlight_split,
            adjustments.calibration.red_hue,
            adjustments.calibration.red_saturation,
            adjustments.calibration.green_hue,
            adjustments.calibration.green_saturation,
            adjustments.calibration.blue_hue,
            adjustments.calibration.blue_saturation,
        ],
    )
    .context("writing profile adjustments")?;

    tx.execute(
        "INSERT INTO profile_sharpening(
            profile_index, scope, present, amount, radius, detail, masking
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            usize_to_i64(profile_index, "profile index")?,
            scope,
            bool_to_i64(sharpening.present),
            sharpening.amount,
            sharpening.radius,
            sharpening.detail,
            sharpening.masking,
        ],
    )
    .context("writing profile sharpening")?;

    for (channel, values) in [
        ("hue", &adjustments.hsl.hue),
        ("saturation", &adjustments.hsl.saturation),
        ("luminance", &adjustments.hsl.luminance),
    ] {
        for (position, value) in values.iter().enumerate() {
            tx.execute(
                "INSERT INTO profile_hsl_values(
                    profile_index, scope, channel, value_index, value
                ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    usize_to_i64(profile_index, "profile index")?,
                    scope,
                    channel,
                    usize_to_i64(position, "hsl value index")?,
                    *value,
                ],
            )
            .context("writing profile hsl value")?;
        }
    }

    for (channel, points) in [
        ("composite", &adjustments.tone_curve.composite),
        ("red", &adjustments.tone_curve.red),
        ("green", &adjustments.tone_curve.green),
        ("blue", &adjustments.tone_curve.blue),
    ] {
        for (position, [x, y]) in points.iter().enumerate() {
            tx.execute(
                "INSERT INTO profile_tone_curve_points(
                    profile_index, scope, channel, point_index, x, y
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    usize_to_i64(profile_index, "profile index")?,
                    scope,
                    channel,
                    usize_to_i64(position, "tone curve point index")?,
                    *x,
                    *y,
                ],
            )
            .context("writing profile tone curve point")?;
        }
    }
    Ok(())
}

fn insert_image(tx: &Transaction<'_>, position: usize, image: &ReviewImage) -> Result<()> {
    let preview_status = enum_text(&image.preview.status)?;
    let rating_source = enum_text(&image.rating_source)?;
    let tags_source = enum_text(&image.tags_source)?;
    let notes_source = enum_text(&image.notes_source)?;
    let codex_status = enum_text(&image.codex.status)?;
    let label = review_label_name(image.label);
    let retouch_crop_x = image.retouch.crop.map(|crop| crop.x);
    let retouch_crop_y = image.retouch.crop.map(|crop| crop.y);
    let retouch_crop_width = image.retouch.crop.map(|crop| crop.width);
    let retouch_crop_height = image.retouch.crop.map(|crop| crop.height);
    tx.execute(
        "INSERT INTO images(
            image_id, position, raw_path, sooc_sidecar_path, relative_path, file_name,
            exif_capture_timestamp, exif_rating, exif_focal_length, exif_aperture,
            exif_shutter_speed, exif_iso, exif_auto_iso, exif_iso_auto_hi_limit,
            exif_white_balance_mode, exif_white_balance_temperature, exif_white_balance_offset,
            exif_camera_model, exif_shutter_count,
            exif_shutter_mode, exif_silent_photography, exif_release_mode, exif_lens_model,
            exif_shooting_mode, exif_exposure_compensation, exif_flash, exif_note,
            exif_active_d_lighting, source_file_size_bytes, source_width, source_height,
            preview_status, preview_path, preview_error, preview_duration_ms, preview_render_key,
            preview_updated_at, selected_profile_index, rating,
            label, notes, rating_source, tags_source, notes_source, codex_status,
            codex_flags_tags, codex_flags_note, codex_flags_rating, codex_model,
            codex_analysis_key, codex_error, codex_updated_at, retouch_exposure,
            retouch_highlights, retouch_shadows, retouch_whites, retouch_blacks,
            retouch_temperature, retouch_offset, retouch_clarity, retouch_crop_x,
            retouch_crop_y, retouch_crop_width, retouch_crop_height,
            retouch_rotation_degrees, publish_profiles_default, updated_at
        ) VALUES (
            :image_id, :position, :raw_path, :sooc_sidecar_path, :relative_path, :file_name,
            :exif_capture_timestamp, :exif_rating, :exif_focal_length, :exif_aperture,
            :exif_shutter_speed, :exif_iso, :exif_auto_iso, :exif_iso_auto_hi_limit,
            :exif_white_balance_mode, :exif_white_balance_temperature, :exif_white_balance_offset,
            :exif_camera_model, :exif_shutter_count,
            :exif_shutter_mode, :exif_silent_photography, :exif_release_mode, :exif_lens_model,
            :exif_shooting_mode, :exif_exposure_compensation, :exif_flash, :exif_note,
            :exif_active_d_lighting, :source_file_size_bytes, :source_width, :source_height,
            :preview_status, :preview_path, :preview_error, :preview_duration_ms, :preview_render_key,
            :preview_updated_at, :selected_profile_index, :rating,
            :label, :notes, :rating_source, :tags_source, :notes_source, :codex_status,
            :codex_flags_tags, :codex_flags_note, :codex_flags_rating, :codex_model,
            :codex_analysis_key, :codex_error, :codex_updated_at, :retouch_exposure,
            :retouch_highlights, :retouch_shadows, :retouch_whites, :retouch_blacks,
            :retouch_temperature, :retouch_offset, :retouch_clarity, :retouch_crop_x,
            :retouch_crop_y, :retouch_crop_width, :retouch_crop_height,
            :retouch_rotation_degrees, :publish_profiles_default, :updated_at
        )",
        named_params! {
            ":image_id": u64_to_i64(image.id, "image id")?,
            ":position": usize_to_i64(position, "image position")?,
            ":raw_path": path_text(&image.raw_path),
            ":sooc_sidecar_path": option_path_text(image.sooc_sidecar_path.as_deref()),
            ":relative_path": image.relative_path,
            ":file_name": image.file_name,
            ":exif_capture_timestamp": image.exif.capture_timestamp,
            ":exif_rating": image.exif.rating.map(u8_to_i64),
            ":exif_focal_length": image.exif.focal_length,
            ":exif_aperture": image.exif.aperture,
            ":exif_shutter_speed": image.exif.shutter_speed,
            ":exif_iso": image.exif.iso,
            ":exif_auto_iso": image.exif.auto_iso.map(bool_to_i64),
            ":exif_iso_auto_hi_limit": image.exif.iso_auto_hi_limit,
            ":exif_white_balance_mode": image.exif.white_balance_mode,
            ":exif_white_balance_temperature": image.exif.white_balance_temperature.map(u32_to_i64),
            ":exif_white_balance_offset": image.exif.white_balance_offset,
            ":exif_camera_model": image.exif.camera_model,
            ":exif_shutter_count": optional_u64_to_i64(image.exif.shutter_count, "EXIF shutter count")?,
            ":exif_shutter_mode": image.exif.shutter_mode,
            ":exif_silent_photography": image.exif.silent_photography.map(bool_to_i64),
            ":exif_release_mode": image.exif.release_mode,
            ":exif_lens_model": image.exif.lens_model,
            ":exif_shooting_mode": image.exif.shooting_mode,
            ":exif_exposure_compensation": image.exif.exposure_compensation,
            ":exif_flash": image.exif.flash,
            ":exif_note": image.exif.note,
            ":exif_active_d_lighting": image.exif.active_d_lighting,
            ":source_file_size_bytes": optional_u64_to_i64(image.exif.file_size_bytes, "source file size")?,
            ":source_width": image.exif.image_width.map(u32_to_i64),
            ":source_height": image.exif.image_height.map(u32_to_i64),
            ":preview_status": preview_status,
            ":preview_path": option_path_text(image.preview.path.as_deref()),
            ":preview_error": image.preview.error,
            ":preview_duration_ms": optional_u64_to_i64(image.preview.duration_ms, "preview duration")?,
            ":preview_render_key": image.preview.render_key,
            ":preview_updated_at": image.preview.updated_at,
            ":selected_profile_index": usize_to_i64(image.selected_profile_index, "selected profile index")?,
            ":rating": u8_to_i64(image.rating),
            ":label": label,
            ":notes": image.notes,
            ":rating_source": rating_source,
            ":tags_source": tags_source,
            ":notes_source": notes_source,
            ":codex_status": codex_status,
            ":codex_flags_tags": bool_to_i64(image.codex.flags.tags),
            ":codex_flags_note": bool_to_i64(image.codex.flags.note),
            ":codex_flags_rating": bool_to_i64(image.codex.flags.rating),
            ":codex_model": image.codex.model,
            ":codex_analysis_key": image.codex.analysis_key,
            ":codex_error": image.codex.error,
            ":codex_updated_at": image.codex.updated_at,
            ":retouch_exposure": image.retouch.adjustments.exposure,
            ":retouch_highlights": image.retouch.adjustments.highlights,
            ":retouch_shadows": image.retouch.adjustments.shadows,
            ":retouch_whites": image.retouch.adjustments.whites,
            ":retouch_blacks": image.retouch.adjustments.blacks,
            ":retouch_temperature": image.retouch.adjustments.temperature,
            ":retouch_offset": image.retouch.adjustments.offset,
            ":retouch_clarity": image.retouch.adjustments.clarity,
            ":retouch_crop_x": retouch_crop_x,
            ":retouch_crop_y": retouch_crop_y,
            ":retouch_crop_width": retouch_crop_width,
            ":retouch_crop_height": retouch_crop_height,
            ":retouch_rotation_degrees": image.retouch.rotation_degrees,
            ":publish_profiles_default": bool_to_i64(image.publish_profile_indexes.is_none()),
            ":updated_at": image.updated_at,
        },
    )
    .context("writing review image")?;

    insert_text_list(tx, "image_exif_tags", image.id, &image.exif.tags)?;
    insert_image_tags(tx, image.id, &image.tags)?;
    for (position, label) in image.labels.iter().enumerate() {
        tx.execute(
            "INSERT INTO image_labels(image_id, position, label) VALUES (?1, ?2, ?3)",
            params![
                u64_to_i64(image.id, "image id")?,
                usize_to_i64(position, "image label position")?,
                review_label_name(*label),
            ],
        )
        .context("writing review image label")?;
    }
    if let Some(indexes) = &image.publish_profile_indexes {
        for (position, profile_index) in indexes.iter().enumerate() {
            tx.execute(
                "INSERT INTO image_publish_profiles(image_id, position, profile_index)
                 VALUES (?1, ?2, ?3)",
                params![
                    u64_to_i64(image.id, "image id")?,
                    usize_to_i64(position, "publish profile position")?,
                    usize_to_i64(*profile_index, "publish profile index")?,
                ],
            )
            .context("writing review image publish profile")?;
        }
    }
    for (position, entry) in image.profile_bw_filters.iter().enumerate() {
        tx.execute(
            "INSERT INTO image_profile_bw_filters(
                image_id, position, profile_index, bw_filter
             ) VALUES (?1, ?2, ?3, ?4)",
            params![
                u64_to_i64(image.id, "image id")?,
                usize_to_i64(position, "profile bw filter position")?,
                usize_to_i64(entry.profile_index, "profile bw filter index")?,
                entry.filter.as_str(),
            ],
        )
        .context("writing review image profile bw filter")?;
    }
    for (position, render) in image.profiles.iter().enumerate() {
        insert_profile_render(tx, image.id, position, render)?;
    }
    Ok(())
}

fn insert_text_list(
    tx: &Transaction<'_>,
    table: &str,
    image_id: u64,
    values: &[String],
) -> Result<()> {
    let sql = format!("INSERT INTO {table}(image_id, position, tag) VALUES (?1, ?2, ?3)");
    for (position, value) in values.iter().enumerate() {
        tx.execute(
            &sql,
            params![
                u64_to_i64(image_id, "image id")?,
                usize_to_i64(position, "text position")?,
                value,
            ],
        )
        .with_context(|| format!("writing {table}"))?;
    }
    Ok(())
}

fn insert_image_tags(tx: &Transaction<'_>, image_id: u64, tags: &[String]) -> Result<()> {
    for (position, tag) in tags.iter().enumerate() {
        tx.execute("INSERT OR IGNORE INTO tags(tag) VALUES (?1)", [tag])
            .context("writing review tag")?;
        let tag_id = tx
            .query_row("SELECT tag_id FROM tags WHERE tag = ?1", [tag], |row| {
                row.get::<_, i64>(0)
            })
            .context("reading review tag id")?;
        tx.execute(
            "INSERT INTO image_tags(image_id, tag_id, position) VALUES (?1, ?2, ?3)",
            params![
                u64_to_i64(image_id, "image id")?,
                tag_id,
                usize_to_i64(position, "image tag position")?,
            ],
        )
        .context("writing image tag")?;
    }
    Ok(())
}

fn insert_profile_render(
    tx: &Transaction<'_>,
    image_id: u64,
    position: usize,
    render: &ReviewProfileRender,
) -> Result<()> {
    let status = enum_text(&render.status)?;
    tx.execute(
        "INSERT INTO image_profile_renders(
            image_id, position, profile_index, profile_stem, display_name, status,
            output_path, error, duration_ms, render_key, processing_key, width, height, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            u64_to_i64(image_id, "image id")?,
            usize_to_i64(position, "profile render position")?,
            usize_to_i64(render.profile_index, "profile render index")?,
            render.profile_stem,
            render.display_name,
            status,
            option_path_text(render.output_path.as_deref()),
            render.error,
            optional_u64_to_i64(render.duration_ms, "profile render duration")?,
            render.render_key,
            render.processing_key,
            render.width.map(u32_to_i64),
            render.height.map(u32_to_i64),
            render.updated_at,
        ],
    )
    .context("writing review profile render")?;
    Ok(())
}

fn enum_text<T: Serialize>(value: &T) -> Result<String> {
    match serde_json::to_value(value).context("serializing enum value")? {
        serde_json::Value::String(text) => Ok(text),
        value => Ok(value.to_string()),
    }
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn option_path_text(path: Option<&Path>) -> Option<String> {
    path.map(path_text)
}

fn bool_to_i64(value: bool) -> i64 {
    i64::from(value)
}

fn u8_to_i64(value: u8) -> i64 {
    i64::from(value)
}

fn u32_to_i64(value: u32) -> i64 {
    i64::from(value)
}

fn u64_to_i64(value: u64, name: &str) -> Result<i64> {
    i64::try_from(value).with_context(|| format!("{name} {value} does not fit sqlite INTEGER"))
}

fn optional_u64_to_i64(value: Option<u64>, name: &str) -> Result<Option<i64>> {
    value.map(|value| u64_to_i64(value, name)).transpose()
}

fn usize_to_i64(value: usize, name: &str) -> Result<i64> {
    i64::try_from(value).with_context(|| format!("{name} {value} does not fit sqlite INTEGER"))
}
