mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::resolve_workspace;

fn code_blocks(html: &str) -> Vec<(bool, String)> {
    use html5ever::tendril::TendrilSink;
    use markup5ever_rcdom::{Handle, NodeData, RcDom};

    fn text(node: &Handle) -> String {
        if let NodeData::Text { contents } = &node.data {
            return contents.borrow().to_string();
        }
        node.children.borrow().iter().map(text).collect()
    }

    fn visit(node: &Handle, blocks: &mut Vec<(bool, String)>) {
        if let NodeData::Element { name, attrs, .. } = &node.data
            && name.local.as_ref() == "pre"
        {
            let highlighted = attrs.borrow().iter().any(|attribute| {
                attribute.name.local.as_ref() == "class"
                    && attribute
                        .value
                        .split_whitespace()
                        .any(|c| c == "syntax-code")
            });
            blocks.push((highlighted, text(node)));
        }
        for child in node.children.borrow().iter() {
            visit(child, blocks);
        }
    }

    let document = html5ever::parse_document(RcDom::default(), Default::default()).one(html);
    let mut blocks = Vec::new();
    visit(&document.document, &mut blocks);
    blocks
}

#[test]
fn highlighting_survives_snapshot_generation_without_sources() {
    let root = support::TestWorkspace::new();
    root.write(
        "diplodocus.toml",
        "[project]\nname='Highlighting'\n\
         [[repository]]\nid='docs'\npath='.'\n\
         [[content]]\nid='guide'\nowner='project'\nrepository='docs'\n\
         path='guide'\nmount='guide'\nformat='qmd'\n",
    );
    root.write(
        "guide/index.qmd",
        "# Examples\n\n\
         ```python\nmessage = '<script>& café</script>'\n```\n\n\
         > ```r\n> x <- 42\n> ```\n\n\
         ```julia\nx = 42\n```\n\n\
         ```{python, echo=false}\nx = 1\n```\n\n\
         ```unknown\nliteral <markup>\n```\n\n\
         ```text\nplain text\n```\n",
    );
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let artifact = tempfile::tempdir().unwrap();
    let path = artifact.path().join("snapshot.sqlite");
    snapshot.publish(&path).unwrap();
    drop(sources);
    drop(root);
    let loaded = Snapshot::load(&path).unwrap();
    let rendered =
        diplodocus::rendering::render_site(&diplodocus::site::Site::new(&loaded).unwrap()).unwrap();
    let html = std::str::from_utf8(&rendered.files()["guide/index.html"].bytes).unwrap();
    assert_eq!(
        code_blocks(html),
        [
            (true, "message = '<script>& café</script>'\n".into()),
            (true, "x <- 42\n".into()),
            (true, "x = 42\n".into()),
            (true, "x = 1\n".into()),
            (false, "literal <markup>\n".into()),
            (false, "plain text\n".into()),
        ]
    );
    assert!(!html.contains("<script>& café</script>"));
    let css = std::str::from_utf8(&rendered.files()["assets/site.css"].bytes).unwrap();
    assert!(css.contains(".syntax-code"));
    assert!(css.contains(".syntax-keyword"));
    assert!(!html.contains("highlight.js"));
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn executed_source_is_highlighted_while_hidden_source_and_literal_outputs_stay_plain() {
    use diplodocus::assembly::WorkspaceExecution;
    use diplodocus::execution::ExecutionDeadlines;
    use diplodocus::validation::resolve_executed_workspace;

    let root = support::TestWorkspace::new();
    root.write(
        "diplodocus.toml",
        "[project]\nname='Execution highlighting'\n\
         [[repository]]\nid='docs'\npath='.'\n\
         [[content]]\nid='guide'\nowner='project'\nrepository='docs'\n\
         path='guide'\nmount='guide'\nformat='qmd'\n\
         [content.execution]\nmode='execute'\nengine='jupyter'\nkernel='python3'\n",
    );
    root.write(
        "guide/index.qmd",
        "# Execution\n\n\
         ```{python}\nprint('<script>& output</script>')\n```\n\n\
         ```{python, echo=false}\nsecret = 'hidden-echo-source'\n```\n\n\
         ```{python, include=false}\nsecret = 'hidden-include-source'\nprint(secret)\n```\n\n\
         ```{python}\nimport sys\nprint('<stderr>', file=sys.stderr)\n```\n\n\
         ```{python, error=true}\nraise ValueError('literal-error')\n```\n\n\
         ```{python}\nfrom IPython.display import Markdown\nMarkdown('```python\\nx = 7\\n```')\n```\n",
    );
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let stage = tempfile::tempdir().unwrap();
    let executed = sources
        .execute(WorkspaceExecution {
            staging_parent: stage.path(),
            deadlines: ExecutionDeadlines::default(),
            cancellation: Box::pin(std::future::pending()),
        })
        .await
        .unwrap();
    let snapshot =
        Snapshot::from_executed(&executed, &resolve_executed_workspace(&executed).unwrap())
            .unwrap();
    executed.discard().unwrap();
    let artifact = tempfile::tempdir().unwrap();
    let path = artifact.path().join("snapshot.sqlite");
    snapshot.publish(&path).unwrap();
    drop(root);
    let loaded = Snapshot::load(&path).unwrap();
    let rendered =
        diplodocus::rendering::render_site(&diplodocus::site::Site::new(&loaded).unwrap()).unwrap();
    let html = std::str::from_utf8(&rendered.files()["guide/index.html"].bytes).unwrap();
    let blocks = code_blocks(html);
    assert_eq!(
        blocks
            .iter()
            .filter(|(highlighted, _)| *highlighted)
            .count(),
        5
    );
    assert!(blocks.contains(&(true, "print('<script>& output</script>')\n".into())));
    assert!(blocks.contains(&(false, "<script>& output</script>\n".into())));
    assert!(blocks.contains(&(false, "<stderr>\n".into())));
    assert!(blocks.contains(&(true, "x = 7\n".into())));
    assert!(
        blocks
            .iter()
            .any(|(highlighted, text)| !highlighted && text.contains("ValueError: literal-error"))
    );
    assert!(!html.contains("hidden-echo-source"));
    assert!(!html.contains("hidden-include-source"));
    assert!(!html.contains("<script>& output</script>"));
}

#[test]
fn footer_survives_snapshot_generation_without_sources_and_escapes_its_content() {
    let root = support::TestWorkspace::new();
    root.write(
        "diplodocus.toml",
        r#"[project]
name = 'Footer'
[presentation.footer]
docs-version = 'dev <preview>'
copyright = '© 2026 A & B'
licenses = [
  {label='MIT', url='https://example.test/LICENSE-MIT'},
  {label='Apache-2.0', url='https://example.test/LICENSE-APACHE'},
]
links = [
  {label='<GitHub>', url='https://example.test/repo?a=1&b=2'},
  {label='Report an issue', url='https://example.test/issues'},
]
built-with = true
[[repository]]
id = 'docs'
path = '.'
[[content]]
id = 'guide'
owner = 'project'
repository = 'docs'
path = 'docs'
mount = ''
format = 'gfm'
"#,
    );
    root.write("docs/index.md", "# Home\n\nWelcome.\n");
    root.write(
        "docs/guide/nested.md",
        "# Nested\n\n## Examples\n\nAn example.\n",
    );
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let artifact = tempfile::tempdir().unwrap();
    let path = artifact.path().join("snapshot.sqlite");
    snapshot.publish(&path).unwrap();
    let footer = snapshot.presentation().footer.clone();
    drop(sources);
    drop(root);
    let loaded = Snapshot::load(&path).unwrap();
    assert_eq!(loaded.presentation().footer, footer);
    let rendered =
        diplodocus::rendering::render_site(&diplodocus::site::Site::new(&loaded).unwrap()).unwrap();
    for route in ["index.html", "guide/nested.html"] {
        let html = std::str::from_utf8(&rendered.files()[route].bytes).unwrap();
        assert_eq!(html.matches("<footer class=\"site-footer\">").count(), 1);
        let footer = html.split("<footer class=\"site-footer\">").nth(1).unwrap();
        assert!(html.find("</main>").unwrap() < html.find("<footer").unwrap());
        assert!(footer.contains("Docs: dev &lt;preview&gt;"));
        assert!(footer.contains("© 2026 A &amp; B"));
        assert!(
            footer.contains("href=\"https://example.test/repo?a=1&amp;b=2\">&lt;GitHub&gt;</a>")
        );
        assert!(footer.contains("href=\"https://example.test/LICENSE-MIT\">MIT</a> or <a"));
        assert!(footer.contains("href=\"https://example.test/LICENSE-APACHE\">Apache-2.0</a>"));
        assert!(footer.contains("href=\"https://example.test/issues\">Report an issue</a>"));
        assert!(footer.contains("Built with <a href=\"https://diplodocus.cc/\">Diplodocus</a>"));
    }
    let search = std::str::from_utf8(&rendered.files()["assets/search.json"].bytes).unwrap();
    assert!(!search.contains("preview"));
    assert!(!search.contains("Report an issue"));
}

#[test]
fn footer_omits_unconfigured_content_and_credit_is_optional() {
    for (settings, expected) in [
        ("", None),
        ("[presentation.footer]\n", None),
        ("[presentation.footer]\nbuilt-with=false\n", None),
        (
            "[presentation.footer]\ndocs-version='main'\n",
            Some("<p>Docs: main</p>"),
        ),
        (
            "[presentation.footer]\nbuilt-with=true\n",
            Some("<p>Built with <a href=\"https://diplodocus.cc/\">Diplodocus</a></p>"),
        ),
    ] {
        let root = support::TestWorkspace::new();
        root.write(
            "diplodocus.toml",
            format!("[project]\nname='Footer'\n{settings}"),
        );
        let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
        let snapshot =
            Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
        let rendered =
            diplodocus::rendering::render_site(&diplodocus::site::Site::new(&snapshot).unwrap())
                .unwrap();
        let html = std::str::from_utf8(&rendered.files()["index.html"].bytes).unwrap();
        match expected {
            Some(content) => {
                assert!(html.contains(&format!("<footer class=\"site-footer\">{content}</footer>")))
            }
            None => {
                assert!(!html.contains("<footer"));
                assert!(!snapshot.canonical_export().unwrap().contains("\"footer\""));
            }
        }
    }
}

#[test]
fn eulerr_datasets_survive_snapshots_and_render_bare_usage_format_and_source() {
    let root = support::acceptance_workspace();
    support::add_r_datasets(&root);
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let artifact = tempfile::tempdir().unwrap();
    let path = artifact.path().join("snapshot.sqlite");
    snapshot.publish(&path).unwrap();
    drop(root);
    let loaded = Snapshot::load(&path).unwrap();
    let site = diplodocus::site::Site::new(&loaded).unwrap();
    let rendered = diplodocus::rendering::render_site(&site).unwrap();
    let search = std::str::from_utf8(&rendered.files()["assets/search.json"].bytes).unwrap();
    for (name, format, source) in [
        ("fruits", "100 observations of 5 variables", None),
        (
            "organisms",
            "7 observations",
            Some("VennMaster/blob/master/data_examples/deploy/example1.list"),
        ),
        (
            "pain",
            "pain distribution in rows",
            Some("Westergren H, Larsson J, Freeman M"),
        ),
        (
            "plants",
            "33,721 plants",
            Some("National Plant Data Center"),
        ),
    ] {
        let html = rendered
            .files()
            .values()
            .filter(|file| file.media_type.starts_with("text/html"))
            .map(|file| std::str::from_utf8(&file.bytes).unwrap())
            .find(|html| html.contains(&format!("<h1><code>{name}</code></h1>")))
            .unwrap();
        assert!(html.contains(format));
        assert_eq!(
            code_blocks(html)
                .iter()
                .filter(|(highlighted, text)| *highlighted && text == name)
                .count(),
            2
        );
        assert!(!html.contains(&format!("{name}()")));
        assert!(html.contains(">Format</h2>"));
        assert!(html.contains(">Usage</h2>"));
        if let Some(source) = source {
            assert!(html.contains(">Source</h2>"));
            assert!(html.contains(source));
        }
        assert!(search.contains(&format!("\"title\":\"{name}\"")));
        let navigation = html
            .split("<nav aria-label=\"Documentation\">")
            .nth(1)
            .unwrap()
            .split("<main")
            .next()
            .unwrap();
        assert!(navigation.contains(&format!("><code>{name}</code></a>")));
    }
}

#[test]
fn shared_r_topics_and_method_navigation_survive_generation_without_sources() {
    use diplodocus::ir::ItemLanguageData;
    use diplodocus::validation::resolve_item;

    let root = support::shared_r_workspace();
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let artifact = tempfile::tempdir().unwrap();
    let path = artifact.path().join("snapshot.sqlite");
    snapshot.publish(&path).unwrap();
    drop(sources);
    drop(root);
    let loaded = Snapshot::load(&path).unwrap();
    let site = diplodocus::site::Site::new(&loaded).unwrap();
    let rendered = diplodocus::rendering::render_site(&site).unwrap();
    for alias in [
        "fit",
        "fit.default",
        "fit.model",
        "model-fitting",
        "eulergram-compose",
        "|.eulergram",
        "/.eulergram",
    ] {
        let reference = resolve_item(loaded.workspace(), Some("rfoo"), alias).unwrap();
        let item = &loaded.workspace().packages["rfoo"].items[&reference.item];
        let html = rendered
            .files()
            .values()
            .filter(|file| file.media_type.starts_with("text/html"))
            .map(|file| std::str::from_utf8(&file.bytes).unwrap())
            .find(|html| html.contains(&format!("<h1><code>{}</code></h1>", item.qualified_name)))
            .unwrap();
        let blocks = code_blocks(html);
        let Some(ItemLanguageData::R(data)) = &item.language_data else {
            panic!()
        };
        let topic = data.help_topic.as_ref().unwrap();
        let method_names = if topic.name == "fit" {
            assert!(html.contains("Observation weights for the model method."));
            assert_eq!(blocks.len(), 3);
            vec!["fit.default", "fit.model"]
        } else {
            assert_eq!(blocks.len(), 3);
            assert!(
                blocks
                    .iter()
                    .any(|(_, source)| source == "`|.eulergram`(e1, e2)")
            );
            assert!(
                blocks
                    .iter()
                    .any(|(_, source)| source == "`/.eulergram`(e1, e2)")
            );
            vec!["|.eulergram", "/.eulergram"]
        };
        assert!(html.contains("aria-label=\"S3 methods by class\""));
        for name in method_names {
            let member = resolve_item(loaded.workspace(), Some("rfoo"), name).unwrap();
            assert!(topic.members.contains(&member));
            let (target, _) = rendered
                .files()
                .iter()
                .filter(|(_, file)| file.media_type.starts_with("text/html"))
                .find(|(_, file)| {
                    std::str::from_utf8(&file.bytes)
                        .unwrap()
                        .contains(&format!("<h1><code>{name}</code></h1>"))
                })
                .unwrap();
            let filename = target.rsplit('/').next().unwrap();
            let href =
                percent_encoding::utf8_percent_encode(filename, percent_encoding::NON_ALPHANUMERIC);
            assert!(html.contains(&format!("href=\"{href}\"")));
        }
    }
    assert_eq!(
        resolve_item(loaded.workspace(), Some("rfoo"), "model-fitting").unwrap(),
        resolve_item(loaded.workspace(), Some("rfoo"), "fit").unwrap()
    );
    assert_eq!(
        resolve_item(loaded.workspace(), Some("rfoo"), "eulergram-compose").unwrap(),
        resolve_item(loaded.workspace(), Some("rfoo"), "|.eulergram").unwrap()
    );
}

#[test]
fn internal_r_topics_survive_snapshots_without_public_navigation_or_search() {
    let root = support::acceptance_workspace();
    let fixture = support::fixture_path("r-internal-topics");
    for directory in ["R", "man"] {
        for file in support::files_under(&fixture.join(directory)) {
            root.write(
                std::path::Path::new("r").join(directory).join(&file),
                std::fs::read(fixture.join(directory).join(&file)).unwrap(),
            );
        }
    }
    root.write(
        "r/NAMESPACE",
        format!(
            "{}\nimport(grid)\nS3method(makeContent, EulerGlyphs)\n",
            root.read("r/NAMESPACE")
        ),
    );
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let artifact = tempfile::tempdir().unwrap();
    let path = artifact.path().join("snapshot.sqlite");
    snapshot.publish(&path).unwrap();
    drop(root);
    let loaded = Snapshot::load(&path).unwrap();
    let site = diplodocus::site::Site::new(&loaded).unwrap();
    let rendered = diplodocus::rendering::render_site(&site).unwrap();
    let search = std::str::from_utf8(&rendered.files()["assets/search.json"].bytes).unwrap();
    assert!(search.contains("mean_squared_error"));
    for name in ["makeContent.EulerGlyphs", "setup_gpar"] {
        assert!(!search.contains(name));
        let mut found = false;
        for file in rendered
            .files()
            .values()
            .filter(|file| file.media_type.starts_with("text/html"))
        {
            let html = std::str::from_utf8(&file.bytes).unwrap();
            let nav = html
                .split("<nav aria-label=\"Documentation\">")
                .nth(1)
                .unwrap()
                .split("</nav>")
                .next()
                .unwrap();
            assert!(!nav.contains(name));
            found |= html.contains(&format!("<h1><code>{name}</code></h1>"));
        }
        assert!(found, "The internal topic remains addressable: {name}");
    }
}

#[test]
fn unsupported_rd_renders_compact_markers_and_authored_source_stays_visible() {
    let root = support::acceptance_case("unsupported-rd");
    let rd = root.read("r/man/experimental_summary.Rd");
    root.write(
        "r/man/experimental_summary.Rd",
        rd.replace(
            "experimental_summary(c(3, 1, 2))",
            "\\Sexpr{stop('must not execute')}",
        ),
    );
    root.write(
        "python/docs/fallbacks.qmd",
        "# Authored fallbacks\n\n<aside>Authored block</aside>\n\nText $x^2$.\n",
    );
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let artifact = tempfile::tempdir().unwrap();
    let path = artifact.path().join("snapshot.sqlite");
    snapshot.publish(&path).unwrap();
    drop(root);
    let loaded = Snapshot::load(&path).unwrap();
    let site = diplodocus::site::Site::new(&loaded).unwrap();
    let rendered = diplodocus::rendering::render_site(&site).unwrap();
    let html = rendered
        .files()
        .values()
        .filter(|file| file.media_type.starts_with("text/html"))
        .map(|file| std::str::from_utf8(&file.bytes).unwrap())
        .find(|html| html.contains("<h1><code>experimental_summary</code></h1>"))
        .unwrap();
    assert!(html.contains("[Unsupported Rd: \\Sexpr]"));
    assert!(html.contains("[Unsupported Rd: examples]"));
    assert!(html.contains("Compute three summary statistics"));
    assert!(!html.contains("\\name{experimental_summary}"));
    assert!(!html.contains("must not execute"));
    let authored =
        std::str::from_utf8(&rendered.files()["packages/python/guide/fallbacks.html"].bytes)
            .unwrap();
    assert!(authored.contains("&lt;aside&gt;Authored block&lt;/aside&gt;"));
    assert!(authored.contains("$x^2$"));
    assert!(!authored.contains("[Unsupported Rd:"));
}

#[test]
fn page_navigation_preserves_authored_ids_and_disambiguates_repeated_headings() {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.toml", "[project]\nname='Navigation'\n[[repository]]\nid='docs'\npath='.'\n[[content]]\nid='guide'\nowner='project'\nrepository='docs'\npath='docs'\nmount=''\nformat='qmd'\n");
    root.write("docs/index.qmd", "# Guide\n\n## **Examples**\n\nFirst.\n\n## Examples {#examples}\n\nSecond.\n\n### Examples\n\nThird.\n");
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let rendered =
        diplodocus::rendering::render_site(&diplodocus::site::Site::new(&snapshot).unwrap())
            .unwrap();
    let html = std::str::from_utf8(&rendered.files()["index.html"].bytes).unwrap();
    assert!(html.contains("aria-label=\"On this page\""));
    for identifier in ["examples", "examples-2", "examples-3"] {
        assert_eq!(html.matches(&format!("id=\"{identifier}\"")).count(), 1);
        assert!(html.contains(&format!("href=\"#{identifier}\"")));
    }
    assert!(html.contains("<h2 id=\"examples\">Examples</h2>"));
}

#[test]
fn authored_titles_reuse_matching_headings_and_preserve_their_anchors() {
    for (source, heading_count, preserved) in [
        (
            "# **Guide** {#overview}\n\n[Top](#overview)\n",
            1,
            "<h1 id=\"overview\"><strong>Guide</strong></h1>",
        ),
        (
            "---\ntitle: Guide\n---\n\n# Guide {#overview}\n",
            1,
            "<h1 id=\"overview\">Guide</h1>",
        ),
        ("---\ntitle: Guide\n---\n\nWelcome.\n", 1, "<h1>Guide</h1>"),
        (
            "---\ntitle: Guide\n---\n\n# Different heading\n",
            2,
            "<h1>Guide</h1>",
        ),
        ("## Guide {#guide}\n", 1, "<h2 id=\"guide\">Guide</h2>"),
    ] {
        let root = support::TestWorkspace::new();
        root.write("diplodocus.toml", "[project]\nname='Titles'\n[[repository]]\nid='docs'\npath='.'\n[[content]]\nid='guide'\nowner='project'\nrepository='docs'\npath='docs'\nmount=''\nformat='qmd'\n");
        root.write("docs/index.qmd", source);
        let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
        let snapshot =
            Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
        let rendered =
            diplodocus::rendering::render_site(&diplodocus::site::Site::new(&snapshot).unwrap())
                .unwrap();
        let html = std::str::from_utf8(&rendered.files()["index.html"].bytes).unwrap();
        assert_eq!(html.matches("<h1").count(), heading_count, "{source}");
        assert!(html.contains(preserved), "{html}");
        assert!(html.contains("<title>Guide · Titles</title>"));
    }
}

#[test]
fn static_site_uses_only_snapshot_bytes_and_preserves_output_on_failure() {
    let root = support::acceptance_workspace();
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolved).unwrap();
    drop(root);
    let site = diplodocus::site::Site::new(&snapshot).unwrap();
    let rendered = diplodocus::rendering::render_site(&site).unwrap();
    assert!(rendered.files().contains_key("index.html"));
    assert!(rendered.files().contains_key("guide/index.html"));
    assert!(rendered.files().contains_key("packages/python/index.html"));
    let html = std::str::from_utf8(&rendered.files()["guide/index.html"].bytes).unwrap();
    assert!(html.contains("<html lang=\"en\">"));
    assert!(html.contains("../assets/"));
    assert!(!html.contains("src=\"/"));
    let output = tempfile::tempdir().unwrap();
    let path = output.path().join("site");
    rendered.publish(&path).unwrap();
    let before = std::fs::read(path.join("index.html")).unwrap();
    std::fs::write(path.join("obsolete.html"), "old").unwrap();
    rendered.publish(&path).unwrap();
    assert!(!path.join("obsolete.html").exists());
    assert_eq!(std::fs::read(path.join("index.html")).unwrap(), before);
    assert_eq!(std::fs::read_dir(output.path()).unwrap().count(), 1);
}

