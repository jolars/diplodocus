use std::collections::BTreeMap;

// The integration coordinator registers this module after merging prerequisites.
mod model {
    pub use diplodocus::extractors::python::*;
}
#[path = "../src/extractors/python/surface.rs"]
mod surface;
use surface::PythonSurface;

use diplodocus::{diagnostics, ir, paths};
use ir::*;

use model::*;

mod support;

fn reconcile(package: &ParsedPythonPackage) -> PythonSurface {
    let result = surface::reconcile(package);
    for item in result.items.values() {
        let mut references: Vec<&str> = item.children.iter().map(String::as_str).collect();
        match &data(item).declaration {
            PythonDeclaration::Callable {
                role: PythonCallableRole::Family { overloads },
                ..
            } => references.extend(overloads.iter().map(|r| r.item.as_str())),
            PythonDeclaration::Callable {
                role: PythonCallableRole::Overload { family },
                ..
            } => references.push(&family.item),
            PythonDeclaration::Class {
                constructor: PythonConstructor::Dataclass { fields, .. },
                ..
            } => references.extend(fields.iter().map(|r| r.item.as_str())),
            PythonDeclaration::Class {
                constructor: PythonConstructor::Explicit { item },
                ..
            } => references.push(&item.item),
            _ => {}
        }
        for reference in references {
            assert!(
                result.items.contains_key(reference),
                "{} has dangling reference {reference}",
                item.qualified_name
            );
        }
    }
    result
}

fn location(path: &str, start: usize) -> SourceLocation {
    SourceLocation {
        repository: "python".into(),
        path: path.try_into().unwrap(),
        span: Some(SourceSpan {
            start,
            end: start + 5,
        }),
    }
}

fn module(name: &str, kind: PythonSourceKind) -> ParsedModule {
    ParsedModule {
        name: name.into(),
        is_package: name == "foo",
        kind,
        source: location(
            &format!(
                "{}.{}",
                name.replace('.', "/"),
                if kind == PythonSourceKind::Stub {
                    "pyi"
                } else {
                    "py"
                }
            ),
            0,
        ),
        docstring: None,
        declarations: vec![],
        imports: vec![],
        exports: vec![],
        valid: true,
    }
}

fn package(modules: Vec<ParsedModule>) -> ParsedPythonPackage {
    ParsedPythonPackage {
        package: "pyfoo".into(),
        target: TargetReference {
            package: "pyfoo".into(),
            target: "api".into(),
        },
        metadata: None,
        modules,
        inputs: BTreeMap::new(),
        diagnostics: vec![],
        provenance: Provenance {
            activity: ProvenanceActivity::Declaration,
            source: None,
            span: None,
            tools: BTreeMap::new(),
        },
    }
}

fn function(module: &ParsedModule, name: &str, annotation: &str) -> ParsedDeclaration {
    let source = SourceLocation {
        span: Some(SourceSpan { start: 10, end: 30 }),
        ..module.source.clone()
    };
    ParsedDeclaration {
        name: name.into(),
        kind: DeclarationKind::Function,
        source: source.clone(),
        signature: Some(SourcedSignature {
            signature: Signature::Callable {
                parameters: vec![],
                returns: Some(SignatureExpression::Name {
                    name: annotation.into(),
                    target: None,
                }),
            },
            sources: vec![SourceEvidence {
                source,
                role: SourceRole::Signature,
                parsers: ["ruff_python_parser".into()].into(),
            }],
        }),
        decorators: vec![],
        is_async: false,
        bases: vec![],
        members: vec![],
        docstring: None,
    }
}

fn all(module: &mut ParsedModule, names: &[&str]) {
    module.exports.push(ExportOperation {
        kind: ExportOperationKind::Assign,
        value: ExportValue::Names(names.iter().map(|s| s.to_string()).collect()),
        source: module.source.clone(),
    });
}

