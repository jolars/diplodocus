mod support;

use diplodocus::{
    assembly::{AssemblyError, assemble_workspace},
    diagnostics::DiagnosticCode,
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
fn executable_includes_fail_explicitly_before_kernel_startup() {
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
    let Err(AssemblyError::Diagnostics(diagnostics)) =
        assemble_workspace(root.path().join("diplodocus.yaml"))
    else {
        panic!("expected explicit execution limitation");
    };
    assert_eq!(diagnostics[0].code, DiagnosticCode::InvalidMarkdownInclude);
    assert!(
        diagnostics[0]
            .message
            .contains("Executable QMD pages with includes")
    );
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
