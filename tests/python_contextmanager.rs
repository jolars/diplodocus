use diplodocus::extractors::python::{parse_target, surface};
use diplodocus::ir::{ItemLanguageData, PythonDecoratorSemantics, Signature, SignatureExpression};
use diplodocus::paths::{ResolvedPackagePaths, ResolvedRepositoryPaths, ResolvedTargetPath};

mod support;

#[test]
fn contextmanagers_are_static_and_private_decorators_do_not_fail_public_extraction() {
    let workspace = support::TestWorkspace::from_fixture("python-contextmanager");
    let root = workspace.path().canonicalize().unwrap();
    let repository = ResolvedRepositoryPaths {
        id: "repo".into(),
        path: root.clone(),
    };
    let target = ResolvedTargetPath {
        id: "api".into(),
        path: root.join("python/context_fixture"),
    };
    let package = ResolvedPackagePaths {
        id: "context-fixture".into(),
        repository_index: 0,
        path: root.clone(),
        metadata_path: root.join("pyproject.toml"),
        targets: vec![target.clone()],
    };
    let parsed = parse_target(&repository, &package, &target);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let surface = surface::reconcile(&parsed);
    let public = surface
        .items
        .values()
        .find(|item| item.qualified_name == "context_fixture._api.public_lease")
        .unwrap();
    let Some(ItemLanguageData::Python(python)) = public.language_data.as_ref() else {
        panic!("Python item")
    };
    assert_eq!(
        python.decorators[0].semantics,
        PythonDecoratorSemantics::ContextManager
    );
    let Signature::Callable {
        returns:
            Some(SignatureExpression::Apply {
                constructor,
                arguments,
            }),
        ..
    } = &public.signatures[0].signature
    else {
        panic!("expected context manager return annotation")
    };
    assert!(
        matches!(&**constructor, SignatureExpression::Name { name, .. } if name == "contextlib.AbstractContextManager")
    );
    assert!(
        matches!(arguments.as_slice(), [SignatureExpression::Name { name, .. }] if name == "str")
    );
    let generator = surface
        .items
        .values()
        .find(|item| item.qualified_name == "context_fixture._api.public_generator")
        .unwrap();
    assert!(matches!(
        &generator.signatures[0].signature,
        Signature::Callable {
            returns: Some(SignatureExpression::Apply { constructor, arguments }),
            ..
        } if matches!(&**constructor, SignatureExpression::Name { name, .. } if name == "contextlib.AbstractContextManager")
            && matches!(arguments.as_slice(), [SignatureExpression::Name { name, .. }] if name == "str")
    ));
    assert!(
        !surface
            .items
            .values()
            .any(|item| item.qualified_name.ends_with("._private_helper"))
    );
    assert!(
        !surface
            .diagnostics
            .iter()
            .any(|d| d.message.contains("_private_helper") || d.message.contains("_private_lease"))
    );
    assert_eq!(surface.diagnostics.len(), 1, "{:?}", surface.diagnostics);
    assert!(surface.diagnostics[0].message.contains("public_unknown"));
    assert_eq!(
        surface.diagnostics[0].code.as_str(),
        "python-unsupported-surface"
    );
    let unknown = surface
        .items
        .values()
        .find(|item| item.qualified_name == "context_fixture._api.public_unknown")
        .unwrap();
    assert!(unknown.signatures.is_empty());
}
