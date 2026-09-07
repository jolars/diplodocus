use pydocstring::model::{FreeSectionKind, SectionKind};
use pydocstring::parse::{Document, Style, parse_numpy};
use pyproject_toml::PyProjectToml;
use ruff_python_ast::{
    Expr, ExprStringLiteral, ModModule, PySourceType, Stmt, StmtAnnAssign, StmtAssign,
    StmtClassDef, StmtFunctionDef,
};
use ruff_python_parser::parse_unchecked_source;
use ruff_text_size::{Ranged, TextRange};

mod support;

const PYTHON_FIXTURE: &str = "acceptance/python";

#[test]
fn pyproject_metadata_is_available_without_a_build_backend() {
    let source = support::load_fixture(format!("{PYTHON_FIXTURE}/pyproject.toml"));
    let metadata = PyProjectToml::new(&source).expect("valid pyproject metadata");
    let project = metadata.project.expect("PEP 621 project table");

    assert_eq!(project.name, "foo-python");
    assert_eq!(
        project.version.expect("static version").to_string(),
        "1.9.0"
    );
    assert_eq!(
        project.description.as_deref(),
        Some("Python bindings for the Foo statistical library.")
    );
    assert_eq!(
        project
            .requires_python
            .expect("Python requirement")
            .to_string(),
        ">=3.11"
    );
    let dependencies = project.dependencies.expect("project dependencies");
    assert_eq!(dependencies.len(), 1);
    assert_eq!(dependencies[0].name.as_ref(), "foo-core");
    assert_eq!(dependencies[0].to_string(), "foo-core>=1.9,<2");
}

