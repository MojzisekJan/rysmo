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
}

impl Timer {
    pub fn due(&mut self, now: f64, pending: bool, generation: u64, settings: Settings) -> bool {
        if self.settings != Some(settings) || self.generation != generation {
            self.pending_since = None;
            self.settings = Some(settings);
            self.generation = generation;
        }
        if !settings.enabled || !pending {
            self.pending_since = None;
            return false;
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
