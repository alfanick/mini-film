use super::model::*;
use super::prelude::*;

const SOOC_RENDER_PIPELINE_KEY: &str = "sooc-managed-symlink-v3";
const PROFILED_COMPRESSED_RENDER_PIPELINE_KEY: &str = "profiled-compressed-render-v3-no-sharpening";
const PROFILED_TIFF_RENDER_PIPELINE_KEY: &str = "profiled-tiff-render-v1-sharpening";

impl ReviewStore {
    const EXIF_SCHEMA_VERSION: u32 = 11;

    pub(super) fn new(profiles: Vec<ReviewProfile>) -> Self {
        Self {
            next_id: 1,
            profiles,
            images: Vec::new(),
            ui: ReviewUiState::default(),
            exif_schema_version: Self::EXIF_SCHEMA_VERSION,
        }
    }

    pub(super) fn sync_profiles(&mut self, profiles: Vec<ReviewProfile>) {
        self.remove_internal_staging_images();
        let mut profiles = profiles;
        for profile in &mut profiles {
            profile.configured_from_cli = true;
            profile.sampler_added = false;
            profile.enabled_by_default = true;
            if profile.identity.trim().is_empty() {
                profile.identity =
                    review_profile_identity(&profile.selector, profile.metadata.as_ref());
            }
        }
        let configured_identities = profiles
            .iter()
            .map(|profile| profile.identity.clone())
            .collect::<HashSet<_>>();
        profiles.extend(
            self.profiles
                .iter()
                .filter(|profile| {
                    profile.sampler_added && !configured_identities.contains(&profile.identity)
                })
                .cloned()
                .map(|mut profile| {
                    profile.configured_from_cli = false;
                    profile
                }),
        );
        make_profile_identities_unique(&mut profiles);
        let old_profiles = self
            .profiles
            .iter()
            .map(|profile| (profile.index, profile.clone()))
            .collect::<HashMap<_, _>>();
        let profiles_changed = profiles.len() != self.profiles.len()
            || profiles.iter().any(|profile| {
                old_profiles
                    .get(&profile.index)
                    .is_none_or(|old_profile| !review_profiles_match(old_profile, profile))
            });
        let unchanged_profile_indexes = profiles
            .iter()
            .filter_map(|profile| {
                old_profiles
                    .get(&profile.index)
                    .filter(|old_profile| review_profiles_match(old_profile, profile))
                    .map(|_| profile.index)
            })
            .collect::<HashSet<_>>();
        self.profiles = profiles;
        let profiles = self.profiles.clone();
        for image in &mut self.images {
            normalize_review_metadata_sources(image);
            if matches!(
                image.preview.status,
                ReviewRenderStatus::Queued | ReviewRenderStatus::Processing
            ) {
                image.preview.status = ReviewRenderStatus::Missing;
                image.preview.updated_at = now_string();
            }
            sync_image_profile_renders(
                image,
                &profiles,
                profiles_changed,
                &unchanged_profile_indexes,
            );
        }
        self.merge_standalone_sooc_sidecars();
        self.normalize_ui();
    }

    pub(super) fn ensure_sampler_profile(&mut self, mut profile: ReviewProfile) -> Result<usize> {
        if profile.identity.trim().is_empty() {
            profile.identity =
                review_profile_identity(&profile.selector, profile.metadata.as_ref());
        }
        if let Some(existing) = self
            .profiles
            .iter()
            .find(|existing| existing.identity == profile.identity)
        {
            return Ok(existing.index);
        }

        let index = self
            .profiles
            .iter()
            .filter(|profile| {
                (SAMPLER_PROFILE_INDEX_BASE..SOOC_PROFILE_INDEX).contains(&profile.index)
            })
            .map(|profile| profile.index)
            .max()
            .map_or(SAMPLER_PROFILE_INDEX_BASE, |index| index + 1);
        if index >= SOOC_PROFILE_INDEX {
            bail!("review sampler profile index space is exhausted");
        }
        profile.index = index;
        profile.stem = unique_sampler_profile_stem(&self.profiles, &profile.stem, index);
        profile.sampler_added = true;
        profile.configured_from_cli = false;
        self.profiles.push(profile);

        let profiles = self.profiles.clone();
        for image in &mut self.images {
            sync_image_profile_renders(image, &profiles, false, &HashSet::new());
        }
        Ok(index)
    }

    pub(super) fn set_profile_enabled_for_image(
        &mut self,
        image_id: u64,
        profile_index: usize,
        enabled: bool,
    ) -> Result<bool> {
        let image = self
            .images
            .iter_mut()
            .find(|image| image.id == image_id)
            .ok_or_else(|| anyhow!("review image {image_id} does not exist"))?;
        set_image_profile_enabled(image, profile_index, enabled)
    }

