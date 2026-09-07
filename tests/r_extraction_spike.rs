use arity_parser::ast::{AssignmentExpr, AstNode, FunctionExpr};
use arity_parser::namespace::{self, DirectiveKind};
use arity_parser::parser;
use arity_parser::syntax::SyntaxNode;

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