fn import(module: &mut ParsedModule, from: &str, name: &str, alias: Option<&str>, level: u32) {
    module.imports.push(ParsedImport {
        module: Some(from.into()),
        level,
        names: vec![ImportedName {
            name: name.into(),
            alias: alias.map(str::to_string),
        }],
        source: module.source.clone(),
    });
}

fn callable_id(name: &str) -> String {
    SemanticIdentity::python(name, PythonIdentityKind::Function)
        .unwrap()
        .item_id()
        .into()
}

fn data(item: &Item) -> &PythonItemData {
    let Some(ItemLanguageData::Python(data)) = &item.language_data else {
        panic!("missing Python data")
    };
    data
}

#[test]
fn literal_exports_are_authoritative_including_empty_and_private_names() {
    let mut m = module("foo", PythonSourceKind::Source);
    m.declarations = vec![
        function(&m, "visible", "int"),
        function(&m, "_selected", "str"),
        function(&m, "omitted", "int"),
    ];
    all(&mut m, &["visible", "_selected"]);
    let result = reconcile(&package(vec![m.clone()]));
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert!(result.items.contains_key(&callable_id("foo.visible")));
    assert!(result.items.contains_key(&callable_id("foo._selected")));
    assert!(!result.items.contains_key(&callable_id("foo.omitted")));
    m.exports.clear();
    all(&mut m, &[]);
    let result = reconcile(&package(vec![m]));
    assert_eq!(result.items.len(), 1);
}

#[test]
fn implicit_surface_excludes_private_definitions_and_includes_explicit_imports() {
    let mut root = module("foo", PythonSourceKind::Source);
    let mut implementation = module("foo._impl", PythonSourceKind::Source);
    implementation.declarations = vec![function(&implementation, "_f", "int")];
    root.declarations = vec![
        function(&root, "visible", "str"),
        function(&root, "_hidden", "str"),
    ];
    import(&mut root, "_impl", "_f", Some("f"), 1);
    import(&mut root, "typing", "Any", None, 0);
    let result = reconcile(&package(vec![root, implementation]));
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert!(!result.items.contains_key(&callable_id("foo._hidden")));
    let f = &result.items[&callable_id("foo._impl._f")];
    assert_eq!(f.aliases[0].qualified_name, "foo.f");
    assert_eq!(data(f).visibility, PythonVisibility::Public);
}

#[test]
fn relative_transitive_reexports_share_identity_and_keep_each_import_source() {
    let mut root = module("foo", PythonSourceKind::Source);
    let mut stub = module("foo", PythonSourceKind::Stub);
    let mut facade = module("foo.api", PythonSourceKind::Source);
    let mut implementation = module("foo.model", PythonSourceKind::Source);
    implementation
        .declarations
        .push(function(&implementation, "fit", "int"));
    import(&mut root, "api", "fit", Some("fit"), 1);
    import(&mut stub, "api", "fit", Some("fit"), 1);
    import(&mut facade, "model", "fit", Some("fit"), 1);
    let result = reconcile(&package(vec![root, stub, facade, implementation]));
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let f = &result.items[&callable_id("foo.model.fit")];
    assert_eq!(
        f.aliases
            .iter()
            .map(|a| a.qualified_name.as_str())
            .collect::<Vec<_>>(),
        ["foo.api.fit", "foo.fit"]
    );
    assert_eq!(f.aliases[1].sources.len(), 2);
    assert_eq!(
        result
            .items
            .values()
            .filter(|i| i.kind == ItemKind::Function)
            .count(),
        1
    );
    support::assert_json_golden(&result, "python-surface/reexports.json");
}

#[test]
fn unresolved_and_cyclic_exports_are_diagnosed_without_invented_items() {
    let mut a = module("foo.a", PythonSourceKind::Source);
    let mut b = module("foo.b", PythonSourceKind::Source);
    import(&mut a, "b", "f", Some("f"), 1);
    import(&mut b, "a", "f", Some("f"), 1);
    import(&mut a, "missing", "x", Some("x"), 1);
    let mut p = package(vec![a, b]);
    let first = reconcile(&p);
    assert_eq!(first.items.len(), 2);
    assert_eq!(first.diagnostics.len(), 3);
    assert!(
        first
            .diagnostics
            .iter()
            .all(|d| d.code.as_str() == "python-unresolved-reexport")
    );
    p.modules.reverse();
    assert_eq!(first, reconcile(&p));
}

