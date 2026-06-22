use super::model::*;
use super::prelude::*;

impl ReviewStore {
    const EXIF_SCHEMA_VERSION: u32 = 2;

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
        let profiles_changed = self.profiles != profiles;
        let old_profiles = self
            .profiles
            .iter()
            .map(|profile| (profile.index, profile.clone()))
            .collect::<HashMap<_, _>>();
        let unchanged_profile_indexes = profiles
            .iter()
            .filter_map(|profile| {
                (old_profiles.get(&profile.index) == Some(profile)).then_some(profile.index)
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
        self.normalize_ui();
    }

    pub(super) fn needs_exif_schema_refresh(&self) -> bool {
        self.exif_schema_version < Self::EXIF_SCHEMA_VERSION
    }

    pub(super) fn mark_exif_schema_refreshed(&mut self) {
        self.exif_schema_version = Self::EXIF_SCHEMA_VERSION;
    }

    pub(super) fn refresh_missing_exif_data(&mut self) {
        self.refresh_missing_exif_data_with_force(false);
    }

    pub(super) fn refresh_missing_exif_data_for_schema(&mut self) {
        self.refresh_missing_exif_data_with_force(true);
    }

    fn refresh_missing_exif_data_with_force(&mut self, force: bool) {
        for image in &mut self.images {
            refresh_image_exif_data(image, force);
            normalize_review_metadata_sources(image);
        }
        self.normalize_ui();
    }

    pub(super) fn ensure_image(
        &mut self,
        input_root: &Path,
        raw: &Path,
    ) -> Result<&mut ReviewImage> {
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
            profiles: Vec::new(),
            updated_at: now_string(),
        };
        sync_image_profile_renders(&mut image, &self.profiles, false, &HashSet::new());
        self.images.push(image);
        self.normalize_ui();
        let index = self.images.len() - 1;
        Ok(&mut self.images[index])
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
    if !force
        && !image.exif.is_empty()
        && image.exif.capture_timestamp.is_some()
        && (image.exif.rating.is_some() || image.rating_source != ReviewMetadataSource::Default)
    {
        image.exif.sanitize_text_fields();
        return;
    }

    let mut refreshed = extract_gallery_exif(&image.raw_path).unwrap_or_default();
    refreshed.sanitize_text_fields();
    if image.exif.is_empty() {
        image.exif = refreshed;
    } else {
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
        if image.exif.lens_model.is_none() {
            image.exif.lens_model = refreshed.lens_model;
        }
        if image.exif.shooting_mode.is_none() {
            image.exif.shooting_mode = refreshed.shooting_mode;
        }
        if image.exif.flash.is_none() {
            image.exif.flash = refreshed.flash;
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

pub(super) fn sync_image_profile_renders(
    image: &mut ReviewImage,
    profiles: &[ReviewProfile],
    profiles_changed: bool,
    unchanged_profile_indexes: &HashSet<usize>,
) {
    if is_jpeg_input_file(&image.raw_path) {
        image.profiles.clear();
        image.selected_profile_index = 0;
        image.publish_profile_indexes = Some(Vec::new());
        return;
    }

    let existing = image
        .profiles
        .iter()
        .cloned()
        .map(|render| (render.profile_index, render))
        .collect::<HashMap<_, _>>();
    image.profiles = profiles
        .iter()
        .map(|profile| {
            existing
                .get(&profile.index)
                .filter(|render| {
                    render.profile_stem == profile.stem
                        && (!profiles_changed || unchanged_profile_indexes.contains(&profile.index))
                })
                .cloned()
                .unwrap_or_else(|| ReviewProfileRender {
                    profile_index: profile.index,
                    profile_stem: profile.stem.clone(),
                    display_name: None,
                    status: ReviewRenderStatus::Missing,
                    output_path: None,
                    error: None,
                    duration_ms: None,
                    render_key: None,
                    width: None,
                    height: None,
                    updated_at: now_string(),
                })
        })
        .collect();
    if image.sooc_sidecar_path.is_some() {
        image.profiles.push(
            existing
                .get(&SOOC_PROFILE_INDEX)
                .filter(|render| render.profile_stem == SOOC_PROFILE_STEM)
                .cloned()
                .unwrap_or_else(|| ReviewProfileRender {
                    profile_index: SOOC_PROFILE_INDEX,
                    profile_stem: SOOC_PROFILE_STEM.to_string(),
                    display_name: Some(SOOC_PROFILE_DISPLAY_NAME.to_string()),
                    status: ReviewRenderStatus::Missing,
                    output_path: None,
                    error: None,
                    duration_ms: None,
                    render_key: None,
                    width: None,
                    height: None,
                    updated_at: now_string(),
                }),
        );
        if let Some(render) = image
            .profiles
            .iter_mut()
            .find(|render| render.profile_index == SOOC_PROFILE_INDEX)
        {
            render.profile_stem = SOOC_PROFILE_STEM.to_string();
            render.display_name = Some(SOOC_PROFILE_DISPLAY_NAME.to_string());
        }
    }
    if profiles_changed {
        image.selected_profile_index = profiles.first().map(|profile| profile.index).unwrap_or(0);
        image.publish_profile_indexes = Some(
            image
                .profiles
                .iter()
                .filter(|profile| profile.profile_index != SOOC_PROFILE_INDEX)
                .map(|profile| profile.profile_index)
                .collect(),
        );
    } else if !image
        .profiles
        .iter()
        .any(|profile| profile.profile_index == image.selected_profile_index)
    {
        image.selected_profile_index = profiles.first().map(|profile| profile.index).unwrap_or(0);
        image.publish_profile_indexes = Some(effective_publish_profile_indexes(image));
    } else {
        image.publish_profile_indexes = Some(effective_publish_profile_indexes(image));
    }
}

pub(super) fn effective_publish_profile_indexes(image: &ReviewImage) -> Vec<usize> {
    if is_jpeg_input_file(&image.raw_path) {
        return Vec::new();
    }
    match &image.publish_profile_indexes {
        Some(indexes) => normalize_publish_profile_indexes(indexes, &image.profiles),
        None => image
            .profiles
            .iter()
            .filter(|profile| profile.profile_index != SOOC_PROFILE_INDEX)
            .map(|profile| profile.profile_index)
            .collect(),
    }
}

pub(super) fn preferred_preview_profile_index(
    image: &ReviewImage,
    _publish_indexes: &[usize],
) -> Option<usize> {
    image
        .profiles
        .iter()
        .find(|profile| profile.profile_index == image.selected_profile_index)
        .or_else(|| image.profiles.first())
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
            selected
                .contains(&profile.profile_index)
                .then_some(profile.profile_index)
        })
        .collect()
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

pub(super) fn load_store(path: &Path) -> Result<Option<ReviewStore>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("parsing {}", path.display()))
        .map(Some)
}

pub(super) fn save_store(path: &Path, store: &ReviewStore) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(store).context("serializing review state")?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, text).with_context(|| format!("writing {}", temp.display()))?;
    fs::rename(&temp, path)
        .with_context(|| format!("renaming {} to {}", temp.display(), path.display()))
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
