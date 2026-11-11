mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::rendering::{render_fragments, render_site};
use diplodocus::site::Site;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;

#[test]
fn page_layouts_survive_snapshot_round_trip_without_sources() {
    for format in ["gfm", "qmd"] {
        for mount in ["", "guide"] {
            let root = support::TestWorkspace::new();
            root.write(
                "diplodocus.yaml",
                format!(
                    r#"project:
  name: Layouts
presentation:
  footer:
    docs-version: dev
repositories:
  - id: docs
    path: .
content:
  - id: guide
    owner: project
    repository: docs
    path: docs
    mount: "{mount}"
    format: {format}
"#
                ),
            );
            let extension = if format == "gfm" { "md" } else { "qmd" };
            for layout in ["wide", "full"] {
                let page = if layout == "wide" { "index" } else { "full" };
                root.write(format!("docs/{page}.{extension}"), format!("---\nlayout: '{layout}'\n---\n\n# Overview\n\n[Guide](guide.{extension})\n\n## Details\n\nWide content.\n"));
            }
            root.write(
                format!("docs/guide.{extension}"),
                "# Guide\n\n## Details\n\nRegular content.\n",
            );
            root.write(
                format!("docs/default.{extension}"),
                "---\nlayout: docs\n---\n\n# Default\n\n## Details\n",
            );
            let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
            let snapshot =
                Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
            let portable = support::TestWorkspace::new();
            let path = portable.path().join("snapshot.sqlite");
            snapshot.publish(&path).unwrap();
            drop(sources);
            drop(root);
            let snapshot = Snapshot::load(&path).unwrap();
            let site = Site::new(&snapshot).unwrap();
            let rendered = render_site(&site).unwrap();
            let prefix = if mount.is_empty() {
                String::new()
            } else {
                format!("{mount}/")
            };
            let fragments = render_fragments(&site).unwrap();
            for layout in ["wide", "full"] {
                let page = if layout == "wide" { "index" } else { "full" };
                let html =
                    std::str::from_utf8(&rendered.files()[&format!("{prefix}{page}.html")].bytes)
                        .unwrap();
                assert!(html.contains(&format!("data-layout=\"{layout}\"")));
                assert!(html.contains("class=\"brand\""));
                assert!(html.contains("class=\"site-search\""));
                assert!(html.contains("class=\"site-footer\""));
                assert!(html.contains(">Overview</h1>"));
                assert!(html.contains("<h2 id=\"details\">"));
                assert!(html.contains("Wide content."));
                assert!(!html.contains("class=\"page-toc"));
                assert!(!html.contains("class=\"home-hero"));
                assert_eq!(html.matches("<h1").count(), 1);
                let markdown =
                    std::str::from_utf8(&rendered.files()[&format!("{prefix}{page}.md")].bytes)
                        .unwrap();
                assert!(markdown.contains("# Overview"));
                assert!(!markdown.contains("layout:"));
                let fragment =
                    std::str::from_utf8(&fragments.files()[&format!("{prefix}{page}.html")].bytes)
                        .unwrap();
                assert!(fragment.contains("Wide content."));
                assert!(!fragment.contains("data-layout"));
            }
            for page in ["guide", "default"] {
                let regular =
                    std::str::from_utf8(&rendered.files()[&format!("{prefix}{page}.html")].bytes)
                        .unwrap();
                assert!(regular.contains("data-layout=\"docs\""));
                assert!(regular.contains("class=\"page-toc\""));
                assert!(regular.contains("Overview</a>"));
            }
        }
    }
}
