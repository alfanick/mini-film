use super::{entities::*, *};
use sea_orm::{ConnectionTrait, EntityTrait, QueryOrder};
use serde::de::DeserializeOwned;

pub(super) async fn load_store<C>(connection: &C) -> Result<Option<ReviewStore>>
where
    C: ConnectionTrait,
{
    let Some(settings) = review_settings::Entity::find_by_id(1)
        .one(connection)
        .await
        .context("reading review settings")?
    else {
        return Ok(None);
    };

    Ok(Some(ReviewStore {
        next_id: i64_to_u64(settings.next_id, "review next_id")?,
        profiles: load_profiles(connection).await?,
        images: load_images(connection).await?,
        ui: ReviewUiState {
            current_image_id: settings
                .current_image_id
                .map(|value| i64_to_u64(value, "current image id"))
                .transpose()?,
            min_rating: i64_to_u8(settings.min_rating, "minimum rating")?,
        },
        exif_schema_version: i64_to_u32(settings.exif_schema_version, "EXIF schema version")?,
    }))
}

async fn load_profiles<C>(connection: &C) -> Result<Vec<ReviewProfile>>
where
    C: ConnectionTrait,
{
    let rows = profiles::Entity::find()
        .order_by_asc(profiles::Column::Position)
        .all(connection)
        .await
        .context("reading review profiles")?;
    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        let position = i64_to_usize(row.position, "profile position")?;
        require_next_position(position, result.len(), "profile")?;
        let profile_index = i64_to_usize(row.profile_index, "profile index")?;
        let metadata_present = i64_to_bool(row.metadata_present, "metadata_present")?;
        let grain = match (row.grain_amount, row.grain_size, row.grain_frequency) {
            (None, None, None) => None,
            (Some(amount), Some(size), Some(frequency)) => Some(ReviewProfileGrain {
                amount: i64_to_u8(amount, "profile grain amount")?,
                size: i64_to_u8(size, "profile grain size")?,
                frequency: i64_to_u8(frequency, "profile grain frequency")?,
            }),
            _ => bail!("profile {profile_index} has incomplete grain settings"),
        };
        let metadata = if metadata_present {
            Some(ReviewProfileMetadata {
                profile_name: row.profile_name.ok_or_else(|| {
                    anyhow!("profile {profile_index} metadata has no profile name")
                })?,
                profile_uuid: row.profile_uuid,
                look_name: row.look_name,
                look_uuid: row.look_uuid,
                source_profile_name: row.source_profile_name,
                source_profile_uuid: row.source_profile_uuid,
                source_adjustments: ReviewProfileAdjustments::default(),
                source_sharpening: ReviewProfileSharpening::default(),
                emulation_adjustments: ReviewProfileAdjustments::default(),
                emulation_sharpening: ReviewProfileSharpening::default(),
                has_camera_raw_settings: i64_to_bool(
                    row.has_camera_raw_settings,
                    "has_camera_raw_settings",
                )?,
                grain,
                has_hald: i64_to_bool(row.has_hald, "has_hald")?,
                has_pp3: i64_to_bool(row.has_pp3, "has_pp3")?,
                pp3_name: row.pp3_name,
                pp3_adjustments: Vec::new(),
            })
        } else {
            if row.profile_name.is_some() || grain.is_some() {
                bail!("profile {profile_index} has metadata values without metadata_present");
            }
            None
        };
        result.push(ReviewProfile {
            index: profile_index,
            selector: row.selector,
            stem: row.stem,
            retouch_base: BasicRetouchAdjustments {
                exposure: real(row.retouch_exposure),
                highlights: real(row.retouch_highlights),
                shadows: real(row.retouch_shadows),
                whites: real(row.retouch_whites),
                blacks: real(row.retouch_blacks),
                temperature: real(row.retouch_temperature),
                offset: real(row.retouch_offset),
                clarity: real(row.retouch_clarity),
            },
            metadata,
            hald_path: None,
        });
    }

    let indexes = result
        .iter()
        .enumerate()
        .map(|(position, profile)| (profile.index, position))
        .collect::<HashMap<_, _>>();
    if indexes.len() != result.len() {
        bail!("review database contains duplicate profile indexes");
    }
    let adjustments = load_profile_adjustments(connection, &mut result, &indexes).await?;
    let sharpening = load_profile_sharpening(connection, &mut result, &indexes).await?;
    load_profile_hsl_values(connection, &mut result, &indexes).await?;
    load_profile_tone_curve_points(connection, &mut result, &indexes).await?;
    load_profile_pp3_sections(connection, &mut result, &indexes).await?;
    load_profile_pp3_entries(connection, &mut result, &indexes).await?;

    for profile in &result {
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
    Ok(result)
}

