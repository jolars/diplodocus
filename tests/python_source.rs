use diplodocus::extractors::python::{
    DeclarationKind, ParsedPythonPackage, PythonSourceKind, parse_target,
};
use diplodocus::ir::{ParameterKind, ProvenanceActivity, Signature};
use diplodocus::paths::{ResolvedPackagePaths, ResolvedRepositoryPaths, ResolvedTargetPath};

mod support;

fn parse(workspace: &support::TestWorkspace) -> ParsedPythonPackage {
    let root = workspace.path().canonicalize().unwrap();
    let repository = ResolvedRepositoryPaths {
        id: "repo".into(),
        path: root.clone(),
    };
    let target = ResolvedTargetPath {
        id: "api".into(),
        path: root.join("python/foo"),
    };
    let package = ResolvedPackagePaths {
        id: "python".into(),
        repository_index: 0,
        path: root.clone(),
        metadata_path: root.join("pyproject.toml"),
        targets: vec![target.clone()],
    };
    parse_target(&repository, &package, &target)
}

#[test]
fn acceptance_sources_are_parsed_without_importing_native_code() {
    let workspace = support::TestWorkspace::from_fixture("acceptance/python");
    let result = parse(&workspace);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let metadata = result.metadata.as_ref().unwrap();
    assert_eq!(metadata.name, "foo-python");
    assert_eq!(metadata.version, "1.9.0");
    assert_eq!(metadata.target_version, "3.11");
    assert_eq!(metadata.dependencies, ["foo-core>=1.9,<2"]);
    assert_eq!(result.modules.len(), 6);
    assert_eq!(result.inputs.len(), 8);
    let native = result
        .modules
        .iter()
        .find(|module| module.name == "foo._native")
        .unwrap();
    assert_eq!(native.kind, PythonSourceKind::Stub);
    let model = result
        .modules
        .iter()
        .find(|module| module.name == "foo.model" && module.kind == PythonSourceKind::Source)
        .unwrap();
    let class = model
        .declarations
        .iter()
        .find(|declaration| declaration.name == "FooModel")
        .unwrap();
    assert_eq!(class.kind, DeclarationKind::Class);
    let predict = class
        .members
        .iter()
        .find(|declaration| declaration.name == "predict")
        .unwrap();
    let Signature::Callable {
        parameters,
        returns,
    } = &predict.signature.as_ref().unwrap().signature
    else {
        panic!("callable")
    };
    assert_eq!(parameters[1].kind, ParameterKind::PositionalOnly);
    assert!(returns.is_some());
    let ProvenanceActivity::Extraction {
        inputs, parsers, ..
    } = &result.provenance.activity
    else {
        panic!("extraction")
    };
    assert_eq!(inputs["repo"].len(), 8);
    assert_eq!(
        parsers["ruff_python_parser"].settings["target_version"],
        "3.11"
    );
}

#[test]
fn malformed_input_is_not_authoritative_and_other_modules_survive() {
    let workspace = support::TestWorkspace::from_fixture("acceptance/python");
    workspace.write("python/foo/model.py", "def broken(:\n    pass\n");
    let result = parse(&workspace);
    let malformed = result
        .modules
        .iter()
        .find(|module| module.name == "foo.model" && module.kind == PythonSourceKind::Source)
        .unwrap();
    assert!(!malformed.valid);
    assert!(malformed.declarations.is_empty());
    assert!(
        result
            .modules
            .iter()
            .any(|module| module.name == "foo._native" && module.valid)
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_str() == "python-syntax")
    );
}

#[test]
fn dynamic_required_metadata_never_invokes_a_backend() {
    let workspace = support::TestWorkspace::from_fixture("acceptance/python");
    workspace.write("pyproject.toml", "[build-system]\nrequires = []\nbuild-backend = 'must_not_run'\n[project]\nname = 'foo'\ndynamic = ['version']\n");
    let result = parse(&workspace);
    assert!(result.metadata.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_str() == "python-dynamic-metadata")
    );
}

