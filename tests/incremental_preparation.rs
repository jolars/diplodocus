mod support;

use std::sync::{Arc, Mutex};

use diplodocus::assembly::AssemblyError;
use diplodocus::commands::{BuildOptions, CheckOptions, CommandError, CommandRunner};
use diplodocus::incremental::CachePolicy;
use diplodocus::reporting::{Event, Reporter};

#[derive(Default)]
struct CacheEvents(Mutex<Vec<(String, String, bool)>>);

impl Reporter for CacheEvents {
    fn report(&self, event: Event<'_>) {
        if let Event::Cache {
            stage, owner, hit, ..
        } = event
        {
            self.0
                .lock()
                .unwrap()
                .push((stage.into(), owner.into(), hit));
        }
    }
}

impl CacheEvents {
    fn take(&self) -> Vec<(String, String, bool)> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

fn contains(events: &[(String, String, bool)], stage: &str, owner: &str, hit: bool) -> bool {
    events
        .iter()
        .any(|event| event == &(stage.into(), owner.into(), hit))
}

#[test]
fn preparation_reuses_across_commands_and_tracks_nested_includes() {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.yaml", "project:\n  name: Cached guide\nrepositories:\n  - id: docs\n    path: .\ncontent:\n  - id: guide\n    owner: project\n    repository: docs\n    path: pages\n    mount: ''\n    format: gfm\n");
    root.write(
        "pages/index.md",
        "# Main\n\n<!--@include: ../parts/first.md-->\n",
    );
    root.write("parts/first.md", "First.\n\n<!--@include: nested.md-->\n");
    root.write("parts/nested.md", "Original nested prose.\n");
    let output = tempfile::tempdir().unwrap();
    let events = Arc::new(CacheEvents::default());
    let build = |path: &str, policy| {
        CommandRunner::with_reporter(events.clone())
            .with_cache_policy(policy)
            .build(BuildOptions {
                config: root.path().join("diplodocus.yaml"),
                output: output.path().join(path),
            })
            .unwrap();
    };

    build("first", CachePolicy::Auto);
    assert!(contains(
        &events.take(),
        "authored-preparation",
        "guide:index.md",
        false
    ));
    build("warm", CachePolicy::Auto);
    assert!(contains(
        &events.take(),
        "authored-preparation",
        "guide:index.md",
        true
    ));
    support::assert_output_tree(&output.path().join("first"), &output.path().join("warm"));

    root.write("parts/nested.md", "Changed nested prose.\n");
    build("changed", CachePolicy::Auto);
    assert!(contains(
        &events.take(),
        "authored-preparation",
        "guide:index.md",
        false
    ));
    assert!(
        std::fs::read_to_string(output.path().join("changed/index.html"))
            .unwrap()
            .contains("Changed nested prose.")
    );
    build("clean", CachePolicy::Bypass);
    assert!(events.take().is_empty());
    support::assert_output_tree(&output.path().join("changed"), &output.path().join("clean"));
}

#[test]
fn static_api_reuse_keeps_unrelated_packages_after_prose_and_source_saves() {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.yaml", "project:\n  name: Static package caches\nrepositories:\n  - id: source\n    path: .\npackages:\n  - id: first\n    name: First\n    slug: first\n    ecosystem: python\n    repository: source\n    path: first\n    metadata-path: pyproject.toml\n    targets:\n      - id: api\n        extractor: python\n        path: first\n        role: public-api\n  - id: second\n    name: Second\n    slug: second\n    ecosystem: python\n    repository: source\n    path: second\n    metadata-path: pyproject.toml\n    targets:\n      - id: api\n        extractor: python\n        path: second\n        role: public-api\ncontent:\n  - id: guide\n    owner: project\n    repository: source\n    path: docs\n    mount: guide\n    format: gfm\n");
    for name in ["first", "second"] {
        root.write(
            format!("{name}/pyproject.toml"),
            format!("[project]\nname = '{name}'\nversion = '1.0.0'\n"),
        );
        root.write(
            format!("{name}/{name}/__init__.py"),
            "def greet(name: str) -> str:\n    return name\n",
        );
    }
    root.write("docs/index.md", "# Guide\n");
    let events = Arc::new(CacheEvents::default());
    let check = || {
        CommandRunner::with_reporter(events.clone())
            .check(CheckOptions {
                config: root.path().join("diplodocus.yaml"),
            })
            .unwrap();
        events.take()
    };
    let first = check();
    assert!(contains(&first, "static-python", "first", false));
    assert!(contains(&first, "static-python", "second", false));
    let warm = check();
    assert!(contains(&warm, "static-python", "first", true));
    assert!(contains(&warm, "static-python", "second", true));

    root.write("docs/index.md", "# Updated guide\n");
    let prose = check();
    assert!(contains(&prose, "static-python", "first", true));
    assert!(contains(&prose, "static-python", "second", true));
    root.write(
        "first/first/__init__.py",
        "# Package source changed.\ndef greet(name: str) -> str:\n    return name\n",
    );
    let source = check();
    assert!(contains(&source, "static-python", "first", false));
    assert!(contains(&source, "static-python", "second", true));
}

#[test]
fn doxygen_restores_verified_output_and_invalidates_changed_headers() {
    let root = support::TestWorkspace::from_fixture("doxygen-api");
    let events = Arc::new(CacheEvents::default());
    let check = || {
        CommandRunner::with_reporter(events.clone())
            .check(CheckOptions {
                config: root.path().join("diplodocus.yaml"),
            })
            .unwrap();
        events.take()
    };
    assert!(contains(&check(), "doxygen", "demo", false));
    assert!(contains(&check(), "doxygen", "demo", true));
    root.write(
        "include/cache_added.hpp",
        "/// Added during preview.\nstruct CacheAdded {};\n",
    );
    assert!(contains(&check(), "doxygen", "demo", false));
}

#[test]
fn python_examples_reuse_only_after_fresh_checkout_validation() {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.yaml", "project:\n  name: Python execution cache\nrepositories:\n  - id: source\n    path: .\npackages:\n  - id: cached\n    name: Cached Python\n    slug: cached\n    ecosystem: python\n    repository: source\n    path: .\n    metadata-path: pyproject.toml\n    targets:\n      - id: api\n        extractor: python\n        path: src/cachepy\n        role: public-api\n");
    root.write(
        "pyproject.toml",
        "[project]\nname = 'cachepy'\nversion = '1.0.0'\n",
    );
    let source = "__all__ = ['answer']\ndef answer():\n    \"\"\"Return the answer.\n\n    Examples\n    --------\n    >>> answer()\n    42\n    \"\"\"\n    return 42\n";
    root.write("src/cachepy/__init__.py", source);
    let output = tempfile::tempdir().unwrap();
    let events = Arc::new(CacheEvents::default());
    let build = || {
        CommandRunner::with_reporter(events.clone()).build(BuildOptions {
            config: root.path().join("diplodocus.yaml"),
            output: output.path().join("site"),
        })
    };
    build().unwrap();
    assert!(contains(
        &events.take(),
        "python-api-examples",
        "cached",
        false
    ));
    build().unwrap();
    assert!(contains(
        &events.take(),
        "python-api-examples",
        "cached",
        true
    ));

    root.write(
        "src/cachepy/__init__.py",
        format!("# Source changed.\n{source}"),
    );
    build().unwrap();
    assert!(contains(
        &events.take(),
        "python-api-examples",
        "cached",
        false
    ));
    let old_site = std::fs::read(output.path().join("site/index.html")).unwrap();
    root.write(
        "src/cachepy/__init__.py",
        format!("__version__ = '9.0.0'\n{source}"),
    );
    let CommandError::Assembly(AssemblyError::Diagnostics(diagnostics)) = build().unwrap_err()
    else {
        panic!("expected native checkout diagnostics");
    };
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("Loaded package version conflicts")
    }));
    assert_eq!(
        std::fs::read(output.path().join("site/index.html")).unwrap(),
        old_site
    );
}

