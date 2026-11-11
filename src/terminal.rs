use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{IsTerminal, Write};
use std::sync::Mutex;
use std::time::Duration;

use clap::ColorChoice;
use console::Style;
use diplodocus::commands::format_diagnostic;
use diplodocus::diagnostics::Severity;
use diplodocus::reporting::{Event, Reporter};
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};

#[derive(Clone, Copy)]
struct Capabilities {
    interactive: bool,
    colors: bool,
    hyperlinks: bool,
}

impl Capabilities {
    fn detect(color: ColorChoice) -> Self {
        let interactive = std::io::stderr().is_terminal() && !plain_environment();
        Self {
            interactive,
            colors: match color {
                ColorChoice::Always => true,
                ColorChoice::Never => false,
                ColorChoice::Auto => interactive && console::colors_enabled_stderr(),
            },
            hyperlinks: interactive && supports_hyperlinks::on(supports_hyperlinks::Stream::Stderr),
        }
    }
}

fn plain_environment() -> bool {
    let ci = std::env::var("CI")
        .is_ok_and(|value| !matches!(value.to_ascii_lowercase().as_str(), "" | "0" | "false"));
    ci || std::env::var("TERM").is_ok_and(|value| value == "dumb")
}

fn no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty())
}

/// Resolve color before Clap can exit while displaying help or a usage error.
pub(super) fn early_color(arguments: &[OsString]) -> ColorChoice {
    let mut color = ColorChoice::Auto;
    let mut arguments = arguments.iter().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--" {
            break;
        }
        let value = if argument == "--color" {
            arguments.next().and_then(|value| value.to_str())
        } else {
            argument
                .to_str()
                .and_then(|value| value.strip_prefix("--color="))
        };
        if let Some(choice) = value.and_then(|value| value.parse().ok()) {
            color = choice;
        }
    }
    match color {
        ColorChoice::Auto if plain_environment() || no_color() => ColorChoice::Never,
        _ => color,
    }
}

pub(super) struct ConsoleReporter {
    capabilities: Capabilities,
    animate: bool,
    quiet: bool,
    progress: MultiProgress,
    state: Mutex<State>,
}

struct State {
    stages: HashMap<u64, ActiveStage>,
    writer: Box<dyn Write + Send>,
}

struct ActiveStage {
    label: &'static str,
    total: Option<usize>,
    completed: usize,
    bar: Option<ProgressBar>,
}

impl ConsoleReporter {
    pub(super) fn new(color: ColorChoice, no_progress: bool, quiet: bool) -> Self {
        let mut capabilities = Capabilities::detect(color);
        if color == ColorChoice::Auto && no_color() {
            capabilities.colors = false;
        }
        Self::with_writer(
            capabilities,
            no_progress,
            quiet,
            Box::new(std::io::stderr()),
        )
    }

    fn with_writer(
        capabilities: Capabilities,
        no_progress: bool,
        quiet: bool,
        writer: Box<dyn Write + Send>,
    ) -> Self {
        let animate = capabilities.interactive && !no_progress && !quiet;
        Self {
            capabilities,
            animate,
            quiet,
            progress: MultiProgress::with_draw_target(if animate {
                ProgressDrawTarget::stderr_with_hz(10)
            } else {
                ProgressDrawTarget::hidden()
            }),
            state: Mutex::new(State {
                stages: HashMap::new(),
                writer,
            }),
        }
    }

    fn paint(&self, text: &str, style: Style) -> String {
        style
            .force_styling(self.capabilities.colors)
            .apply_to(text)
            .to_string()
    }

    fn line(&self, state: &mut State, line: &str) {
        // Unlike MultiProgress::println, suspend also writes when bars are hidden.
        self.progress.suspend(|| {
            let _ = writeln!(state.writer, "{line}");
            let _ = state.writer.flush();
        });
    }

    fn link(&self, url: &str) -> String {
        let url = safe(url);
        let label = self.paint(&url, Style::new().cyan().underlined());
        if self.capabilities.hyperlinks {
            format!("\x1b]8;;{url}\x1b\\{label}\x1b]8;;\x1b\\")
        } else {
            label
        }
    }
}

