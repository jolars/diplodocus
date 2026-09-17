use diplodocus::extractors::python::docstrings;
use diplodocus::ir;
use ir::{Block, Inline, SourceLocation, SourceSpan};

mod support;

use docstrings::{DocstringParse, DocstringSourceSegment, parse_docstring};

fn span(start: usize, end: usize) -> SourceSpan {
    SourceSpan { start, end }
}

fn location(end: usize) -> SourceLocation {
    SourceLocation {
        repository: "python".into(),
        path: "src/demo.py".try_into().unwrap(),
        span: Some(span(100, end)),
    }
}

fn parse(text: &str) -> DocstringParse {
    parse_docstring(
        text,
        location(106 + text.len()),
        Some(&[DocstringSourceSegment {
            decoded: span(0, text.len()),
            source: span(103, 103 + text.len()),
        }]),
    )
}

fn golden(parsed: &DocstringParse, name: &str) {
    let mut parsed = parsed.clone();
    for provenance in &mut parsed.document.provenance {
        support::normalize_build_tool_versions(&mut provenance.tools, &["diplodocus"]);
    }
    support::assert_json_golden(&parsed, format!("python-docstrings/{name}.json"));
}

fn code_count(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .map(|block| match block {
            Block::CodeCell(_) => panic!("docstrings must never create executable cells"),
            Block::CodeBlock { .. } => 1,
            Block::BlockQuote { blocks, .. } | Block::Callout { blocks, .. } => code_count(blocks),
            Block::List { items, .. } => items.iter().map(|item| code_count(&item.blocks)).sum(),
            _ => 0,
        })
        .sum()
}

#[test]
fn pep257_prose_and_empty_docs() {
    let parsed = parse("Résumé.");
    assert!(parsed.diagnostics.is_empty());
    assert_eq!(parsed.document.document.span, span(0, 9));
    assert_eq!(
        parsed.document.document.blocks,
        vec![Block::Paragraph {
            inlines: vec![Inline::Text {
                value: "Résumé.".into(),
                span: span(0, 9)
            }],
            span: span(0, 9),
        }]
    );
    golden(&parsed, "prose");
    for text in ["", " \n\t\n"] {
        let parsed = parse(text);
        assert!(parsed.document.document.blocks.is_empty());
        assert!(parsed.diagnostics.is_empty());
        assert_eq!(parsed.document.raw_source.as_deref(), Some(text));
    }
}

#[test]
fn function_roles_in_acceptance_docstrings_become_semantic_references() {
    let text = "Default convergence tolerance used by :func:`fit`.";
    let parsed = parse(text);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let Block::Paragraph { inlines, .. } = &parsed.document.document.blocks[0] else {
        panic!("paragraph");
    };
    assert!(inlines.iter().any(
        |inline| matches!(inline, Inline::SemanticReference { target, .. } if target == "fit")
    ));
}

#[test]
fn empty_docstrings_only_record_the_parser_that_ran() {
    let parsed = parse("");
    assert!(
        parsed.document.provenance[0]
            .tools
            .contains_key("pydocstring")
    );
    assert!(
        !parsed.document.provenance[0]
            .tools
            .contains_key("panache-parser")
    );
}

const SECTIONS: &str = "Compute a **score**.\n\nExtended *prose* and ``code``.\n\nParameters\n----------\nx, y : float, optional, default=1\n    Input values.\n\nReturns\n-------\nscore : float\n    The score.\n\nRaises\n------\nValueError\n    Invalid input.\n\nNotes\n-----\nKeep the scale.\n\nReferences\n----------\n.. [1] A. Author, Study.\n\nExamples\n--------\n>>> score(1, 2)\n3\n";