#[test]
fn python_module_probes_do_not_reuse_an_earlier_modules_environment() {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.yaml", "project:\n  name: Python module coverage\nrepositories:\n  - id: source\n    path: .\npackages:\n  - id: cached\n    name: Cached Python\n    slug: cached\n    ecosystem: python\n    repository: source\n    path: .\n    metadata-path: pyproject.toml\n    targets:\n      - id: alpha\n        extractor: python\n        path: src/alpha\n        role: public-api\n      - id: zeta\n        extractor: python\n        path: src/zeta\n        role: public-api\n");
    root.write(
        "pyproject.toml",
        "[project]\nname = 'cachemulti'\nversion = '1.0.0'\n",
    );
    let example = "__all__ = ['answer']\ndef answer():\n    \"\"\"Return the answer.\n\n    Examples\n    --------\n    >>> answer()\n    42\n    \"\"\"\n    return 42\n";
    root.write("src/zeta/__init__.py", example);
    let external = tempfile::tempdir().unwrap();
    let marker = external.path().join("opaque-loader");
    let marker_literal = serde_json::to_string(marker.to_str().unwrap()).unwrap();
    root.write("src/alpha/__init__.py", format!("import pathlib\nimport sys\nimport types\ndef _configure_loader():\n    if pathlib.Path({marker_literal}).exists():\n        opaque = types.ModuleType('cachemulti_opaque')\n        opaque.__spec__ = types.SimpleNamespace(origin='opaque-loader')\n        sys.modules['cachemulti_opaque'] = opaque\n_configure_loader()\n\n{example}"));
    let output = tempfile::tempdir().unwrap();
    let events = Arc::new(CacheEvents::default());
    let build = || {
        CommandRunner::with_reporter(events.clone())
            .build(BuildOptions {
                config: root.path().join("diplodocus.yaml"),
                output: output.path().join("site"),
            })
            .unwrap();
        events.take()
    };
    build();
    assert!(contains(&build(), "python-api-examples", "cached", true));

    // The later alpha probe discovers an opaque loader without changing package inputs.
    std::fs::write(&marker, "opaque runtime coverage\n").unwrap();
    let uncertain = build();
    assert!(contains(&uncertain, "python-api-examples", "cached", false));
    assert!(
        !contains(&uncertain, "python-api-examples", "cached", true),
        "An earlier module's coverage must not enable reuse: {uncertain:?}"
    );
    std::fs::remove_file(marker).unwrap();
    assert!(contains(&build(), "python-api-examples", "cached", true));
}

