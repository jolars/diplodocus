mod support;

use diplodocus::{
    assembly::{AssemblyError, assemble_workspace},
    commands::{self, ExtractOptions, GenerateOptions},
    diagnostics::DiagnosticCode,
    provenance::fingerprint_bytes,
    rendering::{render_fragments, render_site},
    site::Site,
    snapshots::Snapshot,
    validation::{DocumentIdentity, ReferenceTarget, resolve_workspace},
};

fn workspace(format: &str) -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.yaml", format!("project:\n  name: Includes\nrepositories:\n  - id: docs\n    path: .\ncontent:\n  - id: guide\n    owner: project\n    repository: docs\n    path: pages/index.{}\n    mount: \"\"\n    format: {}\n", if format == "gfm" { "md" } else { "qmd" }, format));
    root
}

#[test]
fn nested_includes_retain_original_ranges_and_allow_repeated_files() {
    let root = workspace("gfm");
    root.write(
        "pages/index.md",
        "# Main\n\n<!--@include: ../parts/first.md-->\n\n<!--@include: ../parts/first.md-->\n",
    );
    root.write("parts/first.md", "First.\n\n<!--@include: nested.md-->\n");
    root.write("parts/nested.md", "Nested **content**.\n");
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let page = sources.prepared_pages().values().next().unwrap();
    assert_eq!(page.source().matches("Nested **content**.").count(), 2);
    assert!(!page.source().contains("<!--@include"));
    let ranges: Vec<_> = page
        .source_ranges()
        .iter()
        .filter(|r| r.original.path.as_str() == "parts/nested.md")
        .collect();
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[0].line, 1);
    assert_eq!(ranges[0].include_chain.len(), 2);
    for range in ranges {
        assert_eq!(
            &page.source()[range.expanded.start..range.expanded.end],
            "Nested **content**.\n"
        );
        assert_eq!(range.original.span.unwrap().start, 0);
    }
}

#[test]
fn qmd_accepts_quoted_paths_and_collection_root_paths() {
    let root = workspace("qmd");
    root.write(
        "pages/index.qmd",
        "# Main\n\n{{< include \"/parts/my partial.qmd\" >}}\n",
    );
    root.write(
        "pages/parts/my partial.qmd",
        "Included.\n\n{{< include ../other.md >}}\n",
    );
    root.write("pages/other.md", "Other.\n");
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let page = sources.prepared_pages().values().next().unwrap();
    assert!(page.source().contains("Included."));
    assert!(page.source().contains("Other."));
}

#[test]
fn cycles_and_invalid_targets_report_the_requesting_source() {
    for (partial, message) in [
        ("<!--@include: first.md-->\n", "cycle"),
        ("<!--@include: missing.md-->\n", "regular file"),
        ("---\ntitle: Forbidden\n---\n", "frontmatter"),
        ("<!--@include: first.md{1,2}-->\n", "Invalid whole-file"),
        ("<!--@include: /etc/passwd-->\n", "Invalid whole-file"),
    ] {
        let root = workspace("gfm");
        root.write(
            "pages/index.md",
            "# Main\n\n<!--@include: ../parts/first.md-->\n",
        );
        root.write("parts/first.md", partial);
        let Err(AssemblyError::Diagnostics(diagnostics)) =
            assemble_workspace(root.path().join("diplodocus.yaml"))
        else {
            panic!("expected include diagnostic for {partial}");
        };
        assert_eq!(diagnostics[0].code, DiagnosticCode::InvalidMarkdownInclude);
        assert!(
            diagnostics[0].message.contains(message),
            "{:?}",
            diagnostics[0]
        );
        assert!(diagnostics[0].source.is_some());
        assert!(diagnostics[0].span.is_some());
    }
}

#[test]
fn directives_in_fences_and_other_formats_are_literal() {
    for format in ["gfm", "qmd"] {
        let root = workspace(format);
        let extension = if format == "gfm" { "md" } else { "qmd" };
        let content = "# Main\n\n```markdown\n<!--@include: missing.md-->\n\n{{< include missing.qmd >}}\n```\n";
        root.write(format!("pages/index.{extension}"), content);
        let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
        assert_eq!(
            sources.prepared_pages().values().next().unwrap().source(),
            content
        );
    }
}