impl Reporter for ConsoleReporter {
    fn report(&self, event: Event<'_>) {
        let mut state = self.state.lock().expect("console reporting lock");
        match event {
            Event::StageStarted { id, label, total } if !self.quiet => {
                let bar = if self.animate {
                    let bar = self.progress.add(match total {
                        Some(total) => ProgressBar::new(total as u64),
                        None => ProgressBar::new_spinner(),
                    });
                    let template = if total.is_some() {
                        "  {spinner} {prefix} {pos}/{len} [{bar:12}] {elapsed} {wide_msg}"
                    } else {
                        "  {spinner} {prefix} {elapsed} {wide_msg}"
                    };
                    bar.set_style(
                        ProgressStyle::with_template(template)
                            .expect("static progress template")
                            .tick_chars("-\\|/-")
                            .progress_chars("=>-"),
                    );
                    bar.set_prefix(self.paint(label, Style::new().cyan().bold()));
                    bar.enable_steady_tick(Duration::from_millis(100));
                    Some(bar)
                } else {
                    self.line(
                        &mut state,
                        &format!("  {}...", self.paint(label, Style::new().cyan())),
                    );
                    None
                };
                state.stages.insert(
                    id,
                    ActiveStage {
                        label,
                        total,
                        completed: 0,
                        bar,
                    },
                );
            }
            Event::StageProgress {
                id,
                completed,
                item,
            } => {
                if let Some(stage) = state.stages.get_mut(&id) {
                    stage.completed = completed;
                    if let Some(bar) = &stage.bar {
                        bar.set_position(completed as u64);
                        if let Some(item) = item {
                            bar.set_message(safe(item));
                        }
                    }
                }
            }
            Event::StageFinished {
                id,
                elapsed,
                success,
            } => {
                if let Some(stage) = state.stages.remove(&id) {
                    if let Some(bar) = stage.bar {
                        bar.finish_and_clear();
                    }
                    let (word, style) = if success {
                        ("Finished", Style::new().green())
                    } else {
                        ("Failed", Style::new().red())
                    };
                    let count = stage
                        .total
                        .map(|total| format!(" ({}/{total})", stage.completed))
                        .unwrap_or_default();
                    let timing = self.paint(
                        &format!("[{:.1}s]", elapsed.as_secs_f64()),
                        Style::new().dim(),
                    );
                    self.line(
                        &mut state,
                        &format!(
                            "  {} {}{count} {timing}",
                            self.paint(word, style),
                            stage.label
                        ),
                    );
                }
            }
            Event::CommandFinished {
                command,
                elapsed,
                success: true,
                output,
                pages,
            } if !self.quiet => {
                let verb = match command {
                    "build" => "Built",
                    "rebuild" => "Rebuilt",
                    "extract" => "Extracted snapshot",
                    "generate" => "Generated",
                    "check" => "Checked documentation",
                    _ => command,
                };
                let count = pages
                    .map(|pages| format!(" {pages} {}", if pages == 1 { "page" } else { "pages" }))
                    .unwrap_or_default();
                let destination = output
                    .map(|path| format!(" -> {}", safe(&path.display().to_string())))
                    .unwrap_or_default();
                self.line(
                    &mut state,
                    &format!(
                        "{}{} in {:.1}s{destination}",
                        self.paint(verb, Style::new().green().bold()),
                        count,
                        elapsed.as_secs_f64()
                    ),
                );
            }
            Event::Diagnostic(diagnostic) => {
                let line = format_diagnostic(diagnostic);
                let boundary = line.find(']').map_or(0, |index| index + 1);
                let style = match diagnostic.severity {
                    Severity::Warning => Style::new().yellow().bold(),
                    Severity::Error => Style::new().red().bold(),
                };
                self.line(
                    &mut state,
                    &format!(
                        "{}{}",
                        self.paint(&line[..boundary], style),
                        &line[boundary..]
                    ),
                );
            }
            Event::Error(message) => self.line(
                &mut state,
                &format!(
                    "{}: {}",
                    self.paint("error", Style::new().red().bold()),
                    safe(message)
                ),
            ),
            Event::Warning(message) => self.line(
                &mut state,
                &format!(
                    "{}: {}",
                    self.paint("warning", Style::new().yellow().bold()),
                    safe(message)
                ),
            ),
            Event::Serving { url } if !self.quiet => {
                self.line(&mut state, &format!("Serving {}", self.link(url)));
            }
            Event::Watching { retained_previous } if !self.quiet => {
                let message = if retained_previous {
                    "Previous output remains available. Watching for changes. Press Ctrl+C to stop."
                } else {
                    "Watching for changes. Press Ctrl+C to stop."
                };
                self.line(&mut state, &self.paint(message, Style::new().dim()));
            }
            _ => {}
        }
    }
}