    pub(super) fn set_profile_enabled_for_all(
        &mut self,
        profile_index: usize,
        enabled: bool,
    ) -> Result<Vec<u64>> {
        let profile = self
            .profiles
            .iter_mut()
            .find(|profile| profile.index == profile_index)
            .ok_or_else(|| anyhow!("review profile {profile_index} does not exist"))?;
        profile.enabled_by_default = enabled;
        let mut newly_enabled = Vec::new();
        for image in &mut self.images {
            if set_image_profile_enabled(image, profile_index, enabled)? {
                newly_enabled.push(image.id);
            }
        }
        Ok(newly_enabled)
    }

    pub(super) fn needs_exif_schema_refresh(&self) -> bool {
        self.exif_schema_version < Self::EXIF_SCHEMA_VERSION
    }

    pub(super) fn mark_exif_schema_refreshed(&mut self) {
        self.exif_schema_version = Self::EXIF_SCHEMA_VERSION;
    }

    pub(super) fn refresh_missing_exif_data(&mut self) -> usize {
        self.refresh_missing_exif_data_with_force(false)
    }

    pub(super) fn refresh_missing_exif_data_for_schema(&mut self) -> usize {
        self.refresh_missing_exif_data_with_force(true)
    }

    fn refresh_missing_exif_data_with_force(&mut self, force: bool) -> usize {
        let refresh_count = self
            .images
            .iter()
            .filter(|image| gallery_exif_needs_refresh(&image.exif, force))
            .count();
        self.images.par_iter_mut().for_each(|image| {
            refresh_image_exif_data(image, force);
            normalize_review_metadata_sources(image);
        });
        self.normalize_ui();
        refresh_count
    }

    pub(super) fn ensure_image(
        &mut self,
        input_root: &Path,
        raw: &Path,
    ) -> Result<&mut ReviewImage> {
        if is_internal_staging_input_file(raw) {
            bail!(
                "refusing internal panorama staging input: {}",
                raw.display()
            );
        }
        if let Some(index) = self.images.iter().position(|image| image.raw_path == raw) {
            refresh_image_exif_data(&mut self.images[index], false);
            return Ok(&mut self.images[index]);
        }

        let id = self.next_id;
        self.next_id += 1;
        let relative = raw
            .strip_prefix(input_root)
            .unwrap_or(raw)
            .to_string_lossy()
            .to_string();
        let file_name = raw
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
            .to_string();
        let mut exif = extract_gallery_exif(raw).unwrap_or_default();
        exif.sanitize_text_fields();
        let imported_rating = exif.rating.unwrap_or_default().min(5);
        let mut image = ReviewImage {
            id,
            raw_path: raw.to_path_buf(),
            sooc_sidecar_path: None,
            relative_path: relative,
            file_name,
            exif,
            preview: ReviewPreview::default(),
            selected_profile_index: 0,
            rating: imported_rating,
            label: ReviewLabel::None,
            labels: Vec::new(),
            tags: Vec::new(),
            notes: String::new(),
            rating_source: if imported_rating > 0 {
                ReviewMetadataSource::Camera
            } else {
                ReviewMetadataSource::Default
            },
            tags_source: ReviewMetadataSource::Default,
            notes_source: ReviewMetadataSource::Default,
            codex: ReviewCodexAnalysis::default(),
            retouch: RetouchSettings::default(),
            publish_profile_indexes: None,
            profile_bw_filters: Vec::new(),
            profiles: Vec::new(),
            updated_at: now_string(),
        };
        sync_image_profile_renders(&mut image, &self.profiles, false, &HashSet::new());
        self.images.push(image);
        self.normalize_ui();
        let index = self.images.len() - 1;
        Ok(&mut self.images[index])
    }

