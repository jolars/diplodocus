//! Developer-only worker for scripts/bench-pipeline.py. Uses the real preview loop.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use clap::Parser;
use diplodocus::commands::{CommandRunner, ServeOptions};
use diplodocus::reporting::{Event, Reporter};
use serde_json::json;

#[derive(Parser)]
struct Options {
    #[arg(long)]
    config: PathBuf,
    #[arg(long)]
    output: PathBuf,
}

struct JsonReporter(Mutex<std::io::Stdout>);

impl Reporter for JsonReporter {
    fn report(&self, event: Event<'_>) {
        let value = match event {
            Event::CommandStarted { command } => {
                json!({"event": "command-started", "command": command})
            }
            Event::CommandFinished {
                command,
                elapsed,
                success,
                ..
            } => json!({
                "event": "command-finished", "command": command,
                "seconds": elapsed.as_secs_f64(), "success": success,
            }),
            Event::PhaseStarted {
                id,
                phase,
                package,
                target,
            } => json!({
                "event": "phase-started", "id": id, "phase": phase,
                "package": package, "target": target,
            }),
            Event::PhaseFinished {
                id,
                elapsed,
                success,
            } => json!({
                "event": "phase-finished", "id": id,
                "seconds": elapsed.as_secs_f64(), "success": success,
            }),
            Event::NativeTiming {
                phase,
                package,
                target,
                elapsed,
            } => json!({
                "event": "native-timing", "phase": phase, "package": package,
                "target": target, "seconds": elapsed.as_secs_f64(),
            }),
            Event::Serving { .. } => json!({"event": "ready"}),
            Event::Rebuilt => json!({"event": "rebuilt"}),
            Event::Watching { retained_previous } => {
                json!({"event": "watching", "retained_previous": retained_previous})
            }
            Event::Error(message) => json!({"event": "error", "message": message}),
            Event::Diagnostic(diagnostic) => json!({
                "event": "diagnostic", "code": diagnostic.code.as_str(),
                "severity": format!("{:?}", diagnostic.severity),
                "message": diagnostic.message,
            }),
            _ => return,
        };
        let mut output = self.0.lock().expect("benchmark stdout");
        // A disconnected driver still shuts the worker down through SIGTERM.
        let _ = writeln!(output, "{value}");
        let _ = output.flush();
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = Options::parse();
    CommandRunner::with_reporter(Arc::new(JsonReporter(Mutex::new(std::io::stdout())))).serve(
        ServeOptions {
            config: options.config,
            output: options.output,
            host: std::net::Ipv4Addr::LOCALHOST.into(),
            port: 0,
            live_reload: true,
            open: false,
        },
    )?;
    Ok(())
}
