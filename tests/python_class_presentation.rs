mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::rendering::{render_fragments, render_site};
use diplodocus::site::Site;
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::{resolve_item, resolve_workspace};

fn route(item: &diplodocus::ir::ItemReference) -> String {
    format!(
        "packages/python/reference/{}.html",
        diplodocus::provenance::fingerprint_bytes(item.item.as_bytes()).value
    )
}

fn member_anchor(item: &diplodocus::ir::ItemReference) -> String {
    item.item.rsplit('.').next().unwrap().to_owned()
}

#[test]
fn class_pages_contain_fields_methods_and_inherited_links() {
    let root = support::TestWorkspace::from_fixture("python-overloads");
    root.write(
        "python/demo/_api.py",
        r#"
from dataclasses import dataclass, field
from typing import Generic, TypeVar, overload
T = TypeVar("T")

def target() -> None:
    """A helper target."""

@dataclass(frozen=True)
class Base(Generic[T]):
    """A fitted result."""
    size: int
    names: list[str] = field(default_factory=list)
    disabled: bool = field(default=False, init=False)

    @overload
    def draw(self, value: str) -> str: ...
    @overload
    def draw(self, value: bytes) -> bytes: ...
    def draw(self, value: str | bytes) -> str | bytes:
        """Draw a result using :func:`demo.target`.

        Parameters
        ----------
        value:
            The complete drawing input.
        """
        return value

class Child(Base[int]):
    """A specialized result."""

class Override(Base[int]):
    """A customized result."""
    def draw(self, value: int) -> int:
        """Draw the customized result."""
        return value

class Left(Base[int]):
    """An unchanged branch."""

class Right(Base[int]):
    """A changed branch."""
    def draw(self, value: int) -> int:
        """Draw from the right branch."""
        return value

class Diamond(Left, Right):
    """A diamond inheritance example."""
"#,
    );
    root.write("python/demo/__init__.py", "from ._api import Base, Child, Diamond, Left, Override, Right, target\n__all__ = ['Base', 'Child', 'Diamond', 'Left', 'Override', 'Right', 'target']\n");
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let site = Site::new(&snapshot).unwrap();
    let rendered = render_site(&site).unwrap();
    let fragments = render_fragments(&site).unwrap();
    let workspace = snapshot.workspace();
    let base = resolve_item(workspace, Some("python"), "demo.Base").unwrap();
    let child = resolve_item(workspace, Some("python"), "demo.Child").unwrap();
    let override_class = resolve_item(workspace, Some("python"), "demo.Override").unwrap();
    let draw = resolve_item(workspace, Some("python"), "demo.Base.draw").unwrap();
    let size = resolve_item(workspace, Some("python"), "demo.Base.size").unwrap();
    let html = std::str::from_utf8(&rendered.files()[&route(&base)].bytes).unwrap();
    assert!(html.contains("id=\""));
    assert!(html.contains(&format!("id=\"{}\"", member_anchor(&draw))));
    assert!(html.contains(&format!("id=\"{}\"", member_anchor(&size))));
    assert!(html.contains("The complete drawing input."));
    assert!(html.contains("<h3>Parameters</h3>"));
    assert!(html.contains(">demo.target</code></a>"));
    assert!(!rendered.files().contains_key(&route(&draw)));
    let child_html = std::str::from_utf8(&rendered.files()[&route(&child)].bytes).unwrap();
    let base_filename = route(&base).rsplit('/').next().unwrap().to_owned();
    assert!(child_html.contains("Constructor inherited from"));
    assert!(child_html.contains(&format!("{base_filename}#{}", member_anchor(&draw))));
    assert!(child_html.contains(&format!("{base_filename}#{}", member_anchor(&size))));
    let override_html =
        std::str::from_utf8(&rendered.files()[&route(&override_class)].bytes).unwrap();
    assert!(override_html.contains("Draw the customized result."));
    assert!(!override_html.contains(&format!("{base_filename}#{}", member_anchor(&draw))));
    let diamond = resolve_item(workspace, Some("python"), "demo.Diamond").unwrap();
    let right = resolve_item(workspace, Some("python"), "demo.Right").unwrap();
    let diamond_html = std::str::from_utf8(&rendered.files()[&route(&diamond)].bytes).unwrap();
    assert!(diamond_html.contains(&format!(
        "{}#draw",
        route(&right).rsplit('/').next().unwrap()
    )));
    assert!(!diamond_html.contains(&format!("{base_filename}#draw")));
    let search: Vec<serde_json::Value> =
        serde_json::from_slice(&rendered.files()["assets/search.json"].bytes).unwrap();
    assert!(
        !search
            .iter()
            .any(|entry| entry["title"] == "demo.Base.draw")
    );
    assert!(
        search
            .iter()
            .find(|entry| entry["title"] == "demo.Base")
            .unwrap()["text"]
            .as_str()
            .unwrap()
            .contains("The complete drawing input.")
    );
    let documents: serde_json::Value =
        serde_json::from_slice(&fragments.files()["documents.json"].bytes).unwrap();
    assert!(
        documents["documents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|document| document["path"] == route(&base))
            .unwrap()["identities"]
            .as_array()
            .unwrap()
            .len()
            > 1
    );

    let text = code_text(html);
    assert!(
        text.contains("Base(size: int, names: list[str] = <factory>)"),
        "{text}"
    );
    assert!(text.contains("draw(value: str) -> str"), "{text}");
    assert!(text.contains("draw(value: bytes) -> bytes"), "{text}");
    let markdown_route = route(&base).replace(".html", ".md");
    let markdown = std::str::from_utf8(&fragments.files()[&markdown_route].bytes).unwrap();
    assert!(markdown.contains("<a id=\"draw\"></a>"));
    assert!(markdown.contains("<a id=\"constructor\"></a>"));
    assert!(markdown.contains("Base(size: int, names: list[str] = <factory>)"));
    assert!(markdown.contains("draw(value: str) -> str"));
    assert!(markdown.contains("draw(value: bytes) -> bytes"));
    assert!(markdown.contains("The complete drawing input."));
    assert!(markdown.contains("### Parameters"));
}

#[test]
fn explicit_constructors_and_cyclic_bases_remain_bounded() {
    let root = support::TestWorkspace::from_fixture("python-overloads");
    root.write(
        "python/pyproject.toml",
        "[project]\nname = 'overload-demo'\nversion = '0.1.0'\nrequires-python = '>=3.11'\n",
    );
    root.write(
        "python/demo/_api.py",
        r#"
class Plain:
    """A plain class."""
    def __init__(self, size: int, /, *, enabled: bool = True) -> None:
        """Initialize the complete configuration."""

class Derived(Plain):
    """An inherited initializer."""

class First(Second):
    """A deliberately cyclic static declaration."""
    count: int

class Second(First):
    """The other cyclic declaration."""
    label: str

class NativeError(ValueError):
    """A native error surfaced through a maintained stub."""
"#,
    );
    root.write("python/demo/__init__.py", "from ._api import Plain, Derived, First, Second, NativeError\n__all__ = ['Plain', 'Derived', 'First', 'Second', 'NativeError']\n");
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    let plain = resolve_item(snapshot.workspace(), Some("python"), "demo.Plain").unwrap();
    let derived = resolve_item(snapshot.workspace(), Some("python"), "demo.Derived").unwrap();
    let plain_html = std::str::from_utf8(&rendered.files()[&route(&plain)].bytes).unwrap();
    assert!(code_text(plain_html).contains("Plain(size: int, /, *, enabled: bool = True)"));
    assert!(plain_html.contains("Initialize the complete configuration."));
    assert!(plain_html.contains("id=\"__init__\""));
    let derived_html = std::str::from_utf8(&rendered.files()[&route(&derived)].bytes).unwrap();
    assert!(code_text(derived_html).contains("Derived(size: int, /, *, enabled: bool = True)"));
    assert!(derived_html.contains("Constructor inherited from"));
    let error = resolve_item(snapshot.workspace(), Some("python"), "demo.NativeError").unwrap();
    let error_html = std::str::from_utf8(&rendered.files()[&route(&error)].bytes).unwrap();
    assert!(code_text(error_html).contains("ValueError"));
    for name in ["demo.First", "demo.Second"] {
        let item = resolve_item(snapshot.workspace(), Some("python"), name).unwrap();
        assert!(rendered.files().contains_key(&route(&item)));
    }
}

#[test]
fn nested_classes_keep_their_own_page_and_method_anchors() {
    let root = support::TestWorkspace::from_fixture("python-overloads");
    root.write(
        "python/demo/_api.py",
        r#"
class Nested:
    """An outer public class."""
    class Inner:
        """A nested public class."""
        def method(self, value: int) -> int:
            """Keep this method on the nested class page."""
            return value

def usage() -> None:
    """Use :func:`demo.Nested.Inner.method`."""
"#,
    );
    root.write(
        "python/demo/__init__.py",
        "from ._api import Nested, usage\n__all__ = ['Nested', 'usage']\n",
    );
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    let snapshot = Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap();
    let rendered = render_site(&Site::new(&snapshot).unwrap()).unwrap();
    let outer = resolve_item(snapshot.workspace(), Some("python"), "demo.Nested").unwrap();
    let inner = resolve_item(snapshot.workspace(), Some("python"), "demo.Nested.Inner").unwrap();
    let method = resolve_item(
        snapshot.workspace(),
        Some("python"),
        "demo.Nested.Inner.method",
    )
    .unwrap();
    let usage = resolve_item(snapshot.workspace(), Some("python"), "demo.usage").unwrap();
    let outer_html = std::str::from_utf8(&rendered.files()[&route(&outer)].bytes).unwrap();
    let inner_html = std::str::from_utf8(&rendered.files()[&route(&inner)].bytes).unwrap();
    let usage_html = std::str::from_utf8(&rendered.files()[&route(&usage)].bytes).unwrap();
    assert!(!outer_html.contains("A nested public class."));
    assert!(inner_html.contains("A nested public class."));
    assert!(inner_html.contains("id=\"method\""));
    assert!(inner_html.contains("Keep this method on the nested class page."));
    assert!(!rendered.files().contains_key(&route(&method)));
    assert!(usage_html.contains(&format!(
        "{}#method",
        route(&inner).rsplit('/').next().unwrap()
    )));
    let search: Vec<serde_json::Value> =
        serde_json::from_slice(&rendered.files()["assets/search.json"].bytes).unwrap();
    assert_eq!(
        search
            .iter()
            .filter(|entry| entry["title"] == "demo.Nested.Inner")
            .count(),
        1
    );
}

fn code_text(html: &str) -> String {
    use html5ever::tendril::TendrilSink;
    use markup5ever_rcdom::{Handle, NodeData, RcDom};
    fn text(node: &Handle) -> String {
        if let NodeData::Text { contents } = &node.data {
            return contents.borrow().to_string();
        }
        node.children.borrow().iter().map(text).collect()
    }
    let document = html5ever::parse_document(RcDom::default(), Default::default()).one(html);
    text(&document.document)
}
