mod support;

use std::fs;
use std::process::Command;

fn workspace(settings: &str) -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.yaml", format!("project:\n  name: Incremental R\nrepositories:\n  - id: repo\n    path: .\npackages:\n  - id: demo\n    name: Incremental R\n    slug: demo\n    ecosystem: r\n    repository: repo\n    path: package\n    metadata-path: DESCRIPTION\n{settings}    targets:\n      - id: api\n        extractor: r\n        path: .\n        role: public-api\n"));
    root.write("package/DESCRIPTION", "Package: incrementalfixture\nVersion: 0.1.0\nTitle: Incremental R Fixture\nDescription: Exercises persistent native preparation.\nLicense: MIT\nAuthor: Fixture Author\nMaintainer: Fixture Author <fixture@example.com>\nEncoding: UTF-8\nRoxygen: list(markdown = TRUE)\n");
    root.write("package/R/api.R", "#' Return a value\n#' @param value A value.\n#' @export\nidentity <- function(value = 1L) value\n.onLoad <- function(libname, pkgname) cat('loaded\\n', file = 'loading-count', append = TRUE)\n");
    root.write("package/man/roxygen/meta.R", "local({ cat('generated\\n', file = 'generation-count', append = TRUE); list(markdown = TRUE) })\n");
    root
}

fn check(root: &support::TestWorkspace, arguments: &[&str]) -> std::process::Output {
    command(root, "check", arguments)
}

fn command(
    root: &support::TestWorkspace,
    command: &str,
    arguments: &[&str],
) -> std::process::Output {
    let output = Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .current_dir(root.path())
        .env("R_LIBS_USER", root.path().join("library"))
        .arg(command)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn compile_extension(root: &support::TestWorkspace, output: &str, value: i32) {
    root.write("native/extension.cpp", format!("#include <Rinternals.h>\ntemplate<class Tag> struct Retained {{ static int value; }};\ntemplate<class Tag> int Retained<Tag>::value = {value};\nstruct Marker {{}};\nextern \"C\" SEXP native_value() {{ return Rf_ScalarInteger(Retained<Marker>::value); }}\n"));
    let object = root.path().join("native/extension.o");
    if object.exists() {
        fs::remove_file(object).unwrap();
    }
    fs::create_dir_all(root.path().join(output).parent().unwrap()).unwrap();
    let compiled = Command::new("R")
        .current_dir(root.path())
        .args(["CMD", "SHLIB", "native/extension.cpp", "-o", output])
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&compiled.stdout),
        String::from_utf8_lossy(&compiled.stderr)
    );
}

#[test]
#[ignore = "slow incremental acceptance; run explicitly with --ignored"]
fn persistent_generation_and_discovery_reuse_have_separate_lifetimes() {
    let root = workspace("");
    check(&root, &[]);
    let generated = root.read("package/generation-count");
    let loaded = root.read("package/loading-count");
    check(&root, &[]);
    assert_eq!(generated, root.read("package/generation-count"));
    assert_eq!(loaded, root.read("package/loading-count"));
    root.write("package/README.md", "An authored-only edit.\n");
    check(&root, &[]);
    assert_eq!(generated, root.read("package/generation-count"));
    assert_eq!(loaded, root.read("package/loading-count"));

    fs::remove_dir_all(
        root.path()
            .join(".diplodocus/cache/incremental/r-discovery"),
    )
    .unwrap();
    check(&root, &[]);
    assert_eq!(generated, root.read("package/generation-count"));
    assert!(root.read("package/loading-count").len() > loaded.len());

    root.remove("package/man/identity.Rd");
    check(&root, &[]);
    assert!(root.path().join("package/man/identity.Rd").is_file());
    assert!(root.read("package/generation-count").len() > generated.len());
    let generated = root.read("package/generation-count");
    root.write(
        "package/R/api.R",
        format!(
            "{}\n# A source comment changes evaluated package inputs.\n",
            root.read("package/R/api.R")
        ),
    );
    check(&root, &[]);
    assert!(root.read("package/generation-count").len() > generated.len());
}

#[test]
#[ignore = "slow incremental acceptance; run explicitly with --ignored"]
fn forced_recomputation_and_package_opt_out_bypass_r_results() {
    let root = workspace("");
    check(&root, &[]);
    let generated = root.read("package/generation-count");
    check(&root, &["--no-cache"]);
    assert!(root.read("package/generation-count").len() > generated.len());

    let root = workspace("    incremental:\n      cache: off\n");
    check(&root, &[]);
    let generated = root.read("package/generation-count");
    check(&root, &[]);
    assert!(root.read("package/generation-count").len() > generated.len());
    assert!(
        !root
            .path()
            .join(".diplodocus/cache/incremental/r-discovery")
            .exists()
    );
}

