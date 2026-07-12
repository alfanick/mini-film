use super::model::{ReviewCodexJobKey, ReviewCodexScheduler, ScheduledCodexJob};
use super::prelude::*;
use std::sync::Mutex;

const REVIEW_RETOUCH_DEBOUNCE: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReviewMediaKind {
    Thumbnail,
    Preview,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ReviewMediaOrderKey {
    capture_time_missing: bool,
    capture_timestamp: i64,
    relative_path: String,
    image_id: u64,
}

#[derive(Clone, Debug)]
pub(super) struct ScheduledReviewMediaJob {
    pub(super) raw: PathBuf,
    pub(super) image_id: u64,
    order: ReviewMediaOrderKey,
}

pub(super) struct ReviewMediaScheduler {
    pub(super) thumbnails: ArcSwap<HashMap<PathBuf, ScheduledReviewMediaJob>>,
    pub(super) previews: ArcSwap<HashMap<PathBuf, ScheduledReviewMediaJob>>,
    active_thumbnails: Mutex<HashSet<PathBuf>>,
    active_previews: Mutex<HashSet<PathBuf>>,
}

impl Default for ReviewMediaScheduler {
    fn default() -> Self {
        Self {
            thumbnails: ArcSwap::from_pointee(HashMap::new()),
            previews: ArcSwap::from_pointee(HashMap::new()),
            active_thumbnails: Mutex::new(HashSet::new()),
            active_previews: Mutex::new(HashSet::new()),
        }
    }
}

impl ReviewMediaScheduler {
    pub(super) fn schedule(
        &self,
        raw: PathBuf,
        image_id: u64,
        capture_timestamp: Option<i64>,
        relative_path: String,
    ) {
        let job = ScheduledReviewMediaJob {
            raw: raw.clone(),
            image_id,
            order: ReviewMediaOrderKey {
                capture_time_missing: capture_timestamp.is_none(),
                capture_timestamp: capture_timestamp.unwrap_or_default(),
                relative_path,
                image_id,
            },
        };
        for pending in [&self.thumbnails, &self.previews] {
            pending.rcu(|pending| {
                let mut pending = (**pending).clone();
                pending.insert(raw.clone(), job.clone());
                pending
            });
        }
    }

    pub(super) fn next_job(&self, kind: ReviewMediaKind) -> ScheduledReviewMediaJob {
        let (queue, active) = self.queue_and_active(kind);
        loop {
            let mut active = active.lock().unwrap_or_else(|error| error.into_inner());
            let pending = queue.load_full();
            let Some(key) = pending
                .iter()
                .filter(|(key, _)| !active.contains(*key))
                .min_by(|(_, left), (_, right)| left.order.cmp(&right.order))
                .map(|(key, _)| key.clone())
            else {
                drop(active);
                thread::sleep(Duration::from_millis(10));
                continue;
            };
            let mut selected = None;
            queue.rcu(|pending| {
                let mut pending = (**pending).clone();
                selected = pending.remove(&key);
                pending
            });
            if let Some(job) = selected {
                active.insert(key);
                return job;
            }
        }
    }

    pub(super) fn finish(&self, kind: ReviewMediaKind, raw: &Path) {
        let (_, active) = self.queue_and_active(kind);
        active
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(raw);
    }

    fn queue_and_active(
        &self,
        kind: ReviewMediaKind,
    ) -> (
        &ArcSwap<HashMap<PathBuf, ScheduledReviewMediaJob>>,
        &Mutex<HashSet<PathBuf>>,
    ) {
        match kind {
            ReviewMediaKind::Thumbnail => (&self.thumbnails, &self.active_thumbnails),
            ReviewMediaKind::Preview => (&self.previews, &self.active_previews),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct ReviewRetouchJobKey {
    pub(super) raw: PathBuf,
    pub(super) profile_index: Option<usize>,
}

#[derive(Clone, Debug)]
pub(super) struct ScheduledRetouchJob {
    pub(super) raw: PathBuf,
    pub(super) profile_index: Option<usize>,
    pub(super) output: PathBuf,
    pub(super) render_key: String,
    pub(super) due_at: Instant,
}

pub(super) struct ReviewRetouchScheduler {
    pub(super) pending: ArcSwap<HashMap<ReviewRetouchJobKey, ScheduledRetouchJob>>,
}

impl Default for ReviewRetouchScheduler {
    fn default() -> Self {
        Self {
            pending: ArcSwap::from_pointee(HashMap::new()),
        }
    }
}

impl ReviewRetouchScheduler {
    pub(super) fn schedule(
        &self,
        raw: PathBuf,
        profile_index: Option<usize>,
        output: PathBuf,
        render_key: String,
    ) {
        self.schedule_after(
            raw,
            profile_index,
            output,
            render_key,
            REVIEW_RETOUCH_DEBOUNCE,
        );
    }

    pub(super) fn schedule_after(
        &self,
        raw: PathBuf,
        profile_index: Option<usize>,
        output: PathBuf,
        render_key: String,
        delay: Duration,
    ) {
        let key = ReviewRetouchJobKey {
            raw: raw.clone(),
            profile_index,
        };
        let job = ScheduledRetouchJob {
            raw,
            profile_index,
            output,
            render_key,
            due_at: Instant::now() + delay,
        };
        self.pending.rcu(|pending| {
            let mut pending = (**pending).clone();
            pending.insert(key.clone(), job.clone());
            pending
        });
    }

    pub(super) fn next_job(&self) -> ScheduledRetouchJob {
        loop {
            let pending = self.pending.load_full();
            let Some((next_key, next_due)) = pending
                .iter()
                .min_by_key(|(_, job)| job.due_at)
                .map(|(key, job)| (key.clone(), job.due_at))
            else {
                thread::sleep(Duration::from_millis(25));
                continue;
            };
            let now = Instant::now();
            if next_due <= now {
                let mut selected = None;
                self.pending.rcu(|pending| {
                    let mut pending = (**pending).clone();
                    selected = pending.remove(&next_key);
                    pending
                });
                if let Some(job) = selected {
                    return job;
                }
                continue;
            }
            let delay = next_due.saturating_duration_since(now);
            thread::sleep(delay.min(Duration::from_millis(25)));
        }
    }
}

impl ReviewCodexScheduler {
    pub(super) fn schedule(&self, raw: PathBuf, analysis_key: String) {
        let key = ReviewCodexJobKey { raw: raw.clone() };
        let job = ScheduledCodexJob { raw, analysis_key };
        self.pending.rcu(|pending| {
            let mut pending = (**pending).clone();
            pending.insert(key.clone(), job.clone());
            pending
        });
    }

    pub(super) fn next_job(&self) -> ScheduledCodexJob {
        loop {
            let pending = self.pending.load_full();
            let Some(key) = pending.keys().next().cloned() else {
                thread::sleep(Duration::from_millis(25));
                continue;
            };
            let mut selected = None;
            self.pending.rcu(|pending| {
                let mut pending = (**pending).clone();
                selected = pending.remove(&key);
                pending
            });
            if let Some(job) = selected {
                return job;
            }
        }
    }
}