#[test]
fn dynamic_exports_retain_visible_definitions_with_unknown_visibility() {
    let mut m = module("foo.experimental", PythonSourceKind::Source);
    m.declarations.push(function(&m, "visible", "int"));
    m.exports.push(ExportOperation {
        kind: ExportOperationKind::Assign,
        value: ExportValue::Unsupported("discover()".into()),
        source: m.source.clone(),
    });
    let result = reconcile(&package(vec![m]));
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.diagnostics[0].code.as_str(), "python-dynamic-export");
    assert_eq!(
        result.diagnostics[0].severity,
        diagnostics::Severity::Warning
    );
    assert_eq!(
        data(&result.items[&callable_id("foo.experimental.visible")]).visibility,
        PythonVisibility::Unknown
    );
}

#[test]
fn maintained_stubs_supply_signatures_and_source_supplies_documentation() {
    let mut source = module("foo", PythonSourceKind::Source);
    let mut stub = module("foo", PythonSourceKind::Stub);
    let mut original = function(&source, "f", "object");
    original.docstring = Some(ParsedDocstring {
        text: "Implementation prose.".into(),
        source: original.source.clone(),
        segments: vec![],
    });
    source.declarations = vec![original, function(&source, "source_only", "int")];
    stub.declarations = vec![function(&stub, "f", "str")];
    let result = reconcile(&package(vec![source, stub]));
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let f = &result.items[&callable_id("foo.f")];
    assert_eq!(f.signatures[0].sources[0].source.path.as_str(), "foo.pyi");
    assert_eq!(f.source_location.as_ref().unwrap().path.as_str(), "foo.py");
    assert_eq!(
        result.docstrings[&callable_id("foo.f")].text,
        "Implementation prose."
    );
    assert!(!result.items.contains_key(&callable_id("foo.source_only")));
}

#[test]
fn stub_only_overloads_have_stable_addressable_ids_under_reordering() {
    let mut stub = module("foo.native", PythonSourceKind::Stub);
    import(&mut stub, "typing", "overload", None, 0);
    for result in ["int", "str"] {
        let mut declaration = function(&stub, "f", result);
        declaration.decorators.push(ParsedDecorator {
            expression: SignatureExpression::Name {
                name: "overload".into(),
                target: None,
            },
            source: declaration.source.clone(),
        });
        stub.declarations.push(declaration);
    }
    let first = reconcile(&package(vec![stub.clone()]));
    assert!(first.diagnostics.is_empty(), "{:?}", first.diagnostics);
    let family = &first.items[&callable_id("foo.native.f")];
    let PythonDeclaration::Callable {
        role: PythonCallableRole::Family { overloads },
        ..
    } = &data(family).declaration
    else {
        panic!("missing family")
    };
    assert_eq!(overloads.len(), 2);
    for member in overloads {
        assert!(first.items.contains_key(&member.item));
    }
    stub.declarations.reverse();
    let second = reconcile(&package(vec![stub]));
    assert_eq!(
        first.items.keys().collect::<Vec<_>>(),
        second.items.keys().collect::<Vec<_>>()
    );
    support::assert_json_golden(&first, "python-surface/overloads.json");
}

#[test]
fn conflicting_stub_declaration_kinds_fail_visibly() {
    let mut source = module("foo", PythonSourceKind::Source);
    let mut stub = module("foo", PythonSourceKind::Stub);
    source.declarations.push(function(&source, "f", "str"));
    let mut conflicting = function(&stub, "f", "str");
    conflicting.kind = DeclarationKind::Class;
    conflicting.signature = None;
    stub.declarations.push(conflicting);
    let result = reconcile(&package(vec![source, stub]));
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "python-conflicting-stub")
    );
    assert!(!result.items.values().any(|item| item.name == "f"));
}