    pub(super) fn rebind_raw_source(
        &mut self,
        input_root: &Path,
        old_path: &Path,
        new_path: &Path,
    ) -> Result<bool> {
        if old_path == new_path {
            return Ok(false);
        }
        if !new_path.is_file() {
            bail!("replacement RAW is missing: {}", new_path.display());
        }
        if old_path.parent() != new_path.parent() || old_path.file_stem() != new_path.file_stem() {
            bail!(
                "replacement RAW must keep the original directory and file stem: {} -> {}",
                old_path.display(),
                new_path.display()
            );
        }

        let old_index = self
            .images
            .iter()
            .position(|image| image.raw_path == old_path);
        let new_index = self
            .images
            .iter()
            .position(|image| image.raw_path == new_path);
        let Some(old_index) = old_index else {
            if new_index.is_some() {
                return Ok(false);
            }
            bail!(
                "review image for replaced RAW does not exist: {}",
                old_path.display()
            );
        };
        if new_index.is_some_and(|index| index != old_index) {
            bail!(
                "cannot replace {} with {} because both paths already belong to review images",
                old_path.display(),
                new_path.display()
            );
        }

        let image = &mut self.images[old_index];
        image.raw_path = new_path.to_path_buf();
        image.relative_path = new_path
            .strip_prefix(input_root)
            .unwrap_or(new_path)
            .to_string_lossy()
            .to_string();
        image.file_name = new_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
            .to_string();
        refresh_image_exif_data(image, true);
        normalize_review_metadata_sources(image);

        if matches!(
            image.preview.status,
            ReviewRenderStatus::Queued
                | ReviewRenderStatus::Processing
                | ReviewRenderStatus::Failed
        ) {
            image.preview = ReviewPreview::default();
        }
        if matches!(
            image.codex.status,
            ReviewCodexStatus::Queued | ReviewCodexStatus::Processing
        ) {
            image.codex.status = ReviewCodexStatus::Missing;
            image.codex.analysis_key = None;
            image.codex.error = None;
            image.codex.updated_at = now_string();
        }
        for render in &mut image.profiles {
            render.processing_key = Some(
                review_render_processing_key_for_input(new_path, render.profile_index).to_string(),
            );
            if render.status == ReviewRenderStatus::Failed {
                render.status = ReviewRenderStatus::Missing;
                render.output_path = None;
                render.error = None;
                render.duration_ms = None;
                render.render_key = None;
                render.width = None;
                render.height = None;
                render.updated_at = now_string();
            }
        }
        image.updated_at = now_string();
        self.normalize_ui();
        Ok(true)
    }

    fn remove_internal_staging_images(&mut self) {
        self.images
            .retain(|image| !is_internal_staging_input_file(&image.raw_path));
    }

    pub(super) fn claim_sooc_sidecar(&mut self, sidecar: &Path) -> bool {
        if !is_jpeg_input_file(sidecar) {
            return false;
        }
        let Some(raw_index) = self.matching_raw_image_index_for_sidecar(sidecar) else {
            return false;
        };
        let profiles = self.profiles.clone();
        let image = &mut self.images[raw_index];
        if image.sooc_sidecar_path.as_deref() == Some(sidecar) {
            return true;
        }
        if image.sooc_sidecar_path.is_some() {
            return false;
        }
        image.sooc_sidecar_path = Some(sidecar.to_path_buf());
        sync_image_profile_renders(image, &profiles, false, &HashSet::new());
        image.updated_at = now_string();
        self.normalize_ui();
        true
    }

    pub(super) fn merge_standalone_sooc_sidecars(&mut self) -> usize {
        let candidates = self
            .images
            .iter()
            .enumerate()
            .filter(|(_, image)| is_jpeg_input_file(&image.raw_path))
            .map(|(index, image)| (index, image.raw_path.clone()))
            .collect::<Vec<_>>();
        let profiles = self.profiles.clone();
        let mut remove_ids = HashSet::new();
        let mut redirect_ids = HashMap::new();

        for (sidecar_index, sidecar_path) in candidates {
            if remove_ids.contains(&self.images[sidecar_index].id) {
                continue;
            }
            let Some(raw_index) = self.matching_raw_image_index_for_sidecar(&sidecar_path) else {
                continue;
            };
            if raw_index == sidecar_index {
                continue;
            }
            if self.images[raw_index]
                .sooc_sidecar_path
                .as_deref()
                .is_some_and(|existing| existing != sidecar_path)
            {
                continue;
            }

            let sidecar = self.images[sidecar_index].clone();
            let raw = &mut self.images[raw_index];
            raw.sooc_sidecar_path = Some(sidecar.raw_path.clone());
            merge_sidecar_review_metadata(raw, &sidecar);
            sync_image_profile_renders(raw, &profiles, false, &HashSet::new());
            raw.updated_at = now_string();
            remove_ids.insert(sidecar.id);
            redirect_ids.insert(sidecar.id, raw.id);
        }

        if remove_ids.is_empty() {
            return 0;
        }
        if let Some(current) = self
            .ui
            .current_image_id
            .and_then(|id| redirect_ids.get(&id).copied())
        {
            self.ui.current_image_id = Some(current);
        }
        self.images.retain(|image| !remove_ids.contains(&image.id));
        self.normalize_ui();
        remove_ids.len()
    }

    fn matching_raw_image_index_for_sidecar(&self, sidecar: &Path) -> Option<usize> {
        if let Some(index) = self.images.iter().position(|image| {
            is_raw_input_file(&image.raw_path)
                && image.sooc_sidecar_path.as_deref() == Some(sidecar)
        }) {
            return Some(index);
        }
        let raw = matching_raw_for_sidecar(sidecar)?;
        self.images
            .iter()
            .position(|image| is_raw_input_file(&image.raw_path) && image.raw_path == raw)
    }

    pub(super) fn normalize_ui(&mut self) {
        self.ui.min_rating = self.ui.min_rating.min(5);
        let visible = self.visible_image_ids_at(self.ui.min_rating);
        if !self
            .ui
            .current_image_id
            .is_some_and(|id| visible.contains(&id))
        {
            self.ui.current_image_id = visible.first().copied();
        }
    }

