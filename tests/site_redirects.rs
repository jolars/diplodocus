mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::commands::{self, BuildOptions, CheckOptions, GenerateOptions};
use diplodocus::rendering::{render_fragments, render_site};
use diplodocus::site::Site;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;

fn workspace(presentation: &str) -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.yaml", format!("project:\n  name: Redirects\npresentation:\n{presentation}\nrepositories:\n  - id: docs\n    path: .\ncontent:\n  - id: guide\n    owner: project\n    repository: docs\n    path: docs\n    mount: \"\"\n    format: gfm\n"));
    for (path, title) in [
        ("index.md", "Home"),
        ("guide/index.md", "Guide"),
        ("guide/start.md", "Start"),
        ("café &%.md", "Unicode"),
    ] {
        root.write(format!("docs/{path}"), format!("# {title}\n"));
    }
    root
}

fn snapshot(root: &support::TestWorkspace) -> Snapshot {
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap()
}

#[test]
fn redirects_resolve_canonical_styles_prefixes_and_encoded_paths() {
    for (style, start, home, guide, unicode) in [
        (
            "file",
            "guide/start.html",
            "index.html",
            "guide/index.html",
            "caf%C3%A9%20%26%25.html",
        ),
        ("clean", "guide/start", "", "guide/", "caf%C3%A9%20%26%25"),
    ] {
        for (base, prefix) in [
            ("", ""),
            ("  site-url: https://example.test/docs\n", "/docs"),
            (
                "  site-url: https://example.test/docs/:name*/\n",
                "/docs/%3Aname%2A",
            ),
            (
                "  site-url: https://example.test/docs/café%20space\n",
                "/docs/caf%C3%A9%20space",
            ),
        ] {
            let root = workspace(&format!(
                "{base}  canonical-url-style: {style}\n  redirects:\n    /old: guide/start.html\n    /old.html: guide/start.html\n    /old-home: index.html\n    /old-guide/: guide/index.html\n    /café%20old: \"café &%.html\""
            ));
            let snapshot = snapshot(&root);
            let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
            let rules = String::from_utf8(rendered.files()["_redirects"].bytes.clone()).unwrap();
            assert_eq!(
                rules,
                format!(
                    "{prefix}/caf%C3%A9%20old {prefix}/{unicode} 301\n{prefix}/old {prefix}/{start} 301\n{prefix}/old-guide/ {prefix}/{guide} 301\n{prefix}/old-home {prefix}/{home} 301\n{prefix}/old.html {prefix}/{start} 301\n"
                )
            );
            assert_eq!(rendered.redirects()["/old"], format!("/{start}"));
            assert_eq!(
                rendered.redirects()["/caf%C3%A9%20old"],
                format!("/{unicode}")
            );
            assert!(!rendered.files().contains_key("old.html"));
            let search =
                std::str::from_utf8(&rendered.files()["assets/search.json"].bytes).unwrap();
            assert!(!search.contains("old"));
            if !base.is_empty() {
                let sitemap = std::str::from_utf8(&rendered.files()["sitemap.xml"].bytes).unwrap();
                assert!(!sitemap.contains("old"));
                assert_eq!(sitemap.matches("<loc>").count(), 4);
            }
        }
    }
}

#[test]
fn snapshots_regenerate_redirects_without_the_source_checkout() {
    let root = workspace("  canonical-url-style: clean\n  redirects:\n    /old: guide/start.html");
    let snapshot = snapshot(&root);
    let output = tempfile::tempdir().unwrap();
    let database = output.path().join("portable.sqlite");
    let first = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    first.publish(&output.path().join("first")).unwrap();
    snapshot.publish(&database).unwrap();
    let exported = snapshot.canonical_export().unwrap();
    drop(root);
    let loaded = Snapshot::load(&database).unwrap();
    assert_eq!(loaded.canonical_export().unwrap(), exported);
    assert_eq!(
        loaded.presentation().redirects["/old"].as_str(),
        "guide/start.html"
    );
    commands::generate(GenerateOptions {
        input: database,
        output: output.path().join("regenerated"),
    })
    .unwrap();
    support::assert_output_tree(
        &output.path().join("first"),
        &output.path().join("regenerated"),
    );
    let fragments = render_fragments(&Site::new(&loaded).unwrap()).unwrap();
    assert!(!fragments.files().contains_key("_redirects"));
    assert!(fragments.redirects().is_empty());
}

#[test]
fn absent_redirects_keep_existing_output_and_snapshot_shapes() {
    let root = workspace("  redirects: {}");
    let snapshot = snapshot(&root);
    assert!(
        serde_json::to_value(snapshot.presentation())
            .unwrap()
            .get("redirects")
            .is_none()
    );
    let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    assert!(!rendered.files().contains_key("_redirects"));
    assert!(rendered.redirects().is_empty());
}

#[test]
fn redirect_metadata_cannot_replace_a_page_or_its_directory() {
    for (style, path) in [
        ("clean", "_redirects.md"),
        ("clean", "_redirects/index.md"),
        ("file", "_redirects/index.md"),
        ("file", "_redirects/child.md"),
    ] {
        let root = workspace(&format!(
            "  canonical-url-style: {style}\n  redirects:\n    /old: guide/start.html"
        ));
        root.write(format!("docs/{path}"), "# Reserved output\n");
        let error = commands::check(CheckOptions {
            config: root.path().join("diplodocus.yaml"),
        })
        .unwrap_err();
        assert!(
            error.to_string().contains("_redirects hosting file"),
            "{style} {path}: {error}"
        );
    }
}

