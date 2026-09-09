use pydocstring::model::{FreeSectionKind, SectionKind};
use pydocstring::parse::{Document, Style, parse_numpy};
use pydocstring::syntax::SyntaxKind as DocstringSyntaxKind;
use pyproject_toml::PyProjectToml;
use ruff_python_ast::{
    Expr, ExprStringLiteral, ModModule, PySourceType, PythonVersion, Stmt, StmtAnnAssign,
    StmtAssign, StmtClassDef, StmtFunctionDef,
};
use ruff_python_parser::{
    ParseOptions, UnsupportedSyntaxErrorKind, parse_unchecked, parse_unchecked_source,
};
use ruff_text_size::{Ranged, TextRange};

mod support;

const PYTHON_FIXTURE: &str = "acceptance/python";

#[test]
fn python_exploratory_output_matches_golden() {
    use serde_json::json;

    let metadata_source = support::load_fixture(format!("{PYTHON_FIXTURE}/pyproject.toml"));
    let project = PyProjectToml::new(&metadata_source)
        .unwrap()
        .project
        .unwrap();
    let dynamic_workspace = support::acceptance_case("python-dynamic-export");
    let modules = support::fixture_files(format!("{PYTHON_FIXTURE}/python/foo"))
        .into_iter()
        .filter_map(|path| {
            let source_type = match path.extension()?.to_str()? {
                "py" => PySourceType::Python,
                "pyi" => PySourceType::Stub,
                _ => return None,
            };
            let name = path.to_str().unwrap();
            let source = if name == "experimental.py" {
                dynamic_workspace.read("python/python/foo/experimental.py")
            } else {
                python_source(name)
            };
            let module = parse_module(&source, source_type);
            Some(json!({
                "path": format!("python/foo/{name}"),
                "source_type": if source_type == PySourceType::Stub { "stub" } else { "python" },
                "exports": static_exports(&module),
                "export_expression": find_all_expression(&module).map(|value| json!({
                    "text": source_text(&source, value.range()),
                    "range": python_range(value.range()),
                    "is_static": static_exports(&module).is_some(),
                })),
                "reexports": resolved_reexports("foo", &module, &source).iter().map(|value| json!({
                    "public_name": value.public_name,
                    "canonical_name": value.canonical_name,
                    "declaration": value.declaration,
                    "range": python_range(value.range),
                })).collect::<Vec<_>>(),
                "documentation": python_docstring_observation(&module.body),
                "declarations": python_declaration_observations(&module.body, &source),
                "attribute_documentation": attribute_docstrings(&module).iter().map(|name| json!({
                    "name": name,
                    "text": attribute_docstring(&module, name),
                })).collect::<Vec<_>>(),
            }))
        })
        .collect::<Vec<_>>();
    let source = python_source("model.py");
    let implementation = parse_module(&source, PySourceType::Python);
    let stub_source = python_source("model.pyi");
    let stub = parse_module(&stub_source, PySourceType::Stub);
    let reconciled = [
        ("fit", &implementation.body[..], &stub.body[..]),
        (
            "predict",
            &class_named(&implementation.body, "FooModel").body[..],
            &class_named(&stub.body, "FooModel").body[..],
        ),
    ]
    .into_iter()
    .map(|(name, implementation_body, stub_body)| {
        let callable = reconciled_callable(
            name,
            implementation_body,
            &source,
            "model.py",
            stub_body,
            &stub_source,
            "model.pyi",
        );
        json!({
            "name": name,
            "signature_source": callable.signature_source,
            "documentation_source": callable.documentation_source,
            "documentation_range": python_range(callable.documentation_range),
            "summary": callable.documentation,
            "signatures": callable.signatures.iter().map(|signature| json!({
                "declaration": signature.declaration,
                "decorators": signature.decorators,
                "positional_only": signature.positional_only,
                "return_annotation": signature.return_annotation,
                "range": python_range(signature.range),
            })).collect::<Vec<_>>(),
        })
    })
    .collect::<Vec<_>>();
    let annotation = preferred_annotation(
        "DEFAULT_TOLERANCE",
        &implementation,
        &source,
        "model.py",
        &stub,
        &stub_source,
        "model.pyi",
    );

    support::assert_json_golden(
        &json!({
            "schema": "python-spike-observation-v1",
            "mode": "static",
            "metadata": {
                "path": "pyproject.toml",
                "name": project.name,
                "version": project.version.unwrap().to_string(),
                "description": project.description,
                "requires_python": project.requires_python.unwrap().to_string(),
                "dependencies": project.dependencies.unwrap().iter().map(ToString::to_string).collect::<Vec<_>>(),
            },
            "typed_marker": support::fixture_path(format!("{PYTHON_FIXTURE}/python/foo/py.typed")).is_file(),
            "modules": modules,
            "reconciled_callables": reconciled,
            "preferred_annotation": {"text": annotation.text, "path": annotation.source, "range": python_range(annotation.range)},
        }),
        "spikes/python.json",
    );
}