#[test]
#[ignore = "slow incremental acceptance; run explicitly with --ignored"]
fn same_version_installed_dependency_changes_invalidate_native_results() {
    let root = workspace("");
    root.write("dependency/DESCRIPTION", "Package: cachedependency\nVersion: 0.1.0\nTitle: Cache Dependency\nDescription: Supplies a prepared dependency for invalidation.\nLicense: MIT\nAuthor: Fixture Author\nMaintainer: Fixture Author <fixture@example.com>\n");
    root.write("dependency/NAMESPACE", "export(value)\n");
    root.write("dependency/R/api.R", "value <- function() 1L\n");
    fs::create_dir(root.path().join("library")).unwrap();
    let output = Command::new("R")
        .current_dir(root.path())
        .args([
            "CMD",
            "INSTALL",
            "--no-byte-compile",
            "--no-help",
            "--no-html",
            "--no-test-load",
            "-l",
        ])
        .arg(root.path().join("library"))
        .arg(root.path().join("dependency"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    root.write(
        "package/DESCRIPTION",
        format!(
            "{}Imports: cachedependency\n",
            root.read("package/DESCRIPTION")
        ),
    );
    root.write(
        "package/R/api.R",
        root.read("package/R/api.R")
            .replace("@export", "@import cachedependency\n#' @export"),
    );
    check(&root, &[]);
    let generated = root.read("package/generation-count");
    check(&root, &[]);
    assert_eq!(generated, root.read("package/generation-count"));
    root.write(
        "library/unrelated/DESCRIPTION",
        "Package: unrelated\nVersion: 0.1.0\n",
    );
    check(&root, &[]);
    assert_eq!(generated, root.read("package/generation-count"));
    let path = "library/cachedependency/DESCRIPTION";
    root.write(
        path,
        format!("{}CacheMarker: same-version-replacement\n", root.read(path)),
    );
    check(&root, &[]);
    assert!(root.read("package/generation-count").len() > generated.len());
    let regenerated = root.read("package/generation-count");
    check(&root, &[]);
    assert_eq!(regenerated, root.read("package/generation-count"));
}

#[test]
#[ignore = "slow incremental acceptance; run explicitly with --ignored"]
#[cfg(target_os = "linux")]
fn retained_checkout_dlls_allow_persistent_reuse_and_guard_their_bytes() {
    let root = workspace("");
    compile_extension(&root, "package/src/incrementalfixture.so", 1);
    root.write(
        "package/NAMESPACE",
        "# Generated by roxygen2: do not edit by hand\n\nexport(identity)\nuseDynLib(incrementalfixture)\n",
    );
    root.write(
        "package/R/api.R",
        root.read("package/R/api.R").replace(
            "#' @export",
            "#' @useDynLib incrementalfixture\n#' @examples\n#' cat('executed\\n', file = 'execution-count', append = TRUE)\n#' .Call('native_value')\n#' @export",
        ) + "\n.onUnload <- function(libpath) { cat('unloaded\\n', file = 'unloading-count', append = TRUE) }\n.onLoad <- function(libname, pkgname) { .Call('native_value'); cat('loaded\\n', file = 'loading-count', append = TRUE); if (any(grepl('incrementalfixture.so (deleted)', readLines('/proc/self/maps', warn = FALSE), fixed = TRUE))) cat('retained\\n', file = 'retained-count', append = TRUE) }\n",
    );
    command(&root, "build", &["--output", "site"]);
    let generated = root.read("package/generation-count");
    let loaded = root.read("package/loading-count");
    let executed = root.read("package/execution-count");
    assert!(root.read("package/unloading-count").lines().count() >= 2);
    assert!(root.path().join("package/retained-count").is_file());
    assert!(
        root.path()
            .join(".diplodocus/cache/incremental/r-generation")
            .is_dir()
    );
    root.write("package/README.md", "An authored-only change.\n");
    command(&root, "build", &["--output", "site"]);
    command(&root, "build", &["--output", "site"]);
    assert_eq!(generated, root.read("package/generation-count"));
    assert_eq!(loaded, root.read("package/loading-count"));
    assert_eq!(executed, root.read("package/execution-count"));

    compile_extension(&root, "native/replacement.so", 2);
    fs::copy(
        root.path().join("native/replacement.so"),
        root.path().join("package/src/incrementalfixture.so"),
    )
    .unwrap();
    command(&root, "build", &["--output", "site"]);
    assert!(root.read("package/generation-count").len() > generated.len());
    assert!(root.read("package/execution-count").len() > executed.len());
    let regenerated = root.read("package/generation-count");
    command(&root, "build", &["--output", "site"]);
    assert_eq!(regenerated, root.read("package/generation-count"));
}

#[test]
#[ignore = "slow incremental acceptance; run explicitly with --ignored"]
#[cfg(target_os = "linux")]
fn unknown_deleted_native_mappings_disable_reuse_with_a_reason() {
    let root = workspace("");
    compile_extension(&root, "native/foreign.so", 1);
    root.write("package/R/api.R", root.read("package/R/api.R") + "\n.onLoad <- function(libname, pkgname) { if (!is.loaded('native_value')) { dyn.load('../foreign.so'); unlink('../foreign.so') } }\n");
    let mut generated = String::new();
    for _ in 0..2 {
        fs::copy(
            root.path().join("native/foreign.so"),
            root.path().join("foreign.so"),
        )
        .unwrap();
        let output = check(&root, &[]);
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("unrecognized deleted native mapping"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let current = root.read("package/generation-count");
        assert!(current.len() > generated.len());
        generated = current;
    }
    assert!(
        !root
            .path()
            .join(".diplodocus/cache/incremental/r-generation")
            .exists()
    );
}
