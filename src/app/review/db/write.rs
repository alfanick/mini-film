use super::{entities::*, *};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DatabaseTransaction,
    EntityTrait, IntoActiveModel, QueryFilter, QuerySelect, Set, TransactionTrait,
    sea_query::OnConflict,
};

use crate::app::review::store::review_label_name;

#[derive(Debug, PartialEq)]
struct ProfileRows {
    profile: profiles::Model,
    adjustments: Vec<profile_adjustments::Model>,
    sharpening: Vec<profile_sharpening::Model>,
    hsl_values: Vec<profile_hsl_values::Model>,
    tone_curve_points: Vec<profile_tone_curve_points::Model>,
    pp3_sections: Vec<profile_pp3_sections::Model>,
    pp3_entries: Vec<profile_pp3_entries::Model>,
}

#[derive(Clone, Debug, PartialEq)]
struct ImageRows {
    image: images::Model,
    exif_tags: Vec<image_exif_tags::Model>,
    focus_regions: Vec<image_focus_regions::Model>,
    tags: Vec<String>,
    labels: Vec<image_labels::Model>,
    publish_profiles: Vec<image_publish_profiles::Model>,
    bw_filters: Vec<image_profile_bw_filters::Model>,
    renders: Vec<image_profile_renders::Model>,
}

pub(super) async fn replace_store(
    connection: &DatabaseConnection,
    store: &ReviewStore,
    roots: &ReviewPathRoots,
) -> Result<()> {
    let transaction = connection
        .begin()
        .await
        .context("starting review state transaction")?;
    let result = replace_store_in_transaction(&transaction, store, roots).await;
    finish_transaction(transaction, result, "replacing review state").await
}

pub(super) async fn apply_store_delta(
    connection: &DatabaseConnection,
    before: &ReviewStore,
    after: &ReviewStore,
    roots: &ReviewPathRoots,
) -> Result<()> {
    let transaction = connection
        .begin()
        .await
        .context("starting review state transaction")?;
    let result = apply_store_delta_in_transaction(&transaction, before, after, roots).await;
    finish_transaction(transaction, result, "updating review state").await
}

async fn finish_transaction(
    transaction: DatabaseTransaction,
    result: Result<()>,
    action: &str,
) -> Result<()> {
    match result {
        Ok(()) => transaction
            .commit()
            .await
            .with_context(|| format!("committing transaction while {action}")),
        Err(error) => {
            let rollback = transaction.rollback().await;
            if let Err(rollback) = rollback {
                return Err(error)
                    .context(format!("{action}; rolling back failed too: {rollback}"));
            }
            Err(error)
        }
    }
}

async fn replace_store_in_transaction(
    transaction: &DatabaseTransaction,
    store: &ReviewStore,
    roots: &ReviewPathRoots,
) -> Result<()> {
    clear_store(transaction).await?;
    for (position, profile) in store.profiles.iter().enumerate() {
        insert_profile_rows(transaction, profile_rows(position, profile)?).await?;
    }
    let mut tags_by_name = HashMap::new();
    for (position, image) in store.images.iter().enumerate() {
        insert_image_rows(
            transaction,
            image_rows(position, image, roots)?,
            &mut tags_by_name,
        )
        .await?;
    }
    insert_models::<profile_diffusion_settings::Entity, _>(
        transaction,
        profile_diffusion_rows(store)?,
    )
    .await?;
    insert_models::<image_profile_diffusion_settings::Entity, _>(
        transaction,
        image_profile_diffusion_rows(store)?,
    )
    .await?;
    insert_models::<expanded_bursts::Entity, _>(transaction, expanded_burst_rows(store)).await?;
    review_settings::Entity::insert(settings_model(store, roots)?.into_active_model())
        .exec(transaction)
        .await
        .context("writing review settings")?;
    Ok(())
}

