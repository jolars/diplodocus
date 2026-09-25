#![cfg(target_os = "linux")]
mod support;

use std::collections::BTreeMap;
use std::mem::MaybeUninit;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use diplodocus::commands::{self, CheckOptions};
use diplodocus::snapshots::Snapshot;
use rustix::fs::inotify::{self, CreateFlags, ReadFlags, WatchFlags};
use snapbox::cmd::cargo_bin;

// Inotify retains transient accesses and writes that a final tree comparison misses.
struct Monitor {
    fd: rustix::fd::OwnedFd,
    paths: BTreeMap<i32, PathBuf>,
}

impl Monitor {
    fn new() -> Self {
        Self {
            fd: inotify::init(CreateFlags::NONBLOCK | CreateFlags::CLOEXEC).unwrap(),
            paths: BTreeMap::new(),
        }
    }

    fn watch(&mut self, path: &Path, flags: WatchFlags) {
        // Setup directory handles can close after a watch is installed. Opens and
        // reads detect discovery without counting those delayed closes as execution.
        let flags = flags & !WatchFlags::CLOSE_NOWRITE;
        let wd = inotify::add_watch(&self.fd, path, flags | WatchFlags::MASK_ADD).unwrap();
        self.paths.insert(wd, path.to_owned());
    }

    fn tree(&mut self, path: &Path, flags: WatchFlags) {
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                self.tree(&path, flags);
            }
        }
        self.watch(path, flags);
    }

    fn events(&self) -> Vec<(PathBuf, ReadFlags)> {
        let mut buffer = [MaybeUninit::uninit(); 8192];
        let mut reader = inotify::Reader::new(&self.fd, &mut buffer);
        let mut events = Vec::new();
        loop {
            match reader.next() {
                Err(rustix::io::Errno::AGAIN) => return events,
                Err(error) => panic!("filesystem monitor failed: {error}"),
                Ok(event) => {
                    assert!(!event.events().contains(ReadFlags::QUEUE_OVERFLOW));
                    let mut path = self.paths[&event.wd()].clone();
                    if let Some(name) = event.file_name() {
                        path.push(name.to_str().unwrap());
                    }
                    events.push((path, event.events()));
                }
            }
        }
    }

    fn assert_quiet(&self) {
        assert_eq!(self.events(), [], "unexpected execution I/O");
    }
}

fn writes() -> WatchFlags {
    WatchFlags::MODIFY
        | WatchFlags::ATTRIB
        | WatchFlags::CLOSE_WRITE
        | WatchFlags::CREATE
        | WatchFlags::DELETE
        | WatchFlags::DELETE_SELF
        | WatchFlags::MOVED_FROM
        | WatchFlags::MOVED_TO
        | WatchFlags::MOVE_SELF
}

#[test]
fn monitors_ignore_closing_directories_opened_before_watching() {
    let root = support::TestWorkspace::new();
    let directory = std::fs::read_dir(root.path()).unwrap();
    let mut monitor = Monitor::new();
    monitor.watch(root.path(), WatchFlags::ALL_EVENTS);
    drop(directory);
    monitor.assert_quiet();

    drop(std::fs::read_dir(root.path()).unwrap());
    assert!(
        monitor
            .events()
            .iter()
            .any(|(_, flags)| flags.contains(ReadFlags::OPEN))
    );
}

struct Isolation {
    root: support::TestWorkspace,
    private: support::TestWorkspace,
    monitor: Monitor,
}