#[test]
fn ruff_ast_exposes_the_acceptance_surface_with_byte_ranges() {
    let init_source = python_source("__init__.py");
    let init = parse_module(&init_source, PySourceType::Python);
    assert_eq!(
        docstring(&init.body)
            .expect("module docstring")
            .value
            .to_str(),
        "Fit and evaluate statistical models with Foo."
    );
    assert_eq!(
        string_value(assignment_named(&init.body, "__version__").value.as_ref()),
        Some("1.9.0")
    );
    assert_eq!(literal_exports(&init), EXPECTED_EXPORTS);
    let reexports = reexported_names(&init);
    assert_eq!(reexports.len(), EXPECTED_EXPORTS.len());
    assert!(reexports.iter().all(|name| EXPECTED_EXPORTS.contains(name)));

    let model_source = python_source("model.py");
    let model = parse_module(&model_source, PySourceType::Python);
    assert_eq!(
        attribute_docstrings(&model),
        ["DEFAULT_TOLERANCE", "SUPPORTED_SOLVERS"]
    );
    let tolerance = annotated_assignment_named(&model.body, "DEFAULT_TOLERANCE");
    assert_eq!(
        source_text(&model_source, tolerance.annotation.range()),
        "Final"
    );
    assert_eq!(
        source_text(
            &model_source,
            tolerance.value.as_ref().expect("constant value").range()
        ),
        "1e-8"
    );
    assert!(
        attribute_docstring(&model, "DEFAULT_TOLERANCE")
            .expect("constant docstring")
            .contains(":func:`fit`")
    );

    let diagnostics = class_named(&model.body, "FitDiagnostics");
    assert_eq!(
        decorators(&model_source, &diagnostics.decorator_list),
        ["dataclass(frozen=True)"]
    );
    assert_eq!(
        annotated_names(&diagnostics.body),
        ["iterations", "converged"]
    );

    let foo_model = class_named(&model.body, "FooModel");
    assert_eq!(
        function_names(&foo_model.body),
        [
            "__init__",
            "coefficients",
            "intercept",
            "fit",
            "predict",
            "_predict_one"
        ]
    );
    assert!(docstring(&foo_model.body).is_some());
    assert_eq!(
        function_named(&foo_model.body, "__init__")
            .parameters
            .args
            .len(),
        3
    );
    let coefficients = function_named(&foo_model.body, "coefficients");
    let intercept = function_named(&foo_model.body, "intercept");
    assert_eq!(
        decorators(&model_source, &coefficients.decorator_list),
        ["property"]
    );
    assert_eq!(
        decorators(&model_source, &intercept.decorator_list),
        ["property"]
    );
    assert!(
        function_named(&foo_model.body, "_predict_one")
            .name
            .starts_with('_')
    );

    let fit = function_named(&model.body, "fit");
    assert_eq!(fit.parameters.kwonlyargs.len(), 3);
    assert_eq!(
        source_text(
            &model_source,
            fit.returns.as_ref().expect("return annotation").range()
        ),
        "FooModel | tuple[FooModel, FitDiagnostics]"
    );
    assert!(source_text(&model_source, fit.range).starts_with("def fit("));
    let mean_squared_error = function_named(&model.body, "mean_squared_error");
    assert_eq!(mean_squared_error.parameters.args.len(), 2);
    assert!(docstring(&mean_squared_error.body).is_some());

    let stub_source = python_source("model.pyi");
    let stub = parse_module(&stub_source, PySourceType::Stub);
    assert_eq!(
        source_text(
            &stub_source,
            annotated_assignment_named(&stub.body, "DEFAULT_TOLERANCE")
                .annotation
                .range()
        ),
        "Final[float]"
    );
    let fit_overloads = functions_named(&stub.body, "fit");
    assert_eq!(fit_overloads.len(), 2);
    assert!(
        fit_overloads
            .iter()
            .all(|function| { decorators(&stub_source, &function.decorator_list) == ["overload"] })
    );
    assert_eq!(fit_overloads[0].parameters.kwonlyargs.len(), 3);
    assert_eq!(
        source_text(
            &stub_source,
            fit_overloads[0].parameters.kwonlyargs[0]
                .annotation()
                .expect("solver annotation")
                .range(),
        ),
        "Literal[\"normal\", \"qr\"]"
    );

    let stub_model = class_named(&stub.body, "FooModel");
    assert_eq!(
        function_names(&stub_model.body),
        [
            "__init__",
            "coefficients",
            "intercept",
            "fit",
            "predict",
            "predict"
        ]
    );
    let predict_overloads = functions_named(&stub_model.body, "predict");
    assert_eq!(predict_overloads.len(), 2);
    assert!(predict_overloads.iter().all(|function| {
        function.parameters.posonlyargs.len() == 2
            && decorators(&stub_source, &function.decorator_list) == ["overload"]
    }));
    assert_eq!(
        source_text(
            &stub_source,
            function_named(&stub.body, "mean_squared_error")
                .returns
                .as_ref()
                .expect("return annotation")
                .range()
        ),
        "float"
    );

    let native_source = python_source("_native.pyi");
    let native = parse_module(&native_source, PySourceType::Stub);
    assert_eq!(
        class_named(&native.body, "NativeWorkspace").name.as_str(),
        "NativeWorkspace"
    );
    assert_eq!(
        function_named(&native.body, "native_mean")
            .parameters
            .posonlyargs
            .len(),
        1
    );
    assert_eq!(
        annotated_names(&class_named(&native.body, "NativeWorkspace").body),
        ["dimension"]
    );

    let experimental_source = python_source("experimental.py");
    let experimental = parse_module(&experimental_source, PySourceType::Python);
    assert!(matches!(all_expression(&experimental), Expr::Call(_)));
    assert_eq!(
        function_named(&experimental.body, "experimental_rank")
            .name
            .as_str(),
        "experimental_rank"
    );

    let init_stub_source = python_source("__init__.pyi");
    let init_stub = parse_module(&init_stub_source, PySourceType::Stub);
    assert_eq!(literal_exports(&init_stub), EXPECTED_EXPORTS);
    let init_stub_reexports = reexported_names(&init_stub);
    assert_eq!(init_stub_reexports.len(), EXPECTED_EXPORTS.len());
    assert!(
        init_stub_reexports
            .iter()
            .all(|name| EXPECTED_EXPORTS.contains(name))
    );
    assert_eq!(
        source_text(
            &init_stub_source,
            annotated_assignment_named(&init_stub.body, "__version__")
                .annotation
                .range()
        ),
        "str"
    );

    let typed_marker = support::load_fixture(format!("{PYTHON_FIXTURE}/python/foo/py.typed"));
    assert!(typed_marker.is_empty());
}