async fn apply_store_delta_in_transaction(
    transaction: &DatabaseTransaction,
    before: &ReviewStore,
    after: &ReviewStore,
    roots: &ReviewPathRoots,
) -> Result<()> {
    let before_profiles = profile_row_set(before)?;
    let after_profiles = profile_row_set(after)?;
    let profiles_changed = before_profiles != after_profiles;
    if profiles_changed {
        delete_profiles(transaction).await?;
        for rows in after_profiles {
            insert_profile_rows(transaction, rows).await?;
        }
    }

    let before_profile_diffusion = profile_diffusion_rows(before)?;
    let after_profile_diffusion = profile_diffusion_rows(after)?;
    if profiles_changed || before_profile_diffusion != after_profile_diffusion {
        profile_diffusion_settings::Entity::delete_many()
            .exec(transaction)
            .await?;
        insert_models::<profile_diffusion_settings::Entity, _>(
            transaction,
            after_profile_diffusion,
        )
        .await?;
    }
    let before_images = image_row_map(before, roots)?;
    let after_images = image_row_map(after, roots)?;
    for image_id in before_images.keys() {
        if !after_images.contains_key(image_id) {
            images::Entity::delete_by_id(*image_id)
                .exec(transaction)
                .await
                .with_context(|| format!("deleting review image {image_id}"))?;
        }
    }

    move_changed_images_to_temporary_positions(transaction, &before_images, &after_images).await?;
    let mut tags_by_name = load_tag_ids(transaction).await?;
    for (image_id, after_rows) in &after_images {
        let Some(before_rows) = before_images.get(image_id) else {
            insert_image_rows(transaction, after_rows.clone(), &mut tags_by_name).await?;
            continue;
        };
        if before_rows.image != after_rows.image {
            let result = after_rows
                .image
                .clone()
                .into_active_model()
                .reset_all()
                .update(transaction)
                .await
                .with_context(|| format!("updating review image {image_id}"))?;
            if result.image_id != *image_id {
                bail!("updated review image {image_id} returned a different id");
            }
        }
        if before_rows.exif_tags != after_rows.exif_tags {
            replace_exif_tags(transaction, *image_id, &after_rows.exif_tags).await?;
        }
        if before_rows.focus_regions != after_rows.focus_regions {
            replace_focus_regions(transaction, *image_id, &after_rows.focus_regions).await?;
        }
        if before_rows.tags != after_rows.tags {
            replace_image_tags(transaction, *image_id, &after_rows.tags, &mut tags_by_name).await?;
        }
        if before_rows.labels != after_rows.labels {
            replace_labels(transaction, *image_id, &after_rows.labels).await?;
        }
        if before_rows.publish_profiles != after_rows.publish_profiles {
            replace_publish_profiles(transaction, *image_id, &after_rows.publish_profiles).await?;
        }
        if before_rows.bw_filters != after_rows.bw_filters {
            replace_bw_filters(transaction, *image_id, &after_rows.bw_filters).await?;
        }
        if before_rows.renders != after_rows.renders {
            replace_renders(transaction, *image_id, &after_rows.renders).await?;
        }
    }

    let before_image_diffusion = image_profile_diffusion_rows(before)?;
    let after_image_diffusion = image_profile_diffusion_rows(after)?;
    if profiles_changed || before_image_diffusion != after_image_diffusion {
        image_profile_diffusion_settings::Entity::delete_many()
            .exec(transaction)
            .await?;
        insert_models::<image_profile_diffusion_settings::Entity, _>(
            transaction,
            after_image_diffusion,
        )
        .await?;
    }

    if before.expanded_burst_ids != after.expanded_burst_ids {
        expanded_bursts::Entity::delete_many()
            .exec(transaction)
            .await?;
        insert_models::<expanded_bursts::Entity, _>(transaction, expanded_burst_rows(after))
            .await?;
    }

    prune_unused_tags(transaction).await?;
    settings_model(after, roots)?
        .into_active_model()
        .reset_all()
        .update(transaction)
        .await
        .context("updating review settings")?;
    Ok(())
}

fn profile_row_set(store: &ReviewStore) -> Result<Vec<ProfileRows>> {
    store
        .profiles
        .iter()
        .enumerate()
        .map(|(position, profile)| profile_rows(position, profile))
        .collect()
}

fn image_row_map(store: &ReviewStore, roots: &ReviewPathRoots) -> Result<HashMap<i64, ImageRows>> {
    store
        .images
        .iter()
        .enumerate()
        .map(|(position, image)| {
            let rows = image_rows(position, image, roots)?;
            Ok((rows.image.image_id, rows))
        })
        .collect()
}

async fn clear_store<C>(connection: &C) -> Result<()>
where
    C: ConnectionTrait,
{
    review_settings::Entity::delete_many()
        .exec(connection)
        .await?;
    image_profile_renders::Entity::delete_many()
        .exec(connection)
        .await?;
    image_profile_diffusion_settings::Entity::delete_many()
        .exec(connection)
        .await?;
    expanded_bursts::Entity::delete_many()
        .exec(connection)
        .await?;
    profile_diffusion_settings::Entity::delete_many()
        .exec(connection)
        .await?;
    image_profile_bw_filters::Entity::delete_many()
        .exec(connection)
        .await?;
    image_publish_profiles::Entity::delete_many()
        .exec(connection)
        .await?;
    image_labels::Entity::delete_many().exec(connection).await?;
    image_tags::Entity::delete_many().exec(connection).await?;
    image_exif_tags::Entity::delete_many()
        .exec(connection)
        .await?;
    image_focus_regions::Entity::delete_many()
        .exec(connection)
        .await?;
    images::Entity::delete_many().exec(connection).await?;
    tags::Entity::delete_many().exec(connection).await?;
    delete_profiles(connection).await
}