#[test]
fn declared_grammar_and_portable_byte_spans_are_preserved() {
    let workspace = support::TestWorkspace::from_fixture("acceptance/python");
    workspace.write("python/foo/new.py", "type Alias = int\n");
    let result = parse(&workspace);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_str() == "python-unsupported-version")
    );
    let relocated = support::TestWorkspace::from_fixture("acceptance/python");
    relocated.write("python/foo/new.py", "type Alias = int\n");
    assert_eq!(result, parse(&relocated));
}

#[test]
fn malformed_metadata_reports_its_file_and_span() {
    let workspace = support::TestWorkspace::from_fixture("acceptance/python");
    workspace.write(
        "pyproject.toml",
        "[project]\nname = 'foo'\nversion = 'not a version'\n",
    );
    let result = parse(&workspace);
    assert!(result.metadata.is_none());
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code.as_str() == "python-metadata")
        .unwrap();
    assert!(diagnostic.source.is_some());
    assert!(diagnostic.span.is_some());
}

#[test]
fn grammar_selection_respects_patch_constraints_and_explicit_fallback() {
    for (requirement, expected) in [
        (Some(">=3.11.2,!=3.12.*"), "3.11"),
        (Some("~=3.10.5"), "3.10"),
        (Some(">=3.12,<3.14"), "3.12"),
        (None, "3.7"),
    ] {
        let workspace = support::TestWorkspace::from_fixture("acceptance/python");
        let mut metadata = "[project]\nname = 'foo'\nversion = '1'\n".to_owned();
        if let Some(requirement) = requirement {
            metadata.push_str(&format!("requires-python = '{requirement}'\n"));
        }
        workspace.write("pyproject.toml", metadata);
        assert_eq!(parse(&workspace).metadata.unwrap().target_version, expected);
    }
}

#[test]
fn exports_retain_order_and_unrecognized_mutations() {
    use diplodocus::extractors::python::{ExportOperationKind as K, ExportValue};
    let workspace = support::TestWorkspace::from_fixture("acceptance/python");
    workspace.write("python/foo/__init__.py", "names = ['a']\n__all__ = names + ['b']\n__all__ += ('c',)\n__all__.append('d')\n__all__.extend(['e'])\n__all__[0] = 'f'\n__all__.sort()\nif condition:\n    __all__ = ['g']\n");
    let result = parse(&workspace);
    let module = result
        .modules
        .iter()
        .find(|module| module.name == "foo" && module.kind == PythonSourceKind::Source)
        .unwrap();
    assert_eq!(
        module.exports.iter().map(|op| op.kind).collect::<Vec<_>>(),
        [
            K::Assign,
            K::Extend,
            K::Append,
            K::Extend,
            K::Unsupported,
            K::Unsupported,
            K::Unsupported
        ]
    );
    assert!(matches!(module.exports[0].value, ExportValue::Concat(_)));
    assert!(
        module
            .declarations
            .iter()
            .any(|declaration| declaration.name == "names")
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_str() == "python-unsupported-syntax")
    );
}

#[test]
fn decoded_docstrings_have_only_proven_original_byte_mappings() {
    let workspace = support::TestWorkspace::from_fixture("acceptance/python");
    let text = "# π\ndef plain():\n    'café'\ndef escaped():\n    'a\\nb'\ndef joined():\n    ('left' 'right')\n";
    workspace.write("python/foo/docs.py", text);
    let result = parse(&workspace);
    let module = result
        .modules
        .iter()
        .find(|module| module.name == "foo.docs")
        .unwrap();
    let plain = module.declarations[0].docstring.as_ref().unwrap();
    assert_eq!(plain.text, "café");
    assert_eq!(plain.segments.len(), 1);
    let escaped = module.declarations[1].docstring.as_ref().unwrap();
    assert_eq!(escaped.text, "a\nb");
    assert!(escaped.segments.is_empty());
    let joined = module.declarations[2].docstring.as_ref().unwrap();
    assert_eq!(joined.text, "leftright");
    assert_eq!(joined.segments.len(), 2);
    for doc in [plain, escaped, joined] {
        for segment in &doc.segments {
            assert_eq!(
                &doc.text[segment.decoded.start..segment.decoded.end],
                &text[segment.source.start..segment.source.end]
            );
        }
    }
}