    pub(super) fn set_ui(&mut self, update: ReviewUiUpdateRequest) -> Result<()> {
        self.ui.min_rating = update.min_rating.min(5);
        if let Some(id) = update.current_image_id {
            if !self.images.iter().any(|image| image.id == id) {
                bail!("review image {id} does not exist");
            }
            self.ui.current_image_id = Some(id);
        }
        self.normalize_ui();
        Ok(())
    }

    pub(super) fn planned_advance_after(&self, image_id: u64) -> ReviewAdvance {
        let visible = self.visible_image_ids_at(self.ui.min_rating);
        let Some(index) = visible.iter().position(|id| *id == image_id) else {
            return ReviewAdvance::FirstVisible;
        };
        if let Some(next) = visible.get(index + 1) {
            ReviewAdvance::Image(*next)
        } else {
            ReviewAdvance::NextPass
        }
    }

    pub(super) fn apply_advance(&mut self, advance: ReviewAdvance) {
        match advance {
            ReviewAdvance::Image(id) => {
                self.ui.current_image_id = Some(id);
                self.normalize_ui();
            }
            ReviewAdvance::FirstVisible => self.normalize_ui(),
            ReviewAdvance::NextPass => {
                self.ui.min_rating = self.ui.min_rating.saturating_add(1).min(5);
                self.ui.current_image_id = self
                    .visible_image_ids_at(self.ui.min_rating)
                    .first()
                    .copied();
                self.normalize_ui();
            }
        }
    }

    pub(super) fn visible_image_ids_at(&self, min_rating: u8) -> Vec<u64> {
        let mut images = self
            .images
            .iter()
            .filter(|image| image.rating >= min_rating.min(5))
            .collect::<Vec<_>>();
        sort_review_image_refs(&mut images);
        images.into_iter().map(|image| image.id).collect()
    }
}

fn review_profiles_match(left: &ReviewProfile, right: &ReviewProfile) -> bool {
    left.index == right.index
        && left.selector == right.selector
        && left.stem == right.stem
        && left.retouch_base == right.retouch_base
}

fn unique_sampler_profile_stem(
    profiles: &[ReviewProfile],
    requested: &str,
    profile_index: usize,
) -> String {
    let requested = requested.trim();
    let requested = if requested.is_empty() {
        "sampler-profile"
    } else {
        requested
    };
    if profiles.iter().all(|profile| profile.stem != requested) {
        return requested.to_string();
    }
    format!(
        "{requested}-sampler-{}",
        profile_index - SAMPLER_PROFILE_INDEX_BASE + 1
    )
}

fn set_image_profile_enabled(
    image: &mut ReviewImage,
    profile_index: usize,
    enabled: bool,
) -> Result<bool> {
    let render = image
        .profiles
        .iter_mut()
        .find(|render| render.profile_index == profile_index)
        .ok_or_else(|| {
            anyhow!(
                "review profile {profile_index} is not available for image {}",
                image.id
            )
        })?;
    let newly_enabled = enabled && !render.enabled;
    render.enabled = enabled;
    if !enabled {
        render.render_key = None;
    }

    let mut publish = image
        .publish_profile_indexes
        .clone()
        .unwrap_or_else(|| effective_publish_profile_indexes(image));
    publish.retain(|index| *index != profile_index);
    if enabled && profile_index != SOOC_PROFILE_INDEX {
        publish.push(profile_index);
    }
    image.publish_profile_indexes =
        Some(normalize_publish_profile_indexes(&publish, &image.profiles));
    if !image
        .profiles
        .iter()
        .any(|render| render.enabled && render.profile_index == image.selected_profile_index)
    {
        image.selected_profile_index = first_enabled_profile_index(image).unwrap_or_default();
    }
    image.updated_at = now_string();
    Ok(newly_enabled)
}

fn make_profile_identities_unique(profiles: &mut [ReviewProfile]) {
    let mut seen = HashSet::new();
    for profile in profiles {
        let base = profile.identity.clone();
        if seen.insert(base.clone()) {
            continue;
        }
        let mut suffix = 2usize;
        loop {
            let candidate = format!("{base}:duplicate-{suffix}");
            if seen.insert(candidate.clone()) {
                profile.identity = candidate;
                break;
            }
            suffix += 1;
        }
    }
}

fn normalize_review_metadata_sources(image: &mut ReviewImage) {
    if image.rating_source == ReviewMetadataSource::Default && image.rating > 0 {
        image.rating_source = ReviewMetadataSource::Manual;
    }
    if image.tags_source == ReviewMetadataSource::Default && !image.tags.is_empty() {
        image.tags_source = ReviewMetadataSource::Manual;
    }
    if image.notes_source == ReviewMetadataSource::Default && !image.notes.trim().is_empty() {
        image.notes_source = ReviewMetadataSource::Manual;
    }
}