#[test]
fn all_rendered_local_links_and_images_resolve_beneath_a_hosting_prefix() {
    use html5ever::tendril::TendrilSink;
    use markup5ever_rcdom::{Handle, NodeData, RcDom};
    fn links(node: &Handle, urls: &mut Vec<String>, ids: &mut std::collections::BTreeSet<String>) {
        if let NodeData::Element { attrs, .. } = &node.data {
            for attribute in attrs.borrow().iter() {
                match attribute.name.local.as_ref() {
                    "href" | "src" => urls.push(attribute.value.to_string()),
                    "id" => {
                        ids.insert(attribute.value.to_string());
                    }
                    _ => {}
                }
            }
        }
        for child in node.children.borrow().iter() {
            links(child, urls, ids);
        }
    }
    let root = support::acceptance_workspace();
    let sources = assemble_workspace(root.path().join("workspace/diplodocus.toml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let rendered =
        diplodocus::rendering::render_site(&diplodocus::site::Site::new(&snapshot).unwrap())
            .unwrap();
    let mut records = std::collections::BTreeMap::new();
    for (path, file) in rendered
        .files()
        .iter()
        .filter(|(p, _)| p.ends_with(".html"))
    {
        let dom = html5ever::parse_document(RcDom::default(), Default::default())
            .one(std::str::from_utf8(&file.bytes).unwrap());
        let mut urls = Vec::new();
        let mut ids = std::collections::BTreeSet::new();
        links(&dom.document, &mut urls, &mut ids);
        records.insert(path.clone(), (urls, ids));
    }
    for (path, (links, _)) in &records {
        let base = url::Url::parse(&format!("https://example.test/diplodocus/{path}")).unwrap();
        for link in links {
            let target = base.join(link).unwrap();
            if target.host_str() != Some("example.test") {
                continue;
            }
            let route = percent_encoding::percent_decode_str(
                target.path().strip_prefix("/diplodocus/").unwrap(),
            )
            .decode_utf8()
            .unwrap();
            assert!(
                rendered.files().contains_key(route.as_ref()),
                "{path} -> {link}"
            );
            if let Some(fragment) = target.fragment() {
                let fragment = percent_encoding::percent_decode_str(fragment)
                    .decode_utf8()
                    .unwrap();
                assert!(
                    records[route.as_ref()].1.contains(fragment.as_ref()),
                    "{path} -> {link}"
                );
            }
        }
    }
}

fn global_sections(html: &str) -> &str {
    html.split("class=\"global-nav desktop-sections\"")
        .nth(1)
        .unwrap()
        .split("</nav>")
        .next()
        .unwrap()
}

#[test]
fn section_labels_distinguish_packages_and_ignore_hidden_packages() {
    let root = support::acceptance_workspace();
    let path = "workspace/variants/visibility.toml";
    let mut configuration: toml::Value = toml::from_str(&root.read(path)).unwrap();
    let packages = configuration["package"].as_array_mut().unwrap();
    packages[0]["name"] = toml::Value::String("Foo <public> & API".into());
    for package in packages {
        package["targets"] = toml::Value::Array(Vec::new());
    }
    let configuration = toml::to_string(&configuration).unwrap();
    root.write(path, &configuration);
    let sources = assemble_workspace(root.path().join(path)).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let rendered =
        diplodocus::rendering::render_site(&diplodocus::site::Site::new(&snapshot).unwrap())
            .unwrap();
    let home = std::str::from_utf8(&rendered.files()["index.html"].bytes).unwrap();
    let sections = global_sections(home);
    assert!(sections.contains("Foo &lt;public&gt; &amp; API"));
    assert!(sections.contains("Foo internal interfaces"));
    assert!(!sections.contains("Foo hidden native interface"));
    assert!(!sections.contains(">Python</a>"));
    let package =
        std::str::from_utf8(&rendered.files()["packages/python/index.html"].bytes).unwrap();
    assert!(
        global_sections(package).contains("aria-current=\"true\">Foo &lt;public&gt; &amp; API")
    );
    assert!(!global_sections(package).contains("aria-current=\"true\">Guide"));

    root.write(
        path,
        configuration.replace("visibility = \"internal\"", "visibility = \"hidden\""),
    );
    let sources = assemble_workspace(root.path().join(path)).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let rendered =
        diplodocus::rendering::render_site(&diplodocus::site::Site::new(&snapshot).unwrap())
            .unwrap();
    let home = std::str::from_utf8(&rendered.files()["index.html"].bytes).unwrap();
    let sections = global_sections(home);
    assert!(sections.contains(">Python</a>"));
    assert!(!sections.contains("Foo internal interfaces"));
}

#[test]
fn package_sidebar_groups_authored_pages_and_omits_empty_api_sections() {
    let root = support::TestWorkspace::new();
    root.write(
        "diplodocus.toml",
        r#"
[project]
name = "Navigation"
[[repository]]
id = "docs"
path = "."
[[package]]
id = "rstats"
name = "Stats for R"
slug = "r"
ecosystem = "r"
repository = "docs"
path = "."
metadata-path = "DESCRIPTION"
targets = []
[[content]]
id = "shared"
owner = "project"
repository = "docs"
path = "docs"
mount = ""
format = "gfm"
[[content]]
id = "vignettes"
owner = "rstats"
repository = "docs"
path = "r-docs"
mount = "vignettes"
format = "gfm"
"#,
    );
    root.write(
        "DESCRIPTION",
        support::load_fixture("acceptance/r/DESCRIPTION"),
    );
    root.write("docs/index.md", "# Shared introduction\n");
    root.write("docs/concepts/index.md", "# Shared concepts\n");
    root.write("r-docs/index.md", "# R workflows\n");
    root.write("r-docs/evaluating.md", "# Evaluating a model\n");
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let rendered =
        diplodocus::rendering::render_site(&diplodocus::site::Site::new(&snapshot).unwrap())
            .unwrap();
    let page = std::str::from_utf8(&rendered.files()["packages/r/vignettes/evaluating.html"].bytes)
        .unwrap();
    assert!(
        global_sections(page).contains("href=\"../../../index%2Ehtml\""),
        "{}",
        global_sections(page)
    );
    assert!(global_sections(page).contains("href=\"../index%2Ehtml\" aria-current=\"true\">R"));
    assert!(page.contains("aria-label=\"R\""));
    assert!(page.contains("aria-label=\"R workflows\""));
    assert!(page.contains("href=\"evaluating%2Ehtml\" aria-current=\"page\">Evaluating a model"));
    assert!(!page.contains("aria-label=\"API reference\""));
    assert!(!page.contains("aria-label=\"Shared concepts\""));
    let home = std::str::from_utf8(&rendered.files()["index.html"].bytes).unwrap();
    assert!(home.contains("aria-label=\"Shared concepts\""));
    assert!(!home.contains("aria-label=\"R workflows\""));

    root.remove("r-docs/index.md");
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let rendered =
        diplodocus::rendering::render_site(&diplodocus::site::Site::new(&snapshot).unwrap())
            .unwrap();
    let page = std::str::from_utf8(&rendered.files()["packages/r/vignettes/evaluating.html"].bytes)
        .unwrap();
    assert!(page.contains("aria-label=\"Vignettes\""));
}