#[test]
fn signatures_preserve_calling_conventions_decorators_async_and_defaults() {
    let workspace = support::TestWorkspace::from_fixture("acceptance/python");
    workspace.write("python/foo/signature.py", "@decorate(flag=True)\nasync def run(a: int, /, b=0xA, *args: str, c: str='hi', **kwargs) -> list[str]:\n    raise RuntimeError('never run')\n");
    let result = parse(&workspace);
    let module = result
        .modules
        .iter()
        .find(|module| module.name == "foo.signature")
        .unwrap();
    let function = &module.declarations[0];
    assert!(function.is_async);
    assert_eq!(function.decorators.len(), 1);
    let Signature::Callable {
        parameters,
        returns,
    } = &function.signature.as_ref().unwrap().signature
    else {
        panic!("callable")
    };
    assert_eq!(
        parameters
            .iter()
            .map(|parameter| parameter.kind.clone())
            .collect::<Vec<_>>(),
        [
            ParameterKind::PositionalOnly,
            ParameterKind::PositionalOrKeyword,
            ParameterKind::VariadicPositional,
            ParameterKind::KeywordOnly,
            ParameterKind::VariadicKeyword
        ]
    );
    assert!(returns.is_some());
    assert_eq!(
        parameters[1].default.as_ref().unwrap(),
        &diplodocus::ir::SignatureExpression::Literal { text: "10".into() }
    );
}

#[test]
fn module_collisions_invalidate_both_variants_without_losing_inputs() {
    let workspace = support::TestWorkspace::from_fixture("acceptance/python");
    workspace.write("python/foo/model/__init__.py", "class Collision: pass\n");
    let result = parse(&workspace);
    let modules: Vec<_> = result
        .modules
        .iter()
        .filter(|module| module.name == "foo.model" && module.kind == PythonSourceKind::Source)
        .collect();
    assert_eq!(modules.len(), 2);
    assert!(
        modules
            .iter()
            .all(|module| !module.valid && module.declarations.is_empty())
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code.as_str() == "python-module-collision")
            .count(),
        2
    );
}

#[test]
fn invalid_utf8_retains_a_fingerprint_but_no_authoritative_module() {
    let workspace = support::TestWorkspace::from_fixture("acceptance/python");
    workspace.write("python/foo/invalid.py", [255u8]);
    let result = parse(&workspace);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_str() == "python-source-read")
    );
    let ProvenanceActivity::Extraction { inputs, .. } = result.provenance.activity else {
        panic!("extraction")
    };
    assert!(inputs["repo"].contains_key(&"python/foo/invalid.py".try_into().unwrap()));
    assert!(
        !result
            .modules
            .iter()
            .any(|module| module.name == "foo.invalid" && module.valid)
    );
}

#[cfg(unix)]
#[test]
fn source_symlinks_cannot_escape_the_package_boundary() {
    let workspace = support::TestWorkspace::from_fixture("acceptance/python");
    let outside = support::TestWorkspace::new();
    outside.write("outside.py", "SENSITIVE = 'outside'\n");
    std::os::unix::fs::symlink(
        outside.path().join("outside.py"),
        workspace.path().join("python/foo/escape.py"),
    )
    .unwrap();
    let result = parse(&workspace);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_str() == "source-path-outside-boundary")
    );
    assert!(
        !serde_json::to_string(&result)
            .unwrap()
            .contains("SENSITIVE")
    );
}

