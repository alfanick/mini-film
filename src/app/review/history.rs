use super::{model::*, prelude::*, store::*};
use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct HistoryEntry {
    title: String,
    lines: Vec<String>,
}

impl HistoryEntry {
    fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            lines: Vec::new(),
        }
    }

    fn line(&mut self, line: impl Into<String>) {
        self.lines.push(line.into());
    }

    fn change<T>(&mut self, label: &str, before: T, after: T)
    where
        T: PartialEq + std::fmt::Display,
    {
        if before != after {
            self.line(format!("{label}: {before} -> {after}"));
        }
    }

    fn optional_change<T>(&mut self, label: &str, before: Option<T>, after: Option<T>)
    where
        T: PartialEq + std::fmt::Display,
    {
        if before != after {
            self.line(format!(
                "{label}: {} -> {}",
                display_optional(before),
                display_optional(after)
            ));
        }
    }

    fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

pub(super) fn append_history_entry(output_root: &Path, entry: HistoryEntry) -> Result<()> {
    fs::create_dir_all(output_root)
        .with_context(|| format!("creating {}", output_root.display()))?;
    let path = output_root.join("history.txt");
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))?;
    let mut writer = BufWriter::new(file);
    writeln!(writer, "{} | {}", now_string(), entry.title)
        .with_context(|| format!("writing {}", path.display()))?;
    for line in entry.lines {
        writeln!(writer, "  {line}").with_context(|| format!("writing {}", path.display()))?;
    }
    writeln!(writer).with_context(|| format!("writing {}", path.display()))?;
    writer
        .flush()
        .with_context(|| format!("flushing {}", path.display()))
}