#[test]
fn static_export_operations_evaluate_helpers_and_reject_unknown_mutations() {
    let mut m = module("foo", PythonSourceKind::Source);
    m.declarations = vec![
        function(&m, "a", "int"),
        function(&m, "b", "int"),
        function(&m, "c", "int"),
    ];
    let mut helper = function(&m, "_names", "int");
    helper.kind = DeclarationKind::Assignment;
    helper.source.span = Some(SourceSpan { start: 1, end: 9 });
    helper.signature = Some(SourcedSignature {
        signature: Signature::Value {
            annotation: None,
            value: Some(SignatureExpression::LanguageSpecific {
                language: "python".into(),
                name: "list".into(),
                children: vec![SignatureExpression::Literal {
                    text: "\"a\"".into(),
                }],
                source: None,
            }),
        },
        sources: vec![],
    });
    m.declarations.push(helper);
    for (start, kind, value) in [
        (
            50,
            ExportOperationKind::Assign,
            ExportValue::Name("_names".into()),
        ),
        (
            60,
            ExportOperationKind::Extend,
            ExportValue::Concat(vec![
                ExportValue::Names(vec!["b".into()]),
                ExportValue::Names(vec![]),
            ]),
        ),
        (
            70,
            ExportOperationKind::Append,
            ExportValue::Names(vec!["c".into()]),
        ),
    ] {
        m.exports.push(ExportOperation {
            kind,
            value,
            source: SourceLocation {
                span: Some(SourceSpan {
                    start,
                    end: start + 5,
                }),
                ..m.source.clone()
            },
        });
    }
    let first = reconcile(&package(vec![m.clone()]));
    assert!(first.diagnostics.is_empty(), "{:?}", first.diagnostics);
    assert_eq!(first.items.len(), 4);
    m.exports.push(ExportOperation {
        kind: ExportOperationKind::Unsupported,
        value: ExportValue::Unsupported("__all__.sort(key=dynamic)".into()),
        source: location("foo.py", 80),
    });
    let result = reconcile(&package(vec![m]));
    assert_eq!(result.diagnostics[0].code.as_str(), "python-dynamic-export");
}

#[test]
fn module_imports_and_nested_relative_imports_resolve_without_duplicate_modules() {
    let root = module("foo", PythonSourceKind::Source);
    let mut nested = module("foo.nested.api", PythonSourceKind::Source);
    let model = module("foo.model", PythonSourceKind::Source);
    import(&mut nested, "", "model", Some("model"), 2);
    let result = reconcile(&package(vec![root, nested, model]));
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let id = SemanticIdentity::python("foo.model", PythonIdentityKind::Module).unwrap();
    assert_eq!(
        result.items[id.item_id()].aliases[0].qualified_name,
        "foo.nested.api.model"
    );
    assert_eq!(result.items.len(), 3);
}

#[test]
fn duplicate_overloads_and_opaque_overload_signatures_are_diagnosed() {
    let mut m = module("foo", PythonSourceKind::Stub);
    import(&mut m, "typing", "overload", Some("overload"), 0);
    all(&mut m, &["f"]);
    let mut f = function(&m, "f", "str");
    f.decorators.push(ParsedDecorator {
        expression: SignatureExpression::Name {
            name: "overload".into(),
            target: None,
        },
        source: f.source.clone(),
    });
    m.declarations = vec![f.clone(), f.clone()];
    let result = reconcile(&package(vec![m.clone()]));
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "python-duplicate-identity")
    );
    if let Signature::Callable { returns, .. } = &mut f.signature.as_mut().unwrap().signature {
        *returns = Some(SignatureExpression::LanguageSpecific {
            language: "python".into(),
            name: "unsupported".into(),
            children: vec![],
            source: Some("dynamic!".into()),
        });
    }
    m.declarations = vec![f];
    let result = reconcile(&package(vec![m]));
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "python-invalid-identity")
    );
}