async fn load_profile_adjustments<C>(
    connection: &C,
    profiles: &mut [ReviewProfile],
    indexes: &HashMap<usize, usize>,
) -> Result<HashSet<(usize, &'static str)>>
where
    C: ConnectionTrait,
{
    let rows = profile_adjustments::Entity::find()
        .order_by_asc(profile_adjustments::Column::ProfileIndex)
        .order_by_asc(profile_adjustments::Column::Scope)
        .all(connection)
        .await
        .context("reading profile adjustments")?;
    let mut seen = HashSet::new();
    for row in rows {
        let profile_index = i64_to_usize(row.profile_index, "profile index")?;
        let scope = parse_scope(&row.scope)?;
        if !seen.insert((profile_index, scope)) {
            bail!("profile {profile_index} has duplicate {scope} adjustments");
        }
        let adjustments = ReviewProfileAdjustments {
            exposure: real(row.exposure),
            contrast: real(row.contrast),
            highlights: real(row.highlights),
            shadows: real(row.shadows),
            whites: real(row.whites),
            blacks: real(row.blacks),
            saturation: real(row.saturation),
            vibrance: real(row.vibrance),
            clarity: real(row.clarity),
            parametric: ReviewProfileParametricTone {
                shadows: real(row.parametric_shadows),
                darks: real(row.parametric_darks),
                lights: real(row.parametric_lights),
                highlights: real(row.parametric_highlights),
                shadow_split: real(row.parametric_shadow_split),
                midtone_split: real(row.parametric_midtone_split),
                highlight_split: real(row.parametric_highlight_split),
            },
            hsl: ReviewProfileHslAdjustments::default(),
            calibration: ReviewProfileCalibration {
                red_hue: real(row.calibration_red_hue),
                red_saturation: real(row.calibration_red_saturation),
                green_hue: real(row.calibration_green_hue),
                green_saturation: real(row.calibration_green_saturation),
                blue_hue: real(row.calibration_blue_hue),
                blue_saturation: real(row.calibration_blue_saturation),
            },
            tone_curve: ReviewProfileToneCurves::default(),
        };
        let metadata = profile_metadata_mut(profiles, indexes, profile_index)?;
        match scope {
            "source" => metadata.source_adjustments = adjustments,
            "emulation" => metadata.emulation_adjustments = adjustments,
            _ => unreachable!(),
        }
    }
    Ok(seen)
}

async fn load_profile_sharpening<C>(
    connection: &C,
    profiles: &mut [ReviewProfile],
    indexes: &HashMap<usize, usize>,
) -> Result<HashSet<(usize, &'static str)>>
where
    C: ConnectionTrait,
{
    let rows = profile_sharpening::Entity::find()
        .order_by_asc(profile_sharpening::Column::ProfileIndex)
        .order_by_asc(profile_sharpening::Column::Scope)
        .all(connection)
        .await
        .context("reading profile sharpening")?;
    let mut seen = HashSet::new();
    for row in rows {
        let profile_index = i64_to_usize(row.profile_index, "profile index")?;
        let scope = parse_scope(&row.scope)?;
        if !seen.insert((profile_index, scope)) {
            bail!("profile {profile_index} has duplicate {scope} sharpening settings");
        }
        let sharpening = ReviewProfileSharpening {
            present: i64_to_bool(row.present, "profile sharpening present")?,
            amount: real(row.amount),
            radius: real(row.radius),
            detail: real(row.detail),
            masking: real(row.masking),
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

async fn load_profile_hsl_values<C>(
    connection: &C,
    profiles: &mut [ReviewProfile],
    indexes: &HashMap<usize, usize>,
) -> Result<()>
where
    C: ConnectionTrait,
{
    let rows = profile_hsl_values::Entity::find()
        .order_by_asc(profile_hsl_values::Column::ProfileIndex)
        .order_by_asc(profile_hsl_values::Column::Scope)
        .order_by_asc(profile_hsl_values::Column::Channel)
        .order_by_asc(profile_hsl_values::Column::ValueIndex)
        .all(connection)
        .await
        .context("reading profile HSL values")?;
    for row in rows {
        let profile_index = i64_to_usize(row.profile_index, "profile index")?;
        let scope = parse_scope(&row.scope)?;
        let position = i64_to_usize(row.value_index, "HSL value position")?;
        let adjustments = profile_adjustments_mut(
            profile_metadata_mut(profiles, indexes, profile_index)?,
            scope,
        );
        let values = match row.channel.as_str() {
            "hue" => &mut adjustments.hsl.hue,
            "saturation" => &mut adjustments.hsl.saturation,
            "luminance" => &mut adjustments.hsl.luminance,
            channel => bail!("profile {profile_index} has unsupported HSL channel {channel:?}"),
        };
        require_next_position(position, values.len(), "profile HSL value")?;
        values.push(real(row.value));
    }
    Ok(())
}

async fn load_profile_tone_curve_points<C>(
    connection: &C,
    profiles: &mut [ReviewProfile],
    indexes: &HashMap<usize, usize>,
) -> Result<()>
where
    C: ConnectionTrait,
{
    let rows = profile_tone_curve_points::Entity::find()
        .order_by_asc(profile_tone_curve_points::Column::ProfileIndex)
        .order_by_asc(profile_tone_curve_points::Column::Scope)
        .order_by_asc(profile_tone_curve_points::Column::Channel)
        .order_by_asc(profile_tone_curve_points::Column::PointIndex)
        .all(connection)
        .await
        .context("reading profile tone curves")?;
    for row in rows {
        let profile_index = i64_to_usize(row.profile_index, "profile index")?;
        let scope = parse_scope(&row.scope)?;
        let position = i64_to_usize(row.point_index, "tone curve point position")?;
        let adjustments = profile_adjustments_mut(
            profile_metadata_mut(profiles, indexes, profile_index)?,
            scope,
        );
        let points = match row.channel.as_str() {
            "composite" => &mut adjustments.tone_curve.composite,
            "red" => &mut adjustments.tone_curve.red,
            "green" => &mut adjustments.tone_curve.green,
            "blue" => &mut adjustments.tone_curve.blue,
            channel => {
                bail!("profile {profile_index} has unsupported tone curve channel {channel:?}")
            }
        };
        require_next_position(position, points.len(), "tone curve point")?;
        points.push([real(row.x), real(row.y)]);
    }
    Ok(())
}

async fn load_profile_pp3_sections<C>(
    connection: &C,
    profiles: &mut [ReviewProfile],
    indexes: &HashMap<usize, usize>,
) -> Result<()>
where
    C: ConnectionTrait,
{
    let rows = profile_pp3_sections::Entity::find()
        .order_by_asc(profile_pp3_sections::Column::ProfileIndex)
        .order_by_asc(profile_pp3_sections::Column::SectionPosition)
        .all(connection)
        .await
        .context("reading profile PP3 sections")?;
    for row in rows {
        let profile_index = i64_to_usize(row.profile_index, "profile index")?;
        let position = i64_to_usize(row.section_position, "PP3 section position")?;
        let metadata = profile_metadata_mut(profiles, indexes, profile_index)?;
        require_next_position(position, metadata.pp3_adjustments.len(), "PP3 section")?;
        metadata.pp3_adjustments.push(ReviewProfilePp3Section {
            source: row.source,
            section: row.section,
            entries: Vec::new(),
        });
    }
    Ok(())
}

async fn load_profile_pp3_entries<C>(
    connection: &C,
    profiles: &mut [ReviewProfile],
    indexes: &HashMap<usize, usize>,
) -> Result<()>
where
    C: ConnectionTrait,
{
    let rows = profile_pp3_entries::Entity::find()
        .order_by_asc(profile_pp3_entries::Column::ProfileIndex)
        .order_by_asc(profile_pp3_entries::Column::SectionPosition)
        .order_by_asc(profile_pp3_entries::Column::EntryPosition)
        .all(connection)
        .await
        .context("reading profile PP3 entries")?;
    for row in rows {
        let profile_index = i64_to_usize(row.profile_index, "profile index")?;
        let section_position = i64_to_usize(row.section_position, "PP3 section position")?;
        let entry_position = i64_to_usize(row.entry_position, "PP3 entry position")?;
        let section = profile_metadata_mut(profiles, indexes, profile_index)?
            .pp3_adjustments
            .get_mut(section_position)
            .ok_or_else(|| {
                anyhow!(
                    "profile {profile_index} PP3 entry references missing section {section_position}"
                )
            })?;
        require_next_position(entry_position, section.entries.len(), "PP3 entry")?;
        section.entries.push(ReviewProfilePp3Entry {
            key: row.key,
            value: row.value,
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

async fn load_images<C>(connection: &C) -> Result<Vec<ReviewImage>>
where
    C: ConnectionTrait,
{
    let rows = images::Entity::find()
        .order_by_asc(images::Column::Position)
        .all(connection)
        .await
        .context("reading review images")?;
    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        let position = i64_to_usize(row.position, "image position")?;
        require_next_position(position, result.len(), "image")?;
        let image_id = i64_to_u64(row.image_id, "image id")?;
        let crop = match (
            row.retouch_crop_x,
            row.retouch_crop_y,
            row.retouch_crop_width,
            row.retouch_crop_height,
        ) {
            (None, None, None, None) => None,
            (Some(x), Some(y), Some(width), Some(height)) => {
                Some(crate::app::retouch::RetouchCrop {
                    x: real(x),
                    y: real(y),
                    width: real(width),
                    height: real(height),
                })
            }
            _ => bail!("image {image_id} has incomplete crop settings"),
        };
        let publish_profiles_default =
            i64_to_bool(row.publish_profiles_default, "publish_profiles_default")?;
        result.push(ReviewImage {
            id: image_id,
            raw_path: PathBuf::from(row.raw_path),
            sooc_sidecar_path: row.sooc_sidecar_path.map(PathBuf::from),
            relative_path: row.relative_path,
            file_name: row.file_name,
            exif: GalleryExifData {
                capture_timestamp: row.exif_capture_timestamp,
                rating: optional_i64(row.exif_rating, "EXIF rating", i64_to_u8)?,
                file_size_bytes: optional_i64(
                    row.source_file_size_bytes,
                    "source file size",
                    i64_to_u64,
                )?,
                image_width: optional_i64(row.source_width, "source width", i64_to_u32)?,
                image_height: optional_i64(row.source_height, "source height", i64_to_u32)?,
                focal_length: row.exif_focal_length,
                aperture: row.exif_aperture,
                shutter_speed: row.exif_shutter_speed,
                iso: row.exif_iso,
                auto_iso: optional_i64(row.exif_auto_iso, "EXIF Auto ISO", i64_to_bool)?,
                iso_auto_hi_limit: row.exif_iso_auto_hi_limit,
                white_balance_mode: row.exif_white_balance_mode,
                white_balance_temperature: optional_i64(
                    row.exif_white_balance_temperature,
                    "EXIF white balance temperature",
                    i64_to_u32,
                )?,
                white_balance_offset: optional_i64(
                    row.exif_white_balance_offset,
                    "EXIF white balance offset",
                    i64_to_i32,
                )?,
                camera_model: row.exif_camera_model,
                shutter_count: optional_i64(
                    row.exif_shutter_count,
                    "EXIF shutter count",
                    i64_to_u64,
                )?,
                shutter_mode: row.exif_shutter_mode,
                silent_photography: optional_i64(
                    row.exif_silent_photography,
                    "EXIF silent photography",
                    i64_to_bool,
                )?,
                release_mode: row.exif_release_mode,
                lens_model: row.exif_lens_model,
                shooting_mode: row.exif_shooting_mode,
                exposure_compensation: row.exif_exposure_compensation,
                flash: row.exif_flash,
                active_d_lighting: row.exif_active_d_lighting,
                tags: Vec::new(),
                note: row.exif_note,
            },
            preview: ReviewPreview {
                status: parse_enum(&row.preview_status, "preview status")?,
                path: row.preview_path.map(PathBuf::from),
                error: row.preview_error,
                duration_ms: optional_i64(row.preview_duration_ms, "preview duration", i64_to_u64)?,
                render_key: row.preview_render_key,
                updated_at: row.preview_updated_at,
            },
            selected_profile_index: i64_to_usize(
                row.selected_profile_index,
                "selected profile index",
            )?,
            rating: i64_to_u8(row.rating, "image rating")?,
            label: parse_enum(&row.label, "image label")?,
            labels: Vec::new(),
            tags: Vec::new(),
            notes: row.notes,
            rating_source: parse_enum(&row.rating_source, "rating source")?,
            tags_source: parse_enum(&row.tags_source, "tags source")?,
            notes_source: parse_enum(&row.notes_source, "notes source")?,
            codex: ReviewCodexAnalysis {
                status: parse_enum(&row.codex_status, "Codex status")?,
                flags: CodexAnalysisFlags {
                    tags: i64_to_bool(row.codex_flags_tags, "Codex tags flag")?,
                    note: i64_to_bool(row.codex_flags_note, "Codex note flag")?,
                    rating: i64_to_bool(row.codex_flags_rating, "Codex rating flag")?,
                },
                model: row.codex_model,
                analysis_key: row.codex_analysis_key,
                error: row.codex_error,
                updated_at: row.codex_updated_at,
            },
            retouch: RetouchSettings {
                adjustments: BasicRetouchAdjustments {
                    exposure: real(row.retouch_exposure),
                    highlights: real(row.retouch_highlights),
                    shadows: real(row.retouch_shadows),
                    whites: real(row.retouch_whites),
                    blacks: real(row.retouch_blacks),
                    temperature: real(row.retouch_temperature),
                    offset: real(row.retouch_offset),
                    clarity: real(row.retouch_clarity),
                },
                crop,
                rotation_degrees: real(row.retouch_rotation_degrees),
            },
            publish_profile_indexes: (!publish_profiles_default).then(Vec::new),
            profile_bw_filters: Vec::new(),
            profiles: Vec::new(),
            updated_at: row.updated_at,
        });
    }

    let indexes = result
        .iter()
        .enumerate()
        .map(|(position, image)| (image.id, position))
        .collect::<HashMap<_, _>>();
    if indexes.len() != result.len() {
        bail!("review database contains duplicate image ids");
    }
    load_image_exif_tags(connection, &mut result, &indexes).await?;
    load_image_tags(connection, &mut result, &indexes).await?;
    load_image_labels(connection, &mut result, &indexes).await?;
    load_image_publish_profiles(connection, &mut result, &indexes).await?;
    load_image_profile_bw_filters(connection, &mut result, &indexes).await?;
    load_image_profile_renders(connection, &mut result, &indexes).await?;
    Ok(result)
}

async fn load_image_exif_tags<C>(
    connection: &C,
    images: &mut [ReviewImage],
    indexes: &HashMap<u64, usize>,
) -> Result<()>
where
    C: ConnectionTrait,
{
    let rows = image_exif_tags::Entity::find()
        .order_by_asc(image_exif_tags::Column::ImageId)
        .order_by_asc(image_exif_tags::Column::Position)
        .all(connection)
        .await
        .context("reading image EXIF tags")?;
    for row in rows {
        let image_id = i64_to_u64(row.image_id, "image id")?;
        let position = i64_to_usize(row.position, "EXIF tag position")?;
        let image = image_mut(images, indexes, image_id)?;
        require_next_position(position, image.exif.tags.len(), "EXIF tag")?;
        image.exif.tags.push(row.tag);
    }
    Ok(())
}

async fn load_image_tags<C>(
    connection: &C,
    images: &mut [ReviewImage],
    indexes: &HashMap<u64, usize>,
) -> Result<()>
where
    C: ConnectionTrait,
{
    let tags_by_id = tags::Entity::find()
        .all(connection)
        .await
        .context("reading tags")?
        .into_iter()
        .map(|tag| (tag.tag_id, tag.tag))
        .collect::<HashMap<_, _>>();
    let rows = image_tags::Entity::find()
        .order_by_asc(image_tags::Column::ImageId)
        .order_by_asc(image_tags::Column::Position)
        .all(connection)
        .await
        .context("reading image tags")?;
    for row in rows {
        let image_id = i64_to_u64(row.image_id, "image id")?;
        let position = i64_to_usize(row.position, "image tag position")?;
        let tag = tags_by_id
            .get(&row.tag_id)
            .ok_or_else(|| anyhow!("image {image_id} references missing tag {}", row.tag_id))?;
        let image = image_mut(images, indexes, image_id)?;
        require_next_position(position, image.tags.len(), "image tag")?;
        image.tags.push(tag.clone());
    }
    Ok(())
}

async fn load_image_labels<C>(
    connection: &C,
    images: &mut [ReviewImage],
    indexes: &HashMap<u64, usize>,
) -> Result<()>
where
    C: ConnectionTrait,
{
    let rows = image_labels::Entity::find()
        .order_by_asc(image_labels::Column::ImageId)
        .order_by_asc(image_labels::Column::Position)
        .all(connection)
        .await
        .context("reading image labels")?;
    for row in rows {
        let image_id = i64_to_u64(row.image_id, "image id")?;
        let position = i64_to_usize(row.position, "image label position")?;
        let image = image_mut(images, indexes, image_id)?;
        require_next_position(position, image.labels.len(), "image label")?;
        image.labels.push(parse_enum(&row.label, "image label")?);
    }
    Ok(())
}

async fn load_image_publish_profiles<C>(
    connection: &C,
    images: &mut [ReviewImage],
    indexes: &HashMap<u64, usize>,
) -> Result<()>
where
    C: ConnectionTrait,
{
    let rows = image_publish_profiles::Entity::find()
        .order_by_asc(image_publish_profiles::Column::ImageId)
        .order_by_asc(image_publish_profiles::Column::Position)
        .all(connection)
        .await
        .context("reading image publish profiles")?;
    for row in rows {
        let image_id = i64_to_u64(row.image_id, "image id")?;
        let position = i64_to_usize(row.position, "publish profile position")?;
        let profile_index = i64_to_usize(row.profile_index, "publish profile index")?;
        let profiles = image_mut(images, indexes, image_id)?
            .publish_profile_indexes
            .as_mut()
            .ok_or_else(|| {
                anyhow!("image {image_id} has publish profiles while configured to use defaults")
            })?;
        require_next_position(position, profiles.len(), "publish profile")?;
        profiles.push(profile_index);
    }
    Ok(())
}

async fn load_image_profile_bw_filters<C>(
    connection: &C,
    images: &mut [ReviewImage],
    indexes: &HashMap<u64, usize>,
) -> Result<()>
where
    C: ConnectionTrait,
{
    let rows = image_profile_bw_filters::Entity::find()
        .order_by_asc(image_profile_bw_filters::Column::ImageId)
        .order_by_asc(image_profile_bw_filters::Column::Position)
        .all(connection)
        .await
        .context("reading image profile BW filters")?;
    for row in rows {
        let image_id = i64_to_u64(row.image_id, "image id")?;
        let position = i64_to_usize(row.position, "profile BW filter position")?;
        let image = image_mut(images, indexes, image_id)?;
        require_next_position(
            position,
            image.profile_bw_filters.len(),
            "profile BW filter",
        )?;
        image.profile_bw_filters.push(ReviewProfileBwFilter {
            profile_index: i64_to_usize(row.profile_index, "profile BW filter index")?,
            filter: parse_enum(&row.bw_filter, "BW filter")?,
        });
    }
    Ok(())
}

async fn load_image_profile_renders<C>(
    connection: &C,
    images: &mut [ReviewImage],
    indexes: &HashMap<u64, usize>,
) -> Result<()>
where
    C: ConnectionTrait,
{
    let rows = image_profile_renders::Entity::find()
        .order_by_asc(image_profile_renders::Column::ImageId)
        .order_by_asc(image_profile_renders::Column::Position)
        .all(connection)
        .await
        .context("reading image profile renders")?;
    for row in rows {
        let image_id = i64_to_u64(row.image_id, "image id")?;
        let position = i64_to_usize(row.position, "profile render position")?;
        let image = image_mut(images, indexes, image_id)?;
        require_next_position(position, image.profiles.len(), "profile render")?;
        image.profiles.push(ReviewProfileRender {
            profile_index: i64_to_usize(row.profile_index, "profile render index")?,
            profile_stem: row.profile_stem,
            display_name: row.display_name,
            status: parse_enum(&row.status, "profile render status")?,
            output_path: row.output_path.map(PathBuf::from),
            error: row.error,
            duration_ms: optional_i64(row.duration_ms, "profile render duration", i64_to_u64)?,
            render_key: row.render_key,
            processing_key: row.processing_key,
            width: optional_i64(row.width, "profile render width", i64_to_u32)?,
            height: optional_i64(row.height, "profile render height", i64_to_u32)?,
            updated_at: row.updated_at,
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

fn optional_i64<T>(
    value: Option<i64>,
    name: &str,
    convert: fn(i64, &str) -> Result<T>,
) -> Result<Option<T>> {
    value.map(|value| convert(value, name)).transpose()
}

fn real(value: f64) -> f32 {
    value as f32
}

pub(super) fn i64_to_bool(value: i64, name: &str) -> Result<bool> {
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

fn i64_to_i32(value: i64, name: &str) -> Result<i32> {
    i32::try_from(value).with_context(|| format!("{name} {value} exceeds SQLite i32 range"))
}

fn i64_to_u64(value: i64, name: &str) -> Result<u64> {
    u64::try_from(value).with_context(|| format!("{name} {value} does not fit u64"))
}

fn i64_to_usize(value: i64, name: &str) -> Result<usize> {
    usize::try_from(value).with_context(|| format!("{name} {value} does not fit usize"))
}