async fn delete_profiles<C>(connection: &C) -> Result<()>
where
    C: ConnectionTrait,
{
    profile_pp3_entries::Entity::delete_many()
        .exec(connection)
        .await?;
    profile_pp3_sections::Entity::delete_many()
        .exec(connection)
        .await?;
    profile_tone_curve_points::Entity::delete_many()
        .exec(connection)
        .await?;
    profile_hsl_values::Entity::delete_many()
        .exec(connection)
        .await?;
    profile_sharpening::Entity::delete_many()
        .exec(connection)
        .await?;
    profile_adjustments::Entity::delete_many()
        .exec(connection)
        .await?;
    profiles::Entity::delete_many().exec(connection).await?;
    Ok(())
}

fn profile_rows(position: usize, profile: &ReviewProfile) -> Result<ProfileRows> {
    let profile_index = usize_to_i64(profile.index, "profile index")?;
    let metadata = profile.metadata.as_ref();
    let grain = metadata.and_then(|metadata| metadata.grain.as_ref());
    let mut rows = ProfileRows {
        profile: profiles::Model {
            profile_index,
            position: usize_to_i64(position, "profile position")?,
            identity: profile.identity.clone(),
            selector: profile.selector.clone(),
            stem: profile.stem.clone(),
            sampler_added: bool_to_i64(profile.sampler_added),
            enabled_by_default: bool_to_i64(profile.enabled_by_default),
            retouch_exposure: real(profile.retouch_base.exposure),
            retouch_contrast: real(profile.retouch_base.contrast),
            retouch_highlights: real(profile.retouch_base.highlights),
            retouch_shadows: real(profile.retouch_base.shadows),
            retouch_whites: real(profile.retouch_base.whites),
            retouch_blacks: real(profile.retouch_base.blacks),
            retouch_temperature: real(profile.retouch_base.temperature),
            retouch_offset: real(profile.retouch_base.offset),
            retouch_clarity: real(profile.retouch_base.clarity),
            metadata_present: bool_to_i64(metadata.is_some()),
            profile_name: metadata.map(|metadata| metadata.profile_name.clone()),
            profile_uuid: metadata.and_then(|metadata| metadata.profile_uuid.clone()),
            look_name: metadata.and_then(|metadata| metadata.look_name.clone()),
            look_uuid: metadata.and_then(|metadata| metadata.look_uuid.clone()),
            source_profile_name: metadata.and_then(|metadata| metadata.source_profile_name.clone()),
            source_profile_uuid: metadata.and_then(|metadata| metadata.source_profile_uuid.clone()),
            has_camera_raw_settings: bool_to_i64(
                metadata.is_some_and(|metadata| metadata.has_camera_raw_settings),
            ),
            grain_amount: grain.map(|grain| i64::from(grain.amount)),
            grain_size: grain.map(|grain| i64::from(grain.size)),
            grain_frequency: grain.map(|grain| i64::from(grain.frequency)),
            has_hald: bool_to_i64(metadata.is_some_and(|metadata| metadata.has_hald)),
            has_pp3: bool_to_i64(metadata.is_some_and(|metadata| metadata.has_pp3)),
            pp3_name: metadata.and_then(|metadata| metadata.pp3_name.clone()),
        },
        adjustments: Vec::new(),
        sharpening: Vec::new(),
        hsl_values: Vec::new(),
        tone_curve_points: Vec::new(),
        pp3_sections: Vec::new(),
        pp3_entries: Vec::new(),
    };
    if let Some(metadata) = metadata {
        append_profile_adjustment_rows(
            &mut rows,
            profile_index,
            "source",
            &metadata.source_adjustments,
            &metadata.source_sharpening,
        )?;
        append_profile_adjustment_rows(
            &mut rows,
            profile_index,
            "emulation",
            &metadata.emulation_adjustments,
            &metadata.emulation_sharpening,
        )?;
        for (section_position, section) in metadata.pp3_adjustments.iter().enumerate() {
            let section_position = usize_to_i64(section_position, "PP3 section position")?;
            rows.pp3_sections.push(profile_pp3_sections::Model {
                profile_index,
                section_position,
                source: section.source.clone(),
                section: section.section.clone(),
            });
            for (entry_position, entry) in section.entries.iter().enumerate() {
                rows.pp3_entries.push(profile_pp3_entries::Model {
                    profile_index,
                    section_position,
                    entry_position: usize_to_i64(entry_position, "PP3 entry position")?,
                    key: entry.key.clone(),
                    value: entry.value.clone(),
                });
            }
        }
    }
    Ok(rows)
}

