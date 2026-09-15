use diplodocus::ir::{
    IdentityError, IdentityRegistry, ItemReference, Parameter, ParameterKind, PythonIdentityKind,
    RGenericReference, SemanticIdentity, Signature, SignatureExpression,
};
use serde_json::json;

mod support;

fn name(value: &str) -> SignatureExpression {
    SignatureExpression::Name {
        name: value.into(),
        target: None,
    }
}

fn signature(diagnostics: bool) -> Signature {
    Signature::Callable {
        parameters: vec![Parameter {
            name: "return_diagnostics".into(),
            kind: ParameterKind::KeywordOnly,
            annotation: Some(SignatureExpression::Apply {
                constructor: Box::new(name("Literal")),
                arguments: vec![SignatureExpression::Literal {
                    text: if diagnostics { "True" } else { "False" }.into(),
                }],
            }),
            default: (!diagnostics).then(|| SignatureExpression::Literal {
                text: "False".into(),
            }),
        }],
        returns: Some(if diagnostics {
            SignatureExpression::Apply {
                constructor: Box::new(name("tuple")),
                arguments: vec![name("FooModel"), name("FitDiagnostics")],
            }
        } else {
            name("FooModel")
        }),
    }
}

#[test]
fn same_names_overloads_and_r_dispatch_have_distinct_stable_ids() {
    let family = SemanticIdentity::python("foo.model.fit", PythonIdentityKind::Function).unwrap();
    let overloads = [false, true].map(|value| {
        SemanticIdentity::python_overload(
            "foo.model.fit",
            PythonIdentityKind::Function,
            &signature(value),
        )
        .unwrap()
    });
    let generic = SemanticIdentity::r_s3_generic("fit").unwrap();
    let generic_ref = RGenericReference::Workspace {
        item: generic.in_package("rfoo").unwrap(),
    };
    let identities = vec![
        family,
        overloads[0].clone(),
        overloads[1].clone(),
        SemanticIdentity::python("foo.model.fit", PythonIdentityKind::Class).unwrap(),
        SemanticIdentity::python("foo.model.fit", PythonIdentityKind::TypeAlias).unwrap(),
        SemanticIdentity::python("foo.other.fit", PythonIdentityKind::Function).unwrap(),
        SemanticIdentity::r_function("fit").unwrap(),
        generic,
        SemanticIdentity::r_s3_method("fit", &generic_ref, "default").unwrap(),
        SemanticIdentity::r_s3_method("fit", &generic_ref, "foo_model").unwrap(),
        SemanticIdentity::r_s3_method(
            "predict.foo_model",
            &RGenericReference::External {
                package: "stats".into(),
                name: "predict".into(),
            },
            "foo_model",
        )
        .unwrap(),
    ];
    let ids: Vec<_> = identities.iter().map(|id| id.item_id()).collect();
    assert_eq!(
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        ids.len()
    );
    support::assert_json_golden(&ids, "ir/semantic-identities.json");
    assert_ne!(
        identities[0].in_package("pyfoo").unwrap(),
        identities[0].in_package("another-package").unwrap()
    );
    assert_eq!(
        overloads[1],
        SemanticIdentity::python_overload(
            "foo.model.fit",
            PythonIdentityKind::Function,
            &signature(true)
        )
        .unwrap()
    );
}

#[test]
fn aliases_reference_existing_items_and_conflicts_never_pick_a_winner() {
    let identity = SemanticIdentity::python("foo.model.fit", PythonIdentityKind::Function).unwrap();
    let mut registry = IdentityRegistry::new("pyfoo").unwrap();
    let canonical = registry.register(&identity).unwrap();
    for alias in ["foo.fit", "foo.model.fit", "foo.fit"] {
        registry.bind_alias(alias, &canonical).unwrap();
        assert_eq!(registry.resolve_alias(alias).unwrap(), Some(&canonical));
    }
    assert_eq!(registry.len(), 1);
    assert_eq!(
        registry.register(&identity),
        Err(IdentityError::DuplicateIdentity {
            item: canonical.clone()
        })
    );
    let distinct = registry
        .register(&SemanticIdentity::python("foo.fit", PythonIdentityKind::TypeAlias).unwrap())
        .unwrap();
    let conflict = registry.bind_alias("foo.fit", &distinct).unwrap_err();
    assert_eq!(conflict.code(), "conflicting-item-alias");
    assert_eq!(registry.resolve_alias("foo.fit").unwrap_err(), conflict);
    let mut reverse = IdentityRegistry::new("pyfoo").unwrap();
    reverse.register(&identity).unwrap();
    reverse
        .register(&SemanticIdentity::python("foo.fit", PythonIdentityKind::TypeAlias).unwrap())
        .unwrap();
    reverse.bind_alias("foo.fit", &distinct).unwrap();
    assert_eq!(
        reverse.bind_alias("foo.fit", &canonical).unwrap_err(),
        conflict
    );
    let missing = ItemReference {
        package: "pyfoo".into(),
        item: "missing".into(),
    };
    let unknown = registry.bind_alias("foo.missing", &missing).unwrap_err();
    assert_eq!(registry.resolve_alias("foo.missing").unwrap(), None);
    support::assert_json_golden(
        &json!({
            "duplicate": registry.register(&identity).unwrap_err(),
            "conflict": conflict,
            "unknown": unknown,
        }),
        "ir/identity-errors.json",
    );
}

