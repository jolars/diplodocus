mod support;

use diplodocus::{
    assembly::assemble_workspace,
    diagnostics::DiagnosticCode,
    documents::{AuthoredFormat, parse_authored_document},
    ir::{Block, Inline, ItemReference, Workspace},
    rendering::{FragmentIndex, render_fragments, render_site},
    site::{Site, UrlMap},
    snapshots::Snapshot,
    validation::{DocumentIdentity, ReferenceTarget, resolve_workspace},
};

fn api_entries(workspace: &Workspace) -> Vec<(&ItemReference, &[ItemReference], &str)> {
    workspace
        .pages
        .values()
        .find(|page| page.title == "API Reference")
        .unwrap()
        .document
        .document
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::ApiDocumentation {
                item,
                selection,
                identifier,
                ..
            } => Some((item, selection.as_slice(), identifier.as_str())),
            _ => None,
        })
        .collect()
}

#[test]
fn documenter_preserves_a_grouped_api_and_file_selection_in_detached_output() {
    let root = support::TestWorkspace::from_fixture("julia-documenter-grouped");
    root.write("docs/catalog.md", "# Catalog\n\n```@index\n```\n");
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    assert!(
        sources.workspace().diagnostics.is_empty(),
        "{:?}",
        sources.workspace().diagnostics
    );
    let entries = api_entries(sources.workspace());
    let names: Vec<_> = entries
        .iter()
        .map(|(reference, _, _)| {
            sources.workspace().packages[&reference.package].items[&reference.item]
                .qualified_name
                .as_str()
        })
        .collect();
    assert_eq!(
        names,
        [
            "SLOPE",
            "SLOPE.slope",
            "SLOPE.SlopeFit",
            "SLOPE.predict",
            "SLOPE.coef",
            "SLOPE.slopecv",
            "SLOPE.best_α",
            "SLOPE.best_model",
            "SLOPE.refit",
            "SLOPE.SlopeGridResult",
            "SLOPE.SlopeCvResult",
            "SLOPE.plot_cv",
            "SLOPE.plot_path",
            "SLOPE.regweights",
            "SLOPE._normalize",
        ]
    );
    let (_, selection, _) = entries
        .iter()
        .find(|(reference, _, _)| {
            sources.workspace().packages[&reference.package].items[&reference.item].qualified_name
                == "SLOPE.plot_path"
        })
        .unwrap();
    assert_eq!(selection.len(), 1);
    let selected = &sources.workspace().packages["slope"].items[&selection[0].item];
    assert_eq!(
        selected
            .documentation
            .as_ref()
            .unwrap()
            .source_location
            .as_ref()
            .unwrap()
            .path
            .as_str(),
        "src/plots.jl"
    );
    let anchors: Vec<_> = entries
        .iter()
        .map(|(_, _, anchor)| anchor.to_string())
        .collect();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let artifact = tempfile::tempdir().unwrap();
    let original = artifact.path().join("original.sqlite");
    snapshot.publish(&original).unwrap();
    let detached = artifact.path().join("relocated.sqlite");
    std::fs::rename(original, &detached).unwrap();
    drop((snapshot, sources, root));

    let snapshot = Snapshot::load(detached).unwrap();
    let site = Site::new(&snapshot).unwrap();
    let fragments = render_fragments(&site).unwrap();
    let standalone = render_site(&site).unwrap();
    let index: FragmentIndex =
        serde_json::from_slice(&fragments.files()["documents.json"].bytes).unwrap();
    let api = index
        .documents
        .iter()
        .find(|document| document.title == "API Reference")
        .unwrap();
    for anchor in &anchors {
        assert!(
            api.headings
                .iter()
                .any(|(level, id, _)| *level == 3 && id == anchor)
        );
    }
    let markdown = std::str::from_utf8(&fragments.files()[&api.markdown_path].bytes).unwrap();
    assert_eq!(markdown.matches("regweights(x)").count(), 1);
    let bodies = [
        std::str::from_utf8(&fragments.files()[&api.path].bytes).unwrap(),
        std::str::from_utf8(&fragments.files()[&api.markdown_path].bytes).unwrap(),
        std::str::from_utf8(&standalone.files()[&api.path].bytes).unwrap(),
    ];
    for body in bodies {
        for directive in ["@docs", "@autodocs", "@index", "@ref"] {
            assert!(!body.contains(directive), "{body}");
        }
        for text in [
            "Fit integer inputs.",
            "Plot floating-point coefficient paths.",
            "An explicitly selected private utility.",
            "addressable",
        ] {
            assert!(body.contains(text), "{body}");
        }
        for text in [
            "Plot support documented outside the plotting source.",
            "A private helper that the authored page does not select.",
        ] {
            assert!(!body.contains(text), "{body}");
        }
        for anchor in &anchors {
            assert!(body.contains(&format!("id=\"{anchor}\"")), "{body}");
        }
        let mut previous = 0;
        // Use the content section to exclude the sidebar's repeated labels.
        let body = if let Some(start) = body.find("id=\"api-reference\"") {
            &body[start..]
        } else {
            body
        };
        for section in [
            "Modules",
            "Model Fitting",
            "Cross-Validation",
            "Plotting",
            "Utilities",
        ] {
            let position = body[previous..].find(section).unwrap();
            previous += position + section.len();
        }
    }
    let catalog = index
        .documents
        .iter()
        .find(|document| document.title == "Catalog")
        .unwrap();
    for path in [&catalog.path, &catalog.markdown_path] {
        let body = std::str::from_utf8(&fragments.files()[path].bytes).unwrap();
        for anchor in &anchors {
            assert!(body.contains(&format!("#{anchor}")), "{body}");
        }
    }
    let urls = UrlMap {
        format_version: 1,
        documents: index
            .documents
            .iter()
            .enumerate()
            .map(|(i, document)| (document.path.clone(), format!("/host/{i}/")))
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
    for path in [&catalog.path, &catalog.markdown_path] {
        let body = std::str::from_utf8(&mapped.files()[path].bytes).unwrap();
        for anchor in &anchors {
            assert!(
                body.contains(&format!("{}#{anchor}", urls.documents[&api.path])),
                "{body}"
            );
        }
    }
}

#[test]
fn documenter_docs_resolves_aliases_private_bindings_and_exact_methods_in_order() {
    let root = support::TestWorkspace::from_fixture("julia-documenter");
    root.write(
        "src/Demo.jl",
        root.read("src/Demo.jl")
            .replace("module Nested", "const fitting = fit\n\nmodule Nested"),
    );
    root.write("docs/api.md", "# API Reference\n\n```@docs\n# Explicit selection ignores public discovery defaults.\nDemo.internal\nDemo.fitting(::Int)\nDemo.Shape.area\nDemo.qualified\n```\n");
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let entries = api_entries(sources.workspace());
    let names: Vec<_> = entries
        .iter()
        .map(|(reference, _, _)| {
            sources.workspace().packages[&reference.package].items[&reference.item]
                .qualified_name
                .as_str()
        })
        .collect();
    assert_eq!(
        names,
        [
            "Demo.internal",
            "Demo.fit",
            "Demo.Shape.area",
            "Demo.qualified"
        ]
    );
    assert_eq!(entries[1].1, std::slice::from_ref(entries[1].0));
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let fragments = render_fragments(&Site::new(&snapshot).unwrap()).unwrap();
    for path in ["packages/julia/api.html", "packages/julia/api.md"] {
        let body = std::str::from_utf8(&fragments.files()[path].bytes).unwrap();
        assert!(
            body.contains("Fit integer inputs with precise areas."),
            "{body}"
        );
        assert!(!body.contains("Fit a diagram with"), "{body}");
        assert!(body.contains("Private implementation details."), "{body}");
    }
}

#[test]
fn documenter_pages_uses_source_suffixes_and_declared_page_order() {
    for (pages, expected) in [
        (
            "[\"./sub/../more_plots.jl\", \"plots.jl\"]",
            vec!["Demo.zebra", "Demo.alpha"],
        ),
        (
            "[\"plots.jl\", \"more_plots.jl\"]",
            vec!["Demo.alpha", "Demo.zebra"],
        ),
        ("[\"src/plots.jl\"]", vec!["Demo.alpha"]),
        ("[\"missing.jl\"]", vec![]),
        (
            "[]",
            vec![
                "Demo.Shape",
                "Demo.alpha",
                "Demo.fit",
                "Demo.qualified",
                "Demo.zebra",
            ],
        ),
    ] {
        let root = support::TestWorkspace::from_fixture("julia-documenter");
        root.write(
            "src/Demo.jl",
            root.read("src/Demo.jl").replace(
                "module Nested",
                "include(\"plots.jl\")\ninclude(\"more_plots.jl\")\n\nmodule Nested",
            ),
        );
        root.write(
            "src/plots.jl",
            "export alpha\n\"The first source file.\"\nalpha(x) = x\n",
        );
        root.write(
            "src/more_plots.jl",
            "export zebra\n\"The second source file.\"\nzebra(x) = x\n",
        );
        root.write("docs/api.md", format!("# API Reference\n\n```@autodocs\nModules = [Demo]\nPages = {pages}\nPrivate = false\n```\n"));
        let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
        let names: Vec<_> = api_entries(sources.workspace())
            .iter()
            .map(|(reference, _, _)| {
                sources.workspace().packages[&reference.package].items[&reference.item]
                    .qualified_name
                    .as_str()
            })
            .collect();
        assert_eq!(names, expected, "{pages}");
    }
}

#[test]
fn documenter_docs_rejects_unresolved_dynamic_and_duplicate_selections() {
    for blocks in [
        "```@docs\nDemo.missing\n```",
        "```@docs\nDemo\n```",
        "```@docs\nDemo.fit(::Missing)\n```",
        "```@docs\nDemo.Shape.area(::Int)\n```",
        "```@docs\n```",
        "```@docs\nDemo.fit = error(\"never evaluated\")\n```",
        "```@docs\ngetfield(Demo, :fit)\n```",
        "```@docs\nDemo.fit(\n```",
        "```@docs\nDemo.fit\nDemo.fit\n```",
        "```@docs\nDemo.fit\n```\n\n```@docs\nDemo.fit(::Int)\n```",
        "```@docs\nDemo.fit\n```\n\n```@autodocs\nModules = [Demo]\nPrivate = false\n```",
    ] {
        let root = support::TestWorkspace::from_fixture("julia-documenter");
        let source = format!("# API Reference\n\n{blocks}\n");
        root.write("docs/api.md", &source);
        let error = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap_err();
        let diplodocus::assembly::AssemblyError::Diagnostics(diagnostics) = error else {
            panic!("{error}")
        };
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code
                == DiagnosticCode::UnsupportedAuthoredSyntax
                && diagnostic.source.is_some()
                && diagnostic
                    .span
                    .is_some_and(|span| source[span.start..span.end].starts_with("```@"))),
            "{blocks}: {diagnostics:?}"
        );
    }
}