#[test]
fn conflicting_aliases_never_select_an_import_order_winner() {
    let mut root = module("foo", PythonSourceKind::Source);
    let mut left = module("foo.left", PythonSourceKind::Source);
    let mut right = module("foo.right", PythonSourceKind::Source);
    left.declarations.push(function(&left, "f", "int"));
    right.declarations.push(function(&right, "f", "str"));
    import(&mut root, "left", "f", Some("f"), 1);
    import(&mut root, "right", "f", Some("f"), 1);
    let first = reconcile(&package(vec![root.clone(), left.clone(), right.clone()]));
    root.imports.reverse();
    let second = reconcile(&package(vec![right, left, root]));
    assert_eq!(first, second);
    assert_eq!(first.diagnostics.len(), 1);
    assert_eq!(
        first.diagnostics[0].code.as_str(),
        "python-conflicting-alias"
    );
    assert!(first.items.values().all(|item| item.aliases.is_empty()));
}

#[test]
fn unknown_decorators_and_wildcard_imports_have_visible_errors() {
    let mut m = module("foo", PythonSourceKind::Source);
    let mut f = function(&m, "f", "int");
    f.decorators.push(ParsedDecorator {
        expression: SignatureExpression::Name {
            name: "runtime_wrapper".into(),
            target: None,
        },
        source: f.source.clone(),
    });
    m.declarations.push(f);
    import(&mut m, "unknown", "*", None, 0);
    let result = reconcile(&package(vec![m]));
    assert_eq!(result.diagnostics.len(), 2);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.code.as_str() == "python-unsupported-surface")
    );
}

#[test]
fn malformed_modules_contribute_no_surface() {
    let mut m = module("foo", PythonSourceKind::Source);
    m.declarations.push(function(&m, "f", "int"));
    m.valid = false;
    assert!(reconcile(&package(vec![m])).items.is_empty());
}

#[test]
fn class_members_keep_source_dataclass_facts_and_reexport_aliases() {
    let mut source = module("foo.model", PythonSourceKind::Source);
    let mut stub = module("foo.model", PythonSourceKind::Stub);
    let mut root = module("foo", PythonSourceKind::Source);
    import(&mut source, "dataclasses", "dataclass", None, 0);
    let mut class = function(&source, "Model", "int");
    class.kind = DeclarationKind::Class;
    class.signature = None;
    class.decorators.push(ParsedDecorator {
        expression: SignatureExpression::LanguageSpecific {
            language: "python".into(),
            name: "call".into(),
            children: vec![
                SignatureExpression::Name {
                    name: "dataclass".into(),
                    target: None,
                },
                SignatureExpression::LanguageSpecific {
                    language: "python".into(),
                    name: "keyword:frozen".into(),
                    children: vec![SignatureExpression::Literal {
                        text: "True".into(),
                    }],
                    source: None,
                },
            ],
            source: Some("dataclass(frozen=True)".into()),
        },
        source: class.source.clone(),
    });
    let mut maintained = class.clone();
    maintained.source = stub.source.clone();
    maintained.decorators.clear();
    for name in ["z", "a"] {
        let mut field = function(&stub, name, "int");
        field.kind = DeclarationKind::Assignment;
        field.signature = Some(SourcedSignature {
            signature: Signature::Value {
                annotation: Some(SignatureExpression::Name {
                    name: "int".into(),
                    target: None,
                }),
                value: None,
            },
            sources: vec![],
        });
        maintained.members.push(field);
    }
    let mut method = function(&stub, "predict", "float");
    method.decorators.push(ParsedDecorator {
        expression: SignatureExpression::Name {
            name: "staticmethod".into(),
            target: None,
        },
        source: method.source.clone(),
    });
    maintained.members.push(method);
    source.declarations.push(class);
    stub.declarations.push(maintained);
    import(&mut root, "model", "Model", Some("Model"), 1);
    let result = reconcile(&package(vec![root, source, stub]));
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let id = SemanticIdentity::python("foo.model.Model", PythonIdentityKind::Class).unwrap();
    let class = &result.items[id.item_id()];
    let PythonDeclaration::Class {
        constructor:
            PythonConstructor::Dataclass {
                fields,
                init,
                frozen,
            },
        ..
    } = &data(class).declaration
    else {
        panic!("dataclass source facts were lost")
    };
    assert!(*init && *frozen);
    assert_eq!(
        fields
            .iter()
            .map(|r| result.items[&r.item].name.as_str())
            .collect::<Vec<_>>(),
        ["z", "a"]
    );
    let method =
        SemanticIdentity::python("foo.model.Model.predict", PythonIdentityKind::Method).unwrap();
    assert_eq!(
        result.items[method.item_id()].aliases[0].qualified_name,
        "foo.Model.predict"
    );
}