#[test]
fn julia_native_stages_reuse_completed_results_and_invalidate_package_edits() {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.yaml", "project:\n  name: Julia result caches\nrepositories:\n  - id: source\n    path: .\npackages:\n  - id: demo\n    name: Demo\n    slug: demo\n    ecosystem: julia\n    repository: source\n    path: .\n    metadata-path: Project.toml\n    targets:\n      - id: api\n        extractor: julia\n        path: src/Demo.jl\n        role: public-api\ncontent:\n  - id: manual\n    owner: demo\n    repository: source\n    path: docs\n    mount: ''\n    format: documenter\n    execution:\n      mode: execute\n      engine: julia\n      julia-project: Project.toml\n");
    root.write(
        "Project.toml",
        "name = 'Demo'\nuuid = '12345678-1234-1234-1234-123456789abc'\nversion = '1.0.0'\n",
    );
    let source = "module Demo\nexport answer\n\"\"\"Return an answer.\n\n```jldoctest\njulia> answer()\n42\n```\n\"\"\"\nanswer() = 42\nend\n";
    root.write("src/Demo.jl", source);
    root.write(
        "docs/index.md",
        "# Guide\n\n```@example\nusing Demo\nanswer()\n```\n",
    );
    let output = tempfile::tempdir().unwrap();
    let events = Arc::new(CacheEvents::default());
    let build = |destination: &str| {
        CommandRunner::with_reporter(events.clone())
            .build(BuildOptions {
                config: root.path().join("diplodocus.yaml"),
                output: output.path().join(destination),
            })
            .unwrap();
        events.take()
    };
    build("first");
    let warm = build("warm");
    for stage in ["julia-runtime", "julia-api-examples", "documenter-examples"] {
        assert!(
            warm.iter().any(|event| event.0 == stage && event.2),
            "missing {stage} hit: {warm:?}"
        );
    }
    support::assert_output_tree(&output.path().join("first"), &output.path().join("warm"));
    root.write(
        "src/Demo.jl",
        format!("{source}\n# Changed maintained source.\n"),
    );
    let changed = build("changed");
    for stage in ["julia-runtime", "julia-api-examples", "documenter-examples"] {
        assert!(
            changed.iter().any(|event| event.0 == stage && !event.2),
            "missing {stage} miss: {changed:?}"
        );
    }
}
