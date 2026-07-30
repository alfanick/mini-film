use super::model::{
    ReviewCodexJobKey, ReviewCodexScheduler, SOOC_PROFILE_INDEX, ScheduledCodexJob,
};
use super::prelude::*;
use std::sync::Mutex;

const REVIEW_RETOUCH_DEBOUNCE: Duration = Duration::from_secs(2);
const REVIEW_RENDER_PRIORITY_BUCKETS: u8 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ReviewRenderPriorityKey {
    bucket: u8,
    image_order: usize,
    enqueue_sequence: u64,
    image_id: u64,
    profile_index: Option<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct ReviewRenderPrioritySnapshot {
    pub(super) current_image_id: Option<u64>,
    pub(super) images: HashMap<u64, ReviewRenderPriorityImage>,
}

#[derive(Clone, Debug)]
pub(super) struct ReviewRenderPriorityImage {
    pub(super) order: usize,
    pub(super) visible: bool,
    pub(super) main_profile_index: Option<usize>,
    pub(super) enabled_profile_indexes: HashSet<usize>,
}

impl ReviewRenderPrioritySnapshot {
    pub(crate) fn key_for(
        &self,
        image_id: Option<u64>,
        profile_index: Option<usize>,
        enqueue_sequence: u64,
    ) -> Option<ReviewRenderPriorityKey> {
        let Some(image_id) = image_id else {
            return Some(ReviewRenderPriorityKey {
                bucket: REVIEW_RENDER_PRIORITY_BUCKETS,
                image_order: 0,
                enqueue_sequence,
                image_id: 0,
                profile_index,
            });
        };
        let Some(image) = self.images.get(&image_id) else {
            return Some(ReviewRenderPriorityKey {
                bucket: REVIEW_RENDER_PRIORITY_BUCKETS,
                image_order: 0,
                enqueue_sequence,
                image_id,
                profile_index,
            });
        };
        if let Some(profile_index) = profile_index
            && profile_index != SOOC_PROFILE_INDEX
            && !image.enabled_profile_indexes.contains(&profile_index)
        {
            return None;
        }

        let main_profile = profile_index.is_none()
            || profile_index
                .is_some_and(|profile_index| image.main_profile_index == Some(profile_index));
        let current = self.current_image_id == Some(image_id);
        let bucket = match (current, image.visible, main_profile) {
            (true, _, true) => 0,
            (true, _, false) => 1,
            (false, true, true) => 2,
            (false, true, false) => 3,
            (false, false, true) => 4,
            (false, false, false) => 5,
        };
        let image_order = if matches!(bucket, 2..=4) {
            image.order
        } else {
            0
        };
        Some(ReviewRenderPriorityKey {
            bucket,
            image_order,
            enqueue_sequence,
            image_id,
            profile_index,
        })
    }
}

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
    image_id: u64,
    pub(super) profile_index: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct ReviewRetouchRequest {
    pub(super) image_id: u64,
    pub(super) raw: PathBuf,
    pub(super) profile_index: Option<usize>,
    pub(super) output: PathBuf,
    pub(super) render_key: String,
}

#[derive(Clone, Debug)]
pub(super) struct ScheduledRetouchJob {
    pub(super) image_id: u64,
    pub(super) raw: PathBuf,
    pub(super) profile_index: Option<usize>,
    pub(super) output: PathBuf,
    pub(super) render_key: String,
    pub(super) due_at: Instant,
    enqueue_sequence: u64,
}

pub(super) struct ReviewRetouchScheduler {
    pub(super) pending: ArcSwap<HashMap<ReviewRetouchJobKey, ScheduledRetouchJob>>,
    next_enqueue_sequence: AtomicU64,
}

impl Default for ReviewRetouchScheduler {
    fn default() -> Self {
        Self {
            pending: ArcSwap::from_pointee(HashMap::new()),
            next_enqueue_sequence: AtomicU64::new(0),
        }
    }
}

impl ReviewRetouchScheduler {
    pub(super) fn schedule(&self, request: ReviewRetouchRequest) {
        self.schedule_after(request, REVIEW_RETOUCH_DEBOUNCE);
    }

    pub(super) fn schedule_after(&self, request: ReviewRetouchRequest, delay: Duration) {
        let key = ReviewRetouchJobKey {
            image_id: request.image_id,
            profile_index: request.profile_index,
        };
        let job = ScheduledRetouchJob {
            image_id: request.image_id,
            raw: request.raw,
            profile_index: request.profile_index,
            output: request.output,
            render_key: request.render_key,
            due_at: Instant::now() + delay,
            enqueue_sequence: self.next_enqueue_sequence.fetch_add(1, Ordering::Relaxed),
        };
        self.pending.rcu(|pending| {
            let mut pending = (**pending).clone();
            pending.insert(key.clone(), job.clone());
            pending
        });
    }

    pub(super) fn next_job<F>(&self, mut priority_snapshot: F) -> ScheduledRetouchJob
    where
        F: FnMut() -> ReviewRenderPrioritySnapshot,
    {
        loop {
            let pending = self.pending.load_full();
            if pending.is_empty() {
                thread::sleep(Duration::from_millis(25));
                continue;
            }
            let now = Instant::now();
            let next_due = pending
                .values()
                .map(|job| job.due_at)
                .min()
                .expect("non-empty retouch queue has a due time");
            if next_due > now {
                let delay = next_due.saturating_duration_since(now);
                thread::sleep(delay.min(Duration::from_millis(25)));
                continue;
            }
            let priorities = priority_snapshot();
            let mut ineligible = None;
            let next = pending
                .iter()
                .filter_map(|(key, job)| {
                    let Some(priority) = priorities.key_for(
                        Some(job.image_id),
                        job.profile_index,
                        job.enqueue_sequence,
                    ) else {
                        ineligible.get_or_insert_with(|| (key.clone(), job.enqueue_sequence));
                        return None;
                    };
                    (job.due_at <= now).then_some((priority, key.clone(), job.enqueue_sequence))
                })
                .min_by_key(|(priority, _, _)| *priority);
            if let Some((key, enqueue_sequence)) = ineligible {
                self.remove_if_sequence(&key, enqueue_sequence);
                continue;
            }
            if let Some((_, key, enqueue_sequence)) = next {
                if let Some(job) = self.remove_if_sequence(&key, enqueue_sequence) {
                    return job;
                }
                continue;
            }
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn remove_if_sequence(
        &self,
        key: &ReviewRetouchJobKey,
        enqueue_sequence: u64,
    ) -> Option<ScheduledRetouchJob> {
        let mut selected = None;
        self.pending.rcu(|pending| {
            let mut pending = (**pending).clone();
            if pending
                .get(key)
                .is_some_and(|job| job.enqueue_sequence == enqueue_sequence)
            {
                selected = pending.remove(key);
            }
            pending
        });
        selected
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