#[test]
fn proven_assignments_are_aliases_but_named_type_aliases_are_entities() {
    let mut m = module("foo", PythonSourceKind::Source);
    let f = function(&m, "f", "int");
    let mut alias = function(&m, "other", "int");
    alias.kind = DeclarationKind::Assignment;
    alias.signature = Some(SourcedSignature {
        signature: Signature::Value {
            annotation: None,
            value: Some(SignatureExpression::Name {
                name: "f".into(),
                target: None,
            }),
        },
        sources: vec![],
    });
    let mut named_type = alias.clone();
    named_type.name = "Alias".into();
    named_type.kind = DeclarationKind::TypeAlias;
    m.declarations = vec![f, alias, named_type];
    let result = reconcile(&package(vec![m]));
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let f = &result.items[&callable_id("foo.f")];
    assert_eq!(f.aliases[0].qualified_name, "foo.other");
    assert_eq!(f.aliases[0].kind, ItemAliasKind::PythonAssignment);
    assert!(
        result.items.contains_key(
            SemanticIdentity::python("foo.Alias", PythonIdentityKind::TypeAlias)
                .unwrap()
                .item_id()
        )
    );
}

fn parse_fixture(root: &std::path::Path) -> ParsedPythonPackage {
    let path = root.canonicalize().unwrap();
    let repository = paths::ResolvedRepositoryPaths {
        id: "python".into(),
        path: path.clone(),
    };
    let target = paths::ResolvedTargetPath {
        id: "api".into(),
        path: path.join("python/foo"),
    };
    let package = paths::ResolvedPackagePaths {
        id: "pyfoo".into(),
        repository_index: 0,
        path: path.clone(),
        metadata_path: path.join("pyproject.toml"),
        targets: vec![target.clone()],
    };
    model::parse_target(&repository, &package, &target)
}

#[test]
fn acceptance_fixture_has_one_canonical_surface_and_four_addressable_overloads() {
    let parsed = parse_fixture(&support::fixture_path("acceptance/python"));
    let result = reconcile(&parsed);
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    assert_eq!(
        result
            .items
            .values()
            .filter(|item| matches!(
                data(item).declaration,
                PythonDeclaration::Callable {
                    role: PythonCallableRole::Overload { .. },
                    ..
                }
            ))
            .count(),
        4
    );
    let fit = &result.items[&callable_id("foo.model.fit")];
    assert!(
        fit.aliases
            .iter()
            .any(|a| a.qualified_name == "foo.fit" && a.sources.len() == 2)
    );
    assert!(fit.signatures.iter().all(|s| {
        s.sources
            .iter()
            .any(|e| e.source.path.as_str() == "python/foo/model.pyi")
    }));
    assert_eq!(
        result.docstrings[&callable_id("foo.model.fit")]
            .source
            .path
            .as_str(),
        "python/foo/model.py"
    );
    assert!(
        result
            .items
            .contains_key(&callable_id("foo._native.native_mean"))
    );
    assert!(!result.items.values().any(|i| i.name == "__version__"));
    support::assert_json_golden(&result, "python-surface/acceptance.json");
}