pub(super) fn history_server_started(
    input_root: &Path,
    output_root: &Path,
    profiles: &[ReviewProfile],
) -> HistoryEntry {
    let mut entry = HistoryEntry::new("review server started");
    entry.line(format!("input: {}", input_root.display()));
    entry.line(format!("output: {}", output_root.display()));
    entry.line(format!("profiles: {}", profiles.len()));
    if !profiles.is_empty() {
        entry.line(format!(
            "profile stems: {}",
            profiles
                .iter()
                .map(|profile| profile.stem.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    entry
}

pub(super) fn history_image_discovered(
    image: &ReviewImage,
    discovered: bool,
    preview_queued: bool,
) -> HistoryEntry {
    let mut entry = HistoryEntry::new(format!(
        "review image {} #{}",
        image.relative_path, image.id
    ));
    if discovered {
        entry.line("discovered raw");
    }
    if preview_queued {
        entry.line("preview queued");
    }
    entry.line(format!("raw: {}", image.raw_path.display()));
    entry.line(format!("profiles: {}", image.profiles.len()));
    entry
}

pub(super) fn history_preview_changed(
    image: &ReviewImage,
    before: &ReviewPreview,
    after: &ReviewPreview,
) -> Option<HistoryEntry> {
    let mut entry = HistoryEntry::new(format!(
        "review preview changed {} #{}",
        image.relative_path, image.id
    ));
    entry.change(
        "status",
        render_status_text(before.status),
        render_status_text(after.status),
    );
    entry.optional_change(
        "path",
        before.path.as_ref().map(|path| path.display().to_string()),
        after.path.as_ref().map(|path| path.display().to_string()),
    );
    entry.optional_change("error", before.error.as_deref(), after.error.as_deref());
    (!entry.is_empty()).then_some(entry)
}

pub(super) fn history_render_changed(
    image: &ReviewImage,
    before: &ReviewProfileRender,
    after: &ReviewProfileRender,
) -> Option<HistoryEntry> {
    let mut entry = HistoryEntry::new(format!(
        "review render changed {} #{} [{}]",
        image.relative_path, image.id, after.profile_stem
    ));
    entry.change(
        "status",
        render_status_text(before.status),
        render_status_text(after.status),
    );
    entry.optional_change(
        "output",
        before
            .output_path
            .as_ref()
            .map(|path| path.display().to_string()),
        after
            .output_path
            .as_ref()
            .map(|path| path.display().to_string()),
    );
    entry.optional_change("error", before.error.as_deref(), after.error.as_deref());
    entry.optional_change("duration", before.duration_ms, after.duration_ms);
    entry.optional_change(
        "retouch key",
        before.render_key.as_deref(),
        after.render_key.as_deref(),
    );
    (!entry.is_empty()).then_some(entry)
}

pub(super) fn history_review_changed(
    before: &ReviewImage,
    after: &ReviewImage,
) -> Option<HistoryEntry> {
    let mut entry = HistoryEntry::new(format!(
        "review metadata changed {} #{}",
        after.relative_path, after.id
    ));
    entry.change("rating", before.rating, after.rating);
    entry.change("labels", labels_text(before), labels_text(after));
    entry.change("tags", list_text(&before.tags), list_text(&after.tags));
    entry.change("notes", quoted(&before.notes), quoted(&after.notes));
    entry.change(
        "selected profile",
        profile_index_text(before.selected_profile_index, before),
        profile_index_text(after.selected_profile_index, after),
    );
    entry.change(
        "publish profiles",
        publish_profiles_text(before),
        publish_profiles_text(after),
    );
    if before.retouch != after.retouch {
        entry.line(format!(
            "retouch: {} -> {}",
            before.retouch.summary(),
            after.retouch.summary()
        ));
    }
    (!entry.is_empty()).then_some(entry)
}

pub(super) fn history_ui_changed(
    store: &ReviewStore,
    before: &ReviewUiState,
    after: &ReviewUiState,
) -> Option<HistoryEntry> {
    let mut entry = HistoryEntry::new("review UI changed");
    entry.change("minimum rating", before.min_rating, after.min_rating);
    entry.change(
        "current image",
        image_id_text(store, before.current_image_id),
        image_id_text(store, after.current_image_id),
    );
    (!entry.is_empty()).then_some(entry)
}

pub(super) fn history_publish_started(
    job: &ReviewPublishJob,
    args: &ReviewPublishCommandArgs,
) -> HistoryEntry {
    let mut entry = HistoryEntry::new(format!("review publish job #{} started", job.id));
    entry.line(format!("album: {}", job.album));
    entry.line(format!("rating >= {}", args.min_rating));
    entry.line(format!("labels: {}", list_text(&args.labels)));
    entry.line(format!("tags: {}", list_text(&args.tags)));
    entry.line(format!("format: {}", args.output_format));
    entry.line(format!("rerender raw: {}", yes_no(args.rerender_raw)));
    entry.line(format!("jobs: {}", args.jobs));
    if let Some(gallery) = args.gallery {
        entry.line(format!(
            "gallery: {gallery} ({} columns, {} px thumbnails)",
            args.gallery_columns, args.gallery_thumbnail_long_edge
        ));
    } else {
        entry.line("gallery: none");
    }
    entry
}

pub(super) fn history_publish_changed(
    before: &ReviewPublishJob,
    after: &ReviewPublishJob,
) -> Option<HistoryEntry> {
    let mut entry = HistoryEntry::new(format!("review publish job #{} changed", after.id));
    entry.change(
        "status",
        publish_status_text(before.status),
        publish_status_text(after.status),
    );
    entry.change("step", before.step.as_str(), after.step.as_str());
    entry.optional_change(
        "current",
        before.current.as_deref(),
        after.current.as_deref(),
    );
    entry.change("processed", before.processed, after.processed);
    entry.change("total", before.total, after.total);
    entry.change("linked", before.linked, after.linked);
    entry.change("skipped", before.skipped, after.skipped);
    entry.change("galleries", before.galleries, after.galleries);
    entry.optional_change("error", before.error.as_deref(), after.error.as_deref());
    entry.optional_change(
        "finished at",
        before.finished_at.as_deref(),
        after.finished_at.as_deref(),
    );
    (!entry.is_empty()).then_some(entry)
}

fn image_id_text(store: &ReviewStore, image_id: Option<u64>) -> String {
    let Some(image_id) = image_id else {
        return "none".to_string();
    };
    store
        .images
        .iter()
        .find(|image| image.id == image_id)
        .map(|image| format!("#{} {}", image.id, image.relative_path))
        .unwrap_or_else(|| format!("#{image_id}"))
}

fn labels_text(image: &ReviewImage) -> String {
    let labels = image_review_labels(image)
        .into_iter()
        .map(review_label_name)
        .collect::<Vec<_>>();
    if labels.is_empty() {
        "none".to_string()
    } else {
        labels.join(", ")
    }
}

fn publish_profiles_text(image: &ReviewImage) -> String {
    let indexes = effective_publish_profile_indexes(image)
        .into_iter()
        .map(|index| profile_index_text(index, image))
        .collect::<Vec<_>>();
    list_text(&indexes)
}

fn profile_index_text(index: usize, image: &ReviewImage) -> String {
    image
        .profiles
        .iter()
        .find(|profile| profile.profile_index == index)
        .map(|profile| format!("{index}:{}", profile.profile_stem))
        .unwrap_or_else(|| index.to_string())
}

fn render_status_text(status: ReviewRenderStatus) -> &'static str {
    match status {
        ReviewRenderStatus::Missing => "missing",
        ReviewRenderStatus::Queued => "queued",
        ReviewRenderStatus::Processing => "processing",
        ReviewRenderStatus::Done => "done",
        ReviewRenderStatus::Failed => "failed",
    }
}

fn publish_status_text(status: ReviewPublishJobStatus) -> &'static str {
    match status {
        ReviewPublishJobStatus::Running => "running",
        ReviewPublishJobStatus::Done => "done",
        ReviewPublishJobStatus::Failed => "failed",
    }
}

fn list_text<T>(items: &[T]) -> String
where
    T: AsRef<str>,
{
    if items.is_empty() {
        "none".to_string()
    } else {
        items
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn quoted(value: &str) -> String {
    if value.is_empty() {
        "none".to_string()
    } else {
        format!("{value:?}")
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn display_optional<T>(value: Option<T>) -> String
where
    T: std::fmt::Display,
{
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string())
}
