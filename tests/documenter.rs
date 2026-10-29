mod support;

use diplodocus::{
    assembly::assemble_workspace,
    diagnostics::DiagnosticCode,
    documents::{AuthoredFormat, parse_authored_document},
    ir::{Block, Inline},
    rendering::{FragmentIndex, render_fragments, render_site},
    site::{Site, UrlMap},
    snapshots::Snapshot,
    validation::{DocumentIdentity, ReferenceTarget, resolve_workspace},
};

#[test]
fn documenter_parser_preserves_labels_levels_and_source_ranges() {
    let source = "# [A *guide*](@id manual)\n\nSee [API\nReference](@ref), [`fit`](@ref), and [gallery](@ref \"A gallery\").\n";
    let parsed = parse_authored_document(source, AuthoredFormat::Documenter);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let Block::Heading {
        level,
        attributes,
        inlines,
        span,
    } = &parsed.document.blocks[0]
    else {
        panic!("heading")
    };
    assert_eq!(*level, 1);
    assert_eq!(attributes.identifier.as_ref().unwrap().value, "manual");
    assert!(matches!(inlines.last(), Some(Inline::Emphasis { .. })));
    assert!(source[span.start..span.end].starts_with("# [A *guide*]"));
    let Block::Paragraph { inlines, .. } = &parsed.document.blocks[1] else {
        panic!("paragraph")
    };
    let links: Vec<_> = inlines
        .iter()
        .filter_map(|inline| match inline {
            Inline::Link {
                target,
                span,
                title,
                ..
            } => {
                assert!(title.is_none());
                assert!(source[span.start..span.end].starts_with('['));
                Some(target.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        links,
        ["@ref \"API Reference\"", "@ref `fit`", "@ref \"A gallery\""]
    );
}

#[test]
fn documenter_assembles_and_renders_from_a_relocated_snapshot() {
    let root = support::TestWorkspace::from_fixture("julia-documenter");
    root.write("docs/catalog.md", "# Catalog\n\n```@index\n```\n");
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    assert!(
        sources.workspace().diagnostics.is_empty(),
        "{:?}",
        sources.workspace().diagnostics
    );
    let resolved = resolve_workspace(&sources).unwrap();
    let api = sources
        .workspace()
        .pages
        .iter()
        .find(|(_, page)| page.title == "API Reference")
        .unwrap();
    let embedded: Vec<_> = api
        .1
        .document
        .document
        .blocks
        .iter()
        .filter_map(|block| {
            if let Block::ApiDocumentation {
                item, identifier, ..
            } = block
            {
                Some((item, identifier))
            } else {
                None
            }
        })
        .collect();
    let names: Vec<_> = embedded
        .iter()
        .map(|(item, _)| {
            sources.workspace().packages[&item.package].items[&item.item]
                .qualified_name
                .as_str()
        })
        .collect();
    assert_eq!(names, ["Demo.Shape", "Demo.fit", "Demo.qualified"]);
    for (_, anchor) in &embedded {
        let record = resolved
            .records()
            .iter()
            .find(|record| {
                record.document
                    == (DocumentIdentity::Page {
                        page: api.0.clone(),
                    })
            })
            .unwrap();
        assert!(record.anchors.contains(*anchor));
    }
    let guide = resolved
        .records()
        .iter()
        .find(|record| {
            record
                .references
                .iter()
                .any(|r| r.spelling == "@ref \"API Reference\"")
        })
        .unwrap();
    assert!(guide.references.iter().any(|reference| matches!(&reference.target, ReferenceTarget::Page { fragment: Some(id), .. } if id == "api-reference")));
    assert!(
        guide
            .references
            .iter()
            .any(|reference| matches!(&reference.target, ReferenceTarget::Item { .. }))
    );
    let snapshot = Snapshot::from_sources(&sources, &resolved).unwrap();
    let artifact = tempfile::tempdir().unwrap();
    let original = artifact.path().join("original.sqlite");
    snapshot.publish(&original).unwrap();
    let detached = artifact.path().join("relocated.sqlite");
    std::fs::rename(original, &detached).unwrap();
    drop(snapshot);
    drop(sources);
    drop(root);

    let snapshot = Snapshot::load(detached).unwrap();
    let site = Site::new(&snapshot).unwrap();
    let fragments = render_fragments(&site).unwrap();
    let standalone = render_site(&site).unwrap();
    let index: FragmentIndex =
        serde_json::from_slice(&fragments.files()["documents.json"].bytes).unwrap();
    for (path, file) in fragments.files() {
        if path.ends_with(".html") || path.ends_with(".md") {
            let body = std::str::from_utf8(&file.bytes).unwrap();
            assert!(!body.contains("@ref"), "{path}: {body}");
            assert!(!body.contains("@autodocs"), "{path}: {body}");
            assert!(!body.contains("@index"), "{path}: {body}");
        }
    }
    let api = index
        .documents
        .iter()
        .find(|document| document.title == "API Reference")
        .unwrap();
    let html = std::str::from_utf8(&fragments.files()[&api.path].bytes).unwrap();
    let markdown = std::str::from_utf8(&fragments.files()[&api.markdown_path].bytes).unwrap();
    let full = std::str::from_utf8(&standalone.files()[&api.path].bytes).unwrap();
    assert!(html.contains("id=\"api-reference\""));
    assert!(markdown.contains("id=\"api-reference\""));
    for text in [
        "A shape with a retained field.",
        "Fit integer inputs with precise areas.",
        "A public qualified helper.",
    ] {
        assert!(html.contains(text), "{html}");
        assert!(markdown.contains(text), "{markdown}");
        assert!(full.contains(text));
    }
    for text in [
        "Private implementation details.",
        "Documentation from a nested module.",
    ] {
        assert!(!html.contains(text));
        assert!(!markdown.contains(text));
    }
    let anchors: Vec<_> = snapshot
        .workspace()
        .pages
        .values()
        .find(|page| page.title == "API Reference")
        .unwrap()
        .document
        .document
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::ApiDocumentation { identifier, .. } => Some(identifier),
            _ => None,
        })
        .collect();
    for anchor in anchors {
        assert!(html.contains(&format!("id=\"{anchor}\"")));
        assert!(html.contains(&format!("#{anchor}\"")));
        assert!(markdown.contains(&format!("id=\"{anchor}\"")));
        assert!(markdown.contains(&format!("#{anchor}>)")));
    }
    let urls = UrlMap {
        format_version: 1,
        documents: index
            .documents
            .iter()
            .enumerate()
            .map(|(i, document)| (document.path.clone(), format!("/manual/{i}/")))
            .collect(),
        assets: Default::default(),
    };
    let mapped = render_fragments(
        &Site::new(&snapshot)
            .unwrap()
            .with_url_map(urls.clone())
            .unwrap(),
    )
    .unwrap();
    let home = index
        .documents
        .iter()
        .find(|document| document.path == "packages/julia/index.html")
        .unwrap();
    let html = std::str::from_utf8(&mapped.files()[&home.path].bytes).unwrap();
    let markdown = std::str::from_utf8(&mapped.files()[&home.markdown_path].bytes).unwrap();
    let target = format!("{}#api-reference", urls.documents[&api.path]);
    assert!(html.contains(&target), "{html}");
    assert!(markdown.contains(&target), "{markdown}");
    assert!(!html.contains("]("));
    let fit = index
        .documents
        .iter()
        .find(|document| document.qualified_name.as_deref() == Some("Demo.fit"))
        .unwrap();
    let api_html = std::str::from_utf8(&mapped.files()[&api.path].bytes).unwrap();
    let api_md = std::str::from_utf8(&mapped.files()[&api.markdown_path].bytes).unwrap();
    let target = format!("{}#details", urls.documents[&fit.path]);
    assert!(api_html.contains(&target), "{api_html}");
    assert!(api_md.contains(&target), "{api_md}");
}

#[test]
fn documenter_directive_errors_are_attributed_and_never_evaluated() {
    for body in [
        "Private = false",
        "Modules = [Missing]",
        "Modules = [Demo]\nModules = [Demo]",
        "Modules = [Demo]\nPrivate = maybe()",
        "Modules = getmodules()",
        "Modules = [Demo]\nFilter = x -> true",
        "Modules += [Demo]",
    ] {
        let root = support::TestWorkspace::from_fixture("julia-documenter");
        root.write(
            "docs/api.md",
            format!("# API Reference\n\n```@autodocs\n{body}\n```\n"),
        );
        let error = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap_err();
        let diplodocus::assembly::AssemblyError::Diagnostics(diagnostics) = error else {
            panic!("{error}")
        };
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code
                == DiagnosticCode::UnsupportedAuthoredSyntax
                && diagnostic.span.is_some()
                && diagnostic.source.is_some()),
            "{diagnostics:?}"
        );
    }
}

