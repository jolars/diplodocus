//! Observational progress and diagnostic events, independent of terminal rendering.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::diagnostics::Diagnostic;

/// Receives synchronous notifications without taking ownership of pipeline work.
///
/// Implementations should return promptly. Events may arrive from different
/// threads, and borrowed event data is valid only during this call.
pub trait Reporter: Send + Sync {
    /// Observe one pipeline or command event.
    fn report(&self, event: Event<'_>);
}

/// A command, stage, diagnostic, or preview notification.
#[derive(Debug)]
#[non_exhaustive]
pub enum Event<'a> {
    /// A finite command or one watch attempt began.
    CommandStarted {
        /// Command name, such as `build` or `extract`.
        command: &'static str,
    },
    /// A finite command or watch attempt returned, or was dropped.
    CommandFinished {
        /// Name supplied by the corresponding start event.
        command: &'static str,
        /// Wall-clock time spent in this attempt.
        elapsed: Duration,
        /// Whether all work, including publication, succeeded.
        success: bool,
        /// Published destination, when applicable.
        output: Option<&'a Path>,
        /// Number of generated pages, when applicable.
        pages: Option<usize>,
    },
    /// A nonempty stage began.
    StageStarted {
        /// Unique identifier used by subsequent stage events.
        id: u64,
        /// Human-readable activity label.
        label: &'static str,
        /// Selected work units, or `None` for indeterminate work.
        total: Option<usize>,
    },
    /// A stage selected an item or completed a work unit.
    StageProgress {
        /// Identifier from the start event.
        id: u64,
        /// Successfully completed work units.
        completed: usize,
        /// Current package, target, page, or asset.
        item: Option<&'a str>,
    },
    /// A stage completed, failed, or was dropped during cancellation.
    StageFinished {
        /// Identifier from the start event.
        id: u64,
        /// Wall-clock time spent in this stage.
        elapsed: Duration,
        /// Whether this stage completed successfully.
        success: bool,
    },
    /// A deterministically ordered source diagnostic.
    Diagnostic(&'a Diagnostic),
    /// A command failure, followed separately by any source diagnostics.
    Error(&'a str),
    /// A nonfatal operational warning.
    Warning(&'a str),
    /// The first generation is ready to serve at this browser destination.
    Serving {
        /// Complete local URL, including the actual bound port.
        url: &'a str,
    },
    /// A preview rebuild has replaced the served generation.
    Rebuilt,
    /// The watcher is waiting without active work.
    Watching {
        /// A failed attempt retained the previous published generation.
        retained_previous: bool,
    },
}

#[derive(Clone)]
pub(crate) struct Reporting(Arc<dyn Reporter>);

impl std::fmt::Debug for Reporting {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reporting").finish_non_exhaustive()
    }
}

impl Default for Reporting {
    fn default() -> Self {
        Self(Arc::new(LegacyReporter))
    }
}

impl Reporting {
    pub(crate) fn new(reporter: Arc<dyn Reporter>) -> Self {
        Self(reporter)
    }

    pub(crate) fn emit(&self, event: Event<'_>) {
        self.0.report(event);
    }

    pub(crate) fn stage(&self, label: &'static str, total: Option<usize>) -> Stage {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let active = total != Some(0);
        if active {
            self.emit(Event::StageStarted { id, label, total });
        }
        Stage {
            reporting: self.clone(),
            id,
            started: Instant::now(),
            completed: 0,
            active,
        }
    }

    pub(crate) fn run<T, E>(
        &self,
        label: &'static str,
        work: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        let stage = self.stage(label, None);
        let result = work()?;
        stage.finish();
        Ok(result)
    }

    pub(crate) fn command(&self, command: &'static str) -> Operation {
        self.emit(Event::CommandStarted { command });
        Operation {
            reporting: self.clone(),
            command,
            started: Instant::now(),
            finished: false,
        }
    }

    pub(crate) fn error(&self, error: &crate::commands::CommandError) {
        self.emit(Event::Error(&error.to_string()));
        for diagnostic in error
            .diagnostics()
            .iter()
            .chain(error.cleanup_diagnostics())
        {
            self.emit(Event::Diagnostic(diagnostic));
        }
    }
}

pub(crate) struct Stage {
    reporting: Reporting,
    id: u64,
    started: Instant,
    completed: usize,
    active: bool,
}

impl Stage {
    pub(crate) fn item(&self, item: &str) {
        if self.active {
            self.reporting.emit(Event::StageProgress {
                id: self.id,
                completed: self.completed,
                item: Some(item),
            });
        }
    }

    pub(crate) fn advance(&mut self) {
        self.completed += 1;
        if self.active {
            self.reporting.emit(Event::StageProgress {
                id: self.id,
                completed: self.completed,
                item: None,
            });
        }
    }

    pub(crate) fn finish(mut self) {
        self.end(true);
    }

    fn end(&mut self, success: bool) {
        if self.active {
            self.active = false;
            self.reporting.emit(Event::StageFinished {
                id: self.id,
                elapsed: self.started.elapsed(),
                success,
            });
        }
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        self.end(false);
    }
}

pub(crate) struct Operation {
    reporting: Reporting,
    command: &'static str,
    started: Instant,
    finished: bool,
}

impl Operation {
    pub(crate) fn finish(mut self, output: Option<&Path>, pages: Option<usize>) {
        self.finished = true;
        self.reporting.emit(Event::CommandFinished {
            command: self.command,
            elapsed: self.started.elapsed(),
            success: true,
            output,
            pages,
        });
    }
}

impl Drop for Operation {
    fn drop(&mut self) {
        if !self.finished {
            self.reporting.emit(Event::CommandFinished {
                command: self.command,
                elapsed: self.started.elapsed(),
                success: false,
                output: None,
                pages: None,
            });
        }
    }
}

struct LegacyReporter;

impl Reporter for LegacyReporter {
    fn report(&self, event: Event<'_>) {
        match event {
            Event::Diagnostic(diagnostic) => {
                eprintln!("{}", crate::commands::format_diagnostic(diagnostic));
            }
            Event::Error(message) => eprintln!("error: {}", terminal_safe(message)),
            Event::Warning(message) => eprintln!("Warning: {}", terminal_safe(message)),
            Event::Serving { url } => eprintln!("Serving {}", terminal_safe(url)),
            Event::Rebuilt => eprintln!("Rebuilt documentation."),
            _ => {}
        }
    }
}

/// Escape control characters before adding intentional terminal formatting.
pub(crate) fn terminal_safe(text: &str) -> String {
    text.chars()
        .flat_map(|ch| {
            if ch.is_control() {
                ch.escape_default().collect::<Vec<_>>()
            } else {
                vec![ch]
            }
        })
        .collect()
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
    fn empty_stages_are_silent_and_dropped_work_fails_once() {
        let recorder = Arc::new(Recording::default());
        let reporting = Reporting::new(recorder.clone());
        reporting.stage("Empty", Some(0)).finish();
        assert!(recorder.0.lock().unwrap().is_empty());
        let mut stage = reporting.stage("Pages", Some(2));
        stage.item("first.md");
        stage.advance();
        drop(stage);
        let events = recorder.0.lock().unwrap();
        assert_eq!(events.len(), 4);
        assert!(events[2].contains("completed: 1"));
        assert!(events[3].contains("success: false"));
    }

    #[test]
    fn unwinding_finishes_stages_before_the_command() {
        let recorder = Arc::new(Recording::default());
        let reporting = Reporting::new(recorder.clone());
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _command = reporting.command("build");
            let _stage = reporting.stage("Rendering", None);
            panic!("test unwind");
        }));
        let events = recorder.0.lock().unwrap();
        assert!(events[2].starts_with("StageFinished"));
        assert!(events[3].starts_with("CommandFinished"));
        assert!(events[3].contains("success: false"));
    }
}