#[test]
fn numpy_docstrings_are_structured_without_losing_ranges() {
    let source = python_source("model.py");
    let module = parse_module(&source, PySourceType::Python);
    let fit = function_named(&module.body, "fit");
    let literal = docstring(&fit.body).expect("fit docstring");
    let docstring_source = literal.value.to_str();
    let parsed = parse_numpy(docstring_source);
    let document = Document::new(&parsed);

    assert_eq!(parsed.style(), Style::NumPy);
    assert_eq!(
        document.summary().expect("summary").logical_text(),
        "Fit a linear model."
    );
    assert_eq!(
        document
            .sections()
            .map(|section| section.kind())
            .collect::<Vec<_>>(),
        [
            SectionKind::Parameters,
            SectionKind::Returns,
            SectionKind::Raises,
            SectionKind::FreeText(FreeSectionKind::Notes),
            SectionKind::References,
            SectionKind::FreeText(FreeSectionKind::Examples),
        ]
    );

    let parameters = document
        .sections()
        .find(|section| section.kind() == SectionKind::Parameters)
        .expect("parameters section");
    assert_eq!(
        parameters
            .entries()
            .filter_map(|entry| entry.name().map(|name| name.text()))
            .collect::<Vec<_>>(),
        [
            "features",
            "target",
            "solver",
            "tolerance",
            "return_diagnostics"
        ]
    );
    let solver = parameters
        .entries()
        .find(|entry| entry.name().is_some_and(|name| name.text() == "solver"))
        .expect("solver documentation");
    assert_eq!(
        solver.type_annotation().expect("solver type").text(),
        "{\"normal\", \"qr\"}"
    );
    assert_eq!(
        solver.default_value().expect("solver default").text(),
        "\"normal\""
    );
    assert_eq!(
        docstring_source_text(docstring_source, solver.range()),
        "solver : {\"normal\", \"qr\"}, default=\"normal\"\n        Solver used to estimate the coefficients."
    );

    let content_range = literal
        .as_single_part_string()
        .expect("one string literal")
        .content_range();
    assert_eq!(source_text(&source, content_range), docstring_source);
    let absolute_solver_start =
        usize::from(content_range.start()) + usize::from(solver.range().start());
    let absolute_solver_end =
        usize::from(content_range.start()) + usize::from(solver.range().end());
    assert_eq!(
        &source[absolute_solver_start..absolute_solver_end],
        docstring_source_text(docstring_source, solver.range())
    );
}

fn python_source(file: &str) -> String {
    support::load_fixture(format!("{PYTHON_FIXTURE}/python/foo/{file}"))
}

fn parse_module(source: &str, source_type: PySourceType) -> ModModule {
    let parsed = parse_unchecked_source(source, source_type);
    assert!(
        parsed.errors().is_empty(),
        "syntax errors: {:?}",
        parsed.errors()
    );
    assert!(
        parsed.unsupported_syntax_errors().is_empty(),
        "unsupported syntax: {:?}",
        parsed.unsupported_syntax_errors()
    );
    parsed.into_syntax()
}

fn function_named<'a>(body: &'a [Stmt], name: &str) -> &'a StmtFunctionDef {
    functions_named(body, name)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("function `{name}` should exist"))
}

fn functions_named<'a>(body: &'a [Stmt], name: &str) -> Vec<&'a StmtFunctionDef> {
    body.iter()
        .filter_map(|statement| match statement {
            Stmt::FunctionDef(function) if function.name.as_str() == name => Some(function),
            _ => None,
        })
        .collect()
}

fn function_names(body: &[Stmt]) -> Vec<&str> {
    body.iter()
        .filter_map(|statement| match statement {
            Stmt::FunctionDef(function) => Some(function.name.as_str()),
            _ => None,
        })
        .collect()
}

fn class_named<'a>(body: &'a [Stmt], name: &str) -> &'a StmtClassDef {
    body.iter()
        .find_map(|statement| match statement {
            Stmt::ClassDef(class) if class.name.as_str() == name => Some(class),
            _ => None,
        })
        .unwrap_or_else(|| panic!("class `{name}` should exist"))
}

fn docstring(body: &[Stmt]) -> Option<&ExprStringLiteral> {
    match body.first() {
        Some(Stmt::Expr(statement)) => match statement.value.as_ref() {
            Expr::StringLiteral(literal) => Some(literal),
            _ => None,
        },
        _ => None,
    }
}