fn profile_diffusion_rows(store: &ReviewStore) -> Result<Vec<profile_diffusion_settings::Model>> {
    store
        .profile_diffusion_settings
        .iter()
        .map(|entry| {
            crate::app::review::store::validate_diffusion_settings(&entry.settings)?;
            Ok(profile_diffusion_settings::Model {
                profile_index: usize_to_i64(entry.profile_index, "diffusion profile index")?,
                method: enum_text(&entry.settings.method)?,
                softness: i64::from(entry.settings.softness),
                highlight_glow: i64::from(entry.settings.highlight_glow),
                softness_radius_percent: i64::from(entry.settings.softness_radius_percent),
                glow_radius_percent: i64::from(entry.settings.glow_radius_percent),
                intensity_percent: i64::from(entry.settings.intensity_percent),
                highlight_reach: i64::from(entry.settings.highlight_reach),
            })
        })
        .collect()
}

fn image_profile_diffusion_rows(
    store: &ReviewStore,
) -> Result<Vec<image_profile_diffusion_settings::Model>> {
    store
        .image_profile_diffusion_settings
        .iter()
        .map(|entry| {
            crate::app::review::store::validate_diffusion_settings(&entry.settings)?;
            Ok(image_profile_diffusion_settings::Model {
                image_id: u64_to_i64(entry.image_id, "diffusion image id")?,
                profile_index: usize_to_i64(entry.profile_index, "diffusion profile index")?,
                method: enum_text(&entry.settings.method)?,
                softness: i64::from(entry.settings.softness),
                highlight_glow: i64::from(entry.settings.highlight_glow),
                softness_radius_percent: i64::from(entry.settings.softness_radius_percent),
                glow_radius_percent: i64::from(entry.settings.glow_radius_percent),
                intensity_percent: i64::from(entry.settings.intensity_percent),
                highlight_reach: i64::from(entry.settings.highlight_reach),
            })
        })
        .collect()
}

fn append_profile_adjustment_rows(
    rows: &mut ProfileRows,
    profile_index: i64,
    scope: &str,
    adjustments: &ReviewProfileAdjustments,
    sharpening: &ReviewProfileSharpening,
) -> Result<()> {
    rows.adjustments.push(profile_adjustments::Model {
        profile_index,
        scope: scope.to_string(),
        exposure: real(adjustments.exposure),
        contrast: real(adjustments.contrast),
        highlights: real(adjustments.highlights),
        shadows: real(adjustments.shadows),
        whites: real(adjustments.whites),
        blacks: real(adjustments.blacks),
        saturation: real(adjustments.saturation),
        vibrance: real(adjustments.vibrance),
        clarity: real(adjustments.clarity),
        parametric_shadows: real(adjustments.parametric.shadows),
        parametric_darks: real(adjustments.parametric.darks),
        parametric_lights: real(adjustments.parametric.lights),
        parametric_highlights: real(adjustments.parametric.highlights),
        parametric_shadow_split: real(adjustments.parametric.shadow_split),
        parametric_midtone_split: real(adjustments.parametric.midtone_split),
        parametric_highlight_split: real(adjustments.parametric.highlight_split),
        calibration_red_hue: real(adjustments.calibration.red_hue),
        calibration_red_saturation: real(adjustments.calibration.red_saturation),
        calibration_green_hue: real(adjustments.calibration.green_hue),
        calibration_green_saturation: real(adjustments.calibration.green_saturation),
        calibration_blue_hue: real(adjustments.calibration.blue_hue),
        calibration_blue_saturation: real(adjustments.calibration.blue_saturation),
    });
    rows.sharpening.push(profile_sharpening::Model {
        profile_index,
        scope: scope.to_string(),
        present: bool_to_i64(sharpening.present),
        amount: real(sharpening.amount),
        radius: real(sharpening.radius),
        detail: real(sharpening.detail),
        masking: real(sharpening.masking),
    });
    for (channel, values) in [
        ("hue", &adjustments.hsl.hue),
        ("saturation", &adjustments.hsl.saturation),
        ("luminance", &adjustments.hsl.luminance),
    ] {
        for (value_index, value) in values.iter().enumerate() {
            rows.hsl_values.push(profile_hsl_values::Model {
                profile_index,
                scope: scope.to_string(),
                channel: channel.to_string(),
                value_index: usize_to_i64(value_index, "HSL value index")?,
                value: real(*value),
            });
        }
    }
    for (channel, points) in [
        ("composite", &adjustments.tone_curve.composite),
        ("red", &adjustments.tone_curve.red),
        ("green", &adjustments.tone_curve.green),
        ("blue", &adjustments.tone_curve.blue),
    ] {
        for (point_index, [x, y]) in points.iter().enumerate() {
            rows.tone_curve_points
                .push(profile_tone_curve_points::Model {
                    profile_index,
                    scope: scope.to_string(),
                    channel: channel.to_string(),
                    point_index: usize_to_i64(point_index, "tone curve point index")?,
                    x: real(*x),
                    y: real(*y),
                });
        }
    }
    Ok(())
}

