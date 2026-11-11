mod support;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use diplodocus::commands::{BuildOptions, CommandRunner, ExtractOptions};
use diplodocus::execution::ExecutionDeadlines;
use diplodocus::reporting::{Event, Reporter};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
enum Recorded {
    CommandStarted,
    CommandFinished(bool),
    Started(u64, &'static str, Option<usize>),
    Progress(u64, usize, Option<String>),
    Finished(u64, bool),
    Watching(bool),
    Error,
}

#[derive(Default)]
struct Recording {
    events: Mutex<Vec<Recorded>>,
    sender: Option<mpsc::UnboundedSender<Recorded>>,
}

impl Reporter for Recording {
    fn report(&self, event: Event<'_>) {
        let event = match event {
            Event::CommandStarted { .. } => Recorded::CommandStarted,
            Event::CommandFinished {
                success, output, ..
            } => {
                if success && let Some(output) = output {
                    assert!(output.exists());
                }
                Recorded::CommandFinished(success)
            }
            Event::StageStarted { id, label, total } => Recorded::Started(id, label, total),
            Event::StageProgress {
                id,
                completed,
                item,
            } => Recorded::Progress(id, completed, item.map(str::to_owned)),
            Event::StageFinished { id, success, .. } => Recorded::Finished(id, success),
            Event::Watching { retained_previous } => Recorded::Watching(retained_previous),
            Event::Error(_) => Recorded::Error,
            _ => return,
        };
        self.events.lock().unwrap().push(event.clone());
        if let Some(sender) = &self.sender {
            let _ = sender.send(event);
        }
    }
}

fn workspace() -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.yaml", "project:\n  name: Reporting\nrepositories:\n  - id: docs\n    path: .\ncontent:\n  - id: guide\n    owner: project\n    repository: docs\n    path: docs\n    mount: \"\"\n    format: gfm\n");
    root.write("docs/index.md", "# Welcome\n");
    root.write("docs/second.md", "# Second\n");
    root
}

fn balanced_stages(events: &[Recorded]) {
    let mut active = BTreeMap::new();
    for event in events {
        match event {
            Recorded::Started(id, _, total) => {
                assert_ne!(*total, Some(0));
                assert!(active.insert(id, (*total, 0)).is_none());
            }
            Recorded::Progress(id, completed, _) => {
                let (total, previous) = active.get_mut(id).unwrap();
                assert!(*completed >= *previous);
                if let Some(total) = total {
                    assert!(*completed <= *total);
                }
                *previous = *completed;
            }
            Recorded::Finished(id, success) => {
                let (total, completed) = active.remove(id).unwrap();
                if *success && let Some(total) = total {
                    assert_eq!(completed, total);
                }
            }
            Recorded::CommandFinished(_) => assert!(active.is_empty()),
            _ => {}
        }
    }
    assert!(active.is_empty());
}

#[test]
fn builds_report_real_counts_and_one_completion_after_publication() {
    let root = workspace();
    let recording = Arc::new(Recording::default());
    let runner = CommandRunner::with_reporter(recording.clone());
    runner
        .build(BuildOptions {
            config: root.path().join("diplodocus.yaml"),
            output: root.path().join("site"),
        })
        .unwrap();
    let events = recording.events.lock().unwrap();
    balanced_stages(&events);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Recorded::CommandStarted))
            .count(),
        1
    );
    assert!(matches!(
        events.last(),
        Some(Recorded::CommandFinished(true))
    ));
    for label in ["Reading documentation", "Rendering pages"] {
        assert!(
            events.iter().any(
                |event| matches!(event, Recorded::Started(_, name, Some(2)) if *name == label)
            )
        );
    }
    assert!(events.iter().any(
        |event| matches!(event, Recorded::Progress(_, 0, Some(item)) if item.contains("index.md"))
    ));
    assert!(!events.iter().any(
        |event| matches!(event, Recorded::Started(_, label, _) if label.contains("Executing"))
    ));
}

#[test]
fn failed_validation_finishes_the_attempt_without_rendering_or_publication() {
    let root = workspace();
    root.write("docs/index.md", "[`does_not_exist`]\n");
    let recording = Arc::new(Recording::default());
    let runner = CommandRunner::with_reporter(recording.clone());
    assert!(
        runner
            .build(BuildOptions {
                config: root.path().join("diplodocus.yaml"),
                output: root.path().join("site"),
            })
            .is_err()
    );
    let events = recording.events.lock().unwrap();
    balanced_stages(&events);
    assert!(matches!(
        events.last(),
        Some(Recorded::CommandFinished(false))
    ));
    assert!(!events.iter().any(|event| matches!(
        event,
        Recorded::Started(
            _,
            "Rendering pages" | "Publishing snapshot" | "Publishing site",
            _
        )
    )));
}

async fn until_event(
    receiver: &mut mpsc::UnboundedReceiver<Recorded>,
    predicate: impl Fn(&Recorded) -> bool,
) {
    tokio::time::timeout(Duration::from_secs(20), async {
        while let Some(event) = receiver.recv().await {
            if predicate(&event) {
                return;
            }
        }
        panic!("reporter stopped before expected event");
    })
    .await
    .expect("watch did not report expected event");
}

#[tokio::test]
async fn watch_reports_failure_recovery_and_clean_cancellation() {
    let root = workspace();
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let recording = Arc::new(Recording {
        sender: Some(sender),
        ..Recording::default()
    });
    let runner = CommandRunner::with_reporter(recording.clone());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let config = root.path().join("diplodocus.yaml");
    let original = root.read("diplodocus.yaml");
    let task = tokio::spawn(async move {
        runner
            .watch_extract_with(
                ExtractOptions {
                    config,
                    output: None,
                },
                ExecutionDeadlines::default(),
                Box::pin(async {
                    let _ = stopped.await;
                }),
                |_| Ok(()),
            )
            .await
    });
    until_event(&mut receiver, |event| {
        matches!(event, Recorded::Watching(false))
    })
    .await;
    root.write("diplodocus.yaml", "project: [\n");
    until_event(&mut receiver, |event| {
        matches!(event, Recorded::Watching(true))
    })
    .await;
    root.write("diplodocus.yaml", original);
    until_event(&mut receiver, |event| {
        matches!(event, Recorded::Watching(false))
    })
    .await;
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let events = recording.events.lock().unwrap();
    balanced_stages(&events);
    let outcomes: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Recorded::CommandFinished(success) => Some(*success),
            _ => None,
        })
        .collect();
    assert_eq!(outcomes, [true, false, true]);
    assert!(events.iter().any(|event| matches!(event, Recorded::Error)));
}

#[tokio::test]
async fn cancellation_leaves_no_active_stages_or_attempts() {
    let root = workspace();
    let recording = Arc::new(Recording::default());
    let runner = CommandRunner::with_reporter(recording.clone());
    let result = runner
        .extract_with(
            ExtractOptions {
                config: root.path().join("diplodocus.yaml"),
                output: None,
            },
            ExecutionDeadlines::default(),
            Box::pin(std::future::ready(())),
        )
        .await;
    assert!(result.is_err());
    let events = recording.events.lock().unwrap();
    balanced_stages(&events);
    assert!(matches!(
        events.last(),
        Some(Recorded::CommandFinished(false))
    ));
}
