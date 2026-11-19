mod support;

use diplodocus::ir::*;

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
fn annotated_type_aliases_preserve_union_targets_and_signature_links_in_snapshots() {
    for (import, marker) in [
        ("from typing import TypeAlias", "TypeAlias"),
        ("from typing import TypeAlias as TA", "TA"),
        ("import typing", "typing.TypeAlias"),
        ("import typing as t", "t.TypeAlias"),
        ("from typing_extensions import TypeAlias as TA", "TA"),
        ("import typing_extensions as t", "t.TypeAlias"),
    ] {
        let root = support::TestWorkspace::new();
        root.write(
            "pyproject.toml",
            "[project]\nname='foo'\nversion='1.0'\nrequires-python='>=3.11'\n",
        );
        root.write("diplodocus.yaml", "project:\n  name: \"Aliases\"\nrepositories:\n  - id: \"python\"\n    path: \".\"\npackages:\n  - id: \"pyfoo\"\n    name: \"Foo\"\n    slug: \"python\"\n    ecosystem: \"python\"\n    repository: \"python\"\n    path: \".\"\n    metadata-path: \"pyproject.toml\"\n    targets:\n      - id: \"api\"\n        extractor: \"python\"\n        path: \"python/foo\"\n        role: \"public-api\"\n");
        root.write(
            "python/foo/__init__.py",
            "from ._exports import DatasetData\nfrom ._models import SparseDataset, SparseDatasetSplit, fetch\n__all__ = ['DatasetData', 'SparseDataset', 'SparseDatasetSplit', 'fetch']\n",
        );
        // Prepare the external type dependency without requiring pandas itself.
        root.write("python/pandas.py", "class DataFrame: ...\n");
        root.write(
            "python/foo/_exports.py",
            "from ._models import DatasetData as DatasetData\n",
        );
        root.write("python/foo/_models.py", format!("from __future__ import annotations\n{import}\nimport pandas as pd\nclass SparseDataset: ...\nclass SparseDatasetSplit: ...\nDatasetData: {marker} = pd.DataFrame | SparseDataset | SparseDatasetSplit\n\"\"\"Decoded dataset values.\"\"\"\ndef fetch(data: DatasetData) -> DatasetData: ...\n"));
        let sources =
            diplodocus::assembly::assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
        let resolved = diplodocus::validation::resolve_workspace(&sources).unwrap();
        let snapshot = diplodocus::snapshots::Snapshot::from_sources(&sources, &resolved).unwrap();
        let path = root.path().join("snapshot.sqlite");
        snapshot.publish(&path).unwrap();
        let loaded = diplodocus::snapshots::Snapshot::load(path).unwrap();
        assert_eq!(loaded.workspace(), snapshot.workspace());
        let items = &loaded.workspace().packages["pyfoo"].items;
        let alias_id =
            SemanticIdentity::python("foo._models.DatasetData", PythonIdentityKind::TypeAlias)
                .unwrap()
                .item_id()
                .to_owned();
        let alias = &items[&alias_id];
        assert_eq!(alias.kind, ItemKind::Type);
        assert!(
            alias
                .aliases
                .iter()
                .any(|alias| alias.qualified_name == "foo.DatasetData")
        );
        let PythonDeclaration::TypeAlias { target } = &data(alias).declaration else {
            panic!("{import}: expected type alias")
        };
        let SignatureExpression::LanguageSpecific { name, children, .. } = target else {
            panic!("expected union")
        };
        assert_eq!(name, "union");
        assert_eq!(children.len(), 2);
        let SignatureExpression::LanguageSpecific {
            name,
            children: left,
            ..
        } = &children[0]
        else {
            panic!("expected nested union")
        };
        assert_eq!(name, "union");
        assert_eq!(left.len(), 2);
        assert!(
            matches!(&left[0], SignatureExpression::Name { name, target: None } if name == "pandas.DataFrame")
        );
        let sparse_id =
            SemanticIdentity::python("foo._models.SparseDataset", PythonIdentityKind::Class)
                .unwrap()
                .item_id()
                .to_owned();
        assert!(
            matches!(&left[1], SignatureExpression::Name { name, target: Some(reference) } if name == "foo._models.SparseDataset" && reference.item == sparse_id)
        );
        let class_id =
            SemanticIdentity::python("foo._models.SparseDatasetSplit", PythonIdentityKind::Class)
                .unwrap()
                .item_id()
                .to_owned();
        assert!(
            matches!(&children[1], SignatureExpression::Name { name, target: Some(reference) } if name == "foo._models.SparseDatasetSplit" && reference.item == class_id)
        );
        assert!(
            matches!(&alias.signatures[0].signature, Signature::Value { annotation: None, value: Some(value) } if value == target)
        );
        let callable = &items[&callable_id("foo._models.fetch")];
        let Signature::Callable {
            parameters,
            returns,
        } = &callable.signatures[0].signature
        else {
            panic!("expected callable")
        };
        for expression in [
            parameters[0].annotation.as_ref().unwrap(),
            returns.as_ref().unwrap(),
        ] {
            assert!(
                matches!(expression, SignatureExpression::Name { target: Some(reference), .. } if reference.package == "pyfoo" && reference.item == alias_id),
                "{import}: {expression:?}"
            );
        }
    }
}