async fn insert_profile_rows<C>(connection: &C, rows: ProfileRows) -> Result<()>
where
    C: ConnectionTrait,
{
    profiles::Entity::insert(rows.profile.into_active_model())
        .exec(connection)
        .await
        .context("writing review profile")?;
    insert_models::<profile_adjustments::Entity, _>(connection, rows.adjustments).await?;
    insert_models::<profile_sharpening::Entity, _>(connection, rows.sharpening).await?;
    insert_models::<profile_hsl_values::Entity, _>(connection, rows.hsl_values).await?;
    insert_models::<profile_tone_curve_points::Entity, _>(connection, rows.tone_curve_points)
        .await?;
    insert_models::<profile_pp3_sections::Entity, _>(connection, rows.pp3_sections).await?;
    insert_models::<profile_pp3_entries::Entity, _>(connection, rows.pp3_entries).await?;
    Ok(())
}

async fn insert_models<E, C>(connection: &C, models: Vec<E::Model>) -> Result<()>
where
    E: EntityTrait,
    E::Model: IntoActiveModel<E::ActiveModel>,
    C: ConnectionTrait,
{
    for model in models {
        E::insert(model.into_active_model())
            .exec(connection)
            .await?;
    }
    Ok(())
}

fn image_rows(position: usize, image: &ReviewImage, roots: &ReviewPathRoots) -> Result<ImageRows> {
    let image_id = u64_to_i64(image.id, "image id")?;
    let crop = image.retouch.crop;
    Ok(ImageRows {
        image: images::Model {
            image_id,
            position: usize_to_i64(position, "image position")?,
            raw_path: roots.source_to_storage(&image.raw_path, "images.raw_path")?,
            sooc_sidecar_path: image
                .sooc_sidecar_path
                .as_deref()
                .map(|path| roots.source_to_storage(path, "images.sooc_sidecar_path"))
                .transpose()?,
            relative_path: image.relative_path.clone(),
            file_name: image.file_name.clone(),
            exif_capture_timestamp: image.exif.capture_timestamp,
            exif_capture_subsecond: image.exif.capture_subsecond.clone(),
            exif_rating: image.exif.rating.map(i64::from),
            exif_focal_length: image.exif.focal_length.clone(),
            exif_aperture: image.exif.aperture.clone(),
            exif_shutter_speed: image.exif.shutter_speed.clone(),
            exif_iso: image.exif.iso.clone(),
            exif_camera_model: image.exif.camera_model.clone(),
            exif_camera_serial: image.exif.camera_serial.clone(),
            exif_nikon_burst_key: image.exif.nikon_burst_key.clone(),
            exif_nikon_burst_shot_number: image.exif.nikon_burst_shot_number.map(i64::from),
            exif_lens_model: image.exif.lens_model.clone(),
            exif_shooting_mode: image.exif.shooting_mode.clone(),
            exif_exposure_compensation: image.exif.exposure_compensation.clone(),
            exif_flash: image.exif.flash.clone(),
            exif_note: image.exif.note.clone(),
            preview_status: enum_text(&image.preview.status)?,
            preview_path: image
                .preview
                .path
                .as_deref()
                .map(|path| roots.output_to_storage(path, "images.preview_path"))
                .transpose()?,
            preview_error: image.preview.error.clone(),
            preview_duration_ms: optional_u64_to_i64(
                image.preview.duration_ms,
                "preview duration",
            )?,
            preview_render_key: image.preview.render_key.clone(),
            preview_updated_at: image.preview.updated_at.clone(),
            selected_profile_index: usize_to_i64(
                image.selected_profile_index,
                "selected profile index",
            )?,
            rating: i64::from(image.rating),
            label: review_label_name(image.label).to_string(),
            notes: image.notes.clone(),
            rating_source: enum_text(&image.rating_source)?,
            tags_source: enum_text(&image.tags_source)?,
            notes_source: enum_text(&image.notes_source)?,
            codex_status: enum_text(&image.codex.status)?,
            codex_flags_tags: bool_to_i64(image.codex.flags.tags),
            codex_flags_note: bool_to_i64(image.codex.flags.note),
            codex_flags_rating: bool_to_i64(image.codex.flags.rating),
            codex_model: image.codex.model.clone(),
            codex_analysis_key: image.codex.analysis_key.clone(),
            codex_error: image.codex.error.clone(),
            codex_updated_at: image.codex.updated_at.clone(),
            retouch_exposure: real(image.retouch.adjustments.exposure),
            retouch_contrast: real(image.retouch.adjustments.contrast),
            retouch_highlights: real(image.retouch.adjustments.highlights),
            retouch_shadows: real(image.retouch.adjustments.shadows),
            retouch_whites: real(image.retouch.adjustments.whites),
            retouch_blacks: real(image.retouch.adjustments.blacks),
            retouch_temperature: real(image.retouch.adjustments.temperature),
            retouch_offset: real(image.retouch.adjustments.offset),
            retouch_clarity: real(image.retouch.adjustments.clarity),
            retouch_crop_x: crop.map(|crop| real(crop.x)),
            retouch_crop_y: crop.map(|crop| real(crop.y)),
            retouch_crop_width: crop.map(|crop| real(crop.width)),
            retouch_crop_height: crop.map(|crop| real(crop.height)),
            retouch_rotation_degrees: real(image.retouch.rotation_degrees),
            publish_profiles_default: bool_to_i64(image.publish_profile_indexes.is_none()),
            updated_at: image.updated_at.clone(),
            exif_active_d_lighting: image.exif.active_d_lighting.clone(),
            source_file_size_bytes: optional_u64_to_i64(
                image.exif.file_size_bytes,
                "source file size",
            )?,
            source_width: image.exif.image_width.map(i64::from),
            source_height: image.exif.image_height.map(i64::from),
            exif_focus_frame_width: image.exif.focus_frame_width.map(i64::from),
            exif_focus_frame_height: image.exif.focus_frame_height.map(i64::from),
            exif_auto_iso: image.exif.auto_iso.map(bool_to_i64),
            exif_iso_auto_hi_limit: image.exif.iso_auto_hi_limit.clone(),
            exif_white_balance_mode: image.exif.white_balance_mode.clone(),
            exif_white_balance_temperature: image.exif.white_balance_temperature.map(i64::from),
            exif_white_balance_offset: image.exif.white_balance_offset.map(i64::from),
            exif_shutter_count: optional_u64_to_i64(
                image.exif.shutter_count,
                "EXIF shutter count",
            )?,
            exif_shutter_mode: image.exif.shutter_mode.clone(),
            exif_silent_photography: image.exif.silent_photography.map(bool_to_i64),
            exif_release_mode: image.exif.release_mode.clone(),
        },
        exif_tags: image
            .exif
            .tags
            .iter()
            .enumerate()
            .map(|(position, tag)| {
                Ok(image_exif_tags::Model {
                    image_id,
                    position: usize_to_i64(position, "EXIF tag position")?,
                    tag: tag.clone(),
                })
            })
            .collect::<Result<_>>()?,
        focus_regions: image
            .exif
            .focus_regions
            .iter()
            .enumerate()
            .map(|(position, region)| {
                Ok(image_focus_regions::Model {
                    image_id,
                    position: usize_to_i64(position, "focus region position")?,
                    x: real(region.x),
                    y: real(region.y),
                    width: real(region.width),
                    height: real(region.height),
                    primary: bool_to_i64(region.primary),
                })
            })
            .collect::<Result<_>>()?,
        tags: image.tags.clone(),
        labels: image
            .labels
            .iter()
            .enumerate()
            .map(|(position, label)| {
                Ok(image_labels::Model {
                    image_id,
                    position: usize_to_i64(position, "image label position")?,
                    label: review_label_name(*label).to_string(),
                })
            })
            .collect::<Result<_>>()?,
        publish_profiles: image
            .publish_profile_indexes
            .as_deref()
            .unwrap_or_default()
            .iter()
            .enumerate()
            .map(|(position, profile_index)| {
                Ok(image_publish_profiles::Model {
                    image_id,
                    position: usize_to_i64(position, "publish profile position")?,
                    profile_index: usize_to_i64(*profile_index, "publish profile index")?,
                })
            })
            .collect::<Result<_>>()?,
        bw_filters: image
            .profile_bw_filters
            .iter()
            .enumerate()
            .map(|(position, entry)| {
                Ok(image_profile_bw_filters::Model {
                    image_id,
                    position: usize_to_i64(position, "profile BW filter position")?,
                    profile_index: usize_to_i64(entry.profile_index, "profile BW filter index")?,
                    bw_filter: entry.filter.as_str().to_string(),
                })
            })
            .collect::<Result<_>>()?,
        renders: image
            .profiles
            .iter()
            .enumerate()
            .map(|(position, render)| profile_render_row(image_id, position, render, roots))
            .collect::<Result<_>>()?,
    })
}

