use arity_parser::ast::{AssignmentExpr, AstNode, FunctionExpr};
use arity_parser::namespace::{self, DirectiveKind};
use arity_parser::parser;
use arity_parser::syntax::SyntaxNode;
use rd_ast::{
    RdDynamicMarkupEvent, RdDynamicMarkupState, RdInlineSpanKind, RdNode, RdPath, RdPathSegment,
    RdSexprResults, RdSexprStage, text_contents,
};

mod support;

const R_FIXTURE: &str = "acceptance/r";

macro_rules! range_text {
    ($source:expr, $range:expr $(,)?) => {{
        let range = $range;
        &$source[usize::from(range.start())..usize::from(range.end())]
    }};
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

    let dynamic = parse_rd_fixture("experimental_summary.Rd");
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