#[test]
fn required_sections_preserve_entries_and_markup() {
    let parsed = parse(SECTIONS);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let headings: Vec<_> = parsed
        .document
        .document
        .blocks
        .iter()
        .filter_map(|block| {
            if let Block::Heading { inlines, .. } = block
                && let [Inline::Text { value, .. }] = inlines.as_slice()
            {
                return Some(value.as_str());
            }
            None
        })
        .collect();
    assert_eq!(
        headings,
        [
            "Parameters",
            "Returns",
            "Raises",
            "Notes",
            "References",
            "Examples"
        ]
    );
    let entries: Vec<_> = parsed
        .document
        .document
        .blocks
        .iter()
        .filter_map(|block| {
            if let Block::List { items, .. } = block {
                Some(items.len())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(entries, [1, 1, 1, 1]);
    assert_eq!(code_count(&parsed.document.document.blocks), 1);
    golden(&parsed, "sections");
}

#[test]
fn incomplete_syntax_keeps_text_and_original_diagnostic_range() {
    let text = "Résumé.\n\nParameters\n----------\nvalue :\n";
    let parsed = parse(text);
    assert!(
        parsed
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "python-incomplete-docstring")
    );
    assert!(parsed.diagnostics.iter().all(|d| {
        d.span
            .is_some_and(|s| s.start >= 103 && s.end <= 103 + text.len())
    }));
    assert_eq!(parsed.document.raw_source.as_deref(), Some(text));
    golden(&parsed, "incomplete");
}

#[test]
fn escapes_and_missing_maps_use_enclosing_literal() {
    let text = "first\nsecond";
    let source = location(119);
    let parsed = parse_docstring(text, source.clone(), None);
    assert_eq!(parsed.document.source_location, Some(source.clone()));
    assert_eq!(parsed.document.document.span, span(0, text.len()));
    assert_eq!(parsed.diagnostics.len(), 1);
    assert_eq!(
        parsed.diagnostics[0].code.as_str(),
        "python-docstring-source-attribution"
    );
    assert_eq!(parsed.diagnostics[0].span, source.span);
    assert!(parsed.source_map.is_empty());
}

#[test]
fn concatenated_source_maps_preserve_gaps_without_guessing() {
    let text = "Résumé.\n\nParameters\n----------\nvalue :\n";
    let segments = vec![
        DocstringSourceSegment {
            decoded: span(0, 9),
            source: span(103, 112),
        },
        DocstringSourceSegment {
            decoded: span(9, text.len()),
            source: span(119, 119 + text.len() - 9),
        },
    ];
    let source = location(119 + text.len());
    let parsed = parse_docstring(text, source, Some(&segments));
    assert_eq!(parsed.source_map, segments);
    assert!(
        !parsed
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "python-docstring-source-attribution")
    );
    assert!(
        parsed
            .diagnostics
            .iter()
            .all(|d| d.span.unwrap().start >= 119)
    );
}

#[test]
fn invalid_or_partial_maps_are_diagnosed_without_panics() {
    let text = "Résumé.";
    for segments in [
        vec![DocstringSourceSegment {
            decoded: span(0, 99),
            source: span(103, 202),
        }],
        vec![DocstringSourceSegment {
            decoded: span(2, 9),
            source: span(103, 110),
        }],
        vec![DocstringSourceSegment {
            decoded: span(0, 9),
            source: span(103, 104),
        }],
        vec![DocstringSourceSegment {
            decoded: span(0, 1),
            source: span(103, 104),
        }],
    ] {
        let parsed = parse_docstring(text, location(115), Some(&segments));
        assert!(
            parsed
                .diagnostics
                .iter()
                .any(|d| d.code.as_str() == "python-docstring-source-attribution")
        );
    }
}

#[test]
fn unsupported_sections_directives_and_html_are_visible() {
    let text = "Summary <script>alert(1)</script>.\n\n.. deprecated:: 1.0\n    Old API.\n\nMystery\n-------\nKeep me.\n";
    let parsed = parse(text);
    assert!(
        parsed
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "python-unsupported-docstring")
    );
    let encoded = serde_json::to_string(&parsed).unwrap();
    assert!(encoded.contains("unsupported"));
    assert!(encoded.contains("Keep me."));
    assert!(encoded.contains("Old API."));
    golden(&parsed, "unsupported");
}

#[test]
fn examples_are_display_only_and_preserve_code_indentation() {
    let text = "Summary.\n\nExamples\n--------\n    >>> if True:\n    ...     print('never run')\n    never run\n\n    ```{python}\n    #| eval: true\n    raise RuntimeError('never run')\n    ```\n";
    let parsed = parse(text);
    assert_eq!(code_count(&parsed.document.document.blocks), 2);
    let code: String = parsed
        .document
        .document
        .blocks
        .iter()
        .filter_map(|block| {
            if let Block::CodeBlock {
                source,
                source_segments,
                ..
            } = block
            {
                assert_eq!(
                    *source,
                    source_segments
                        .iter()
                        .map(|s| s.text.as_str())
                        .collect::<String>()
                );
                Some(source.as_str())
            } else {
                None
            }
        })
        .collect();
    assert!(code.contains("...     print('never run')"));
    assert!(code.contains("#| eval: true"));
    golden(&parsed, "examples");
}