#[test]
fn acceptance_dynamic_exports_and_relocation_are_deterministic() {
    let workspace = support::acceptance_case("python-dynamic-export");
    let mut parsed = parse_fixture(&workspace.path().join("python"));
    let first = reconcile(&parsed);
    assert_eq!(first.diagnostics.len(), 1, "{:?}", first.diagnostics);
    assert_eq!(first.diagnostics[0].code.as_str(), "python-dynamic-export");
    let function = &first.items[&callable_id("foo.experimental.experimental_rank")];
    assert_eq!(data(function).visibility, PythonVisibility::Unknown);
    assert!(function.aliases.is_empty());
    parsed.modules.reverse();
    assert_eq!(first, reconcile(&parsed));
    let relocated = support::acceptance_case("python-dynamic-export");
    assert_eq!(
        first,
        reconcile(&parse_fixture(&relocated.path().join("python")))
    );
    support::assert_json_golden(
        &first.diagnostics,
        "python-surface/dynamic-diagnostics.json",
    );
}

#[test]
fn module_alias_members_resolve_transitively() {
    let mut root = module("foo", PythonSourceKind::Source);
    let mut implementation = module("foo.model", PythonSourceKind::Source);
    let mut facade = module("foo.facade", PythonSourceKind::Source);
    implementation
        .declarations
        .push(function(&implementation, "f", "int"));
    root.imports.push(ParsedImport {
        module: None,
        level: 0,
        names: vec![ImportedName {
            name: "foo.model".into(),
            alias: Some("api".into()),
        }],
        source: root.source.clone(),
    });
    import(&mut facade, "foo.api", "f", Some("g"), 0);
    let result = reconcile(&package(vec![root, implementation, facade]));
    assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
    let item = &result.items[&callable_id("foo.model.f")];
    assert_eq!(
        item.aliases
            .iter()
            .map(|a| a.qualified_name.as_str())
            .collect::<Vec<_>>(),
        ["foo.api.f", "foo.facade.g"]
    );
}

#[test]
fn incompatible_stub_callable_binding_is_diagnosed() {
    let mut source = module("foo", PythonSourceKind::Source);
    let mut stub = module("foo", PythonSourceKind::Stub);
    let mut original = function(&source, "C", "int");
    original.kind = DeclarationKind::Class;
    original.signature = None;
    original.members.push(function(&source, "f", "int"));
    let mut maintained = original.clone();
    maintained.source = stub.source.clone();
    maintained.members[0].decorators.push(ParsedDecorator {
        expression: SignatureExpression::Name {
            name: "staticmethod".into(),
            target: None,
        },
        source: stub.source.clone(),
    });
    source.declarations.push(original);
    stub.declarations.push(maintained);
    let result = reconcile(&package(vec![source, stub]));
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "python-conflicting-stub")
    );
}

#[test]
fn a_stub_cannot_hide_ambiguous_implementation_definitions() {
    let mut source = module("foo", PythonSourceKind::Source);
    let mut stub = module("foo", PythonSourceKind::Stub);
    source.declarations = vec![function(&source, "f", "int"), function(&source, "f", "str")];
    stub.declarations = vec![function(&stub, "f", "str")];
    let result = reconcile(&package(vec![source, stub]));
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "python-conflicting-stub")
    );
    assert!(!result.items.contains_key(&callable_id("foo.f")));
}

#[test]
fn private_modules_need_public_reexports_to_expose_their_members() {
    let root = module("foo", PythonSourceKind::Source);
    let mut private = module("foo._private", PythonSourceKind::Source);
    private.declarations = vec![
        function(&private, "f", "int"),
        function(&private, "unrelated", "int"),
    ];
    let hidden = reconcile(&package(vec![root.clone(), private.clone()]));
    assert!(!hidden.items.contains_key(&callable_id("foo._private.f")));
    let mut root = root;
    import(&mut root, "_private", "f", Some("f"), 1);
    let exported = reconcile(&package(vec![root.clone(), private.clone()]));
    assert!(exported.items.contains_key(&callable_id("foo._private.f")));
    assert!(
        !exported
            .items
            .contains_key(&callable_id("foo._private.unrelated"))
    );
    let module_id = SemanticIdentity::python("foo._private", PythonIdentityKind::Module).unwrap();
    assert_eq!(
        data(&exported.items[module_id.item_id()]).visibility,
        PythonVisibility::Public
    );
    root.imports.clear();
    import(&mut root, "", "_private", Some("api"), 1);
    let exported_module = reconcile(&package(vec![root, private]));
    assert!(
        exported_module.items[&callable_id("foo._private.unrelated")]
            .aliases
            .iter()
            .any(|a| a.qualified_name == "foo.api.unrelated")
    );
}