#[test]
fn identity_encoding_cannot_confuse_delimiters_or_omit_signature_semantics() {
    let a = SemanticIdentity::python("foo:a/b%20", PythonIdentityKind::Function).unwrap();
    let b = SemanticIdentity::python("foo%3Aa%2Fb%2520", PythonIdentityKind::Function).unwrap();
    assert_ne!(a, b);
    let base = signature(false);
    let identity =
        SemanticIdentity::python_overload("foo.fit", PythonIdentityKind::Function, &base).unwrap();
    let mut resolved = base.clone();
    let Signature::Callable { returns, .. } = &mut resolved else {
        unreachable!()
    };
    *returns = Some(SignatureExpression::Name {
        name: "FooModel".into(),
        target: Some(ItemReference {
            package: "pyfoo".into(),
            item: "resolved-model".into(),
        }),
    });
    assert_eq!(
        identity,
        SemanticIdentity::python_overload("foo.fit", PythonIdentityKind::Function, &resolved)
            .unwrap()
    );
    let Signature::Callable { parameters, .. } = &mut resolved else {
        unreachable!()
    };
    parameters[0].kind = ParameterKind::PositionalOnly;
    assert_ne!(
        identity,
        SemanticIdentity::python_overload("foo.fit", PythonIdentityKind::Function, &resolved)
            .unwrap()
    );
    assert!(
        SemanticIdentity::python_overload("foo.fit", PythonIdentityKind::Class, &base).is_err()
    );
    assert!(
        SemanticIdentity::python_overload(
            "foo.fit",
            PythonIdentityKind::Function,
            &Signature::Value {
                annotation: None,
                value: None
            }
        )
        .is_err()
    );
    assert!(SemanticIdentity::python("", PythonIdentityKind::Function).is_err());
    assert!(IdentityRegistry::new("").is_err());
}

#[test]
fn relocation_source_edits_and_url_changes_do_not_enter_identity_inputs() {
    let a = support::acceptance_workspace();
    let b = support::acceptance_workspace();
    assert_ne!(a.path(), b.path());
    b.write(
        "python/python/foo/model.pyi",
        format!(
            "\n# A harmless shift in source ranges.\n{}",
            b.read("python/python/foo/model.pyi")
        ),
    );
    let identities = |workspace: &support::TestWorkspace| {
        let source = workspace.read("python/python/foo/model.pyi");
        assert!(source.contains("def fit("));
        [false, true].map(|diagnostics| {
            SemanticIdentity::python_overload(
                "foo.model.fit",
                PythonIdentityKind::Function,
                &signature(diagnostics),
            )
            .unwrap()
            .in_package("pyfoo")
            .unwrap()
        })
    };
    b.write(
        "workspace/diplodocus.toml",
        b.read("workspace/diplodocus.toml")
            .replace("slug = \"python\"", "slug = \"new-python-route\""),
    );
    assert_eq!(identities(&a), identities(&b));
    let bytes = serde_json::to_string(&identities(&b)).unwrap();
    for forbidden in [
        a.path().to_str().unwrap(),
        b.path().to_str().unwrap(),
        "new-python-route",
        "model.pyi",
    ] {
        assert!(!bytes.contains(forbidden));
    }
}

#[test]
fn opaque_signatures_fail_and_diagnostic_spelling_does_not_change_structured_keys() {
    let mut value = signature(false);
    let set_return = |value: &mut Signature, source: &str, children: Vec<SignatureExpression>| {
        let Signature::Callable { returns, .. } = value else {
            unreachable!()
        };
        *returns = Some(SignatureExpression::LanguageSpecific {
            language: "python".into(),
            name: "union".into(),
            children,
            source: Some(source.into()),
        });
    };
    set_return(&mut value, "int | str", vec![name("int"), name("str")]);
    let first =
        SemanticIdentity::python_overload("foo.fit", PythonIdentityKind::Function, &value).unwrap();
    set_return(&mut value, "int  |  str", vec![name("int"), name("str")]);
    assert_eq!(
        first,
        SemanticIdentity::python_overload("foo.fit", PythonIdentityKind::Function, &value).unwrap()
    );
    set_return(&mut value, "opaque()", vec![]);
    let error = SemanticIdentity::python_overload("foo.fit", PythonIdentityKind::Function, &value)
        .unwrap_err();
    assert_eq!(error.code(), "invalid-identity");
    support::assert_json_golden(&error, "ir/opaque-identity-error.json");
}