fn merge_sidecar_review_metadata(raw: &mut ReviewImage, sidecar: &ReviewImage) {
    if sidecar.rating > 0
        && metadata_source_rank(sidecar.rating_source) > metadata_source_rank(raw.rating_source)
    {
        raw.rating = sidecar.rating;
        raw.rating_source = sidecar.rating_source;
    }

    let labels = image_review_labels(raw)
        .into_iter()
        .chain(image_review_labels(sidecar))
        .collect::<Vec<_>>();
    raw.labels = normalize_review_labels(labels);
    raw.label = first_review_label(&raw.labels);

    if !sidecar.tags.is_empty() {
        let mut tags = raw.tags.clone();
        tags.extend(sidecar.tags.clone());
        raw.tags = normalize_tags(tags);
        if metadata_source_rank(sidecar.tags_source) > metadata_source_rank(raw.tags_source) {
            raw.tags_source = sidecar.tags_source;
        }
    }

    let sidecar_notes = sidecar.notes.trim();
    if !sidecar_notes.is_empty() {
        let raw_notes = raw.notes.trim();
        if raw_notes.is_empty() {
            raw.notes = sidecar_notes.to_string();
            raw.notes_source = sidecar.notes_source;
        } else if raw_notes != sidecar_notes && !raw_notes.contains(sidecar_notes) {
            raw.notes = format!(
                "{}\n\nSOOC sidecar note:\n{}",
                raw.notes.trim_end(),
                sidecar_notes
            );
            raw.notes_source = stronger_metadata_source(raw.notes_source, sidecar.notes_source);
        }
    }

    if raw.retouch.clone().normalized() == RetouchSettings::default()
        && sidecar.retouch.clone().normalized() != RetouchSettings::default()
    {
        raw.retouch = sidecar.retouch.clone().normalized();
    }
}

fn metadata_source_rank(source: ReviewMetadataSource) -> u8 {
    match source {
        ReviewMetadataSource::Default => 0,
        ReviewMetadataSource::Codex => 1,
        ReviewMetadataSource::Camera => 2,
        ReviewMetadataSource::Manual => 3,
    }
}

