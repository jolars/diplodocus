//! Transient timing observations, never semantic or execution evidence.

use super::*;

/// Stable phase names for developer measurements, independent of progress labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Phase {
    /// Fingerprint current inputs and dependencies before reusing a result.
    CacheValidation,
    /// Recheck selected sources and completed dependency observations.
    SourceVerification,
    /// Capture source and dependency subscriptions for a completed attempt.
    WatchCapture,
    /// Scan watched inputs without including the idle polling interval.
    WatchPoll,
    /// Importing or loading a prepared package in a native helper.
    PackageLoading,
    /// Producing native documentation input, such as Doxygen XML.
    DocumentationGeneration,
    /// Translating API declarations and documentation into the shared IR.
    Extraction,
    /// Native process lifetime, including startup, loading, work, and shutdown.
    NativeProcess,
    /// Completing package metadata in the native environment.
    PackageMetadata,
    /// Executing documentation examples or authored documents.
    Examples,
    /// Resolving documentation references and validating relationships.
    ReferenceResolution,
    /// Constructing routes, navigation, and presentation.
    SitePreparation,
    /// Rendering documents and assets.
    Rendering,
    /// Storing a completed snapshot or publishing a site.
    Publication,
}

pub(crate) struct PhaseTiming {
    reporting: Reporting,
    id: u64,
    started: Instant,
    finished: bool,
}

impl Reporting {
    pub(crate) fn phase(
        &self,
        phase: Phase,
        package: Option<&str>,
        target: Option<&str>,
    ) -> PhaseTiming {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        self.emit(Event::PhaseStarted {
            id,
            phase,
            package,
            target,
        });
        PhaseTiming {
            reporting: self.clone(),
            id,
            started: Instant::now(),
            finished: false,
        }
    }

    pub(crate) fn run_phase<T, E>(
        &self,
        phase: Phase,
        label: &'static str,
        work: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        let timing = self.phase(phase, None, None);
        let result = self.run(label, work)?;
        timing.finish();
        Ok(result)
    }

    /// Read optional helper telemetry from private staging, outside the result protocol.
    /// Telemetry cannot make successful semantic work fail or enter a snapshot.
    pub(crate) fn native_timings(&self, path: &Path, package: &str, target: Option<&str>) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        for line in text.lines() {
            let Some((name, seconds)) = line.split_once('\t') else {
                continue;
            };
            let phase = match name {
                "package-loading" => Phase::PackageLoading,
                "documentation-generation" => Phase::DocumentationGeneration,
                "extraction" => Phase::Extraction,
                "examples" => Phase::Examples,
                _ => continue,
            };
            if let Ok(seconds) = seconds.parse::<f64>()
                && let Ok(elapsed) = Duration::try_from_secs_f64(seconds)
            {
                self.emit(Event::NativeTiming {
                    phase,
                    package,
                    target,
                    elapsed,
                });
            }
        }
    }
}

impl PhaseTiming {
    pub(crate) fn finish(mut self) {
        self.end(true);
    }

    fn end(&mut self, success: bool) {
        if !self.finished {
            self.finished = true;
            self.reporting.emit(Event::PhaseFinished {
                id: self.id,
                elapsed: self.started.elapsed(),
                success,
            });
        }
    }
}

impl Drop for PhaseTiming {
    fn drop(&mut self) {
        self.end(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recording(Mutex<Vec<String>>);
    impl Reporter for Recording {
        fn report(&self, event: Event<'_>) {
            self.0.lock().unwrap().push(format!("{event:?}"));
        }
    }

    #[test]
    fn native_telemetry_is_optional_and_rejects_invalid_durations() {
        let recording = Arc::new(Recording::default());
        let reporting = Reporting::new(recording.clone());
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("timings");
        reporting.native_timings(&path, "demo", None);
        std::fs::write(&path, "examples\tNaN\nexamples\t-1\nexamples\tinf\nunknown\t2\ninvalid\npackage-loading\t0.25\n").unwrap();
        reporting.native_timings(&path, "demo", None);
        let events = recording.0.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert!(events[0].contains("PackageLoading"));
    }

    #[test]
    fn failed_and_unwound_phases_finish_once_before_the_command() {
        let recording = Arc::new(Recording::default());
        let reporting = Reporting::new(recording.clone());
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _command = reporting.command("build");
            let _timing = reporting.phase(Phase::Extraction, Some("demo"), Some("api"));
            panic!("test unwind");
        }));
        let events = recording.0.lock().unwrap();
        assert_eq!(events.len(), 4);
        assert!(events[2].starts_with("PhaseFinished"));
        assert!(events[2].contains("success: false"));
        assert!(events[3].starts_with("CommandFinished"));
    }
}