#[test]
fn check_rejects_invalid_sources_targets_collisions_and_chains() {
    for (source, target) in [
        ("old", "guide/start.html"),
        ("https://example.test/old", "guide/start.html"),
        ("//example.test/old", "guide/start.html"),
        ("/old//page", "guide/start.html"),
        ("/old//", "guide/start.html"),
        ("/old/../page", "guide/start.html"),
        ("/old/%2e%2e/page", "guide/start.html"),
        ("/old%2Fpage", "guide/start.html"),
        ("/old%5cpage", "guide/start.html"),
        ("/old*", "guide/start.html"),
        ("/old/:name", "guide/start.html"),
        ("/old%3Aname", "guide/start.html"),
        ("/old?x=1", "guide/start.html"),
        ("/old#anchor", "guide/start.html"),
        ("/old page", "guide/start.html"),
        ("/old%", "guide/start.html"),
        ("/old%0Apage", "guide/start.html"),
        ("/old%FF", "guide/start.html"),
        ("/old", "missing.html"),
        ("/old", "guide/start.md"),
        ("/", "guide/start.html"),
        ("/index.html", "guide/start.html"),
        ("/guide", "guide/start.html"),
        ("/guide/", "guide/start.html"),
        ("/guide/index.html", "guide/start.html"),
        ("/guide/start", "guide/start.html"),
        ("/guide/start.html", "index.html"),
        ("/guide/start.md", "index.html"),
        ("/%67uide/start", "index.html"),
        ("/assets/site.css", "index.html"),
        ("/__diplodocus/revision", "index.html"),
        ("/.diplodocus-site", "index.html"),
        ("/sitemap.xml", "index.html"),
        ("/robots.txt", "index.html"),
        ("/_redirects", "index.html"),
        ("/documents.json", "index.html"),
    ] {
        let root = workspace(&format!(
            "  redirects:\n    {}: {}",
            serde_json::to_string(source).unwrap(),
            serde_json::to_string(target).unwrap()
        ));
        let config = root.path().join("diplodocus.yaml");
        let error = commands::check(CheckOptions { config }).unwrap_err();
        assert!(
            error.to_string().contains("redirect"),
            "{source} -> {target}: {error}"
        );
        assert!(!root.path().join(".diplodocus").exists());
    }
    for rules in [
        "    /old: other.html\n    /other.html: guide/start.html",
        "    /old.html: other.html\n    /other.html: old.html",
        "    /old: guide/start.html\n    /%6Fld: index.html",
    ] {
        let root = workspace(&format!("  redirects:\n{rules}"));
        assert!(Site::new(&snapshot(&root)).is_err(), "{rules}");
    }
}

#[test]
fn hosting_limits_fail_before_rules_are_silently_dropped() {
    for redirects in [
        (0..2_001)
            .map(|index| format!("    /old-{index}: index.html\n"))
            .collect::<String>(),
        format!("    /{}: index.html", "x".repeat(1_000)),
    ] {
        let root = workspace(&format!("  redirects:\n{redirects}"));
        assert!(Site::new(&snapshot(&root)).is_err());
    }
}

#[test]
fn redirect_destinations_follow_package_visibility() {
    for visibility in ["public", "internal", "hidden"] {
        let root = support::acceptance_workspace();
        let mut configuration = support::fixture_configuration(&root, "workspace/diplodocus.yaml");
        configuration["packages"][1]
            .as_object_mut()
            .unwrap()
            .insert("visibility".into(), visibility.into());
        configuration.as_object_mut().unwrap().insert(
            "presentation".into(),
            serde_json::json!({
                "redirects": {"/old-package": "packages/r/index.html"}
            }),
        );
        root.write(
            "workspace/diplodocus.yaml",
            serde_saphyr::to_string(&configuration).unwrap(),
        );
        let result = commands::check(CheckOptions {
            config: root.path().join("workspace/diplodocus.yaml"),
        });
        if visibility == "hidden" {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("visible generated HTML page")
            );
        } else {
            result.unwrap();
        }
    }
}

#[test]
fn invalid_redirects_preserve_the_published_site_and_removal_cleans_rules() {
    let root = workspace("  redirects:\n    /old: guide/start.html");
    let config = root.path().join("diplodocus.yaml");
    let output = root.path().join("site");
    commands::build(BuildOptions {
        config: config.clone(),
        output: output.clone(),
    })
    .unwrap();
    let before = std::fs::read(output.join("_redirects")).unwrap();
    let configuration = root.read("diplodocus.yaml");
    root.write(
        "diplodocus.yaml",
        configuration.replace("/old: guide/start.html", "/old: missing.html"),
    );
    assert!(
        commands::build(BuildOptions {
            config: config.clone(),
            output: output.clone()
        })
        .is_err()
    );
    assert_eq!(std::fs::read(output.join("_redirects")).unwrap(), before);
    root.write(
        "diplodocus.yaml",
        configuration.replace(
            "  redirects:\n    /old: guide/start.html",
            "  redirects: {}",
        ),
    );
    commands::build(BuildOptions {
        config,
        output: output.clone(),
    })
    .unwrap();
    assert!(!output.join("_redirects").exists());
}