#[test]
fn parser_diagnostics_point_into_included_source() {
    let root = workspace("qmd");
    root.write(
        "pages/index.qmd",
        "# Main\n\n{{< include ../parts/broken.qmd >}}\n",
    );
    root.write("parts/broken.qmd", "```{python}\n#| eval: [\n1\n```\n");
    let Err(AssemblyError::Diagnostics(diagnostics)) =
        assemble_workspace(root.path().join("diplodocus.yaml"))
    else {
        panic!("expected parser diagnostics");
    };
    assert!(diagnostics.iter().any(|d| matches!(&d.source, Some(diplodocus::diagnostics::DiagnosticSource::Repository { path, .. }) if path.as_str() == "parts/broken.qmd") && d.message.contains("Included from pages/index.qmd")));
}

#[test]
fn includes_preserve_paragraph_boundaries_and_crlf_source_offsets() {
    let root = workspace("gfm");
    root.write(
        "pages/index.md",
        "# Main\r\n\r\n<!--@include: @/partial.md-->\r\n\r\nAfter.\r\n",
    );
    root.write("pages/partial.md", "Partial without a final newline.");
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let page = sources.prepared_pages().values().next().unwrap();
    assert!(
        page.source()
            .contains("Partial without a final newline.\r\n\r\nAfter.")
    );
    let after = page.source_ranges().last().unwrap();
    assert_eq!(after.original.path.as_str(), "pages/index.md");
    assert_eq!(after.line, 3);
    sources.revalidate().unwrap();
    root.write("pages/partial.md", "Changed.");
    assert!(sources.revalidate().is_err());
}

#[cfg(unix)]
#[test]
fn includes_reject_repository_escapes_and_detect_symlink_cycles() {
    use std::os::unix::fs::symlink;
    let root = workspace("gfm");
    let outside = support::TestWorkspace::new();
    outside.write("outside.md", "Outside.");
    root.write("pages/index.md", "<!--@include: link.md-->\n");
    symlink(
        outside.path().join("outside.md"),
        root.path().join("pages/link.md"),
    )
    .unwrap();
    let Err(AssemblyError::Diagnostics(diagnostics)) =
        assemble_workspace(root.path().join("diplodocus.yaml"))
    else {
        panic!("expected boundary error");
    };
    assert!(
        diagnostics[0]
            .message
            .contains("inside the declared repository")
    );
    std::fs::remove_file(root.path().join("pages/link.md")).unwrap();
    symlink("index.md", root.path().join("pages/link.md")).unwrap();
    let Err(AssemblyError::Diagnostics(diagnostics)) =
        assemble_workspace(root.path().join("diplodocus.yaml"))
    else {
        panic!("expected cycle error");
    };
    assert!(diagnostics[0].message.contains("cycle"));
}

#[test]
fn executable_includes_prepare_expanded_cells_without_starting_a_kernel() {
    let root = workspace("qmd");
    let configuration = std::fs::read_to_string(root.path().join("diplodocus.yaml")).unwrap();
    root.write(
        "diplodocus.yaml",
        configuration
            + "    execution:\n      mode: execute\n      engine: jupyter\n      kernel: python3\n",
    );
    root.write(
        "pages/index.qmd",
        "{{< include partial.qmd >}}\n\n```{python}\n1 + 1\n```\n",
    );
    // Empty partials still count as includes for execution authority.
    root.write("pages/partial.qmd", "");
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let page = sources.prepared_pages().values().next().unwrap();
    assert!(page.preparation().unwrap().execution_eligible);
    assert_eq!(page.preparation().unwrap().cells.len(), 1);
    assert!(!page.source().contains("{{< include"));
    assert!(!root.path().join(".diplodocus").exists());
}

#[test]
fn invalid_utf8_extensions_and_generated_storage_produce_include_errors() {
    for target in ["invalid.md", "code.py", ".diplodocus/generated.md"] {
        let root = workspace("gfm");
        root.write("pages/index.md", format!("<!--@include: ../{target}-->\n"));
        root.write(target, "Content.");
        if target == "invalid.md" {
            std::fs::write(root.path().join(target), [0xff]).unwrap();
        }
        let Err(AssemblyError::Diagnostics(diagnostics)) =
            assemble_workspace(root.path().join("diplodocus.yaml"))
        else {
            panic!("expected invalid include target");
        };
        assert_eq!(diagnostics[0].code, DiagnosticCode::InvalidMarkdownInclude);
    }
}