fn stronger_metadata_source(
    left: ReviewMetadataSource,
    right: ReviewMetadataSource,
) -> ReviewMetadataSource {
    if metadata_source_rank(right) > metadata_source_rank(left) {
        right
    } else {
        left
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReviewAdvance {
    Image(u64),
    FirstVisible,
    NextPass,
}

pub(super) fn sort_review_images(images: &mut [ReviewImage]) {
    images.sort_by(compare_review_images);
}

fn sort_review_image_refs(images: &mut [&ReviewImage]) {
    images.sort_by(|left, right| compare_review_images(left, right));
}

fn compare_review_images(left: &ReviewImage, right: &ReviewImage) -> std::cmp::Ordering {
    match (left.exif.capture_timestamp, right.exif.capture_timestamp) {
        (Some(left_time), Some(right_time)) => left_time
            .cmp(&right_time)
            .then_with(|| left.relative_path.cmp(&right.relative_path))
            .then_with(|| left.id.cmp(&right.id)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => left
            .relative_path
            .cmp(&right.relative_path)
            .then_with(|| left.id.cmp(&right.id)),
    }
}

fn refresh_image_exif_data(image: &mut ReviewImage, force: bool) {
    if !gallery_exif_needs_refresh(&image.exif, force) {
        image.exif.sanitize_text_fields();
        return;
    }

    let mut refreshed = extract_gallery_exif(&image.raw_path).unwrap_or_default();
    refreshed.sanitize_text_fields();
    if image.exif.is_empty() {
        image.exif = refreshed;
    } else {
        if force || image.exif.file_size_bytes.is_none() {
            image.exif.file_size_bytes = refreshed.file_size_bytes.or(image.exif.file_size_bytes);
        }
        if force || image.exif.image_width.is_none() {
            image.exif.image_width = refreshed.image_width.or(image.exif.image_width);
        }
        if force || image.exif.image_height.is_none() {
            image.exif.image_height = refreshed.image_height.or(image.exif.image_height);
        }
        merge_refreshed_focus_data(&mut image.exif, &refreshed, force);
        if image.exif.focal_length.is_none() {
            image.exif.focal_length = refreshed.focal_length;
        }
        if image.exif.aperture.is_none() {
            image.exif.aperture = refreshed.aperture;
        }
        if image.exif.shutter_speed.is_none() {
            image.exif.shutter_speed = refreshed.shutter_speed;
        }
        if image.exif.iso.is_none() {
            image.exif.iso = refreshed.iso;
        }
        if image.exif.camera_model.is_none() {
            image.exif.camera_model = refreshed.camera_model;
        }
        if force || image.exif.auto_iso.is_none() {
            image.exif.auto_iso = refreshed.auto_iso.or(image.exif.auto_iso);
        }
        if force || image.exif.iso_auto_hi_limit.is_none() {
            image.exif.iso_auto_hi_limit = refreshed
                .iso_auto_hi_limit
                .or(image.exif.iso_auto_hi_limit.take());
        }
        if force || image.exif.white_balance_mode.is_none() {
            image.exif.white_balance_mode = refreshed
                .white_balance_mode
                .or(image.exif.white_balance_mode.take());
        }
        if force || image.exif.white_balance_temperature.is_none() {
            image.exif.white_balance_temperature = refreshed
                .white_balance_temperature
                .or(image.exif.white_balance_temperature);
        }
        if force || image.exif.white_balance_offset.is_none() {
            image.exif.white_balance_offset = refreshed
                .white_balance_offset
                .or(image.exif.white_balance_offset);
        }
        if force || image.exif.shutter_count.is_none() {
            image.exif.shutter_count = refreshed.shutter_count.or(image.exif.shutter_count);
        }
        if force || image.exif.shutter_mode.is_none() {
            image.exif.shutter_mode = refreshed.shutter_mode.or(image.exif.shutter_mode.take());
        }
        if force || image.exif.silent_photography.is_none() {
            image.exif.silent_photography = refreshed
                .silent_photography
                .or(image.exif.silent_photography);
        }
        if force || image.exif.release_mode.is_none() {
            image.exif.release_mode = refreshed.release_mode.or(image.exif.release_mode.take());
        }
        if image.exif.lens_model.is_none() {
            image.exif.lens_model = refreshed.lens_model;
        }
        if image.exif.shooting_mode.is_none() {
            image.exif.shooting_mode = refreshed.shooting_mode;
        }
        if image.exif.flash.is_none() {
            image.exif.flash = refreshed.flash;
        }
        if image.exif.active_d_lighting.is_none() {
            image.exif.active_d_lighting = refreshed.active_d_lighting;
        }
        image.exif.capture_timestamp = image.exif.capture_timestamp.or(refreshed.capture_timestamp);
        image.exif.rating = image.exif.rating.or(refreshed.rating);
    }
    if image.rating_source == ReviewMetadataSource::Default
        && let Some(rating) = image.exif.rating
        && rating > 0
    {
        image.rating = rating.min(5);
        image.rating_source = ReviewMetadataSource::Camera;
    }
    image.exif.sanitize_text_fields();
}

pub(super) fn gallery_exif_needs_refresh(exif: &GalleryExifData, force: bool) -> bool {
    force
        || exif.file_size_bytes.is_none()
        || exif.image_width.is_none()
        || exif.image_height.is_none()
}

fn merge_refreshed_focus_data(
    existing: &mut GalleryExifData,
    refreshed: &GalleryExifData,
    force: bool,
) {
    let refreshed_has_focus_data = !refreshed.focus_regions.is_empty()
        && refreshed.focus_frame_width.is_some()
        && refreshed.focus_frame_height.is_some();
    if refreshed_has_focus_data
        && (force
            || existing.focus_regions.is_empty()
            || existing.focus_frame_width.is_none()
            || existing.focus_frame_height.is_none())
    {
        existing.focus_frame_width = refreshed.focus_frame_width;
        existing.focus_frame_height = refreshed.focus_frame_height;
        existing.focus_regions.clone_from(&refreshed.focus_regions);
    }
}

pub(super) fn sync_image_profile_renders(
    image: &mut ReviewImage,
    profiles: &[ReviewProfile],
    profiles_changed: bool,
    unchanged_profile_indexes: &HashSet<usize>,
) {
    let profiles_apply_to_compressed = profiles
        .iter()
        .any(|profile| !profile.selector.trim().is_empty());
    if is_rendered_input_file(&image.raw_path) && !profiles_apply_to_compressed {
        image.profiles.clear();
        image.selected_profile_index = 0;
        image.publish_profile_indexes = Some(Vec::new());
        image.profile_bw_filters.clear();
        return;
    }

    let enabling_profiled_compressed = is_rendered_input_file(&image.raw_path)
        && profiles_apply_to_compressed
        && image.profiles.is_empty();

    let existing = image
        .profiles
        .iter()
        .cloned()
        .map(|render| (render.profile_index, render))
        .collect::<HashMap<_, _>>();
    image.profiles = profiles
        .iter()
        .map(|profile| {
            let processing_key =
                review_render_processing_key_for_input(&image.raw_path, profile.index);
            existing
                .get(&profile.index)
                .filter(|render| {
                    render.profile_stem == profile.stem
                        && render.processing_key.as_deref() == Some(processing_key)
                        && (!profiles_changed || unchanged_profile_indexes.contains(&profile.index))
                })
                .cloned()
                .map(|mut render| {
                    render.processing_key = Some(processing_key.to_string());
                    render
                })
                .unwrap_or_else(|| {
                    missing_profile_render(
                        profile.index,
                        profile.stem.clone(),
                        None,
                        profile.enabled_by_default,
                        processing_key,
                    )
                })
        })
        .collect();
    let include_sooc_profile = image.sooc_sidecar_path.is_some()
        || (is_rendered_input_file(&image.raw_path) && profiles_apply_to_compressed);
    if include_sooc_profile {
        let processing_key = review_render_processing_key(SOOC_PROFILE_INDEX);
        image.profiles.push(
            existing
                .get(&SOOC_PROFILE_INDEX)
                .filter(|render| render.profile_stem == SOOC_PROFILE_STEM)
                .filter(|render| render.processing_key.as_deref() == Some(processing_key))
                .cloned()
                .map(|mut render| {
                    render.processing_key = Some(processing_key.to_string());
                    render
                })
                .unwrap_or_else(|| {
                    missing_profile_render(
                        SOOC_PROFILE_INDEX,
                        SOOC_PROFILE_STEM.to_string(),
                        Some(SOOC_PROFILE_DISPLAY_NAME.to_string()),
                        true,
                        processing_key,
                    )
                }),
        );
        if let Some(render) = image
            .profiles
            .iter_mut()
            .find(|render| render.profile_index == SOOC_PROFILE_INDEX)
        {
            render.profile_stem = SOOC_PROFILE_STEM.to_string();
            render.display_name = Some(SOOC_PROFILE_DISPLAY_NAME.to_string());
            render.processing_key = Some(processing_key.to_string());
        }
    }
    if profiles_changed || enabling_profiled_compressed {
        image.selected_profile_index = first_enabled_profile_index(image).unwrap_or(0);
        image.publish_profile_indexes = Some(
            image
                .profiles
                .iter()
                .filter(|profile| profile.enabled && profile.profile_index != SOOC_PROFILE_INDEX)
                .map(|profile| profile.profile_index)
                .collect(),
        );
    } else if !image
        .profiles
        .iter()
        .any(|profile| profile.enabled && profile.profile_index == image.selected_profile_index)
    {
        image.selected_profile_index = first_enabled_profile_index(image).unwrap_or(0);
        image.publish_profile_indexes = Some(effective_publish_profile_indexes(image));
    } else {
        image.publish_profile_indexes = Some(effective_publish_profile_indexes(image));
    }
    image.profile_bw_filters =
        normalize_profile_bw_filters(&image.profile_bw_filters, &image.profiles);
}

pub(super) fn review_render_processing_key(profile_index: usize) -> &'static str {
    review_render_processing_key_for_input(Path::new("image.raw"), profile_index)
}

pub(super) fn review_render_processing_key_for_input(
    input: &Path,
    profile_index: usize,
) -> &'static str {
    if profile_index == SOOC_PROFILE_INDEX {
        SOOC_RENDER_PIPELINE_KEY
    } else if is_tiff_input_file(input) {
        PROFILED_TIFF_RENDER_PIPELINE_KEY
    } else if is_jpeg_input_file(input) {
        PROFILED_COMPRESSED_RENDER_PIPELINE_KEY
    } else {
        RAW_RENDER_PIPELINE_KEY
    }
}

fn missing_profile_render(
    profile_index: usize,
    profile_stem: String,
    display_name: Option<String>,
    enabled: bool,
    processing_key: &str,
) -> ReviewProfileRender {
    ReviewProfileRender {
        profile_index,
        profile_stem,
        display_name,
        enabled,
        status: ReviewRenderStatus::Missing,
        output_path: None,
        error: None,
        duration_ms: None,
        render_key: None,
        processing_key: Some(processing_key.to_string()),
        width: None,
        height: None,
        updated_at: now_string(),
    }
}

pub(super) fn effective_publish_profile_indexes(image: &ReviewImage) -> Vec<usize> {
    if image_is_direct_compressed(image) {
        return Vec::new();
    }
    match &image.publish_profile_indexes {
        Some(indexes) => normalize_publish_profile_indexes(indexes, &image.profiles),
        None => image
            .profiles
            .iter()
            .filter(|profile| profile.enabled && profile.profile_index != SOOC_PROFILE_INDEX)
            .map(|profile| profile.profile_index)
            .collect(),
    }
}

pub(super) fn image_uses_profile_pipeline(image: &ReviewImage) -> bool {
    !is_rendered_input_file(&image.raw_path)
        || image
            .profiles
            .iter()
            .any(|profile| profile.enabled && profile.profile_index != SOOC_PROFILE_INDEX)
}

pub(super) fn image_is_direct_compressed(image: &ReviewImage) -> bool {
    is_rendered_input_file(&image.raw_path) && !image_uses_profile_pipeline(image)
}

pub(super) fn preferred_preview_profile_index(
    image: &ReviewImage,
    _publish_indexes: &[usize],
) -> Option<usize> {
    image
        .profiles
        .iter()
        .find(|profile| profile.enabled && profile.profile_index == image.selected_profile_index)
        .or_else(|| image.profiles.iter().find(|profile| profile.enabled))
        .map(|profile| profile.profile_index)
}

pub(super) fn normalize_publish_profile_indexes(
    indexes: &[usize],
    profiles: &[ReviewProfileRender],
) -> Vec<usize> {
    let selected = indexes.iter().copied().collect::<HashSet<_>>();
    profiles
        .iter()
        .filter_map(|profile| {
            (profile.enabled && selected.contains(&profile.profile_index))
                .then_some(profile.profile_index)
        })
        .collect()
}

fn first_enabled_profile_index(image: &ReviewImage) -> Option<usize> {
    image
        .profiles
        .iter()
        .find(|profile| profile.enabled)
        .map(|profile| profile.profile_index)
}

pub(super) fn validate_publish_profile_indexes(
    indexes: &[usize],
    profiles: &[ReviewProfileRender],
) -> Result<()> {
    let valid = profiles
        .iter()
        .map(|profile| profile.profile_index)
        .collect::<HashSet<_>>();
    for index in indexes {
        if !valid.contains(index) {
            bail!("publish profile index {index} is not available");
        }
    }
    Ok(())
}

pub(super) fn now_string() -> String {
    chrono::Local::now().to_rfc3339()
}

pub(super) fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for tag in tags {
        let tag = tag.trim();
        if tag.is_empty() || normalized.iter().any(|existing| existing == tag) {
            continue;
        }
        normalized.push(tag.to_string());
    }
    normalized
}