#[test]
fn examples_keep_explanatory_prose_outside_display_code() {
    let text = "Summary.\n\nExamples\n--------\nA **small** example.\n\n>>> f()\n1\n\nMore explanation.\n\n```{python}\n#| eval: true\n\nraise RuntimeError('inert')\n```\n";
    let parsed = parse(text);
    let kinds: Vec<_> = parsed
        .document
        .document
        .blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph { .. } => "prose",
            Block::Heading { .. } => "heading",
            Block::CodeBlock { .. } => "code",
            _ => "other",
        })
        .collect();
    assert_eq!(
        kinds,
        ["prose", "heading", "prose", "code", "prose", "code"]
    );
    assert!(parsed.diagnostics.is_empty());
    golden(&parsed, "mixed-examples");
}

#[test]
fn unsupported_rst_roles_and_directives_keep_their_original_spelling() {
    let text = "Use :class:`Thing` here.\n\nNotes\n-----\n.. math:: x + y\n";
    let parsed = parse(text);
    assert!(
        parsed
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "python-unsupported-docstring")
    );
    let Block::Paragraph { inlines, .. } = &parsed.document.document.blocks[0] else {
        panic!("prose")
    };
    assert!(inlines.iter().any(
        |inline| matches!(inline, Inline::Unsupported { raw, .. } if raw.contains(":class:`Thing`"))
    ));
}

#[test]
fn code_quoting_rst_syntax_is_not_interpreted() {
    let parsed = parse("Use ``:class:`Thing` `` as literal code.");
    assert!(parsed.diagnostics.is_empty());
    let Block::Paragraph { inlines, .. } = &parsed.document.document.blocks[0] else {
        panic!("prose")
    };
    assert!(inlines.iter().any(
        |inline| matches!(inline, Inline::Code { value, .. } if value.contains(":class:`Thing`"))
    ));
}

#[test]
fn file_only_and_partial_attribution_stay_explicit() {
    let text = "Résumé.\n\nParameters\n----------\nvalue :\n";
    let mut source = location(100 + text.len());
    source.span = None;
    let parsed = parse_docstring(text, source, None);
    assert_eq!(parsed.diagnostics.len(), 2);
    assert!(
        parsed
            .diagnostics
            .iter()
            .all(|d| d.span.is_none() && d.source.is_some())
    );
    golden(&parsed, "unmapped");

    let segments = [DocstringSourceSegment {
        decoded: span(0, 9),
        source: span(103, 112),
    }];
    let source = location(100 + text.len());
    let parsed = parse_docstring(text, source.clone(), Some(&segments));
    assert_eq!(parsed.source_map, segments);
    assert!(parsed.diagnostics.iter().all(|d| d.span == source.span));
    for text in ["", " \n"] {
        assert!(
            parse_docstring(text, source.clone(), None)
                .diagnostics
                .is_empty()
        );
    }
}

#[test]
fn diagnostic_crossing_a_concatenation_gap_uses_the_enclosure() {
    let text = "Summary.\n\nUnknown\n-------\nBody.";
    let segments = [
        DocstringSourceSegment {
            decoded: span(0, 14),
            source: span(103, 117),
        },
        DocstringSourceSegment {
            decoded: span(14, text.len()),
            source: span(122, 122 + text.len() - 14),
        },
    ];
    let source = location(122 + text.len());
    let parsed = parse_docstring(text, source.clone(), Some(&segments));
    assert_eq!(parsed.source_map, segments);
    assert_eq!(parsed.diagnostics.len(), 2);
    assert!(parsed.diagnostics.iter().all(|d| d.span == source.span));
}

#[test]
fn prose_paragraphs_and_byte_offsets_survive_crlf_and_indentation() {
    let text = "Résumé.\r\n\r\n    First paragraph.\r\n    With a [link](https://example.com).\r\n\r\n    Second paragraph.\r\n";
    let parsed = parse(text);
    assert!(parsed.diagnostics.is_empty());
    assert_eq!(parsed.document.document.blocks.len(), 3);
    fn check(value: &serde_json::Value, text: &str) {
        match value {
            serde_json::Value::Object(object) => {
                if let Some(span) = object.get("span") {
                    let start = span["start"].as_u64().unwrap() as usize;
                    let end = span["end"].as_u64().unwrap() as usize;
                    assert!(start <= end && end <= text.len());
                    assert!(text.is_char_boundary(start) && text.is_char_boundary(end));
                }
                for child in object.values() {
                    check(child, text);
                }
            }
            serde_json::Value::Array(array) => {
                for child in array {
                    check(child, text);
                }
            }
            _ => {}
        }
    }
    check(
        &serde_json::to_value(&parsed.document.document).unwrap(),
        text,
    );
    assert_eq!(
        serde_json::to_string(&parsed).unwrap(),
        serde_json::to_string(&parse(text)).unwrap()
    );
    golden(&parsed, "paragraphs");
}
