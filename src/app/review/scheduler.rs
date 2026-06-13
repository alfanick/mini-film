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

#[derive(Default)]
pub(super) struct ReviewRetouchScheduler {
    pub(super) state: Mutex<ReviewRetouchSchedulerState>,
    pub(super) changed: Condvar,
}

#[derive(Default)]
pub(super) struct ReviewRetouchSchedulerState {
    pub(super) pending: HashMap<ReviewRetouchJobKey, ScheduledRetouchJob>,
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
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.pending.insert(key, job);
        self.changed.notify_one();
    }

    pub(super) fn next_job(&self) -> ScheduledRetouchJob {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        loop {
            if state.pending.is_empty() {
                state = self
                    .changed
                    .wait(state)
                    .unwrap_or_else(|poison| poison.into_inner());
                continue;
            }

            let (next_key, next_due) = state
                .pending
                .iter()
                .min_by_key(|(_, job)| job.due_at)
                .map(|(key, job)| (key.clone(), job.due_at))
                .expect("pending retouch job exists");
            let now = Instant::now();
            if next_due <= now {
                return state
                    .pending
                    .remove(&next_key)
                    .expect("pending retouch job still exists");
            }
            let timeout = next_due.saturating_duration_since(now);
            let (next_state, _) = self
                .changed
                .wait_timeout(state, timeout)
                .unwrap_or_else(|poison| poison.into_inner());
            state = next_state;
        }
    }
}

impl ReviewCodexScheduler {
    pub(super) fn schedule(&self, raw: PathBuf, analysis_key: String) {
        let key = ReviewCodexJobKey { raw: raw.clone() };
        let job = ScheduledCodexJob { raw, analysis_key };
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.pending.insert(key, job);
        self.changed.notify_one();
    }

    pub(super) fn next_job(&self) -> ScheduledCodexJob {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        loop {
            if let Some(key) = state.pending.keys().next().cloned() {
                return state
                    .pending
                    .remove(&key)
                    .expect("pending Codex job still exists");
            }
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(|poison| poison.into_inner());
        }
    }
}