#[test]
fn import_aliases_normalize_callable_identity_inputs() {
    let workspace = support::TestWorkspace::from_fixture("acceptance/python");
    workspace.write("python/foo/aliases.py", "from collections.abc import Sequence as Seq\nfrom typing import Literal as Lit\ndef run(x: Seq[Lit['a']] = ()) -> Seq[int]: ...\n");
    let first = parse(&workspace);
    workspace.write("python/foo/aliases.py", "from collections.abc import Sequence\nfrom typing import Literal\ndef run(x: Sequence[Literal['a']] = ()) -> Sequence[int]: ...\n");
    let second = parse(&workspace);
    let signature = |result: &ParsedPythonPackage| {
        result
            .modules
            .iter()
            .find(|module| module.name == "foo.aliases")
            .unwrap()
            .declarations[0]
            .signature
            .as_ref()
            .unwrap()
            .signature
            .clone()
    };
    assert_eq!(signature(&first), signature(&second));
}

#[test]
fn source_adapter_matches_reviewed_acceptance_observations() {
    let workspace = support::TestWorkspace::from_fixture("acceptance/python");
    let mut result = parse(&workspace);
    support::normalize_build_tool_versions(&mut result.provenance.tools, &["diplodocus", "python"]);
    let inputs: Vec<_> = result
        .inputs
        .values()
        .map(|input| {
            serde_json::json!({
                "kind": input.kind, "source": input.source, "fingerprint": input.fingerprint,
            })
        })
        .collect();
    support::assert_json_golden(
        &serde_json::json!({
            "metadata": result.metadata, "inputs": inputs, "modules": result.modules,
            "diagnostics": result.diagnostics, "provenance": result.provenance,
        }),
        "python-source/acceptance.json",
    );
}

#[test]
fn local_types_are_qualified_without_breaking_property_accessors() {
    use diplodocus::ir::SignatureExpression as E;
    let workspace = support::TestWorkspace::from_fixture("acceptance/python");
    workspace.write("python/foo/local.py", "class Model:\n    @property\n    def value(self) -> float: ...\n    @value.setter\n    def value(self, x: float) -> None: ...\ndef f(x: Model) -> Model: ...\n");
    let result = parse(&workspace);
    let module = result
        .modules
        .iter()
        .find(|module| module.name == "foo.local")
        .unwrap();
    let Signature::Callable { returns, .. } =
        &module.declarations[1].signature.as_ref().unwrap().signature
    else {
        panic!("callable")
    };
    assert_eq!(
        returns.as_ref().unwrap(),
        &E::Name {
            name: "foo.local.Model".into(),
            target: None
        }
    );
    assert_eq!(
        module.declarations[0].members[1].decorators[0].expression,
        E::Name {
            name: "value.setter".into(),
            target: None
        }
    );
}

#[test]
fn empty_container_defaults_have_semantic_overload_identities() {
    use diplodocus::ir::{PythonIdentityKind, SemanticIdentity};
    let workspace = support::TestWorkspace::from_fixture("acceptance/python");
    workspace.write("python/foo/empty.py", "def f(x=(), y=[], z={}): ...\n");
    let result = parse(&workspace);
    let module = result
        .modules
        .iter()
        .find(|module| module.name == "foo.empty")
        .unwrap();
    let signature = &module.declarations[0].signature.as_ref().unwrap().signature;
    assert!(
        SemanticIdentity::python_overload("foo.empty.f", PythonIdentityKind::Function, signature)
            .is_ok()
    );
}

#[test]
fn rebinding_an_import_does_not_keep_its_decorator_semantics() {
    use diplodocus::ir::SignatureExpression as E;
    let workspace = support::TestWorkspace::from_fixture("acceptance/python");
    workspace.write(
        "python/foo/shadow.py",
        "from typing import overload\noverload = custom\n@overload\ndef f(x: int): ...\n",
    );
    let result = parse(&workspace);
    let module = result
        .modules
        .iter()
        .find(|module| module.name == "foo.shadow")
        .unwrap();
    assert_eq!(
        module.declarations[1].decorators[0].expression,
        E::Name {
            name: "foo.shadow.overload".into(),
            target: None
        }
    );
}