pub(super) fn review_label_name(label: ReviewLabel) -> &'static str {
    match label {
        ReviewLabel::None => "none",
        ReviewLabel::Red => "red",
        ReviewLabel::Yellow => "yellow",
        ReviewLabel::Green => "green",
        ReviewLabel::Blue => "blue",
        ReviewLabel::Purple => "purple",
    }
}

pub(super) fn normalize_review_labels<I>(labels: I) -> Vec<ReviewLabel>
where
    I: IntoIterator<Item = ReviewLabel>,
{
    let selected = labels
        .into_iter()
        .filter(|label| *label != ReviewLabel::None)
        .collect::<HashSet<_>>();
    [
        ReviewLabel::Red,
        ReviewLabel::Yellow,
        ReviewLabel::Green,
        ReviewLabel::Blue,
        ReviewLabel::Purple,
    ]
    .into_iter()
    .filter(|label| selected.contains(label))
    .collect()
}

pub(super) fn first_review_label(labels: &[ReviewLabel]) -> ReviewLabel {
    labels.first().copied().unwrap_or(ReviewLabel::None)
}

pub(super) fn image_review_labels(image: &ReviewImage) -> Vec<ReviewLabel> {
    if image.labels.is_empty() {
        normalize_review_labels([image.label])
    } else {
        normalize_review_labels(image.labels.clone())
    }
}