fn expanded_burst_rows(store: &ReviewStore) -> Vec<expanded_bursts::Model> {
    store
        .expanded_burst_ids
        .iter()
        .map(|burst_id| expanded_bursts::Model {
            burst_id: burst_id.clone(),
        })
        .collect()
}

fn profile_render_row(
    image_id: i64,
    position: usize,
    render: &ReviewProfileRender,
    roots: &ReviewPathRoots,
) -> Result<image_profile_renders::Model> {
    Ok(image_profile_renders::Model {
        image_id,
        position: usize_to_i64(position, "profile render position")?,
        profile_index: usize_to_i64(render.profile_index, "profile render index")?,
        profile_stem: render.profile_stem.clone(),
        display_name: render.display_name.clone(),
        enabled: bool_to_i64(render.enabled),
        status: enum_text(&render.status)?,
        output_path: render
            .output_path
            .as_deref()
            .map(|path| roots.output_to_storage(path, "image_profile_renders.output_path"))
            .transpose()?,
        error: render.error.clone(),
        duration_ms: optional_u64_to_i64(render.duration_ms, "profile render duration")?,
        render_key: render.render_key.clone(),
        width: render.width.map(i64::from),
        height: render.height.map(i64::from),
        updated_at: render.updated_at.clone(),
        processing_key: render.processing_key.clone(),
        dcp_profile_filename: render.dcp_profile_filename.clone(),
        lcp_profile_filename: render.lcp_profile_filename.clone(),
    })
}