#[test]
fn provenance_records_only_invoked_parsers_for_unreadable_inputs() {
    let workspace = support::TestWorkspace::from_fixture("acceptance/python");
    workspace.write("pyproject.toml", [255u8]);
    for path in support::files_under(&workspace.path().join("python/foo")) {
        if matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("py" | "pyi")
        ) {
            workspace.write(std::path::Path::new("python/foo").join(path), [255u8]);
        }
    }
    let result = parse(&workspace);
    let ProvenanceActivity::Extraction {
        parsers, inputs, ..
    } = result.provenance.activity
    else {
        panic!("extraction")
    };
    assert!(parsers.is_empty());
    assert_eq!(inputs["repo"].len(), 8);
    assert!(
        inputs["repo"]
            .values()
            .all(|input| input.parsers.is_empty())
    );
}

#[test]
fn module_names_require_python_identifiers() {
    let workspace = support::TestWorkspace::from_fixture("acceptance/python");
    workspace.write("python/foo/123bad.py", "x = 1\n");
    workspace.write("python/foo/emoji😀.py", "x = 1\n");
    workspace.write("python/foo/café.py", "x = 1\n");
    let result = parse(&workspace);
    assert!(
        !result
            .modules
            .iter()
            .any(|module| module.name == "foo.123bad" || module.name == "foo.emoji😀")
    );
    assert!(
        result
            .modules
            .iter()
            .any(|module| module.name == "foo.café" && module.valid)
    );
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code.as_str() == "python-source-read")
            .count(),
        2
    );
}

#[test]
fn source_loading_does_not_execute_top_level_statements() {
    let workspace = support::TestWorkspace::from_fixture("acceptance/python");
    workspace.write(
        "python/foo/inert.py",
        "raise RuntimeError('must never execute')\nimport missing_native_module\ndef api(): ...\n",
    );
    let result = parse(&workspace);
    assert!(result.diagnostics.is_empty());
    assert!(
        result
            .modules
            .iter()
            .any(|module| module.name == "foo.inert" && module.valid)
    );
}

#[test]
fn failure_diagnostics_match_their_portable_golden() {
    let mut cases = std::collections::BTreeMap::new();
    for (name, path, text) in [
        (
            "metadata",
            "pyproject.toml",
            "[project]\nname = 'foo'\nversion = 'not a version'\n",
        ),
        (
            "dynamic-metadata",
            "pyproject.toml",
            "[project]\nname = 'foo'\ndynamic = ['version', 'description']\n",
        ),
        ("syntax", "python/foo/model.py", "def broken(:\n    pass\n"),
        ("version", "python/foo/new.py", "type Alias = int\n"),
        (
            "conditional",
            "python/foo/new.py",
            "if platform_check():\n    def api(): ...\n",
        ),
    ] {
        let workspace = support::TestWorkspace::from_fixture("acceptance/python");
        workspace.write(path, text);
        cases.insert(name, parse(&workspace).diagnostics);
    }
    support::assert_json_golden(&cases, "python-source/failures.json");
}

#[test]
fn invalid_metadata_does_not_invent_a_declared_python_version() {
    let workspace = support::TestWorkspace::from_fixture("acceptance/python");
    workspace.write(
        "pyproject.toml",
        "[project]\nname = 'foo'\nversion = 'invalid'\nrequires-python = '>=3.11'\n",
    );
    let result = parse(&workspace);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_str() == "python-unsupported-version")
    );
    assert!(
        result
            .modules
            .iter()
            .all(|module| !module.valid && module.declarations.is_empty())
    );
}

#[test]
fn dynamic_version_does_not_discard_a_known_grammar_requirement() {
    let workspace = support::TestWorkspace::from_fixture("acceptance/python");
    workspace.write(
        "pyproject.toml",
        "[project]\nname = 'foo'\ndynamic = ['version']\nrequires-python = '>=3.12'\n",
    );
    workspace.write("python/foo/new.py", "type Alias = int\n");
    let result = parse(&workspace);
    assert!(result.metadata.is_none());
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_str() == "python-unsupported-version")
    );
    let ProvenanceActivity::Extraction { parsers, .. } = result.provenance.activity else {
        panic!("extraction")
    };
    assert_eq!(
        parsers["ruff_python_parser"].settings["target_version"],
        "3.12"
    );
}
