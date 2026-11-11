#![cfg(target_os = "linux")]
mod support;

use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::process::Stdio;
use std::time::Duration;

use support::TestWorkspace;
use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::{Child, ChildStderr, Command};

fn workspace() -> TestWorkspace {
    let root = TestWorkspace::new();
    root.write(
        "diplodocus.yaml",
        "project:\n  name: Preview\npresentation:\n  site-url: https://example.test/production/\nrepositories:\n  - id: docs\n    path: .\ncontent:\n  - id: guide\n    owner: project\n    repository: docs\n    path: guide\n    mount: ''\n    format: gfm\n",
    );
    root.write("guide/index.md", "# First generation\n");
    root.write(
        "fake launchers/xdg-open",
        "#!/bin/sh\nDIPLODOCUS_TEST_BROWSER_URL=\"$1\" exec \"$DIPLODOCUS_TEST_BROWSER_PROBE\" --exact browser_launcher_probe --nocapture\n",
    );
    std::fs::set_permissions(
        root.path().join("fake launchers/xdg-open"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    root
}

fn command(root: &TestWorkspace, port: u16) -> Command {
    let mut command = Command::new(snapbox::cmd::cargo_bin("diplodocus"));
    command
        .current_dir(root.path())
        .args(["serve", "--port", &port.to_string()])
        .env("PATH", root.path().join("fake launchers"))
        .env(
            "DIPLODOCUS_TEST_BROWSER_PROBE",
            std::env::current_exe().unwrap(),
        )
        .env(
            "DIPLODOCUS_TEST_BROWSER_LOG",
            root.path().join("browser.log"),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

async fn line_containing(lines: &mut Lines<BufReader<ChildStderr>>, needle: &str) -> String {
    tokio::time::timeout(Duration::from_secs(15), async {
        while let Some(line) = lines.next_line().await.unwrap() {
            if line.contains(needle) {
                return line;
            }
        }
        panic!("preview stopped before printing {needle}");
    })
    .await
    .expect("preview did not print expected line")
}

async fn start(command: &mut Command) -> (Child, Lines<BufReader<ChildStderr>>, String) {
    let mut child = command.spawn().unwrap();
    let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
    let serving = line_containing(&mut lines, "Serving ").await;
    let url = serving.strip_prefix("Serving ").unwrap().to_owned();
    (child, lines, url)
}

async fn until(mut condition: impl AsyncFnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(15), async {
        while !condition().await {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("preview condition did not become true");
}

fn request(url: &str) -> String {
    let address = url.strip_prefix("http://").unwrap().trim_end_matches('/');
    let address: std::net::SocketAddr = address.parse().unwrap();
    let mut stream =
        std::net::TcpStream::connect_timeout(&address, Duration::from_secs(5)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    response
}

// The fake system launcher runs only this test in a separate process so it can
// verify HTTP readiness at invocation without opening a desktop browser.
#[test]
fn browser_launcher_probe() {
    let Ok(url) = std::env::var("DIPLODOCUS_TEST_BROWSER_URL") else {
        return;
    };
    let mut input = [0];
    assert_eq!(std::io::stdin().read(&mut input).unwrap(), 0);
    let log = std::env::var_os("DIPLODOCUS_TEST_BROWSER_LOG").unwrap();
    let log = std::path::PathBuf::from(log);
    let mut output = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .unwrap();
    writeln!(output, "{url}").unwrap();
    assert!(request(&url).contains("generation"));
    std::fs::write(log.with_extension("ready"), &url).unwrap();
    if std::env::var_os("DIPLODOCUS_TEST_BROWSER_FAIL").is_some() {
        std::process::exit(7);
    }
    if std::env::var_os("DIPLODOCUS_TEST_BROWSER_WAIT").is_some() {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while !log.with_extension("release").exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        std::fs::write(log.with_extension("finished"), "").unwrap();
    }
}

#[tokio::test]
async fn opens_ready_local_url_once_with_assigned_port_and_live_reload() {
    let root = workspace();
    let (mut child, _lines, url) =
        start(command(&root, 0).args(["--open", "--live-reload", "--host", "0.0.0.0"])).await;
    assert!(url.starts_with("http://127.0.0.1:"), "{url}");
    assert!(!url.ends_with(":0/"));
    until(async || root.path().join("browser.ready").exists()).await;
    assert_eq!(root.read("browser.log"), format!("{url}\n"));
    assert!(root.path().join("site/index.html").is_file());
    root.write("guide/index.md", "# Second generation\n");
    until(async || request(&url).contains("Second generation")).await;
    assert_eq!(root.read("browser.log"), format!("{url}\n"));
    child.kill().await.unwrap();
}

#[tokio::test]
async fn browser_opening_is_opt_in() {
    let root = workspace();
    let (mut child, _lines, url) = start(&mut command(&root, 0)).await;
    assert!(request(&url).contains("First generation"));
    root.write("guide/index.md", "# Second generation\n");
    until(async || request(&url).contains("Second generation")).await;
    assert!(!root.path().join("browser.log").exists());
    child.kill().await.unwrap();
}

#[tokio::test]
async fn startup_failures_never_invoke_the_browser() {
    let root = workspace();
    root.write("diplodocus.yaml", "project: [");
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        command(&root, 0).arg("--open").output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("Serving "));
    assert!(!root.path().join("browser.log").exists());

    let root = workspace();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        command(&root, listener.local_addr().unwrap().port())
            .arg("--open")
            .output(),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("Serving "));
    assert!(!root.path().join("browser.log").exists());
}

#[tokio::test]
async fn launcher_failures_warn_and_keep_preview_running() {
    for missing in [false, true] {
        let root = workspace();
        if missing {
            root.remove("fake launchers/xdg-open");
        }
        let (mut child, mut lines, url) = start(
            command(&root, 0)
                .arg("--open")
                .env("DIPLODOCUS_TEST_BROWSER_FAIL", "1"),
        )
        .await;
        let warning = line_containing(&mut lines, "could not open browser").await;
        assert!(warning.contains(&url), "{warning}");
        assert!(request(&url).contains("First generation"));
        root.write("guide/index.md", "# Still rebuilding\n");
        until(async || request(&url).contains("Still rebuilding")).await;
        child.kill().await.unwrap();
    }
}

#[tokio::test]
async fn running_launcher_does_not_block_rebuilds_or_shutdown() {
    let root = workspace();
    let (mut child, _lines, url) = start(
        command(&root, 0)
            .arg("--open")
            .env("DIPLODOCUS_TEST_BROWSER_WAIT", "1"),
    )
    .await;
    until(async || root.path().join("browser.ready").exists()).await;
    root.write("guide/index.md", "# Rebuilt while launcher runs\n");
    until(async || request(&url).contains("Rebuilt while launcher runs")).await;
    assert!(!root.path().join("browser.finished").exists());

    rustix::process::kill_process(
        rustix::process::Pid::from_raw(child.id().unwrap() as i32).unwrap(),
        rustix::process::Signal::TERM,
    )
    .unwrap();
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(status.success());
    root.write("browser.release", "");
    until(async || root.path().join("browser.finished").exists()).await;
}