impl Drop for ConsoleReporter {
    fn drop(&mut self) {
        let _ = self.progress.clear();
    }
}

fn safe(text: &str) -> String {
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
    use std::sync::Arc;

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn hidden_progress_keeps_diagnostics_and_plain_stage_summaries() {
        let buffer = Buffer::default();
        let reporter = ConsoleReporter::with_writer(
            Capabilities {
                interactive: false,
                colors: false,
                hyperlinks: false,
            },
            false,
            false,
            Box::new(buffer.clone()),
        );
        reporter.report(Event::StageStarted {
            id: 1,
            label: "Rendering pages",
            total: Some(2),
        });
        reporter.report(Event::StageProgress {
            id: 1,
            completed: 1,
            item: Some("a.md"),
        });
        reporter.report(Event::Warning("a warning"));
        reporter.report(Event::StageFinished {
            id: 1,
            elapsed: Duration::from_secs(1),
            success: false,
        });
        reporter.report(Event::Serving {
            url: "http://127.0.0.1:8000/",
        });
        let bytes = buffer.0.lock().unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("Rendering pages...\nwarning: a warning\n"));
        assert!(text.contains("Failed Rendering pages (1/2) [1.0s]"));
        assert!(text.contains("Serving http://127.0.0.1:8000/"));
        assert!(!text.contains('\x1b'));
        assert!(reporter.state.lock().unwrap().stages.is_empty());
    }

    #[test]
    fn links_colors_and_control_characters_are_independent() {
        let buffer = Buffer::default();
        let reporter = ConsoleReporter::with_writer(
            Capabilities {
                interactive: false,
                colors: true,
                hyperlinks: true,
            },
            true,
            false,
            Box::new(buffer.clone()),
        );
        reporter.report(Event::Serving {
            url: "http://localhost/",
        });
        reporter.report(Event::Error("unsafe\n\x1b[31m"));
        let bytes = buffer.0.lock().unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("\x1b]8;;http://localhost/\x1b\\"));
        assert!(text.contains("\x1b]8;;\x1b\\"));
        assert!(text.contains("unsafe\\n\\u{1b}[31m"));
        assert!(!text.contains("unsafe\n"));
    }

    #[test]
    fn quiet_suppresses_status_but_keeps_warnings_and_errors() {
        let buffer = Buffer::default();
        let reporter = ConsoleReporter::with_writer(
            Capabilities {
                interactive: true,
                colors: false,
                hyperlinks: true,
            },
            false,
            true,
            Box::new(buffer.clone()),
        );
        reporter.report(Event::StageStarted {
            id: 1,
            label: "Rendering",
            total: None,
        });
        reporter.report(Event::Serving {
            url: "http://localhost/",
        });
        reporter.report(Event::Warning("warning retained"));
        reporter.report(Event::Error("error retained"));
        assert_eq!(
            &*buffer.0.lock().unwrap(),
            b"warning: warning retained\nerror: error retained\n"
        );
        assert!(reporter.state.lock().unwrap().stages.is_empty());
    }
}
