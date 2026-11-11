mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::rendering::render_site;
use diplodocus::site::Site;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;

#[test]
fn header_repository_selection_survives_detached_generation() {
    for (selection, names) in [
        ("", vec!["example/main", "forge.test/example/other"]),
        ("    repositories: [main]\n", vec!["Main &amp; source"]),
        (
            "    repositories: [other, main, alias]\n",
            vec!["forge.test/example/other", "example/main"],
        ),
        (
            "    repositories: [main, alias]\n",
            vec!["Main &amp; source"],
        ),
        ("    repositories: []\n", vec![]),
        ("    repositories: [local]\n", vec![]),
    ] {
        let root = support::TestWorkspace::new();
        root.write(
            "diplodocus.yaml",
            format!(
                "project:\n  name: Repositories\npresentation:\n  header:\n{selection}  footer:\n    links:\n      - label: Main & source\n        url: https://github.com/example/main\n      - label: Other source\n        url: https://forge.test/example/other\nrepositories:\n  - id: main\n    path: .\n    url: https://github.com/example/main\n  - id: alias\n    path: .\n    url: https://github.com/example/main/\n  - id: other\n    path: .\n    url: https://forge.test/example/other\n  - id: local\n    path: .\ncontent:\n  - id: guide\n    owner: project\n    repository: main\n    path: docs\n    mount: ''\n    format: gfm\n"
            ).replace("  header:\n  footer:", "  header: {}\n  footer:"),
        );
        root.write("docs/index.md", "# Home\n\nWelcome.\n");
        let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
        let snapshot =
            Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
        let portable = support::TestWorkspace::new();
        let path = portable.path().join("snapshot.sqlite");
        snapshot.publish(&path).unwrap();
        drop(sources);
        drop(root);
        let snapshot = Snapshot::load(&path).unwrap();
        let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
        let html = std::str::from_utf8(&rendered.files()["index.html"].bytes).unwrap();
        let header = html.split("</header>").next().unwrap();
        let desktop = header
            .split("<details class=\"mobile-menu\">")
            .next()
            .unwrap();
        assert_eq!(
            desktop.contains("class=\"repository-menu\""),
            names.len() > 1,
            "{selection}"
        );
        assert_eq!(
            desktop.matches("class=\"repository-link\"").count(),
            names.len(),
            "{selection}"
        );
        let mut position = 0;
        for name in &names {
            position += desktop[position..].find(name).unwrap() + name.len();
        }
        assert_eq!(
            header.contains("class=\"mobile-repositories\""),
            names.len() > 1
        );
        let footer = html.split("<footer class=\"site-footer\">").nth(1);
        if names.is_empty() {
            let footer = footer.unwrap();
            assert!(footer.contains("Main &amp; source"));
            assert!(footer.contains("Other source"));
        } else if names.len() == 1 {
            let footer = footer.unwrap();
            assert!(!footer.contains("Main &amp; source"));
            assert!(footer.contains("Other source"));
        } else {
            assert!(footer.is_none());
        }
    }
}