async fn insert_image_rows<C>(
    connection: &C,
    rows: ImageRows,
    tags_by_name: &mut HashMap<String, i64>,
) -> Result<()>
where
    C: ConnectionTrait,
{
    let image_id = rows.image.image_id;
    images::Entity::insert(rows.image.into_active_model())
        .exec(connection)
        .await
        .with_context(|| format!("writing review image {image_id}"))?;
    insert_models::<image_exif_tags::Entity, _>(connection, rows.exif_tags).await?;
    insert_models::<image_focus_regions::Entity, _>(connection, rows.focus_regions).await?;
    insert_image_tags(connection, image_id, &rows.tags, tags_by_name).await?;
    insert_models::<image_labels::Entity, _>(connection, rows.labels).await?;
    insert_models::<image_publish_profiles::Entity, _>(connection, rows.publish_profiles).await?;
    insert_models::<image_profile_bw_filters::Entity, _>(connection, rows.bw_filters).await?;
    insert_models::<image_profile_renders::Entity, _>(connection, rows.renders).await?;
    Ok(())
}

async fn move_changed_images_to_temporary_positions<C>(
    connection: &C,
    before: &HashMap<i64, ImageRows>,
    after: &HashMap<i64, ImageRows>,
) -> Result<()>
where
    C: ConnectionTrait,
{
    for (image_id, before_rows) in before {
        let Some(after_rows) = after.get(image_id) else {
            continue;
        };
        if before_rows.image.position != after_rows.image.position {
            let mut active = before_rows.image.clone().into_active_model();
            active.position = Set(-image_id - 1);
            active
                .update(connection)
                .await
                .with_context(|| format!("temporarily moving review image {image_id}"))?;
        }
    }
    Ok(())
}

async fn replace_exif_tags<C>(
    connection: &C,
    image_id: i64,
    rows: &[image_exif_tags::Model],
) -> Result<()>
where
    C: ConnectionTrait,
{
    image_exif_tags::Entity::delete_many()
        .filter(image_exif_tags::Column::ImageId.eq(image_id))
        .exec(connection)
        .await?;
    insert_models::<image_exif_tags::Entity, _>(connection, rows.to_vec()).await
}

async fn replace_focus_regions<C>(
    connection: &C,
    image_id: i64,
    rows: &[image_focus_regions::Model],
) -> Result<()>
where
    C: ConnectionTrait,
{
    image_focus_regions::Entity::delete_many()
        .filter(image_focus_regions::Column::ImageId.eq(image_id))
        .exec(connection)
        .await?;
    insert_models::<image_focus_regions::Entity, _>(connection, rows.to_vec()).await
}

async fn replace_labels<C>(
    connection: &C,
    image_id: i64,
    rows: &[image_labels::Model],
) -> Result<()>
where
    C: ConnectionTrait,
{
    image_labels::Entity::delete_many()
        .filter(image_labels::Column::ImageId.eq(image_id))
        .exec(connection)
        .await?;
    insert_models::<image_labels::Entity, _>(connection, rows.to_vec()).await
}

async fn replace_publish_profiles<C>(
    connection: &C,
    image_id: i64,
    rows: &[image_publish_profiles::Model],
) -> Result<()>
where
    C: ConnectionTrait,
{
    image_publish_profiles::Entity::delete_many()
        .filter(image_publish_profiles::Column::ImageId.eq(image_id))
        .exec(connection)
        .await?;
    insert_models::<image_publish_profiles::Entity, _>(connection, rows.to_vec()).await
}