#[test]
fn nested_include_sources_and_assets_survive_portable_generation() {
    for format in ["gfm", "qmd"] {
        let root = workspace(format);
        let extension = if format == "gfm" { "md" } else { "qmd" };
        let configuration = root.read("diplodocus.yaml");
        root.write(
            "diplodocus.yaml",
            configuration.replace(&format!("pages/index.{extension}"), "pages"),
        );
        let directive = |path: &str| {
            if format == "gfm" {
                format!("<!--@include: \"{path}\"-->")
            } else {
                format!("{{{{< include \"{path}\" >}}}}")
            }
        };
        root.write(format!("pages/index.{extension}"), format!(
            "# Main\r\n\r\n[Main download](data.txt) ![Main image](chart.svg)\r\n\r\n[Defined in partial][partial-data]\r\n\r\n{}\r\n\r\n[main-data]: data.txt\r\n",
            directive(&format!("../shared % space/first.{extension}"))
        ));
        root.write(format!("shared % space/first.{extension}"), format!(
            "## Included\n\n- **[Included download](data.txt)** ![Included image](chart.svg)\n- [Main definition][main-data]\n\n{}\n",
            directive(&format!("nested/second.{extension}"))
        ));
        let nested = "## Nested {#nested}\n\nUnicode: π.\n\n> [Nested download](data.txt) ![Nested image][nested-image]\n\n[Page](../../pages/target.EXT#target) [Local](#nested) [External](https://example.com/a%2Fb)\n\n[partial-data]: data%23file.txt#section\n[nested-image]: chart.svg\n".replace("EXT", extension);
        root.write(format!("shared % space/nested/second.{extension}"), &nested);
        root.write(format!("pages/target.{extension}"), "# Target {#target}\n");
        let mut destinations = Vec::new();
        for (directory, label) in [
            ("pages", "Main"),
            ("shared % space", "Included"),
            ("shared % space/nested", "Nested"),
        ] {
            root.write(format!("{directory}/data.txt"), label);
            let svg =
                format!("<svg xmlns='http://www.w3.org/2000/svg'><title>{label}</title></svg>");
            root.write(format!("{directory}/chart.svg"), &svg);
            destinations.push((
                label,
                fingerprint_bytes(label.as_bytes()).value,
                fingerprint_bytes(svg.as_bytes()).value,
            ));
        }
        root.write(
            "shared % space/nested/data#file.txt",
            "Reference definition",
        );
        let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
        let (page_id, prepared) = sources
            .prepared_pages()
            .iter()
            .find(|(_, p)| p.source().contains("# Main"))
            .unwrap();
        assert!(prepared.source().contains("[Included download](data.txt)"));
        let identity = DocumentIdentity::Page {
            page: page_id.clone(),
        };
        let source_ranges = prepared.source_ranges().to_vec();
        assert!(source_ranges.iter().any(|r| r.include_chain.len() == 2));
        for range in &source_ranges {
            let original = root.read(range.original.path.as_str());
            let span = range.original.span.unwrap();
            assert_eq!(
                &prepared.source()[range.expanded.start..range.expanded.end],
                &original[span.start..span.end]
            );
            assert_eq!(
                range.line,
                original[..span.start]
                    .bytes()
                    .filter(|b| *b == b'\n')
                    .count()
                    + 1
            );
            for entry in &range.include_chain {
                let original = root.read(entry.path.as_str());
                let span = entry.span.unwrap();
                let target = if entry.path.as_str().starts_with("pages/") {
                    format!("../shared % space/first.{extension}")
                } else {
                    format!("nested/second.{extension}")
                };
                assert_eq!(&original[span.start..span.end], directive(&target));
            }
        }
        let resolved = resolve_workspace(&sources).unwrap();
        assert_eq!(resolved.assets().len(), 7);
        assert!(resolved.records().iter().flat_map(|record| &record.references).any(|reference|
            matches!(&reference.target, ReferenceTarget::Page { fragment: Some(fragment), .. } if fragment == "target")));
        let snapshot = Snapshot::from_sources(&sources, &resolved).unwrap();
        let target = tempfile::tempdir().unwrap();
        let input = target.path().join("documentation.sqlite");
        commands::extract(ExtractOptions {
            config: root.path().join("diplodocus.yaml"),
            output: Some(input.clone()),
        })
        .unwrap();
        let export = snapshot.canonical_export().unwrap();
        let documents = snapshot.documents().to_vec();
        let assets = snapshot.assets().clone();
        let source_path = root.path().to_owned();
        assert!(!export.contains(source_path.to_str().unwrap()));
        drop((snapshot, sources, resolved, root));
        assert!(!source_path.exists());

        let loaded = Snapshot::load(&input).unwrap();
        assert_eq!(loaded.canonical_export().unwrap(), export);
        assert_eq!(loaded.documents(), documents);
        assert_eq!(loaded.assets(), &assets);
        assert_eq!(
            loaded
                .documents()
                .iter()
                .find(|r| r.document == identity)
                .unwrap()
                .source_ranges,
            source_ranges
        );
        let output = target.path().join("site");
        commands::generate(GenerateOptions {
            input,
            output: output.clone(),
        })
        .unwrap();
        let site = Site::new(&loaded).unwrap();
        for (path, file) in render_site(&site).unwrap().files() {
            assert_eq!(std::fs::read(output.join(path)).unwrap(), file.bytes);
        }
        for rendered in [
            render_site(&site).unwrap(),
            render_fragments(&site).unwrap(),
        ] {
            let html = std::str::from_utf8(&rendered.files()["index.html"].bytes).unwrap();
            let markdown = std::str::from_utf8(&rendered.files()["index.md"].bytes).unwrap();
            for (label, download, image) in &destinations {
                assert_eq!(
                    rendered.files()[&format!("assets/{download}.bin")].bytes,
                    label.as_bytes()
                );
                assert_eq!(
                    rendered.files()[&format!("assets/{image}.svg")].bytes,
                    assets[image].bytes
                );
                assert!(
                    html.contains(&format!("href=\"assets/{download}.bin\"")),
                    "{label}: {html}"
                );
                assert!(
                    html.contains(&format!("src=\"assets/{image}.svg\"")),
                    "{label}: {html}"
                );
                assert!(markdown.contains(download), "{label}: {markdown}");
                assert!(markdown.contains(image), "{label}: {markdown}");
            }
            let main_download = &destinations[0].1;
            assert!(html.contains(&format!(
                "href=\"assets/{main_download}.bin\">Main definition</a>"
            )));
            let defined_download = fingerprint_bytes(b"Reference definition").value;
            assert!(html.contains(&format!(
                "href=\"assets/{defined_download}.bin#section\">Defined in partial</a>"
            )));
            assert!(html.contains("href=\"target.html#target\""));
            assert!(html.contains("href=\"#nested\""));
            assert!(html.contains("href=\"https://example.com/a%2Fb\""));
            assert!(markdown.contains("target.html#target"));
        }
    }
}

