use std::collections::BTreeMap;

use diplodocus::ir::{
    IdentityRegistry, Item, ItemLanguageData, ItemReference, PythonDeclaration, PythonIdentityKind,
    RDeclaration, RGenericReference, SemanticIdentity, Signature,
};
use serde_json::json;

mod support;

fn item(language_data: serde_json::Value) -> serde_json::Value {
    json!({"kind": "function", "name": "fit", "qualified_name": "foo.model.fit",
        "signatures": [], "documentation": null, "source_location": null,
        "children": [], "provenance": [], "language_data": language_data})
}

#[test]
fn typed_items_preserve_callables_and_dispatch_without_flattening() {
    let family: Item = serde_json::from_value(item(json!({"language": "python", "data": {
        "visibility": "public", "decorators": [],
        "declaration": {"kind": "callable", "binding": "function", "is_async": false,
            "role": {"kind": "family", "overloads": [{"package": "pyfoo", "item": "overload-one"}, {"package": "pyfoo", "item": "overload-two"}]}}
    }}))).unwrap();
    assert!(
        matches!(family.language_data, Some(ItemLanguageData::Python(ref data)) if matches!(data.declaration, PythonDeclaration::Callable { .. }))
    );
    let method: Item = serde_json::from_value(item(json!({"language": "r", "data": {
        "exported": false, "declaration": {"kind": "s3-method", "generic": {"kind": "external", "package": "stats", "name": "predict"}, "class": "foo_model", "registration": []}
    }}))).unwrap();
    assert!(
        matches!(method.language_data, Some(ItemLanguageData::R(ref data)) if matches!(data.declaration, RDeclaration::S3Method { .. }))
    );
    let mut unknown = serde_json::to_value(family).unwrap();
    unknown["language_data"]["data"]["declaration"]["invented"] = json!(true);
    assert!(serde_json::from_value::<Item>(unknown).is_err());
}

fn name(value: &str) -> serde_json::Value {
    json!({"kind": "name", "name": value, "target": null})
}

fn apply(value: &str, arguments: Vec<serde_json::Value>) -> serde_json::Value {
    json!({"kind": "apply", "constructor": name(value), "arguments": arguments})
}

fn source(repository: &str, path: &str, role: &str) -> serde_json::Value {
    json!({"source": {"repository": repository, "path": path, "span": null}, "role": role, "parsers": []})
}

fn python(declaration: serde_json::Value) -> serde_json::Value {
    json!({"language": "python", "data": {"visibility": "public", "declaration": declaration, "decorators": []}})
}

fn r(declaration: serde_json::Value, exported: bool) -> serde_json::Value {
    json!({"language": "r", "data": {"exported": exported, "declaration": declaration}})
}

fn declaration(kind: &str, qualified_name: &str, data: serde_json::Value) -> Item {
    let mut value = item(data);
    value["kind"] = json!(kind);
    value["qualified_name"] = json!(qualified_name);
    value["name"] = json!(qualified_name.rsplit('.').next().unwrap());
    serde_json::from_value(value).unwrap()
}

fn predict_signature(matrix: bool) -> Signature {
    let features = if matrix {
        apply("Sequence", vec![apply("Sequence", vec![name("float")])])
    } else {
        apply("Sequence", vec![name("float")])
    };
    serde_json::from_value(json!({"kind": "callable", "parameters": [
        {"name": "self", "kind": {"kind": "positional-only"}, "annotation": null, "default": null},
        {"name": "features", "kind": {"kind": "positional-only"}, "annotation": features, "default": null}
    ], "returns": if matrix { apply("list", vec![name("float")]) } else { name("float") }})).unwrap()
}

fn fit_signature(diagnostics: bool) -> Signature {
    serde_json::from_value(json!({"kind": "callable", "parameters": [
        {"name": "features", "kind": {"kind": "positional-or-keyword"}, "annotation": apply("Sequence", vec![apply("Sequence", vec![name("float")])]), "default": null},
        {"name": "target", "kind": {"kind": "positional-or-keyword"}, "annotation": apply("Sequence", vec![name("float")]), "default": null},
        {"name": "solver", "kind": {"kind": "keyword-only"}, "annotation": apply("Literal", vec![json!({"kind": "literal", "text": "\"normal\""}), json!({"kind": "literal", "text": "\"qr\""})]), "default": {"kind": "literal", "text": "\"normal\""}},
        {"name": "tolerance", "kind": {"kind": "keyword-only"}, "annotation": name("float"), "default": {"kind": "literal", "text": "..."}},
        {"name": "return_diagnostics", "kind": {"kind": "keyword-only"}, "annotation": apply("Literal", vec![json!({"kind": "literal", "text": if diagnostics { "True" } else { "False" }})]), "default": if diagnostics { serde_json::Value::Null } else { json!({"kind": "literal", "text": "False"}) }}
    ], "returns": if diagnostics { apply("tuple", vec![name("FooModel"), name("FitDiagnostics")]) } else { name("FooModel") }})).unwrap()
}