async fn replace_bw_filters<C>(
    connection: &C,
    image_id: i64,
    rows: &[image_profile_bw_filters::Model],
) -> Result<()>
where
    C: ConnectionTrait,
{
    image_profile_bw_filters::Entity::delete_many()
        .filter(image_profile_bw_filters::Column::ImageId.eq(image_id))
        .exec(connection)
        .await?;
    insert_models::<image_profile_bw_filters::Entity, _>(connection, rows.to_vec()).await
}

async fn replace_renders<C>(
    connection: &C,
    image_id: i64,
    rows: &[image_profile_renders::Model],
) -> Result<()>
where
    C: ConnectionTrait,
{
    image_profile_renders::Entity::delete_many()
        .filter(image_profile_renders::Column::ImageId.eq(image_id))
        .exec(connection)
        .await?;
    insert_models::<image_profile_renders::Entity, _>(connection, rows.to_vec()).await
}

async fn load_tag_ids<C>(connection: &C) -> Result<HashMap<String, i64>>
where
    C: ConnectionTrait,
{
    Ok(tags::Entity::find()
        .all(connection)
        .await
        .context("reading review tags")?
        .into_iter()
        .map(|tag| (tag.tag, tag.tag_id))
        .collect())
}

async fn replace_image_tags<C>(
    connection: &C,
    image_id: i64,
    values: &[String],
    tags_by_name: &mut HashMap<String, i64>,
) -> Result<()>
where
    C: ConnectionTrait,
{
    image_tags::Entity::delete_many()
        .filter(image_tags::Column::ImageId.eq(image_id))
        .exec(connection)
        .await?;
    insert_image_tags(connection, image_id, values, tags_by_name).await
}

async fn insert_image_tags<C>(
    connection: &C,
    image_id: i64,
    values: &[String],
    tags_by_name: &mut HashMap<String, i64>,
) -> Result<()>
where
    C: ConnectionTrait,
{
    for (position, tag) in values.iter().enumerate() {
        let tag_id = if let Some(tag_id) = tags_by_name.get(tag) {
            *tag_id
        } else {
            tags::Entity::insert(tags::ActiveModel {
                tag: Set(tag.clone()),
                ..Default::default()
            })
            .on_conflict(
                OnConflict::column(tags::Column::Tag)
                    .do_nothing()
                    .to_owned(),
            )
            .exec_without_returning(connection)
            .await
            .context("writing review tag")?;
            let model = tags::Entity::find()
                .filter(tags::Column::Tag.eq(tag.clone()))
                .one(connection)
                .await
                .context("reading review tag id")?
                .ok_or_else(|| anyhow!("inserted review tag {tag:?} is missing"))?;
            tags_by_name.insert(tag.clone(), model.tag_id);
            model.tag_id
        };
        image_tags::Entity::insert(
            image_tags::Model {
                image_id,
                tag_id,
                position: usize_to_i64(position, "image tag position")?,
            }
            .into_active_model(),
        )
        .exec(connection)
        .await
        .context("writing image tag")?;
    }
    Ok(())
}

async fn prune_unused_tags<C>(connection: &C) -> Result<()>
where
    C: ConnectionTrait,
{
    let used = image_tags::Entity::find()
        .select_only()
        .column(image_tags::Column::TagId)
        .into_tuple::<i64>()
        .all(connection)
        .await
        .context("reading used tag ids")?;
    let mut delete = tags::Entity::delete_many();
    if !used.is_empty() {
        delete = delete.filter(tags::Column::TagId.is_not_in(used));
    }
    delete
        .exec(connection)
        .await
        .context("pruning unused review tags")?;
    Ok(())
}

fn settings_model(store: &ReviewStore, roots: &ReviewPathRoots) -> Result<review_settings::Model> {
    Ok(review_settings::Model {
        id: 1,
        next_id: u64_to_i64(store.next_id, "review next_id")?,
        current_image_id: optional_u64_to_i64(store.ui.current_image_id, "current image id")?,
        min_rating: i64::from(store.ui.min_rating),
        exif_schema_version: i64::from(store.exif_schema_version),
        input_root: roots.input_root().to_string_lossy().into_owned(),
        output_root: roots.output_root().to_string_lossy().into_owned(),
        cache_root: roots.cache_root().to_string_lossy().into_owned(),
    })
}

fn enum_text<T: Serialize>(value: &T) -> Result<String> {
    match serde_json::to_value(value).context("serializing enum value")? {
        serde_json::Value::String(text) => Ok(text),
        value => Ok(value.to_string()),
    }
}

fn real(value: f32) -> f64 {
    f64::from(value)
}

fn bool_to_i64(value: bool) -> i64 {
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