#[test]
fn same_directory_partials_can_reference_assets_outside_the_collection() {
    let root = workspace("gfm");
    root.write("pages/index.md", "<!--@include: partial.md-->\n");
    root.write("pages/partial.md", "[Download](../assets/data.txt)\n");
    root.write("assets/data.txt", "Original");
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let resolved = resolve_workspace(&sources).unwrap();
    assert_eq!(resolved.assets().len(), 1);
    assert!(
        resolved
            .input_paths()
            .any(|path| path.ends_with("assets/data.txt"))
    );
    resolved.revalidate().unwrap();
    root.write("assets/data.txt", "Changed");
    assert!(resolved.revalidate().is_err());
}

#[test]
fn included_resource_errors_identify_the_partial_and_include_chain() {
    for target in ["missing.svg", "invalid.svg", "../.diplodocus/hidden.svg"] {
        let root = workspace("gfm");
        root.write(
            "pages/index.md",
            "# Main\n\n<!--@include: ../parts/first.md-->\n",
        );
        root.write("parts/first.md", "First.\n\n<!--@include: nested.md-->\n");
        let partial = format!("Nested.\n\n![Broken]({target})\n");
        root.write("parts/nested.md", &partial);
        root.write("parts/invalid.svg", "Not an image");
        root.write(".diplodocus/hidden.svg", "<svg/>");
        let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
        let error = resolve_workspace(&sources).unwrap_err();
        let diagnostic = &error.diagnostics()[0];
        assert!(
            matches!(&diagnostic.source, Some(diplodocus::diagnostics::DiagnosticSource::Repository { path, .. }) if path.as_str() == "parts/nested.md")
        );
        assert_eq!(
            diagnostic.span.unwrap().start,
            partial.find("![Broken]").unwrap()
        );
        assert!(diagnostic.message.contains("Included from pages/index.md"));
        assert!(diagnostic.message.contains("Included from parts/first.md"));
    }
}

#[cfg(unix)]
#[test]
fn included_resources_reject_symlinks_outside_the_repository() {
    use std::os::unix::fs::symlink;
    let root = workspace("gfm");
    let outside = support::TestWorkspace::new();
    outside.write("secret.txt", "Outside");
    root.write("pages/index.md", "<!--@include: ../parts/first.md-->\n");
    root.write("parts/first.md", "[Escape](outside/secret.txt)\n");
    symlink(outside.path(), root.path().join("parts/outside")).unwrap();
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    assert!(resolve_workspace(&sources).is_err());
    // Reentering the repository must not hide the escaped intermediate path.
    symlink(root.path().join("parts"), outside.path().join("back")).unwrap();
    root.write("parts/first.md", "[Escape](outside/back/data.txt)\n");
    root.write("parts/data.txt", "Inside");
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    assert!(resolve_workspace(&sources).is_err());
}