#[test]
fn documenter_diagnoses_unsupported_directive_options() {
    for source in [
        "```@docs; canonical=false\nDemo.fit\n```",
        "```@docs name\nDemo.fit\n```",
        "```@autodocs; canonical=false\nModules = [Demo]\n```",
    ] {
        let parsed = parse_authored_document(source, AuthoredFormat::Documenter);
        assert!(
            parsed.diagnostics.iter().any(|diagnostic| diagnostic.code
                == DiagnosticCode::UnsupportedAuthoredSyntax
                && diagnostic.span.is_some()),
            "{parsed:?}"
        );
        assert!(
            matches!(&parsed.document.blocks[0], Block::Unsupported { raw, .. } if raw.contains(source))
        );
    }
}

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
        "Modules = [Demo]\nPages = getpages()",
        "Modules = [Demo]\nPages = [plots.jl]",
        "Modules = [Demo]\nPages = [\"$(error(\"never evaluated\"))\"]",
        "Modules = [Demo]\nPages = [raw\"plots.jl\"]",
        "Modules = [Demo]\nPages = [\"plots.jl\"]\nPages = []",
        "Modules = [Demo]\nPages = [\"plots.jl\", \"./plots.jl\"]",
        "Modules = [Demo]\nPages = [\"\"]",
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