impl Isolation {
    fn new(mode: &str, populated: bool) -> Self {
        let root = support::TestWorkspace::new();
        let private = support::TestWorkspace::new();
        root.write("diplodocus.toml", format!("[project]\nname='Isolation'\n[[repository]]\nid='docs'\npath='.'\n[[content]]\nid='guide'\nowner='project'\nrepository='docs'\npath='guide'\nmount=''\nformat='qmd'\n{mode}"));
        root.write("guide/index.qmd", source("Initial"));
        if populated {
            root.write(
                ".diplodocus/cache/execution/v1/sha256/poison/artifact.json",
                "invalid cached artifact",
            );
            root.write(
                ".diplodocus/cache/execution/v1/sha256/poison/assets/retained",
                "retained asset",
            );
        }
        private.write("runtime/keep", "runtime sentinel");
        private.write("data/kernels/sentinel/kernel.json", serde_json::to_vec(&serde_json::json!({
            "argv": [std::fs::canonicalize("/bin/sh").unwrap(), "-c",
                "printf started > \"$DIPLODOCUS_START_MARKER\"; exit 1", "sentinel", "{connection_file}"],
            "language": "python", "display_name": "Startup sentinel",
            "env": {"DIPLODOCUS_START_MARKER": private.path().join("started")}
        })).unwrap());
        let mut monitor = Monitor::new();
        monitor.tree(&private.path().join("data"), WatchFlags::ALL_EVENTS);
        if populated {
            monitor.tree(
                &root.path().join(".diplodocus/cache"),
                WatchFlags::ALL_EVENTS,
            );
        }
        Self {
            root,
            private,
            monitor,
        }
    }

    fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut command = Command::new(program);
        command
            .current_dir(self.root.path())
            .env("JUPYTER_PATH", self.private.path().join("data"))
            .env("JUPYTER_DATA_DIR", self.private.path().join("data"))
            .env("TMPDIR", self.private.path().join("runtime"))
            .env("PATH", "");
        command
    }

    fn assert_quiet(&self) {
        self.monitor.assert_quiet();
        assert!(!self.private.path().join("started").exists());
        assert!(!self.root.path().join("guide/executed").exists());
        assert_eq!(
            support::files_under(&self.private.path().join("runtime")),
            [PathBuf::from("keep")]
        );
    }
}

fn source(title: &str) -> String {
    format!(
        "# {title}\n\n```{{python}}\n#| eval: true\nfrom pathlib import Path\nPath('executed').write_text('ran')\nfrom IPython.display import display, SVG\ndisplay(SVG(\"<svg xmlns='http://www.w3.org/2000/svg'><circle r='3'/></svg>\"))\n```\n\n```{{r}}\nwriteLines('ran', 'executed')\nIRdisplay::display_svg(\"<svg xmlns='http://www.w3.org/2000/svg'><circle r='3'/></svg>\")\n```\n"
    )
}

const ENABLED: &str = "[content.execution]\nmode='execute'\nengine='jupyter'\nkernel='sentinel'\n";
const NEVER: &str = "[content.execution]\nmode='never'\n";

