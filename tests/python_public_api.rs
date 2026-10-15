mod support;

use diplodocus::{assembly::assemble_workspace, ir::*, validation::resolve_workspace};

#[test]
fn complete_public_package_keeps_class_facts_and_contextual_references() {
    let root = support::TestWorkspace::from_fixture("python-overloads");
    root.write(
        "python/demo/__init__.py",
        "from ._api import Result, Derived, run\n__all__ = ['Result', 'Derived', 'run']\n",
    );
    root.write(
        "python/demo/_api.py",
        r#""""Private implementation prose with :func:`missing_private_helper`."""
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Generic, TypeVar
if TYPE_CHECKING:
    from matplotlib.axes import Axes
S = TypeVar("S")
@dataclass(frozen=True, repr=False)
class Result(Generic[S]):
    """Result data.

    Attributes
    ----------
    value:
        The stored value.
    """
    value: S
    cache: dict[str, S] = field(default_factory=dict, repr=False, init=False)
    def plot(self, ax: Axes | None = None) -> Axes:
        """Plot the result."""
    def other(self) -> None:
        """See :meth:`plot` and :attr:`value`."""
class Derived(Result[S]):
    """See :meth:`plot` and :attr:`~Derived.value`."""
def run() -> Result[int]:
    """Return a :class:`Result`.

    See :func:`run`, :meth:`~Result.plot`, :class:`external.Thing`,
    :data:`S`, and :func:`demo._api.private_helper`.
    """
def private_helper(): ...
"#,
    );
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let items = &sources.workspace().packages["python"].items;
    let result = items
        .values()
        .find(|item| item.qualified_name == "demo._api.Result")
        .unwrap();
    let Some(ItemLanguageData::Python(data)) = &result.language_data else {
        panic!("Python class");
    };
    let PythonDeclaration::Class {
        constructor:
            PythonConstructor::Dataclass {
                fields,
                frozen,
                init,
            },
        ..
    } = &data.declaration
    else {
        panic!("dataclass constructor");
    };
    assert!(*frozen && *init);
    assert_eq!(
        fields
            .iter()
            .map(|field| items[&field.item].name.as_str())
            .collect::<Vec<_>>(),
        ["value"]
    );
    assert!(
        items
            .values()
            .any(|item| item.qualified_name == "demo._api.Result.cache")
    );
    let module = items
        .values()
        .find(|item| item.qualified_name == "demo._api")
        .unwrap();
    assert!(!module.discoverable());
    assert!(module.documentation.is_none());
    assert!(
        !items
            .values()
            .any(|item| item.qualified_name == "demo._api.private_helper")
    );
    let resolution = resolve_workspace(&sources).unwrap();
    let documents = resolution.records();
    let other = items
        .iter()
        .find(|(_, item)| item.qualified_name == "demo._api.Result.other")
        .unwrap();
    let resolved = documents.iter().find(|document| matches!(&document.document, diplodocus::validation::DocumentIdentity::Item { item } if &item.item == other.0)).unwrap();
    assert_eq!(resolved.references.len(), 2);
    let derived = items
        .iter()
        .find(|(_, item)| item.qualified_name == "demo._api.Derived")
        .unwrap();
    let resolved = documents.iter().find(|document| matches!(&document.document, diplodocus::validation::DocumentIdentity::Item { item } if &item.item == derived.0)).unwrap();
    assert_eq!(resolved.references.len(), 2);
    let warnings: Vec<_> = resolution
        .diagnostics()
        .iter()
        .filter(|diagnostic| diagnostic.code.as_str() == "python-unsupported-docstring")
        .collect();
    assert_eq!(warnings.len(), 3, "{warnings:?}");
    assert!(warnings.iter().all(|diagnostic| diagnostic.span.is_some()));
    assert!(
        warnings
            .iter()
            .all(|diagnostic| !diagnostic.message.contains("missing_private_helper"))
    );
}

#[test]
fn inherited_documentation_references_follow_python_mro() {
    let root = support::TestWorkspace::from_fixture("python-overloads");
    root.write(
        "python/demo/__init__.py",
        "from ._api import Base, First, Second, Child\n__all__ = ['Base', 'First', 'Second', 'Child']\n",
    );
    root.write("python/demo/_api.py", "class Base:\n    def draw(self): ...\nclass First(Base): ...\nclass Second(Base):\n    def draw(self): ...\nclass Child(First, Second):\n    \"\"\"See :meth:`draw`.\"\"\"\n");
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let resolution = resolve_workspace(&sources).unwrap();
    let items = &sources.workspace().packages["python"].items;
    let target = items
        .iter()
        .find(|(_, item)| item.qualified_name == "demo._api.Second.draw")
        .unwrap()
        .0;
    assert!(resolution.records().iter().flat_map(|document| &document.references).any(|reference| matches!(&reference.target, diplodocus::validation::ReferenceTarget::Item { item } if &item.item == target)));
}