fn assignment_named<'a>(body: &'a [Stmt], name: &str) -> &'a StmtAssign {
    body.iter()
        .find_map(|statement| match statement {
            Stmt::Assign(assignment)
                if assignment
                    .targets
                    .iter()
                    .any(|target| matches!(target, Expr::Name(target) if target.id == name)) =>
            {
                Some(assignment)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("assignment to `{name}` should exist"))
}

fn annotated_assignment_named<'a>(body: &'a [Stmt], name: &str) -> &'a StmtAnnAssign {
    body.iter()
        .find_map(|statement| match statement {
            Stmt::AnnAssign(assignment)
                if matches!(assignment.target.as_ref(), Expr::Name(target) if target.id == name) =>
            {
                Some(assignment)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("annotated assignment to `{name}` should exist"))
}

fn string_value(expression: &Expr) -> Option<&str> {
    match expression {
        Expr::StringLiteral(literal) => Some(literal.value.to_str()),
        _ => None,
    }
}

fn attribute_docstrings(module: &ModModule) -> Vec<&str> {
    module
        .body
        .windows(2)
        .filter_map(|statements| match (&statements[0], &statements[1]) {
            (Stmt::AnnAssign(assignment), Stmt::Expr(documentation))
                if matches!(documentation.value.as_ref(), Expr::StringLiteral(_)) =>
            {
                match assignment.target.as_ref() {
                    Expr::Name(name) => Some(name.id.as_str()),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect()
}

fn attribute_docstring<'a>(module: &'a ModModule, name: &str) -> Option<&'a str> {
    module
        .body
        .windows(2)
        .find_map(|statements| match (&statements[0], &statements[1]) {
            (Stmt::AnnAssign(assignment), Stmt::Expr(documentation))
                if matches!(assignment.target.as_ref(), Expr::Name(target) if target.id == name) =>
            {
                string_value(documentation.value.as_ref())
            }
            _ => None,
        })
}

fn annotated_names(body: &[Stmt]) -> Vec<&str> {
    body.iter()
        .filter_map(|statement| match statement {
            Stmt::AnnAssign(assignment) => match assignment.target.as_ref() {
                Expr::Name(name) => Some(name.id.as_str()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn literal_exports(module: &ModModule) -> Vec<&str> {
    match all_expression(module) {
        Expr::List(list) => list
            .elts
            .iter()
            .map(|expression| match expression {
                Expr::StringLiteral(literal) => literal.value.to_str(),
                _ => panic!("literal `__all__` should contain only strings"),
            })
            .collect(),
        _ => panic!("fixture should use a literal `__all__`"),
    }
}

fn all_expression(module: &ModModule) -> &Expr {
    module
        .body
        .iter()
        .find_map(|statement| match statement {
            Stmt::Assign(assignment)
                if assignment
                    .targets
                    .iter()
                    .any(|target| matches!(target, Expr::Name(name) if name.id == "__all__")) =>
            {
                Some(assignment.value.as_ref())
            }
            _ => None,
        })
        .expect("module should assign `__all__`")
}

fn reexported_names(module: &ModModule) -> Vec<&str> {
    module
        .body
        .iter()
        .filter_map(|statement| match statement {
            Stmt::ImportFrom(import) if import.level > 0 => Some(import),
            _ => None,
        })
        .flat_map(|import| import.names.iter())
        .map(|alias| alias.asname.as_ref().unwrap_or(&alias.name).as_str())
        .collect()
}

fn decorators<'a>(source: &'a str, decorators: &'a [ruff_python_ast::Decorator]) -> Vec<&'a str> {
    decorators
        .iter()
        .map(|decorator| source_text(source, decorator.expression.range()))
        .collect()
}

fn source_text(source: &str, range: TextRange) -> &str {
    &source[usize::from(range.start())..usize::from(range.end())]
}

fn docstring_source_text(source: &str, range: pydocstring::text::TextRange) -> &str {
    &source[usize::from(range.start())..usize::from(range.end())]
}

const EXPECTED_EXPORTS: &[&str] = &[
    "DEFAULT_TOLERANCE",
    "SUPPORTED_SOLVERS",
    "FitDiagnostics",
    "FooModel",
    "NativeWorkspace",
    "experimental",
    "fit",
    "mean_squared_error",
    "native_mean",
];
