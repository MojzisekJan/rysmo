//! Browser recovery preferences and a timer that also saves during continuous edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Settings {
    pub enabled: bool,
    pub minutes: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self { enabled: true, minutes: 1 }
    }
}

#[derive(Default)]
pub(crate) struct Timer {
    pending_since: Option<f64>,
    settings: Option<Settings>,
    generation: u64,
    retry_at: Option<f64>,
}

impl Timer {
    /// A failed write remains pending, with a short delay before another attempt.
    #[cfg(any(target_arch = "wasm32", test))]
    pub fn retry(&mut self, now: f64) {
        self.retry_at = Some(now + 5.);
    }

    pub fn due(&mut self, now: f64, pending: bool, generation: u64, settings: Settings) -> bool {
        if self.settings != Some(settings) || self.generation != generation {
            self.pending_since = None;
            self.retry_at = None;
            self.settings = Some(settings);
            self.generation = generation;
        }
        if !settings.enabled || !pending {
            self.retry_at = None;
            self.pending_since = None;
            return false;
        }
        if let Some(at) = self.retry_at {
            if now < at {
                return false;
            }
            self.retry_at = None;
            self.pending_since = None;
            return true;
        }
        let since = *self.pending_since.get_or_insert(now);
        if now - since >= settings.minutes.clamp(1, 120) as f64 * 60. {
            self.pending_since = None;
            true
        } else {
            false
        }
    }
}

/// Revisions repeat after New/Open, so asynchronous results also identify the session.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DocumentVersion {
    pub generation: u64,
    pub revision: u64,
}
#[cfg(any(target_arch = "wasm32", test))]
impl DocumentVersion {
    pub fn same_document(self, current: Self) -> bool {
        self.generation == current.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn continuous_edits_are_saved_at_the_interval_without_repeated_idle_writes() {
        let mut timer = Timer::default();
        let settings = Settings { minutes: 2, ..Default::default() };
        for second in 0..120 {
            assert!(!timer.due(second as f64, true, 0, settings));
        }
        assert!(timer.due(120., true, 0, settings));
        assert!(!timer.due(121., false, 0, settings));
        assert!(!timer.due(999., false, 0, settings));
    }

    #[test]
    fn disabling_changing_interval_or_replacing_document_restarts_the_timer() {
        let mut timer = Timer::default();
        let on = Settings::default();
        assert!(!timer.due(0., true, 0, on));
        assert!(!timer.due(61., true, 0, Settings { enabled: false, ..on }));
        assert!(!timer.due(100., true, 0, on));
        assert!(!timer.due(159., true, 0, on));
        assert!(!timer.due(160., true, 1, on));
        assert!(!timer.due(220., true, 1, Settings { minutes: 2, ..on }));
        assert!(timer.due(340., true, 1, Settings { minutes: 2, ..on }));
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    #[test]
    fn failed_writes_retry_without_waiting_another_full_interval() {
        let mut timer = Timer::default();
        let settings = Settings::default();
        assert!(!timer.due(0., true, 1, settings));
        assert!(timer.due(60., true, 1, settings));
        timer.retry(61.);
        assert!(!timer.due(65., true, 1, settings));
        assert!(timer.due(66., true, 1, settings));
        assert!(!timer.due(67., false, 1, settings));
        assert!(!timer.due(500., false, 1, settings));
    }
    #[test]
    fn disabling_or_replacing_the_document_cancels_a_failed_write_retry() {
        let mut timer = Timer::default();
        let settings = Settings::default();
        assert!(!timer.due(0., true, 1, settings));
        timer.retry(1.);
        assert!(!timer.due(2., true, 1, Settings { enabled: false, ..settings }));
        assert!(!timer.due(8., true, 1, settings));
        timer.retry(9.);
        assert!(!timer.due(15., true, 2, settings));
        assert!(timer.due(75., true, 2, settings));
    }
    #[test]
    fn equal_revisions_in_different_documents_cannot_accept_a_save_result() {
        let saved = DocumentVersion { generation: 1, revision: 3 };
        assert!(!saved.same_document(DocumentVersion { generation: 2, revision: 3 }));
        assert!(saved.same_document(DocumentVersion { generation: 1, revision: 4 }));
        assert_ne!(saved, DocumentVersion { generation: 1, revision: 4 });
        assert_eq!(saved, DocumentVersion { generation: 1, revision: 3 });
    }
}