#[test]
fn documenter_public_and_module_filters_are_explicit() {
    for (options, expected) in [
        ("Modules = [Demo]\nPublic = false", vec!["Demo.internal"]),
        (
            "Modules = [Demo.Nested]\nPrivate = false",
            vec!["Demo.Nested.nested"],
        ),
        ("Modules = [Demo]\nPublic = false\nPrivate = false", vec![]),
    ] {
        let root = support::TestWorkspace::from_fixture("julia-documenter");
        root.write(
            "docs/api.md",
            format!("# API Reference\n\n```@autodocs\n{options}\n```\n"),
        );
        let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
        let names: Vec<_> = sources
            .workspace()
            .pages
            .values()
            .find(|page| page.title == "API Reference")
            .unwrap()
            .document
            .document
            .blocks
            .iter()
            .filter_map(|block| {
                if let Block::ApiDocumentation { item, .. } = block {
                    Some(
                        sources.workspace().packages[&item.package].items[&item.item]
                            .qualified_name
                            .as_str(),
                    )
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(names, expected);
    }
}

#[test]
fn documenter_requires_native_execution_settings_and_unique_heading_references() {
    let root = support::TestWorkspace::from_fixture("julia-documenter");
    root.write(
        "diplodocus.yaml",
        root.read("diplodocus.yaml") + "    execution:\n      mode: execute\n",
    );
    assert!(
        assemble_workspace(root.path().join("diplodocus.yaml"))
            .unwrap_err()
            .to_string()
            .contains("engine: julia")
    );
    let root = support::TestWorkspace::from_fixture("julia-documenter");
    root.write("docs/duplicate.md", "# API Reference\n");
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    assert!(resolve_workspace(&sources).is_err());
}
