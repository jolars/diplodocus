mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::rendering::render_site;
use diplodocus::site::{Site, SiteError};
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;

fn workspace(style: &str, sections: &str) -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write(
        "diplodocus.yaml",
        format!(
            "project:\n  name: \"Sections\"\npresentation:\n  canonical-url-style: \"{style}\"\n{sections}\nrepositories:\n  - id: \"docs\"\n    path: \".\"\ncontent:\n  - id: \"docs\"\n    owner: \"project\"\n    repository: \"docs\"\n    path: \"docs\"\n    mount: \"\"\n    format: \"gfm\"\n"
        ),
    );
    root.write(
        "docs/index.md",
        "# Guide\n\n[Reference](reference/index.md)\n",
    );
    root.write("docs/intro.md", "# Introduction\n");
    root.write(
        "docs/reference/index.md",
        "# Reference\n\n[Commands](cli.md)\n",
    );
    root.write(
        "docs/reference/cli.md",
        "# Commands\n\n[Guide](../index.md)\n",
    );
    root.write("docs/reference/nested/index.md", "# Nested reference\n");
    root
}

#[test]
fn project_sections_keep_their_navbar_and_sidebar_after_snapshot_round_trip() {
    for style in ["file", "clean"] {
        let root = workspace(
            style,
            "  sections:\n    - label: \"Reference & tools\"\n      path: \"reference/index.html\"",
        );
        let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
        let snapshot =
            Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
        let path = root.path().join("snapshot.sqlite");
        snapshot.publish(&path).unwrap();
        std::fs::remove_dir_all(root.path().join("docs")).unwrap();
        let snapshot = Snapshot::load(path).unwrap();
        let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
        let guide = std::str::from_utf8(&rendered.files()["index.html"].bytes).unwrap();
        let commands = std::str::from_utf8(&rendered.files()["reference/cli.html"].bytes).unwrap();
        let guide_sidebar = guide
            .split("class=\"contextual-nav\"")
            .nth(1)
            .unwrap()
            .split("</details>")
            .next()
            .unwrap();
        let reference_sidebar = commands
            .split("class=\"contextual-nav\"")
            .nth(1)
            .unwrap()
            .split("</details>")
            .next()
            .unwrap();
        assert!(guide_sidebar.contains("Introduction"));
        assert!(!guide_sidebar.contains("Commands"));
        assert!(reference_sidebar.contains("Reference &amp; tools"));
        assert!(reference_sidebar.contains("Commands"));
        assert!(!reference_sidebar.contains("Introduction"));
        assert!(commands.contains("data-section=\"reference/index.html\""));
        let route = if style == "file" { "index.html" } else { "./" };
        assert!(commands.contains(&format!(
            "href=\"{route}\" aria-current=\"true\">Reference &amp; tools</a>"
        )));
        assert!(!commands.contains("aria-current=\"true\">Guide</a>"));
        assert!(guide.contains("aria-current=\"true\">Guide</a>"));
    }
}

#[test]
fn project_sections_reject_missing_overlapping_and_non_index_destinations() {
    for sections in [
        "  sections:\n    - label: \"Reference\"\n      path: \"missing/index.html\"",
        "  sections:\n    - label: \"Reference\"\n      path: \"reference/cli.html\"",
        "  sections:\n    - label: \"Guide\"\n      path: \"index.html\"",
        "  sections:\n    - label: \" \"\n      path: \"reference/index.html\"",
        "  sections:\n    - label: \"Reference\"\n      path: \"reference/index.html\"\n    - label: \"Nested\"\n      path: \"reference/nested/index.html\"",
    ] {
        let root = workspace("file", sections);
        let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
        let snapshot =
            Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
        assert!(
            matches!(Site::new(&snapshot), Err(SiteError::Presentation(_))),
            "{sections}"
        );
    }
}
