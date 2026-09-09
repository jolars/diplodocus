use arity_parser::ast::{AssignmentExpr, AstNode, FunctionExpr};
use arity_parser::dcf::{self, VersionOp, dependency_entries};
use arity_parser::namespace::{self, DirectiveKind};
use arity_parser::parser;
use arity_parser::syntax::SyntaxNode;
use rd_ast::{
    RdDynamicMarkupEvent, RdDynamicMarkupState, RdInlineSpanKind, RdNode, RdPath, RdPathSegment,
    RdSexprResults, RdSexprStage, RdShapeErrorKind, RdTag, text_contents,
};
use rd_source::{DiagnosticCode, Severity};

mod support;

const R_FIXTURE: &str = "acceptance/r";

macro_rules! range_text {
    ($source:expr, $range:expr $(,)?) => {{
        let range = $range;
        &$source[usize::from(range.start())..usize::from(range.end())]
    }};
}

#[test]
fn r_exploratory_output_matches_golden() {
    use serde_json::json;

    let description_source = support::load_fixture(format!("{R_FIXTURE}/DESCRIPTION"));
    let description = dcf::parse(&description_source);
    assert!(description.diagnostics.is_empty());
    let fields = [
        "Package",
        "Type",
        "Title",
        "Version",
        "Authors@R",
        "Author",
        "Maintainer",
        "Description",
        "License",
        "Encoding",
        "Depends",
        "Imports",
        "Suggests",
        "Config/testthat/edition",
        "NeedsCompilation",
    ]
    .into_iter()
    .map(|name| {
        let field = description.document().field(name).unwrap();
        json!({"name": name, "value": field.folded_value()})
    })
    .collect::<Vec<_>>();
    let dependencies = ["Depends", "Imports", "Suggests"]
        .into_iter()
        .map(|name| {
            let entries = dependency_entries(&description.document().field(name).unwrap());
            json!({"field": name, "entries": entries.iter().map(|entry| json!({
            "name": entry.name.as_str(),
            "malformed": entry.malformed_constraint(),
            "constraints": entry.constraints.iter().map(|constraint| json!({
                "operator": format!("{:?}", constraint.op), "version": constraint.version.as_str(),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>()})
        })
        .collect::<Vec<_>>();

    let namespace_source = support::load_fixture(format!("{R_FIXTURE}/NAMESPACE"));
    let namespace = namespace::parse(&namespace_source);
    assert!(namespace.diagnostics.is_empty());
    let directives = namespace.document().directives().map(|directive| {
        let range = directive.text_range();
        json!({
            "name": directive.name(), "range": [u32::from(range.start()), u32::from(range.end())],
            "source": range_text!(&namespace_source, range),
            "arguments": directive.arguments().map(|argument| {
                range_text!(&namespace_source, argument.value_range().unwrap()).to_owned()
            }).collect::<Vec<_>>(),
        })
    }).collect::<Vec<_>>();

    let sources = support::fixture_files(format!("{R_FIXTURE}/R")).into_iter().map(|path| {
        let source = support::load_fixture(format!("{R_FIXTURE}/R/{}", path.display()));
        let parsed = parser::parse(&source);
        assert!(parsed.diagnostics.is_empty());
        let functions = parsed.cst.descendants().filter_map(AssignmentExpr::cast).filter_map(|assignment| {
            let name = assignment.target_name()?;
            let function = FunctionExpr::cast(assignment.value_element()?.into_node()?)?;
            let range = assignment.syntax().text_range();
            Some(json!({
                "name": name, "range": [u32::from(range.start()), u32::from(range.end())],
                "source": range_text!(&source, range),
                "formals": function.formals().iter().map(|formal| {
                    let range = formal.text_range();
                    json!({
                        "name": formal.name(), "source": range_text!(&source, range),
                        "range": [u32::from(range.start()), u32::from(range.end())],
                        "default": formal.default_range().map(|range| range_text!(&source, range)),
                    })
                }).collect::<Vec<_>>(),
            }))
        }).collect::<Vec<_>>();
        json!({"path": format!("R/{}", path.display()), "functions": functions})
    }).collect::<Vec<_>>();

    let dynamic_workspace = support::acceptance_case("unsupported-rd");
    let topics = support::fixture_files(format!("{R_FIXTURE}/man")).into_iter().map(|path| {
        let parsed = if path == std::path::Path::new("experimental_summary.Rd") {
            rd_source::parse(dynamic_workspace.read("r/man/experimental_summary.Rd").as_bytes()).unwrap()
        } else {
            parse_rd_fixture(path.to_str().unwrap())
        };
        assert!(parsed.diagnostics().is_empty());
        let document = parsed.document();
        let dynamic = document.inspect_dynamic_markup().map(|event| {
            let RdDynamicMarkupEvent::Sexpr(sexpr) = event.unwrap() else { panic!("unexpected dynamic markup") };
            json!({
                "code": sexpr.view().code(),
                "stage": format!("{:?}", sexpr.effective_options().stage),
                "results": format!("{:?}", sexpr.effective_options().results),
                "unresolved": matches!(sexpr.state(), RdDynamicMarkupState::Unresolved { .. }),
            })
        }).collect::<Vec<_>>();
        json!({
            "path": format!("man/{}", path.display()),
            "range": null,
            "name": text_contents(document.inspect_name().unwrap().unwrap()).trim(),
            "aliases": document.aliases().collect::<Vec<_>>(),
            "keywords": document.keywords().collect::<Vec<_>>(),
            "nodes": rd_node_observations(document.nodes()),
            "dynamic": dynamic,
        })
    }).collect::<Vec<_>>();

    support::assert_json_golden(
        &json!({
            "schema": "r-spike-observation-v1", "mode": "static",
            "description": {"path": "DESCRIPTION", "fields": fields, "dependencies": dependencies},
            "namespace": {"path": "NAMESPACE", "directives": directives},
            "sources": sources, "topics": topics,
        }),
        "spikes/r.json",
    );
}

fn rd_node_observations(nodes: &[RdNode]) -> Vec<serde_json::Value> {
    use serde_json::json;

    nodes
        .iter()
        .map(|node| match node {
            RdNode::Text(text) => json!({"kind": "text", "text": text}),
            RdNode::RCode(text) => json!({"kind": "r-code", "text": text}),
            RdNode::Verb(text) => json!({"kind": "verbatim", "text": text}),
            RdNode::Comment(text) => json!({"kind": "comment", "text": text}),
            RdNode::Group(group) => {
                json!({"kind": "group", "children": rd_node_observations(group.children())})
            }
            RdNode::Tagged(tagged) => json!({
                "kind": "markup", "tag": tagged.tag().as_rd_tag(),
                "option": tagged.option().map(rd_node_observations),
                "children": rd_node_observations(tagged.children()),
            }),
            other => panic!("unrepresented Rd spike node: {other:?}"),
        })
        .collect()
}

#[test]
fn dcf_description_surface_exposes_the_acceptance_contract() {
    let source = support::load_fixture(format!("{R_FIXTURE}/DESCRIPTION"));
    let output = dcf::parse(&source);

    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_eq!(dcf::reconstruct(&source), source);

    let document = output.document();
    assert_eq!(document.field("Package").unwrap().folded_value(), "foo");
    assert_eq!(document.field("Version").unwrap().folded_value(), "1.8.0");
    assert_eq!(
        document.field("Title").unwrap().folded_value(),
        "Statistical Models with Foo"
    );

    let depends = dependency_entries(&document.field("Depends").unwrap());
    assert_eq!(depends.len(), 1);
    assert_eq!(depends[0].name.as_str(), "R");
    assert_eq!(depends[0].constraints[0].op, VersionOp::Ge);
    assert_eq!(depends[0].constraints[0].version.as_str(), "4.3");
}

#[test]
fn dcf_reports_malformed_lines_but_preserves_metadata_bytes() {
    let malformed = "Package foo\nVersion: 1.0.0\n";
    let output = dcf::parse(malformed);
    let [diagnostic] = output.diagnostics.as_slice() else {
        panic!("expected one DCF diagnostic, got {:?}", output.diagnostics);
    };
    assert_eq!(
        diagnostic.message,
        "malformed line: expected 'Field: value' or an indented continuation line"
    );
    assert_eq!(&malformed[diagnostic.start..diagnostic.end], "Package foo");
    assert_eq!(dcf::reconstruct(malformed), malformed);
    assert_eq!(
        output.document().field("Version").unwrap().folded_value(),
        "1.0.0"
    );

    let semantic = dcf::parse("Imports: stats (=> 4.0)\n");
    assert!(semantic.diagnostics.is_empty());
    let imports = dependency_entries(&semantic.document().field("Imports").unwrap());
    assert_eq!(imports.len(), 1);
    assert!(imports[0].malformed_constraint());
    support::assert_json_golden(
        &serde_json::json!({
            "malformed": {"source": malformed, "message": diagnostic.message, "range": [diagnostic.start, diagnostic.end]},
            "recovered_version": output.document().field("Version").unwrap().folded_value(),
            "dependency": {"source": "Imports: stats (=> 4.0)\n", "parse_diagnostics": semantic.diagnostics.len(), "malformed_constraint": imports[0].malformed_constraint()},
        }),
        "spikes/failures/r-metadata.json",
    );
}

#[test]
fn arity_reports_malformed_r_syntax_with_a_lossless_tree() {
    let source = "fit <-\n";
    let output = parser::parse(source);
    let [diagnostic] = output.diagnostics.as_slice() else {
        panic!("expected one R diagnostic, got {:?}", output.diagnostics);
    };
    assert_eq!(diagnostic.message, "expected assignment right-hand side");
    assert_eq!(&source[diagnostic.start..diagnostic.end], "<-");
    assert_eq!(parser::reconstruct(source), source);
    support::assert_json_golden(
        &serde_json::json!({
            "source": source, "message": diagnostic.message, "range": [diagnostic.start, diagnostic.end],
            "reconstructed": parser::reconstruct(source),
        }),
        "spikes/failures/r-syntax.json",
    );
}

#[test]
fn arity_namespace_surface_exposes_the_acceptance_contract() {
    let source = support::load_fixture(format!("{R_FIXTURE}/NAMESPACE"));
    let output = namespace::parse(&source);

    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_eq!(namespace::reconstruct(&source), source);

    let directives: Vec<_> = output.document().directives().collect();
    assert_eq!(directives.len(), 8);
    assert_eq!(
        directives
            .iter()
            .map(|directive| directive.kind())
            .collect::<Vec<_>>(),
        [
            DirectiveKind::Export,
            DirectiveKind::Export,
            DirectiveKind::Export,
            DirectiveKind::Export,
            DirectiveKind::S3Method,
            DirectiveKind::S3Method,
            DirectiveKind::S3Method,
            DirectiveKind::ImportFrom,
        ]
    );
    assert_eq!(
        directives
            .iter()
            .map(|directive| directive.name().unwrap())
            .collect::<Vec<_>>(),
        [
            "export",
            "export",
            "export",
            "export",
            "S3method",
            "S3method",
            "S3method",
            "importFrom",
        ]
    );

    let export = &directives[0];
    assert_eq!(
        range_text!(&source, export.text_range()),
        "export(experimental_summary)"
    );
    let export_name = export.name_token().expect("export name token");
    assert_eq!(range_text!(&source, export_name.text_range()), "export");
    let export_argument = export.arguments().next().expect("export argument");
    assert_eq!(
        range_text!(
            &source,
            export_argument.value_range().expect("export value")
        ),
        "experimental_summary"
    );

    let method = &directives[6];
    assert_eq!(
        range_text!(&source, method.text_range()),
        "S3method(predict,foo_model)"
    );
    let method_arguments: Vec<_> = method.arguments().collect();
    assert_eq!(method_arguments.len(), 2);
    assert_eq!(
        range_text!(
            &source,
            method_arguments[0].value_range().expect("generic value")
        ),
        "predict"
    );
    assert_eq!(
        range_text!(
            &source,
            method_arguments[1].value_range().expect("class value")
        ),
        "foo_model"
    );

    let import = &directives[7];
    assert_eq!(
        range_text!(&source, import.text_range()),
        "importFrom(stats,predict)"
    );
    assert_eq!(
        import
            .arguments()
            .map(|argument| {
                range_text!(
                    &source,
                    argument.value_range().expect("import argument value"),
                )
            })
            .collect::<Vec<_>>(),
        ["stats", "predict"]
    );
}

#[test]
fn namespace_reports_unsupported_directives_without_hiding_them() {
    let source = "futureDirective(foo)\n";
    let output = namespace::parse(source);
    let [diagnostic] = output.diagnostics.as_slice() else {
        panic!(
            "expected one NAMESPACE diagnostic, got {:?}",
            output.diagnostics
        );
    };
    assert_eq!(diagnostic.message, "unsupported NAMESPACE directive");
    assert_eq!(&source[diagnostic.start..diagnostic.end], "futureDirective");

    let directive = output.document().directives().next().unwrap();
    assert_eq!(directive.kind(), DirectiveKind::Unsupported);
    assert_eq!(
        range_text!(source, directive.text_range()),
        "futureDirective(foo)"
    );
    assert_eq!(namespace::reconstruct(source), source);
    support::assert_json_golden(
        &serde_json::json!({
            "source": source, "message": diagnostic.message, "range": [diagnostic.start, diagnostic.end],
            "retained_directive": range_text!(source, directive.text_range()),
        }),
        "spikes/failures/r-namespace-unknown.json",
    );
}

#[test]
fn namespace_retains_but_does_not_evaluate_dynamic_conditions() {
    let source = "if (getRversion() >= \"4.0\") export(foo) else export(bar)\n";
    let output = namespace::parse(source);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);

    let exported = output
        .document()
        .directives()
        .map(|directive| {
            let argument = directive.arguments().next().expect("export argument");
            range_text!(
                source,
                argument.value_range().expect("export argument value")
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(exported, ["foo", "bar"]);
    assert_eq!(namespace::reconstruct(source), source);
    support::assert_json_golden(
        &serde_json::json!({
            "source": source, "flattened_exports": exported,
            "parse_diagnostics": output.diagnostics.len(), "reconstructed": namespace::reconstruct(source),
        }),
        "spikes/failures/r-namespace-conditional.json",
    );
}

#[test]
fn arity_function_formals_expose_the_acceptance_contract() {
    let source = support::load_fixture(format!("{R_FIXTURE}/R/fit.R"));
    let output = parser::parse(&source);

    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let function = named_function(&output.cst, "fit.default");
    let formals = function.formals();

    assert_eq!(
        formals
            .iter()
            .map(|formal| formal.name())
            .collect::<Vec<_>>(),
        ["x", "y", "solver", "tolerance", "..."]
    );
    assert_eq!(
        formals
            .iter()
            .map(|formal| range_text!(&source, formal.text_range()))
            .collect::<Vec<_>>(),
        [
            "x",
            "y",
            "solver = c(\"normal\", \"qr\")",
            "tolerance = 1e-8",
            "...",
        ]
    );
    assert_eq!(
        formals
            .iter()
            .map(|formal| {
                formal
                    .default_range()
                    .map(|range| range_text!(&source, range))
            })
            .collect::<Vec<_>>(),
        [
            None,
            None,
            Some("c(\"normal\", \"qr\")"),
            Some("1e-8"),
            None
        ]
    );
    assert_eq!(
        formals
            .iter()
            .filter_map(|formal| formal.default())
            .map(|default| default.to_string())
            .collect::<Vec<_>>(),
        ["c(\"normal\", \"qr\")", "1e-8"]
    );
}

#[test]
fn rd_source_parses_every_acceptance_document() {
    for (file, topic) in [
        ("fit.Rd", "fit"),
        ("foo_model.Rd", "foo_model"),
        ("mean_squared_error.Rd", "mean_squared_error"),
        ("experimental_summary.Rd", "experimental_summary"),
    ] {
        let parsed = parse_rd_fixture(file);
        assert!(
            parsed.diagnostics().is_empty(),
            "{file}: {:?}",
            parsed.diagnostics()
        );

        let document = parsed.document();
        assert_eq!(
            text_contents(document.inspect_name().unwrap().unwrap()).trim(),
            topic
        );
        assert!(document.inspect_title().unwrap().is_some());
        assert!(document.inspect_usage().unwrap().is_some());
        assert!(document.inspect_description().unwrap().is_some());
        assert!(document.inspect_value().unwrap().is_some());
        assert!(document.inspect_examples().unwrap().is_some());
        assert!(document.aliases().any(|alias| alias == topic));
    }
}

#[test]
fn rd_ast_exposes_the_acceptance_semantics_without_evaluation() {
    let fit = parse_rd_fixture("fit.Rd");
    let document = fit.document();

    assert_eq!(
        document.aliases().collect::<Vec<_>>(),
        ["fit", "fit.default", "fit.foo_model"]
    );
    assert!(document.inspect_details().unwrap().is_some());
    assert!(document.inspect_references().unwrap().is_some());
    assert_eq!(document.keywords().collect::<Vec<_>>(), ["models"]);

    let arguments = document
        .inspect_arguments()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        arguments
            .iter()
            .map(|argument| text_contents(argument.name).trim().to_owned())
            .collect::<Vec<_>>(),
        ["x", "...", "y", "solver", "tolerance", "features", "target"]
    );

    let root = RdPath::new(vec![]);
    let methods = document
        .usage()
        .unwrap()
        .iter()
        .filter_map(|node| node.method(&root))
        .map(|method| (method.generic().to_owned(), method.qualifier().to_owned()))
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        [
            ("fit".into(), "default".into()),
            ("fit".into(), "foo_model".into())
        ]
    );

    let inline_kinds = rd_nodes(document.nodes())
        .into_iter()
        .filter_map(|(node, path)| node.inline_span(&path))
        .map(|span| span.kind())
        .collect::<Vec<_>>();
    assert!(inline_kinds.contains(&RdInlineSpanKind::Code));
    assert!(inline_kinds.contains(&RdInlineSpanKind::Emph));

    let baseline = parse_rd_fixture("experimental_summary.Rd");
    assert_eq!(baseline.document().inspect_dynamic_markup().count(), 0);
    let source = support::acceptance_case("unsupported-rd").read("r/man/experimental_summary.Rd");
    let dynamic = rd_source::parse(source.as_bytes()).unwrap();
    let dynamic_document = dynamic.document();
    assert!(dynamic_document.inspect_description().unwrap().is_some());
    assert!(dynamic_document.inspect_details().unwrap().is_some());
    assert!(dynamic_document.inspect_examples().unwrap().is_some());

    let events = dynamic_document
        .inspect_dynamic_markup()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let [RdDynamicMarkupEvent::Sexpr(sexpr)] = events.as_slice() else {
        panic!("expected one dynamic Rd expression, got {events:?}");
    };
    assert_eq!(
        sexpr.view().code(),
        "paste(\"computed\", \"documentation\")"
    );
    assert_eq!(sexpr.effective_options().stage, RdSexprStage::Render);
    assert_eq!(sexpr.effective_options().results, RdSexprResults::Text);
    assert!(matches!(
        sexpr.state(),
        RdDynamicMarkupState::Unresolved {
            stage: RdSexprStage::Render
        }
    ));
}

#[test]
fn rd_source_reports_unknown_markup_and_preserves_an_opaque_node() {
    let source = r"\name{topic}\unknown{payload}";
    let parsed = rd_source::parse(source.as_bytes()).expect("recoverable Rd source");
    let [diagnostic] = parsed.diagnostics() else {
        panic!("expected one Rd diagnostic, got {:?}", parsed.diagnostics());
    };
    assert_eq!(diagnostic.severity(), &Severity::Error);
    assert_eq!(diagnostic.code(), &DiagnosticCode::UnknownTag);
    assert_eq!(&source[diagnostic.span().bytes()], r"\unknown");

    let unknown = rd_nodes(parsed.document().nodes())
        .into_iter()
        .find_map(|(node, _)| {
            node.as_tagged()
                .filter(|tagged| matches!(tagged.tag(), RdTag::Unknown(_)))
        })
        .expect("unknown Rd node");
    assert_eq!(unknown.tag(), &RdTag::Unknown(r"\unknown".into()));
    assert_eq!(text_contents(unknown.children()), "payload");
    support::assert_json_golden(
        &serde_json::json!({
            "source": source,
            "diagnostic": {
                "severity": format!("{:?}", diagnostic.severity()), "code": format!("{:?}", diagnostic.code()),
                "range": [diagnostic.span().bytes().start, diagnostic.span().bytes().end],
            },
            "nodes": rd_node_observations(parsed.document().nodes()),
        }),
        "spikes/failures/rd-unknown.json",
    );
}

#[test]
fn rd_strict_views_report_information_loss_with_structural_paths() {
    let source = r"\description{first}\description{second}";
    let parsed = rd_source::parse(source.as_bytes()).expect("recoverable Rd source");
    assert!(parsed.diagnostics().is_empty());

    let error = parsed
        .document()
        .inspect_description()
        .expect_err("duplicate description should not use first-wins projection");
    assert!(matches!(error.kind(), RdShapeErrorKind::Duplicate { .. }));
    assert_eq!(error.path().to_string(), "top-level[1]");
    assert_eq!(
        error.to_string(),
        "duplicate \\description for \\description at top-level[1]"
    );
    support::assert_json_golden(
        &serde_json::json!({
            "source": source, "parse_diagnostics": parsed.diagnostics().len(),
            "shape_error": error.to_string(), "structural_path": error.path().to_string(),
            "source_range": null,
            "nodes": rd_node_observations(parsed.document().nodes()),
        }),
        "spikes/failures/rd-shape.json",
    );
}

fn parse_rd_fixture(file: &str) -> rd_source::Parsed {
    let source = support::load_fixture(format!("{R_FIXTURE}/man/{file}"));
    rd_source::parse(source.as_bytes()).unwrap_or_else(|error| panic!("{file}: {error}"))
}

fn rd_nodes(nodes: &[RdNode]) -> Vec<(&RdNode, RdPath)> {
    fn walk<'a>(node: &'a RdNode, path: RdPath, output: &mut Vec<(&'a RdNode, RdPath)>) {
        output.push((node, path.clone()));
        let children = match node {
            RdNode::Tagged(tagged) => tagged.children(),
            RdNode::Group(group) => group.children(),
            _ => return,
        };
        for (index, child) in children.iter().enumerate() {
            walk(child, path.with_child(index), output);
        }
    }

    let mut output = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        walk(
            node,
            RdPath::new(vec![RdPathSegment::TopLevel(index)]),
            &mut output,
        );
    }
    output
}

fn named_function(root: &SyntaxNode, name: &str) -> FunctionExpr {
    root.descendants()
        .filter_map(AssignmentExpr::cast)
        .find_map(|assignment| {
            (assignment.target_name().as_deref() == Some(name))
                .then(|| assignment.value_element())
                .flatten()
                .and_then(|value| value.into_node())
                .and_then(FunctionExpr::cast)
        })
        .unwrap_or_else(|| panic!("expected function `{name}`"))
}
