use super::model::{ReviewCodexJobKey, ReviewCodexScheduler, ScheduledCodexJob};
use super::prelude::*;

const REVIEW_RETOUCH_DEBOUNCE: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct ReviewRetouchJobKey {
    pub(super) raw: PathBuf,
    pub(super) profile_index: usize,
}

#[derive(Clone, Debug)]
pub(super) struct ScheduledRetouchJob {
    pub(super) raw: PathBuf,
    pub(super) profile_index: usize,
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
        profile_index: usize,
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
        profile_index: usize,
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