fn finish(mut child: std::process::Child) -> Output {
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= until {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!("command timed out: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn run(command: &mut Command) -> Output {
    finish(
        command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    )
}

// The library runs in a child so the test never changes a shared process environment.
#[test]
fn library_check_child() {
    let Ok(expected) = std::env::var("DIPLODOCUS_ISOLATED_CHECK") else {
        return;
    };
    let result = commands::check(CheckOptions {
        config: "diplodocus.toml".into(),
    });
    assert_eq!(result.is_ok(), expected == "success", "{result:?}");
}

#[test]
fn library_execute_child() {
    if std::env::var_os("DIPLODOCUS_ISOLATED_EXECUTE").is_none() {
        return;
    }
    let sources = diplodocus::assembly::assemble_workspace("diplodocus.toml").unwrap();
    let staging = PathBuf::from(std::env::var_os("TMPDIR").unwrap());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let executed = runtime
        .block_on(sources.execute(diplodocus::assembly::WorkspaceExecution {
            staging_parent: &staging,
            deadlines: diplodocus::execution::ExecutionDeadlines::default(),
            cancellation: Box::pin(std::future::pending()),
        }))
        .unwrap();
    assert!(executed.executed_pages().is_empty());
    executed.discard().unwrap();
}

#[test]
fn every_check_exit_path_avoids_discovery_execution_and_writes() {
    for mode in ["", NEVER, ENABLED] {
        for populated in [false, true] {
            for case in [
                "success",
                "warning",
                "missing-config",
                "invalid-config",
                "invalid-declaration",
                "invalid-execution",
                "document-authority",
                "missing-content",
                "invalid-qmd",
                "reference",
                "asset",
            ] {
                let mut fixture = Isolation::new(mode, populated);
                let config = fixture.root.read("diplodocus.toml");
                match case {
                    "missing-config" => fixture.root.remove("diplodocus.toml"),
                    "invalid-config" => fixture.root.write("diplodocus.toml", "["),
                    "invalid-declaration" => fixture.root.write(
                        "diplodocus.toml",
                        config.replace("owner='project'", "owner='missing'"),
                    ),
                    "invalid-execution" => fixture.root.write(
                        "diplodocus.toml",
                        format!(
                            "{}[content.execution]\nmode='never'\nkernel='sentinel'\n",
                            config.split("[content.execution]").next().unwrap()
                        ),
                    ),
                    "document-authority" => fixture
                        .root
                        .write("guide/index.qmd", "---\njupyter: sentinel\n---\n"),
                    "missing-content" => fixture.root.write(
                        "diplodocus.toml",
                        config.replace("path='guide'", "path='absent'"),
                    ),
                    "invalid-qmd" => fixture
                        .root
                        .write("guide/index.qmd", "---\nexecute: [true]\n---\n"),
                    "reference" => fixture.root.write("guide/index.qmd", "[`missing-item`]\n"),
                    "asset" => fixture
                        .root
                        .write("guide/index.qmd", "![missing](absent.png)\n"),
                    "warning" => fixture.root.write(
                        "guide/index.qmd",
                        format!("{}\n<div>raw html</div>\n", source("Warning")),
                    ),
                    _ => {}
                }
                fixture.monitor.tree(fixture.root.path(), writes());
                fixture
                    .monitor
                    .tree(&fixture.private.path().join("runtime"), writes());
                let _ = fixture.monitor.events();
                let success = matches!(case, "success" | "warning");
                for explicit in [false, true] {
                    let mut command = fixture.command(cargo_bin("diplodocus"));
                    command.arg("check");
                    if explicit {
                        command.args(["--config", "diplodocus.toml"]);
                    }
                    let output = run(&mut command);
                    assert_eq!(
                        output.status.code(),
                        Some(if success { 0 } else { 1 }),
                        "{mode} {case}: {output:?}"
                    );
                    assert!(output.stdout.is_empty());
                    if case == "warning" {
                        assert!(
                            String::from_utf8_lossy(&output.stderr).contains("warning["),
                            "{output:?}"
                        );
                    }
                    fixture.assert_quiet();
                }
                let output = run(fixture
                    .command(std::env::current_exe().unwrap())
                    .args(["--exact", "library_check_child", "--nocapture"])
                    .env(
                        "DIPLODOCUS_ISOLATED_CHECK",
                        if success { "success" } else { "error" },
                    ));
                assert!(output.status.success(), "{case}: {output:?}");
                fixture.assert_quiet();
            }
        }
    }
    for arguments in [
        vec!["check", "--help"],
        vec!["check", "--unknown"],
        vec!["check", "--config"],
    ] {
        let mut fixture = Isolation::new(ENABLED, true);
        fixture.monitor.tree(fixture.root.path(), writes());
        fixture
            .monitor
            .tree(&fixture.private.path().join("runtime"), writes());
        let _ = fixture.monitor.events();
        let output = run(fixture.command(cargo_bin("diplodocus")).args(&arguments));
        assert_eq!(
            output.status.code(),
            Some(if arguments[1] == "--help" { 0 } else { 2 })
        );
        fixture.assert_quiet();
    }
}

fn assert_static_snapshot(path: &Path) {
    let snapshot = Snapshot::load(path).unwrap();
    assert!(snapshot.assets().is_empty());
    assert!(!snapshot.workspace().pages.is_empty());
    for (id, page) in &snapshot.workspace().pages {
        assert!(snapshot.executed_page(id).is_none());
        assert!(!page.document.provenance.iter().any(|p| matches!(
            p.activity,
            diplodocus::ir::ProvenanceActivity::Execution { .. }
        )));
    }
}

#[test]
fn never_extract_build_and_watched_serve_leave_execution_untouched() {
    for mode in ["", NEVER] {
        for populated in [false, true] {
            let fixture = Isolation::new(mode, populated);
            let mut staging = Monitor::new();
            staging.tree(&fixture.private.path().join("runtime"), writes());
            let output = run(fixture
                .command(std::env::current_exe().unwrap())
                .args(["--exact", "library_execute_child", "--nocapture"])
                .env("DIPLODOCUS_ISOLATED_EXECUTE", "1"));
            assert!(output.status.success(), "{output:?}");
            staging.assert_quiet();
            fixture.assert_quiet();
            for operation in ["extract", "build"] {
                let output = run(fixture.command(cargo_bin("diplodocus")).arg(operation));
                assert!(output.status.success(), "{operation}: {output:?}");
                fixture.assert_quiet();
                assert_static_snapshot(
                    &fixture.root.path().join(".diplodocus/documentation.sqlite"),
                );
            }
            let mut child = fixture
                .command(cargo_bin("diplodocus"))
                .args(["serve", "--port", "0"])
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            // Wait for a completed initial build before editing the watched input.
            use std::io::BufRead;
            let stderr = child.stderr.take().unwrap();
            let (send, receive) = std::sync::mpsc::channel();
            let reader = std::thread::spawn(move || {
                for line in std::io::BufReader::new(stderr).lines() {
                    let _ = send.send(line.unwrap());
                }
            });
            let await_line = |child: &mut std::process::Child, needle: &str| {
                let deadline = Instant::now() + Duration::from_secs(20);
                loop {
                    let line = match receive
                        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    {
                        Ok(line) => line,
                        Err(error) => {
                            let _ = child.kill();
                            let _ = child.wait();
                            panic!("preview did not report {needle}: {error}");
                        }
                    };
                    if line.contains(needle) {
                        break;
                    }
                }
            };
            await_line(&mut child, "Serving http://");
            fixture.root.write("guide/index.qmd", source("Rebuilt"));
            await_line(&mut child, "Rebuilt documentation.");
            rustix::process::kill_process(
                rustix::process::Pid::from_raw(child.id() as i32).unwrap(),
                rustix::process::Signal::TERM,
            )
            .unwrap();
            let output = finish(child);
            reader.join().unwrap();
            assert!(output.status.success(), "{output:?}");
            assert!(fixture.root.read("site/index.html").contains("Rebuilt"));
            fixture.assert_quiet();
            assert_static_snapshot(&fixture.root.path().join(".diplodocus/documentation.sqlite"));
            if !populated {
                assert!(!fixture.root.path().join(".diplodocus/cache").exists());
            }
        }
    }
}

#[test]
fn monitors_detect_discovery_startup_and_transient_writes() {
    let fixture = Isolation::new(ENABLED, true);
    let output = run(fixture.command(cargo_bin("diplodocus")).arg("build"));
    assert!(!output.status.success());
    assert!(
        fixture.private.path().join("started").exists(),
        "{output:?}"
    );
    assert!(!fixture.monitor.events().is_empty());
    let mut monitor = Monitor::new();
    monitor.tree(&fixture.private.path().join("runtime"), writes());
    fixture.private.write("runtime/transient", "asset");
    fixture.private.remove("runtime/transient");
    let events = monitor.events();
    assert!(
        events
            .iter()
            .any(|(_, flags)| flags.contains(ReadFlags::CREATE))
    );
    assert!(
        events
            .iter()
            .any(|(_, flags)| flags.contains(ReadFlags::MODIFY))
    );
    assert!(
        events
            .iter()
            .any(|(_, flags)| flags.contains(ReadFlags::DELETE))
    );
}