pub(super) fn review_labels_text(labels: &[ReviewLabel]) -> String {
    if labels.is_empty() {
        "none".to_string()
    } else {
        labels
            .iter()
            .map(|label| review_label_name(*label))
            .collect::<Vec<_>>()
            .join(",")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_optional_exif_does_not_invalidate_cached_metadata() {
        let exif = GalleryExifData {
            file_size_bytes: Some(42),
            image_width: Some(6000),
            image_height: Some(4000),
            ..GalleryExifData::default()
        };

        assert!(!gallery_exif_needs_refresh(&exif, false));
        assert!(gallery_exif_needs_refresh(&exif, true));
    }

    #[test]
    fn exif_schema_refresh_merges_focus_data_without_replacing_existing_metadata() {
        let mut existing = GalleryExifData {
            camera_model: Some("Saved camera".to_string()),
            ..GalleryExifData::default()
        };
        let refreshed = GalleryExifData {
            camera_model: Some("Refreshed camera".to_string()),
            focus_frame_width: Some(8256),
            focus_frame_height: Some(5504),
            focus_regions: vec![GalleryFocusRegion {
                x: 0.4,
                y: 0.45,
                width: 0.05,
                height: 0.08,
                primary: true,
            }],
            ..GalleryExifData::default()
        };

        merge_refreshed_focus_data(&mut existing, &refreshed, true);

        assert_eq!(existing.camera_model.as_deref(), Some("Saved camera"));
        assert_eq!(existing.focus_frame_width, Some(8256));
        assert_eq!(existing.focus_frame_height, Some(5504));
        assert_eq!(existing.focus_regions, refreshed.focus_regions);
    }
}