#[test]
fn dataclass_private_fields_remain_addressable_without_public_aliases() {
    let mut m = module("foo", PythonSourceKind::Source);
    import(&mut m, "dataclasses", "dataclass", None, 0);
    let mut class = function(&m, "C", "int");
    class.kind = DeclarationKind::Class;
    class.signature = None;
    class.decorators.push(ParsedDecorator {
        expression: SignatureExpression::Name {
            name: "dataclass".into(),
            target: None,
        },
        source: class.source.clone(),
    });
    let mut field = function(&m, "_value", "int");
    field.kind = DeclarationKind::Assignment;
    field.signature = Some(SourcedSignature {
        signature: Signature::Value {
            annotation: Some(SignatureExpression::Name {
                name: "int".into(),
                target: None,
            }),
            value: None,
        },
        sources: vec![],
    });
    class.members.push(field);
    m.declarations.push(class);
    let result = reconcile(&package(vec![m]));
    let class = &result.items[SemanticIdentity::python("foo.C", PythonIdentityKind::Class)
        .unwrap()
        .item_id()];
    let PythonDeclaration::Class {
        constructor: PythonConstructor::Dataclass { fields, .. },
        ..
    } = &data(class).declaration
    else {
        panic!("missing constructor")
    };
    assert!(result.items.contains_key(&fields[0].item));
    assert_eq!(
        data(&result.items[&fields[0].item]).visibility,
        PythonVisibility::Private
    );
    assert!(result.items[&fields[0].item].aliases.is_empty());
}

#[test]
fn nested_module_alias_collision_with_canonical_name_is_diagnosed() {
    let mut root = module("foo", PythonSourceKind::Source);
    import(&mut root, "", "model", Some("api"), 1);
    let mut implementation = module("foo.model", PythonSourceKind::Source);
    implementation
        .declarations
        .push(function(&implementation, "f", "int"));
    let other = module("foo.api.f", PythonSourceKind::Source);
    let first = reconcile(&package(vec![
        root.clone(),
        implementation.clone(),
        other.clone(),
    ]));
    assert!(
        first
            .diagnostics
            .iter()
            .any(|d| d.code.as_str() == "python-conflicting-alias")
    );
    assert!(
        first.items[&callable_id("foo.model.f")]
            .aliases
            .iter()
            .all(|a| a.qualified_name != "foo.api.f")
    );
    assert_eq!(
        first,
        reconcile(&package(vec![other, implementation, root]))
    );
}

#[test]
fn invalid_export_operation_shapes_stay_dynamic_through_the_real_parser() {
    for expression in [
        "__all__ = 'f'",
        "__all__ = []; __all__.append(['f'])",
        "__all__ = []; __all__.extend('f')",
    ] {
        let workspace = support::TestWorkspace::new();
        workspace.write(
            "pyproject.toml",
            "[project]\nname = 'foo'\nversion = '1.0'\nrequires-python = '>=3.11'\n",
        );
        workspace.write(
            "python/foo/__init__.py",
            format!("def f(): ...\n{expression}\n"),
        );
        let result = reconcile(&parse_fixture(workspace.path()));
        assert_eq!(result.diagnostics.len(), 1, "{:?}", result.diagnostics);
        assert_eq!(result.diagnostics[0].code.as_str(), "python-dynamic-export");
        assert_eq!(
            data(&result.items[&callable_id("foo.f")]).visibility,
            PythonVisibility::Unknown
        );
    }
}