fn r_signature(name: &str) -> diplodocus::ir::SourcedSignature {
    let names: &[&str] = match name {
        "fit" => &["x", "..."],
        "fit.default" => &["x", "y", "solver", "tolerance", "..."],
        "fit.foo_model" => &["x", "features", "target", "tolerance", "..."],
        "predict.foo_model" => &["object", "newdata", "..."],
        _ => unreachable!(),
    };
    let parameters = names.iter().map(|name| json!({
        "name": name,
        "kind": if *name == "..." { json!({"kind": "language-specific", "language": "r", "name": "dots"}) } else { json!({"kind": "positional-or-keyword"}) },
        "annotation": null,
        "default": match *name {
            "solver" => apply("c", vec![json!({"kind": "literal", "text": "\"normal\""}), json!({"kind": "literal", "text": "\"qr\""})]),
            "tolerance" => json!({"kind": "literal", "text": "1e-8"}),
            _ => serde_json::Value::Null,
        }
    })).collect::<Vec<_>>();
    serde_json::from_value(json!({"signature": {"kind": "callable", "parameters": parameters, "returns": null}, "sources": [source("r", "R/fit.R", "signature")]})).unwrap()
}

// These are hand-assembled contracts for the maintained fixture, not extraction.
fn linked_items() -> BTreeMap<String, BTreeMap<String, Item>> {
    let mut packages = BTreeMap::new();
    let mut py = BTreeMap::new();
    for (qualified, alias, kind, binding, signatures) in [
        (
            "foo.model.fit",
            "foo.fit",
            PythonIdentityKind::Function,
            "function",
            [fit_signature(false), fit_signature(true)],
        ),
        (
            "foo.model.FooModel.predict",
            "foo.FooModel.predict",
            PythonIdentityKind::Method,
            "instance-method",
            [predict_signature(false), predict_signature(true)],
        ),
    ] {
        let identity = SemanticIdentity::python(qualified, kind).unwrap();
        let family = identity.in_package("pyfoo").unwrap();
        let overloads = signatures
            .iter()
            .map(|signature| {
                SemanticIdentity::python_overload(qualified, kind, signature)
                    .unwrap()
                    .in_package("pyfoo")
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let item_kind = if binding == "function" {
            "function"
        } else {
            "method"
        };
        let mut family_item = declaration(
            item_kind,
            qualified,
            python(
                json!({"kind": "callable", "binding": binding, "is_async": false, "role": {"kind": "family", "overloads": overloads}}),
            ),
        );
        family_item.aliases = serde_json::from_value(json!([{"qualified_name": alias, "kind": "python-reexport", "sources": [source("python", "python/foo/__init__.py", "export"), source("python", "python/foo/__init__.pyi", "export")]}])).unwrap();
        family_item.source_location = serde_json::from_value(
            json!({"repository": "python", "path": "python/foo/model.py", "span": null}),
        )
        .unwrap();
        family_item.documentation = serde_json::from_value(json!({
            "document": {"span": {"start": 0, "end": 0}, "frontmatter": null, "blocks": []},
            "source_format": {"kind": "extracted", "name": "numpy-docstring"},
            "source_location": {"repository": "python", "path": "python/foo/model.py", "span": null},
            "raw_source": null, "provenance": []
        })).unwrap();
        for (signature, overload) in signatures.into_iter().zip(overloads) {
            let sourced = serde_json::from_value(json!({"signature": signature, "sources": [source("python", "python/foo/model.pyi", "signature")]})).unwrap();
            family_item.signatures.push(sourced);
            let mut member = declaration(
                item_kind,
                qualified,
                python(
                    json!({"kind": "callable", "binding": binding, "is_async": false, "role": {"kind": "overload", "family": family}}),
                ),
            );
            member
                .signatures
                .push(family_item.signatures.last().unwrap().clone());
            if let Some(ItemLanguageData::Python(data)) = &mut member.language_data {
                data.decorators = serde_json::from_value(json!([{"expression": name("overload"), "semantics": "overload", "sources": [source("python", "python/foo/model.pyi", "definition")]}])).unwrap();
            }
            py.insert(overload.item, member);
        }
        py.insert(family.item, family_item);
    }
    packages.insert("pyfoo".into(), py);
    let mut r_items = BTreeMap::new();
    let generic = SemanticIdentity::r_s3_generic("fit")
        .unwrap()
        .in_package("rfoo")
        .unwrap();
    let generic_ref = RGenericReference::Workspace {
        item: generic.clone(),
    };
    let mut methods = vec![];
    for (name, class, generic_ref, rd) in [
        ("fit.default", "default", generic_ref.clone(), "man/fit.Rd"),
        ("fit.foo_model", "foo_model", generic_ref, "man/fit.Rd"),
        (
            "predict.foo_model",
            "foo_model",
            RGenericReference::External {
                package: "stats".into(),
                name: "predict".into(),
            },
            "man/foo_model.Rd",
        ),
    ] {
        let identity = SemanticIdentity::r_s3_method(name, &generic_ref, class)
            .unwrap()
            .in_package("rfoo")
            .unwrap();
        if name.starts_with("fit.") {
            methods.push(identity.clone());
        }
        let mut method = declaration(
            "method",
            name,
            r(
                json!({"kind": "s3-method", "generic": generic_ref, "class": class, "registration": [source("r", "NAMESPACE", "registration")]}),
                false,
            ),
        );
        method.name = name.into();
        method.signatures.push(r_signature(name));
        method.source_location =
            serde_json::from_value(json!({"repository": "r", "path": "R/fit.R", "span": null}))
                .unwrap();
        method.aliases = serde_json::from_value(json!([{"qualified_name": name, "kind": "rd-alias", "sources": [source("r", rd, "documentation")]}])).unwrap();
        r_items.insert(identity.item, method);
    }
    let mut family = declaration(
        "function",
        "fit",
        r(
            json!({"kind": "s3-generic", "dispatch_name": "fit", "dispatch_object": null, "methods": methods}),
            true,
        ),
    );
    family.aliases = serde_json::from_value(json!([{"qualified_name": "fit", "kind": "rd-alias", "sources": [source("r", "man/fit.Rd", "documentation")]}])).unwrap();
    family.signatures.push(r_signature("fit"));
    r_items.insert(generic.item, family);
    packages.insert("rfoo".into(), r_items);
    packages
}

#[test]
fn golden_families_have_addressable_members_and_canonical_aliases() {
    let items = linked_items();
    let stub = support::load_fixture("acceptance/python/python/foo/model.pyi");
    assert_eq!(stub.matches("@overload").count(), 4);
    assert_eq!(items["pyfoo"].len(), 6);
    assert_eq!(items["rfoo"].len(), 4);
    for item in items["rfoo"].values() {
        assert_eq!(item.signatures.len(), 1);
        let Signature::Callable { parameters, .. } = &item.signatures[0].signature else {
            panic!("R formals")
        };
        assert!(
            matches!(parameters.last().unwrap().kind, diplodocus::ir::ParameterKind::LanguageSpecific { ref name, .. } if name == "dots")
        );
    }
    for (package, members) in &items {
        for (id, item) in members {
            let targets = match item.language_data.as_ref().unwrap() {
                ItemLanguageData::Python(data) => match &data.declaration {
                    PythonDeclaration::Callable {
                        role: diplodocus::ir::PythonCallableRole::Family { overloads },
                        ..
                    } => overloads.clone(),
                    PythonDeclaration::Callable {
                        role: diplodocus::ir::PythonCallableRole::Overload { family },
                        ..
                    } => {
                        assert!(item.aliases.is_empty());
                        assert_eq!(item.signatures.len(), 1);
                        vec![family.clone()]
                    }
                    _ => unreachable!(),
                },
                ItemLanguageData::R(data) => match &data.declaration {
                    RDeclaration::S3Generic { methods, .. } => methods.clone(),
                    RDeclaration::S3Method {
                        generic: RGenericReference::Workspace { item },
                        ..
                    } => vec![item.clone()],
                    RDeclaration::S3Method {
                        generic: RGenericReference::External { package, name },
                        ..
                    } => {
                        assert_eq!((package.as_str(), name.as_str()), ("stats", "predict"));
                        vec![]
                    }
                    _ => unreachable!(),
                },
            };
            for target in targets {
                assert_eq!(&target.package, package);
                assert_ne!(&target.item, id);
                assert!(members.contains_key(&target.item));
            }
        }
    }
    let family_id =
        SemanticIdentity::python("foo.model.fit", PythonIdentityKind::Function).unwrap();
    let mut registry = IdentityRegistry::new("pyfoo").unwrap();
    let canonical = registry.register(&family_id).unwrap();
    for alias in &items["pyfoo"][family_id.item_id()].aliases {
        registry
            .bind_alias(&alias.qualified_name, &canonical)
            .unwrap();
    }
    assert_eq!(registry.resolve_alias("foo.fit").unwrap(), Some(&canonical));
    assert_eq!(registry.len(), 1);
    support::assert_json_golden(&items, "ir/language-items.json");
    let bytes = serde_json::to_string(&items).unwrap();
    assert_eq!(
        serde_json::from_str::<BTreeMap<String, BTreeMap<String, Item>>>(&bytes).unwrap(),
        items
    );
    let reordered = items
        .into_iter()
        .rev()
        .map(|(package, items)| (package, items.into_iter().rev().collect::<BTreeMap<_, _>>()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(bytes, serde_json::to_string(&reordered).unwrap());
}

#[test]
fn remaining_acceptance_language_semantics_round_trip_with_a_golden() {
    let field = ItemReference {
        package: "pyfoo".into(),
        item: SemanticIdentity::python(
            "foo.model.FitDiagnostics.iterations",
            PythonIdentityKind::Field,
        )
        .unwrap()
        .item_id()
        .into(),
    };
    let constructor = SemanticIdentity::python(
        "foo.model.FooModel.__init__",
        PythonIdentityKind::Constructor,
    )
    .unwrap()
    .in_package("pyfoo")
    .unwrap();
    let converged = SemanticIdentity::python(
        "foo.model.FitDiagnostics.converged",
        PythonIdentityKind::Field,
    )
    .unwrap()
    .in_package("pyfoo")
    .unwrap();
    let mut dataclass = python(
        json!({"kind": "class", "bases": [], "constructor": {"kind": "dataclass", "fields": [field, converged], "init": true, "frozen": true}}),
    );
    dataclass["data"]["decorators"] = json!([{"expression": apply("dataclass", vec![json!({"kind": "language-specific", "language": "python", "name": "keyword-argument", "children": [name("frozen"), {"kind": "literal", "text": "True"}], "source": null})]), "semantics": "dataclass", "sources": [source("python", "python/foo/model.py", "definition")]}]);
    let values = vec![
        python(
            json!({"kind": "module", "source": "implementation-and-stub", "exports": {"kind": "explicit", "names": ["fit", "FooModel", "NativeWorkspace"], "sources": [source("python", "python/foo/__init__.py", "export")]}}),
        ),
        python(json!({"kind": "module", "source": "stub-only", "exports": {"kind": "implicit"}})),
        python(
            json!({"kind": "module", "source": "implementation", "exports": {"kind": "dynamic", "expression": apply("_exported_names", vec![]), "sources": []}}),
        ),
        dataclass,
        python(
            json!({"kind": "class", "bases": [name("object")], "constructor": {"kind": "explicit", "item": constructor}}),
        ),
        python(json!({"kind": "class", "bases": [], "constructor": {"kind": "unspecified"}})),
        python(json!({"kind": "property", "has_setter": false, "has_deleter": false})),
        python(json!({"kind": "constant"})),
        python(json!({"kind": "field"})),
        python(json!({"kind": "type-alias", "target": apply("Sequence", vec![name("float")])})),
        r(json!({"kind": "function"}), true),
        r(
            json!({"kind": "constructor", "classes": ["foo_model"]}),
            true,
        ),
    ];
    let mut data = values
        .into_iter()
        .map(|value| serde_json::from_value::<ItemLanguageData>(value).unwrap())
        .collect::<Vec<_>>();
    for binding in [
        "function",
        "instance-method",
        "class-method",
        "static-method",
        "constructor",
    ] {
        data.push(serde_json::from_value(python(json!({"kind": "callable", "binding": binding, "is_async": true, "role": {"kind": "family", "overloads": []}}))).unwrap());
    }
    support::assert_json_golden(&data, "ir/language-variants.json");
    for value in data {
        let bytes = serde_json::to_string(&value).unwrap();
        assert_eq!(
            serde_json::from_str::<ItemLanguageData>(&bytes).unwrap(),
            value
        );
    }
}