fn python_range(range: TextRange) -> [u32; 2] {
    [range.start().into(), range.end().into()]
}

fn python_docstring_observation(body: &[Stmt]) -> serde_json::Value {
    use serde_json::json;

    let Some(literal) = docstring(body) else {
        return serde_json::Value::Null;
    };
    let text = literal.value.to_str();
    let parsed = parse_numpy(text);
    let document = Document::new(&parsed);
    json!({
        "text": text,
        "literal_range": python_range(literal.range()),
        "summary": document.summary().map(|summary| summary.logical_text()),
        "sections": document.sections().map(|section| json!({
            "kind": format!("{:?}", section.kind()),
            "entries": section.entries().map(|entry| json!({
                "name": entry.name().map(|name| name.text().to_owned()),
                "type": entry.type_annotation().map(|annotation| annotation.text().to_owned()),
                "default": entry.default_value().map(|value| value.text().to_owned()),
                "text": docstring_source_text(text, entry.range()),
                "decoded_range": [u32::from(entry.range().start()), u32::from(entry.range().end())],
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

fn python_declaration_observations(body: &[Stmt], source: &str) -> Vec<serde_json::Value> {
    use serde_json::json;

    body.iter().filter_map(|statement| {
        let mut value = match statement {
            Stmt::FunctionDef(function) => json!({
                "kind": "function", "name": function.name.as_str(),
                "parameters": source_text(source, function.parameters.range()),
                "returns": function.returns.as_ref().map(|value| source_text(source, value.range())),
                "decorators": decorators(source, &function.decorator_list),
                "documentation": python_docstring_observation(&function.body),
            }),
            Stmt::ClassDef(class) => json!({
                "kind": "class", "name": class.name.as_str(),
                "decorators": decorators(source, &class.decorator_list),
                "documentation": python_docstring_observation(&class.body),
                "members": python_declaration_observations(&class.body, source),
            }),
            Stmt::AnnAssign(assignment) => json!({
                "kind": "annotated-assignment",
                "name": source_text(source, assignment.target.range()),
                "annotation": source_text(source, assignment.annotation.range()),
                "value": assignment.value.as_ref().map(|value| source_text(source, value.range())),
            }),
            Stmt::Assign(assignment) => json!({
                "kind": "assignment",
                "targets": assignment.targets.iter().map(|value| source_text(source, value.range())).collect::<Vec<_>>(),
                "value": source_text(source, assignment.value.range()),
            }),
            _ => return None,
        };
        value["range"] = json!(python_range(statement.range()));
        Some(value)
    }).collect()
}

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
fn pyproject_metadata_distinguishes_malformed_and_dynamic_values() {
    let malformed = "[project]\nname = \"foo\"\nversion = \"not a version\"\n";
    let error = PyProjectToml::new(malformed).expect_err("invalid PEP 440 version");
    let span = error.span().expect("TOML diagnostic span");
    assert!(
        malformed[span.clone()].contains("not a version"),
        "unexpected metadata diagnostic: {error}"
    );

    let dynamic = "[project]\nname = \"foo\"\ndynamic = [\"version\"]\n";
    let project = PyProjectToml::new(dynamic)
        .expect("valid dynamic metadata declaration")
        .project
        .expect("project table");
    assert!(project.version.is_none());
    assert_eq!(
        project.dynamic.as_ref().expect("dynamic fields"),
        &["version"]
    );
    support::assert_json_golden(
        &serde_json::json!({
            "malformed": {"source": malformed, "message": error.to_string(), "range": [span.start, span.end]},
            "dynamic": {"source": dynamic, "fields": project.dynamic, "has_version": project.version.is_some()},
        }),
        "spikes/failures/python-metadata.json",
    );
}

#[test]
fn ruff_separates_malformed_syntax_from_unsupported_python_versions() {
    let malformed = "def broken(:\n    pass\n";
    let parsed = parse_unchecked_source(malformed, PySourceType::Python);
    assert!(!parsed.errors().is_empty());
    assert!(parsed.unsupported_syntax_errors().is_empty());
    assert!(parsed.errors().iter().all(|error| {
        usize::from(error.location.start()) <= malformed.len()
            && usize::from(error.location.end()) <= malformed.len()
    }));
    let malformed_observation = serde_json::json!({
        "source": malformed,
        "errors": parsed.errors().iter().map(|error| serde_json::json!({
            "message": error.to_string(), "range": python_range(error.location),
        })).collect::<Vec<_>>(),
        "unsupported_count": parsed.unsupported_syntax_errors().len(),
    });

    let versioned = "type Alias = int\n";
    let parsed = parse_unchecked(
        versioned,
        ParseOptions::from(PySourceType::Python).with_target_version(PythonVersion::PY311),
    );
    assert!(parsed.errors().is_empty());
    let [unsupported] = parsed.unsupported_syntax_errors() else {
        panic!(
            "expected one unsupported-version diagnostic, got {:?}",
            parsed.unsupported_syntax_errors()
        );
    };
    assert_eq!(
        unsupported.kind,
        UnsupportedSyntaxErrorKind::TypeAliasStatement
    );
    assert_eq!(unsupported.target_version, PythonVersion::PY311);
    assert_eq!(source_text(versioned, unsupported.range), "type");
    support::assert_json_golden(
        &serde_json::json!({
            "malformed": malformed_observation,
            "versioned": {
                "source": versioned, "target": "3.11", "error_count": parsed.errors().len(),
                "kind": format!("{:?}", unsupported.kind), "range": python_range(unsupported.range),
            },
        }),
        "spikes/failures/python-syntax.json",
    );
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

    let baseline = parse_module(&python_source("experimental.py"), PySourceType::Python);
    assert_eq!(literal_exports(&baseline), ["experimental_rank"]);
    let experimental_source =
        support::acceptance_case("python-dynamic-export").read("python/python/foo/experimental.py");
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
fn package_surface_is_reconciled_statically_without_importing() {
    assert!(
        !support::fixture_path(format!("{PYTHON_FIXTURE}/python/foo/_native.py")).exists(),
        "the native module must remain unavailable to import"
    );

    let init_source = python_source("__init__.py");
    let init = parse_module(&init_source, PySourceType::Python);
    let init_stub_source = python_source("__init__.pyi");
    let init_stub = parse_module(&init_stub_source, PySourceType::Stub);

    let exports = static_exports(&init).expect("literal implementation exports");
    assert_eq!(exports, EXPECTED_EXPORTS);
    assert_eq!(
        static_exports(&init_stub).expect("literal stub exports"),
        exports
    );

    let implementation_reexports = resolved_reexports("foo", &init, &init_source);
    let reexports = resolved_reexports("foo", &init_stub, &init_stub_source);
    assert_eq!(
        implementation_reexports
            .iter()
            .map(|reexport| (
                reexport.public_name.as_str(),
                reexport.canonical_name.as_str()
            ))
            .collect::<Vec<_>>(),
        reexports
            .iter()
            .map(|reexport| (
                reexport.public_name.as_str(),
                reexport.canonical_name.as_str()
            ))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        reexports
            .iter()
            .map(|reexport| (
                reexport.public_name.as_str(),
                reexport.canonical_name.as_str()
            ))
            .collect::<Vec<_>>(),
        [
            ("experimental", "foo.experimental"),
            ("NativeWorkspace", "foo._native.NativeWorkspace"),
            ("native_mean", "foo._native.native_mean"),
            ("DEFAULT_TOLERANCE", "foo.model.DEFAULT_TOLERANCE"),
            ("SUPPORTED_SOLVERS", "foo.model.SUPPORTED_SOLVERS"),
            ("FitDiagnostics", "foo.model.FitDiagnostics"),
            ("FooModel", "foo.model.FooModel"),
            ("fit", "foo.model.fit"),
            ("mean_squared_error", "foo.model.mean_squared_error"),
        ]
    );
    assert!(reexports.iter().all(|reexport| {
        source_text(&init_stub_source, reexport.range) == reexport.declaration
    }));
    assert!(
        implementation_reexports
            .iter()
            .all(|reexport| { source_text(&init_source, reexport.range) == reexport.declaration })
    );

    let implementation_source = python_source("model.py");
    let implementation = parse_module(&implementation_source, PySourceType::Python);
    let stub_source = python_source("model.pyi");
    let stub = parse_module(&stub_source, PySourceType::Stub);

    let tolerance = preferred_annotation(
        "DEFAULT_TOLERANCE",
        &implementation,
        &implementation_source,
        "model.py",
        &stub,
        &stub_source,
        "model.pyi",
    );
    assert_eq!(tolerance.text, "Final[float]");
    assert_eq!(tolerance.source, "model.pyi");
    assert_eq!(source_text(&stub_source, tolerance.range), tolerance.text);

    let fit = reconciled_callable(
        "fit",
        &implementation.body,
        &implementation_source,
        "model.py",
        &stub.body,
        &stub_source,
        "model.pyi",
    );
    assert_eq!(fit.signature_source, "model.pyi");
    assert_eq!(fit.documentation_source, "model.py");
    assert_eq!(fit.documentation, "Fit a linear model.");
    assert_eq!(fit.signatures.len(), 2);
    assert_eq!(
        fit.signatures
            .iter()
            .map(|signature| signature.return_annotation.as_str())
            .collect::<Vec<_>>(),
        ["FooModel", "tuple[FooModel, FitDiagnostics]"]
    );
    assert!(fit.signatures.iter().all(|signature| {
        signature.decorators == ["overload"]
            && source_text(&stub_source, signature.range) == signature.declaration
    }));
    assert_eq!(
        source_text(&implementation_source, fit.documentation_range),
        fit.documentation_text
    );

    let implementation_model = class_named(&implementation.body, "FooModel");
    let stub_model = class_named(&stub.body, "FooModel");
    let predict = reconciled_callable(
        "predict",
        &implementation_model.body,
        &implementation_source,
        "model.py",
        &stub_model.body,
        &stub_source,
        "model.pyi",
    );
    assert_eq!(predict.signatures.len(), 2);
    assert_eq!(
        predict
            .signatures
            .iter()
            .map(|signature| signature.return_annotation.as_str())
            .collect::<Vec<_>>(),
        ["float", "list[float]"]
    );
    assert!(
        predict
            .signatures
            .iter()
            .all(|signature| signature.positional_only == ["self", "features"])
    );

    let diagnostics = class_named(&implementation.body, "FitDiagnostics");
    assert_eq!(
        decorators(&implementation_source, &diagnostics.decorator_list),
        ["dataclass(frozen=True)"]
    );
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

#[test]
fn docstring_recovery_and_decoding_require_adapter_diagnostics() {
    let incomplete = "Summary.\n\nParameters\n----------\nvalue :\n";
    let parsed = parse_numpy(incomplete);
    let section = parsed
        .root()
        .nodes(DocstringSyntaxKind::SECTION)
        .next()
        .expect("parameters section");
    let entry = section
        .nodes(DocstringSyntaxKind::ENTRY)
        .next()
        .expect("parameter entry");
    let missing_type = entry
        .find_missing(DocstringSyntaxKind::TYPE)
        .expect("missing type placeholder");
    assert!(missing_type.range().is_empty());

    let source = "def example():\n    \"\"\"first\\nsecond\"\"\"\n";
    let module = parse_module(source, PySourceType::Python);
    let literal =
        docstring(&function_named(&module.body, "example").body).expect("example docstring");
    let content_range = literal
        .as_single_part_string()
        .expect("one string literal")
        .content_range();
    assert_eq!(source_text(source, content_range), r"first\nsecond");
    assert_eq!(literal.value.to_str(), "first\nsecond");
    assert_ne!(source_text(source, content_range), literal.value.to_str());
    support::assert_json_golden(
        &serde_json::json!({
            "incomplete": {
                "source": incomplete,
                "missing_type_range": [u32::from(missing_type.range().start()), u32::from(missing_type.range().end())],
            },
            "decoded": {
                "source": source, "content_range": python_range(content_range),
                "raw": source_text(source, content_range), "value": literal.value.to_str(),
            },
        }),
        "spikes/failures/python-docstring.json",
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

#[derive(Debug, PartialEq, Eq)]
struct ResolvedReexport {
    public_name: String,
    canonical_name: String,
    declaration: String,
    range: TextRange,
}

fn resolved_reexports(package: &str, module: &ModModule, source: &str) -> Vec<ResolvedReexport> {
    module
        .body
        .iter()
        .filter_map(|statement| match statement {
            Stmt::ImportFrom(import) if import.level > 0 => Some(import),
            _ => None,
        })
        .flat_map(|import| {
            let imported_module = match &import.module {
                Some(module) => format!("{package}.{module}"),
                None => package.to_owned(),
            };
            import.names.iter().map(move |alias| ResolvedReexport {
                public_name: alias.asname.as_ref().unwrap_or(&alias.name).to_string(),
                canonical_name: format!("{imported_module}.{}", alias.name),
                declaration: source_text(source, alias.range).to_owned(),
                range: alias.range,
            })
        })
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
struct SourceValue {
    text: String,
    source: &'static str,
    range: TextRange,
}

fn preferred_annotation(
    name: &str,
    implementation: &ModModule,
    implementation_source: &str,
    implementation_path: &'static str,
    stub: &ModModule,
    stub_source: &str,
    stub_path: &'static str,
) -> SourceValue {
    let (assignment, source, path) = stub
        .body
        .iter()
        .find_map(|statement| match statement {
            Stmt::AnnAssign(assignment)
                if matches!(assignment.target.as_ref(), Expr::Name(target) if target.id == name) =>
            {
                Some(assignment)
            }
            _ => None,
        })
        .map_or_else(
            || {
                (
                    annotated_assignment_named(&implementation.body, name),
                    implementation_source,
                    implementation_path,
                )
            },
            |assignment| (assignment, stub_source, stub_path),
        );
    let range = assignment.annotation.range();

    SourceValue {
        text: source_text(source, range).to_owned(),
        source: path,
        range,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ReconciledCallable {
    signatures: Vec<Signature>,
    signature_source: &'static str,
    documentation: String,
    documentation_text: String,
    documentation_source: &'static str,
    documentation_range: TextRange,
}

#[derive(Debug, PartialEq, Eq)]
struct Signature {
    declaration: String,
    decorators: Vec<String>,
    positional_only: Vec<String>,
    return_annotation: String,
    range: TextRange,
}

fn reconciled_callable(
    name: &str,
    implementation_body: &[Stmt],
    implementation_source: &str,
    implementation_path: &'static str,
    stub_body: &[Stmt],
    stub_source: &str,
    stub_path: &'static str,
) -> ReconciledCallable {
    let implementation = function_named(implementation_body, name);
    let stub_declarations = functions_named(stub_body, name);
    let overloads = stub_declarations
        .iter()
        .copied()
        .filter(|function| decorators(stub_source, &function.decorator_list).contains(&"overload"))
        .collect::<Vec<_>>();
    let (declarations, signature_source, signature_path) = if overloads.is_empty() {
        if stub_declarations.is_empty() {
            (
                vec![implementation],
                implementation_source,
                implementation_path,
            )
        } else {
            (stub_declarations, stub_source, stub_path)
        }
    } else {
        (overloads, stub_source, stub_path)
    };

    let documentation = docstring(&implementation.body).expect("implementation docstring");
    let documentation_text = documentation.value.to_str().to_owned();
    let parsed = parse_numpy(&documentation_text);
    let summary = Document::new(&parsed)
        .summary()
        .expect("docstring summary")
        .logical_text();
    let documentation_range = documentation
        .as_single_part_string()
        .expect("one string literal")
        .content_range();

    ReconciledCallable {
        signatures: declarations
            .into_iter()
            .map(|function| Signature {
                declaration: source_text(signature_source, function.range).to_owned(),
                decorators: decorators(signature_source, &function.decorator_list)
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                positional_only: function
                    .parameters
                    .posonlyargs
                    .iter()
                    .map(|parameter| parameter.name().to_string())
                    .collect(),
                return_annotation: source_text(
                    signature_source,
                    function
                        .returns
                        .as_ref()
                        .expect("return annotation")
                        .range(),
                )
                .to_owned(),
                range: function.range,
            })
            .collect(),
        signature_source: signature_path,
        documentation: summary,
        documentation_text,
        documentation_source: implementation_path,
        documentation_range,
    }
}

fn static_exports(module: &ModModule) -> Option<Vec<&str>> {
    match find_all_expression(module)? {
        Expr::List(list) => list
            .elts
            .iter()
            .map(|expression| match expression {
                Expr::StringLiteral(literal) => Some(literal.value.to_str()),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

fn literal_exports(module: &ModModule) -> Vec<&str> {
    static_exports(module).expect("fixture should use a literal string `__all__`")
}

fn all_expression(module: &ModModule) -> &Expr {
    find_all_expression(module).expect("module should assign `__all__`")
}

fn find_all_expression(module: &ModModule) -> Option<&Expr> {
    module.body.iter().find_map(|statement| match statement {
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
