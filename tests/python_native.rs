mod support;

use std::fs;
use std::sync::{Arc, Mutex};

use diplodocus::assembly::assemble_workspace;
#[cfg(target_os = "linux")]
use diplodocus::commands::ExtractOptions;
use diplodocus::commands::{BuildOptions, CheckOptions, CommandRunner, GenerateOptions, generate};
use diplodocus::ir::*;
use diplodocus::reporting::{Event, Reporter};
use diplodocus::snapshots::Snapshot;
use diplodocus::validation::{resolve_item, resolve_workspace};

fn workspace(source: &str) -> support::TestWorkspace {
    let root = support::TestWorkspace::from_fixture("generated-python");
    root.write("generated_demo/__init__.py", source);
    root
}

fn source_snapshot(root: &support::TestWorkspace) -> Snapshot {
    let sources = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap();
    Snapshot::from_sources(&sources, &resolve_workspace(&sources).unwrap()).unwrap()
}

fn item<'a>(snapshot: &'a Snapshot, name: &str) -> &'a Item {
    let reference = resolve_item(snapshot.workspace(), Some("demo"), name).unwrap();
    &snapshot.workspace().packages["demo"].items[&reference.item]
}

#[test]
#[ignore = "native Python and Sphinx acceptance"]
fn standard_typing_bases_are_symbolic() {
    let root = workspace(
        r#"
from typing import Protocol as _Protocol, NamedTuple as _NamedTuple, TypedDict as _TypedDict

class Reader(_Protocol):
    def read(self) -> str: ...

class Row(_NamedTuple):
    value: str

class Payload(_TypedDict):
    value: str
"#,
    );
    let snapshot = source_snapshot(&root);
    for (name, expected) in [
        ("Reader", "typing.Protocol"),
        ("Row", "typing.NamedTuple"),
        ("Payload", "typing.TypedDict"),
    ] {
        let class = item(&snapshot, &format!("generated_demo.{name}"));
        assert!(
            matches!(
                &class.language_data,
                Some(ItemLanguageData::Python(PythonItemData {
                    declaration: PythonDeclaration::Class { bases, .. },
                    ..
                })) if matches!(bases.as_slice(), [SignatureExpression::Name { name, .. }] if name == expected)
            ),
            "{class:?}"
        );
    }
}

#[test]
#[ignore = "native Python and Sphinx acceptance"]
fn exports_include_lazy_bindings_and_keep_proven_value_aliases() {
    let root = workspace(
        r#"
from __future__ import annotations
from collections import UserList
from ._impl import VALUE as VALUE, __version__ as __version__
PUBLIC = VALUE
DEFAULT = None

class Output:
    pass

def __getattr__(name):
    if name == 'lazy':
        def compute(value: int) -> Output:
            """A lazily exported function."""
            return Output()
        return compute
    raise AttributeError(name)

def नमस्ते():
    """A function with combining marks in its name."""

__all__ = UserList(['lazy', 'Output', '__version__', 'DEFAULT', 'PUBLIC', 'VALUE', '__all__', 'नमस्ते'])
"#,
    );
    root.write(
        "generated_demo/_impl.py",
        "VALUE: int = 42\n__version__ = '1.0.0'\n",
    );
    root.write(
        "generated_demo/shadowed.py",
        r#"
from math import sqrt, floor, ceil

try:
    from typing import Optional
except ImportError:
    from typing_extensions import Optional

def sqrt(value: str) -> str:
    """A local definition."""
    return value

if True:
    def floor(value: str) -> str:
        """Another local definition."""
        return value
"#,
    );
    let snapshot = source_snapshot(&root);
    let lazy = item(&snapshot, "generated_demo.lazy");
    assert_eq!(
        lazy.documentation.as_ref().unwrap().raw_source.as_deref(),
        Some("A lazily exported function.")
    );
    let output = resolve_item(snapshot.workspace(), Some("demo"), "generated_demo.Output").unwrap();
    assert!(matches!(&lazy.signatures[0].signature,
        Signature::Callable { returns: Some(SignatureExpression::Name { target: Some(target), .. }), .. } if target == &output
    ));
    assert_eq!(
        item(&snapshot, "generated_demo.__version__").qualified_name,
        "generated_demo._impl.__version__"
    );
    assert_eq!(
        item(&snapshot, "generated_demo.__all__").kind,
        ItemKind::Constant
    );
    assert_eq!(
        item(&snapshot, "generated_demo.नमस्ते").kind,
        ItemKind::Function
    );
    assert!(matches!(
        &item(&snapshot, "generated_demo.DEFAULT").signatures[0].signature,
        Signature::Value { value: Some(SignatureExpression::Literal { text }), .. } if text == "None"
    ));
    let value = item(&snapshot, "generated_demo.VALUE");
    assert_eq!(value.qualified_name, "generated_demo._impl.VALUE");
    assert_eq!(item(&snapshot, "generated_demo.PUBLIC"), value);
    assert!(matches!(
        &value.signatures[0].signature,
        Signature::Value { annotation: Some(SignatureExpression::Name { name, .. }), .. } if name == "int"
    ));
    for (name, kind) in [
        ("VALUE", ItemAliasKind::PythonReexport),
        ("PUBLIC", ItemAliasKind::PythonAssignment),
    ] {
        assert!(value.aliases.iter().any(|alias| alias.qualified_name
            == format!("generated_demo.{name}")
            && alias.kind == kind
            && alias.sources[0].source.span.is_some()));
    }
    for name in ["sqrt", "floor"] {
        assert!(
            item(&snapshot, &format!("generated_demo.shadowed.{name}"))
                .documentation
                .is_some()
        );
    }
    for name in ["ceil", "Optional"] {
        assert!(
            resolve_item(
                snapshot.workspace(),
                Some("demo"),
                &format!("generated_demo.shadowed.{name}")
            )
            .is_err()
        );
    }
    assert!(
        !snapshot
            .workspace()
            .diagnostics
            .iter()
            .any(|d| d.message.contains("Ignoring invalid __all__"))
    );
}

#[test]
#[ignore = "native Python and Sphinx acceptance"]
fn dataclass_constructors_preserve_order_kinds_and_opaque_defaults() {
    let root = workspace(
        r#"
from dataclasses import dataclass, field

@dataclass
class Base:
    first: str

@dataclass
class Record(Base):
    option: str = field(default='option', kw_only=True)
    second: str = 'second'
    opaque: object = field(default=object(), kw_only=True)
    internal: list = field(default_factory=lambda: [], init=False)
"#,
    );
    let snapshot = source_snapshot(&root);
    let base_field = resolve_item(
        snapshot.workspace(),
        Some("demo"),
        "generated_demo.Base.first",
    )
    .unwrap();
    assert!(
        resolve_item(
            snapshot.workspace(),
            Some("demo"),
            "generated_demo.Record.first"
        )
        .is_err()
    );
    assert!(
        item(&snapshot, "generated_demo.Base")
            .children
            .contains(&base_field.item)
    );
    let record_item = item(&snapshot, "generated_demo.Record");
    assert!(matches!(
        &record_item.language_data,
        Some(ItemLanguageData::Python(PythonItemData {
            declaration: PythonDeclaration::Class {
                constructor: PythonConstructor::Dataclass { fields, .. },
                ..
            },
            ..
        })) if fields.first() == Some(&base_field)
    ));
    let record = resolve_item(snapshot.workspace(), Some("demo"), "generated_demo.Record").unwrap();
    let route = format!(
        "packages/python/reference/{}.md",
        diplodocus::provenance::fingerprint_bytes(record.item.as_bytes()).value
    );
    let site = diplodocus::site::Site::new(&snapshot).unwrap();
    let fragments = diplodocus::rendering::render_fragments(&site).unwrap();
    let markdown = std::str::from_utf8(&fragments.files()[&route].bytes).unwrap();
    assert!(
        markdown.contains(concat!(
            "Record(\n",
            "    first: str,\n",
            "    second: str = \"second\",\n",
            "    *,\n",
            "    option: str = \"option\",\n",
            "    opaque: object = ...\n",
            ")"
        )),
        "{markdown}"
    );
    assert!(snapshot.workspace().diagnostics.iter().any(|d| {
        d.message
            .contains("runtime default for `generated_demo.Record.opaque`")
    }));
    assert!(
        !snapshot
            .canonical_export()
            .unwrap()
            .contains("object at 0x")
    );
}

#[test]
#[ignore = "native Python and Sphinx acceptance"]
fn stub_docstrings_are_cleaned_and_docutils_diagnostics_are_portable() {
    fn build() -> String {
        let root = workspace(
            r#"
def broken():
    """An invalid directive.

    .. unknown-runtime::
    """
"#,
        );
        root.write(
            "generated_demo/__init__.pyi",
            r#"
def broken() -> None: ...

def numpy(value: int) -> None:
    """A NumPy description.

    Parameters
    ----------
    value : int
        The NumPy value.
    """

def google(value: int) -> None:
    """A Google description.

    Args:
        value (int): The Google value.
    """

def malformed() -> None:
    """Another invalid directive.

    .. unknown-stub::
    """
"#,
        );
        let snapshot = source_snapshot(&root);
        for name in ["numpy", "google"] {
            let document = item(&snapshot, &format!("generated_demo.{name}"))
                .documentation
                .as_ref()
                .unwrap();
            assert!(document.document.blocks.iter().any(|block| matches!(
                block,
                Block::Container {
                    kind: DocumentContainerKind::DefinitionList,
                    ..
                }
            )));
            assert!(document.raw_source.as_ref().unwrap().contains("\n    "));
        }
        let canonical = snapshot.canonical_export().unwrap();
        assert!(!canonical.contains(root.path().to_str().unwrap()));
        assert!(canonical.contains("unknown-runtime") && canonical.contains("unknown-stub"));
        canonical
    }
    assert_eq!(build(), build());
}

#[test]
#[ignore = "native Python documentation and doctest acceptance"]
fn runtime_docstrings_redact_paths_without_changing_doctest_execution() {
    fn build() -> String {
        let root = workspace(
            r#"
def source_file() -> str:
    return __file__

source_file.__doc__ = f"""Loaded from {__file__}.

>>> source_file()
{__file__!r}
"""
"#,
        );
        successful(
            cli(&root)
                .args(["build", "--output", "site"])
                .output()
                .unwrap(),
        );
        let snapshot =
            Snapshot::load(root.path().join(".diplodocus/documentation.sqlite")).unwrap();
        let doc = item(&snapshot, "generated_demo.source_file")
            .documentation
            .as_ref()
            .unwrap();
        let raw = doc.raw_source.as_deref().unwrap();
        assert!(raw.contains("Loaded from <repository>/generated_demo/__init__.py."));
        let examples = doc
            .provenance
            .iter()
            .find_map(|p| match &p.activity {
                ProvenanceActivity::ApiExamples { examples, .. } => Some(examples),
                _ => None,
            })
            .unwrap();
        let example = &examples[0];
        assert_eq!(example.actual, example.expected);
        assert!(raw[example.span.start..example.span.end].starts_with(">>> source_file()"));
        let canonical = snapshot.canonical_export().unwrap();
        assert!(!canonical.contains(root.path().to_str().unwrap()));
        canonical
    }
    assert_eq!(build(), build());
}

#[test]
#[ignore = "native Python and Sphinx acceptance"]
fn inherited_doctests_use_the_defining_module() {
    let root = workspace("");
    root.write(
        "generated_demo/base.py",
        r#"
def _helper():
    return 42

class Base:
    def run(self):
        """Run an example in the defining module.

        >>> _helper()
        42
        """
"#,
    );
    root.write(
        "generated_demo/derived.py",
        r#"
from .base import Base

class Derived(Base):
    def run(self):
        pass
"#,
    );
    CommandRunner::default()
        .build(BuildOptions {
            config: root.path().join("diplodocus.yaml"),
            output: root.path().join("site"),
        })
        .unwrap();
    let snapshot = Snapshot::load(root.path().join(".diplodocus/documentation.sqlite")).unwrap();
    for name in [
        "generated_demo.base.Base.run",
        "generated_demo.derived.Derived.run",
    ] {
        let document = item(&snapshot, name).documentation.as_ref().unwrap();
        assert!(
            document.provenance.iter().any(|record| matches!(
                &record.activity,
                ProvenanceActivity::ApiExamples { examples, .. } if examples[0].actual == "42\n"
            )),
            "{name}"
        );
    }
}

#[test]
#[ignore = "native Python doctest acceptance"]
fn a_stub_only_doctest_requires_its_runtime_module() {
    let root = workspace("def _answer():\n    return 42\n");
    root.write(
        "generated_demo/only.pyi",
        r#""""A maintained module example.

>>> _answer()
42
"""
"#,
    );
    let extract = || cli(&root).arg("extract").output().unwrap();
    let output = extract();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No module named 'generated_demo.only'"),
        "{stderr}"
    );
    let database = root.path().join(".diplodocus/documentation.sqlite");
    assert!(!database.exists());

    root.write(
        "generated_demo/__init__.py",
        "def _answer():\n    return 99\n",
    );
    root.write("generated_demo/only.py", "def _answer():\n    return 42\n");
    successful(extract());
    let snapshot = Snapshot::load(database).unwrap();
    let doc = item(&snapshot, "generated_demo.only")
        .documentation
        .as_ref()
        .unwrap();
    assert!(doc.provenance.iter().any(|record| matches!(
        &record.activity,
        ProvenanceActivity::ApiExamples { examples, .. } if examples[0].actual == "42\n"
    )));
}

fn cli(root: &support::TestWorkspace) -> std::process::Command {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_diplodocus"));
    command.current_dir(root.path());
    command
}

fn successful(output: std::process::Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[cfg(unix)]
#[ignore = "native Python interpreter selection and Sphinx acceptance"]
fn discovery_and_doctests_share_the_caller_relative_interpreter() {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    let root = workspace(
        "def answer():\n    \"\"\"Return an answer.\n\n    >>> answer()\n    42\n    \"\"\"\n    return 42\n",
    );
    let checkout = root.path().join("checkout");
    fs::create_dir(&checkout).unwrap();
    for entry in ["generated_demo", "pyproject.toml"] {
        fs::rename(root.path().join(entry), checkout.join(entry)).unwrap();
    }
    root.write(
        "diplodocus.yaml",
        root.read("diplodocus.yaml").replace(
            "    path: .\n    metadata-path:",
            "    path: checkout\n    metadata-path:",
        ),
    );
    let real = std::process::Command::new("python")
        .args(["-c", "import sys; print(sys.executable)"])
        .output()
        .unwrap();
    assert!(real.status.success());
    let real = PathBuf::from(String::from_utf8(real.stdout).unwrap().trim());
    assert!(real.is_absolute());
    for (path, script) in [
        (
            "bin/python",
            "#!/bin/sh\nexec \"$DIPLODOCUS_TEST_PYTHON\" -I \"$@\"\n",
        ),
        ("checkout/bin/python", "#!/bin/sh\nexit 90\n"),
    ] {
        root.write(path, script);
        fs::set_permissions(root.path().join(path), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let current = std::env::var_os("PATH").unwrap();
    let path = std::env::join_paths(
        std::iter::once(Path::new("bin").to_owned()).chain(std::env::split_paths(&current)),
    )
    .unwrap();
    successful(
        cli(&root)
            .env("PATH", path)
            .env("DIPLODOCUS_TEST_PYTHON", real)
            .args(["build", "--no-cache", "--output", "site"])
            .output()
            .unwrap(),
    );
    let snapshot = Snapshot::load(root.path().join(".diplodocus/documentation.sqlite")).unwrap();
    assert!(
        item(&snapshot, "generated_demo.answer")
            .documentation
            .as_ref()
            .unwrap()
            .provenance
            .iter()
            .any(|record| matches!(&record.activity, ProvenanceActivity::ApiExamples { .. }))
    );
}

#[test]
#[ignore = "native Python staging and snapshot storage acceptance"]
fn helper_staging_and_snapshot_storage_stay_out_of_python_inputs() {
    let root = workspace(
        "def answer():\n    \"\"\"Return an answer.\n\n    >>> answer()\n    42\n    \"\"\"\n    return 42\n",
    );
    let tmp = root.path().join("generated_demo/temporary");
    fs::create_dir(&tmp).unwrap();
    let command = || {
        let mut command = cli(&root);
        command
            .env("TMPDIR", &tmp)
            .env("TMP", &tmp)
            .env("TEMP", &tmp);
        command
    };
    successful(command().arg("check").output().unwrap());
    let build = || {
        successful(
            command()
                .args(["build", "--output", "site"])
                .output()
                .unwrap(),
        );
        Snapshot::load(root.path().join(".diplodocus/documentation.sqlite")).unwrap()
    };
    let original = build();
    for path in ["generated_demo/export.py", "generated_demo/export.pyi"] {
        original.publish(root.path().join(path)).unwrap();
    }
    assert_eq!(
        original.canonical_export().unwrap(),
        build().canonical_export().unwrap()
    );
    assert_eq!(fs::read_dir(tmp).unwrap().count(), 0);
}

#[test]
#[ignore = "native Python documentation and doctest acceptance"]
fn doctests_use_only_sphinx_transcripts_and_keep_original_ranges() {
    let root = workspace(
        r#"
"""Illustrative module prose.

.. code-block:: pycon

   >>> raise RuntimeError("illustrative")
"""

def combined():
    pass

combined.__doc__ = """The snippets in this document use different contexts.

.. code-block:: pycon

   >>> state = "illustrative"  # doctest: +UNKNOWN_FLAG
   >>> raise RuntimeError("illustrative")

The following syntax is illustrative too::

    >>> raise RuntimeError("literal")

Examples
--------
>>> state = "real"
>>> state
'real'

>>> state.upper()
'REAL'

>>> raise RuntimeError("skipped")  # doctest: +SKIP
"""

def from_stub():
    return 42
"#,
    );
    root.write(
        "generated_demo/__init__.pyi",
        r#"
def combined() -> None: ...

def from_stub() -> int:
    """
    Maintained stub example.

    >>> from_stub()
    42
    """
"#,
    );
    successful(
        cli(&root)
            .env("PYTHONSAFEPATH", "1")
            .args(["build", "--output", "site"])
            .output()
            .unwrap(),
    );
    let snapshot = Snapshot::load(root.path().join(".diplodocus/documentation.sqlite")).unwrap();
    let examples = |name| {
        let document = item(&snapshot, name).documentation.as_ref().unwrap();
        let raw = document.raw_source.as_deref().unwrap();
        document
            .provenance
            .iter()
            .find_map(|p| match &p.activity {
                ProvenanceActivity::ApiExamples { examples, .. } => Some(
                    examples
                        .iter()
                        .map(|example| (example, &raw[example.span.start..example.span.end]))
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .unwrap_or_default()
    };
    let combined = examples("generated_demo.combined");
    assert_eq!(combined.len(), 4);
    assert_eq!(combined[1].0.actual, "'real'\n");
    assert_eq!(combined[2].0.actual, "'REAL'\n");
    assert!(combined[2].1.starts_with(">>> state.upper()"));
    assert_eq!(combined[3].0.outcome, "explicit-skip");
    let stub = examples("generated_demo.from_stub");
    assert_eq!(stub.len(), 1);
    assert_eq!(stub[0].0.actual, "42\n");
    assert!(stub[0].1.trim_start().starts_with(">>> from_stub()"));
    assert!(examples("generated_demo").is_empty());
    let doc = item(&snapshot, "generated_demo")
        .documentation
        .as_ref()
        .unwrap();
    assert!(doc.document.blocks.iter().any(|block| {
        matches!(block, Block::CodeBlock { source, .. } if source.contains("illustrative"))
    }));
}

#[test]
#[cfg(unix)]
#[ignore = "native Python checkout input verification acceptance"]
fn retargeting_checkout_data_during_discovery_prevents_publication() {
    let root = workspace(
        r#"
from pathlib import Path as _Path

_path = _Path('defaults.txt')
_default = _path.read_text().strip()
if _Path('retarget').is_file():
    _path.unlink()
    _path.symlink_to('other.txt')

def answer(value=_default):
    return value
"#,
    );
    root.write("first.txt", "first\n");
    // The old and new targets have identical bytes. The target itself is evidence.
    root.write("other.txt", "first\n");
    std::os::unix::fs::symlink("first.txt", root.path().join("defaults.txt")).unwrap();
    let check = || cli(&root).args(["check", "--no-cache"]).output().unwrap();
    successful(check());
    root.write("retarget", "");
    let output = check();
    assert!(!output.status.success(), "accepted a retargeted input");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("inputs changed"), "{stderr}");
}

#[test]
#[cfg(target_os = "linux")]
#[ignore = "native Python dependency cache acceptance"]
fn dependency_edits_during_discovery_cannot_seed_a_cache_entry() {
    let root = workspace(
        r#"
import prepared_dependency as _dep
from pathlib import Path as _Path

def answer():
    return _dep.VALUE

answer.__doc__ = f'Dependency value: {_dep.VALUE}.'
# Model an edit after the package loaded the old bytes and before discovery
# completed. Both versions deliberately have the same size.
if _dep.VALUE == 'first':
    _Path(_dep.__file__).write_text("VALUE = 'other'\n")
__all__ = ['answer']
"#,
    );
    let dependencies = support::TestWorkspace::new();
    dependencies.write("prepared_dependency.py", "VALUE = 'first'\n");
    let build = || {
        cli(&root)
            .env("PYTHONPATH", dependencies.path())
            .args(["build", "--output", "site"])
            .output()
            .unwrap()
    };
    let first = build();
    assert!(
        !first.status.success(),
        "discovery accepted a dependency changed during import"
    );
    assert!(
        String::from_utf8_lossy(&first.stderr).contains("inputs changed"),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let database = root.path().join(".diplodocus/documentation.sqlite");
    assert!(!database.exists());
    for _ in 0..2 {
        successful(build());
        let canonical = Snapshot::load(&database)
            .unwrap()
            .canonical_export()
            .unwrap();
        assert!(canonical.contains("Dependency value: other."));
        assert!(!canonical.contains("Dependency value: first."));
    }
    assert_eq!(
        fs::read_dir(
            root.path()
                .join(".diplodocus/cache/incremental/python-native")
        )
        .unwrap()
        .count(),
        1
    );
}

#[test]
#[ignore = "native Python checkout input verification acceptance"]
fn edits_to_a_loaded_sibling_prevent_publication_with_or_without_caching() {
    for no_cache in [false, true] {
        let root = workspace("");
        root.write(
            "diplodocus.yaml",
            root.read("diplodocus.yaml").replace(
                "path: generated_demo\n",
                "path: generated_demo/api/entry.py\n",
            ),
        );
        let support = "generated_demo/support.py";
        let replacement = "VALUE = 'other'\n";
        root.write(support, "VALUE = 'first'\n");
        root.write("generated_demo/api/__init__.py", "");
        root.write(
            "generated_demo/api/entry.py",
            r#"
from pathlib import Path as _Path
from .. import support

def value():
    return support.VALUE

value.__doc__ = f'Loaded value: {support.VALUE}.'
if support.VALUE == 'first':
    _Path(support.__file__).write_text("VALUE = 'other'\n")
"#,
        );
        let build = || {
            let mut command = cli(&root);
            command.args(["build", "--output", "site"]);
            if no_cache {
                command.arg("--no-cache");
            }
            command.output().unwrap()
        };
        let output = build();
        assert!(!output.status.success(), "accepted changed input");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("inputs changed"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let database = root.path().join(".diplodocus/documentation.sqlite");
        assert!(!database.exists());
        successful(build());
        let snapshot = Snapshot::load(database).unwrap();
        let documentation = item(&snapshot, "generated_demo.api.entry.value")
            .documentation
            .as_ref()
            .unwrap();
        assert_eq!(
            documentation.raw_source.as_deref(),
            Some("Loaded value: other.")
        );
        let expected = diplodocus::provenance::fingerprint_bytes(replacement.as_bytes());
        assert!(snapshot.workspace().provenance.iter().any(|record| matches!(
            &record.activity, ProvenanceActivity::PythonRuntime { inputs, .. }
            if inputs.iter().any(|input| input.source.path.as_str() == support && input.fingerprint == expected)
        )));
    }
}

#[test]
#[cfg(target_os = "linux")]
#[ignore = "native Python checkout input cache acceptance"]
fn checkout_data_reads_need_cache_coverage() {
    let root = workspace(
        r#"
from pathlib import Path as _Path

def answer(value=_Path('defaults.txt').read_text().strip()):
    return value
"#,
    );
    root.write("defaults.txt", "first\n");
    let config = root.read("diplodocus.yaml");
    let config_path = root.path().join("diplodocus.yaml");
    let events = Arc::new(CacheEvents::default());
    let runner = CommandRunner::with_reporter(events.clone());
    let database = root.path().join(".diplodocus/documentation.sqlite");
    let extract = |expected, hit| {
        events.0.lock().unwrap().clear();
        runner
            .extract(ExtractOptions {
                config: config_path.clone(),
                output: None,
            })
            .unwrap();
        let observed = events.0.lock().unwrap();
        assert!(
            observed.contains(&("python-native".into(), hit)),
            "{observed:?}"
        );
        let snapshot = Snapshot::load(&database).unwrap();
        let answer = item(&snapshot, "generated_demo.answer");
        assert!(matches!(
            &answer.signatures[0].signature,
            Signature::Callable { parameters, .. }
                if matches!(&parameters[0].default, Some(SignatureExpression::Literal { text }) if text == &format!("{expected:?}"))
        ));
    };
    extract("first", false);
    root.write("defaults.txt", "other\n");
    extract("other", false);

    root.write(
        "diplodocus.yaml",
        config.replace(
            "    targets:",
            "    incremental:\n      extra-inputs: [defaults.txt]\n    targets:",
        ),
    );
    extract("other", false);
    extract("other", true);
}

#[test]
#[ignore = "native Python checkout input verification acceptance"]
fn editing_checkout_data_during_uncached_discovery_preserves_the_snapshot() {
    let root = workspace(
        r#"
from pathlib import Path as _Path

_path = _Path('defaults.txt')
_default = _path.read_text().strip()
if _default == 'edit':
    _path.write_text('other\n')

def answer(value=_default):
    return value
"#,
    );
    root.write(
        "diplodocus.yaml",
        root.read("diplodocus.yaml").replace(
            "    targets:",
            "    incremental:\n      extra-inputs: [defaults.txt]\n    targets:",
        ),
    );
    let extract = || cli(&root).args(["extract", "--no-cache"]).output().unwrap();
    root.write("defaults.txt", "first\n");
    successful(extract());
    let database = root.path().join(".diplodocus/documentation.sqlite");
    let original = fs::read(&database).unwrap();

    root.write("defaults.txt", "edit\n");
    let output = extract();
    assert!(
        !output.status.success(),
        "publication accepted changed input"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("inputs changed"), "{stderr}");
    assert_eq!(fs::read(database).unwrap(), original);
}

#[test]
#[cfg(target_os = "linux")]
#[ignore = "native Python symlinked dependency cache acceptance"]
fn retargeting_a_pythonpath_symlink_invalidates_discovery() {
    let root = workspace(
        "import prepared_dependency as _dep\ndef answer():\n    return _dep.VALUE\nanswer.__doc__ = f'Dependency value: {_dep.VALUE}.'\n",
    );
    let dependencies = support::TestWorkspace::new();
    dependencies.write("first/prepared_dependency.py", "VALUE = 'first'\n");
    dependencies.write("other/prepared_dependency.py", "VALUE = 'other'\n");
    let search_path = dependencies.path().join("selected");
    let link = |target| {
        std::os::unix::fs::symlink(dependencies.path().join(target), &search_path).unwrap()
    };
    link("first");
    let build = || {
        successful(
            cli(&root)
                .env("PYTHONPATH", &search_path)
                .args(["build", "--output", "site"])
                .output()
                .unwrap(),
        );
        Snapshot::load(root.path().join(".diplodocus/documentation.sqlite"))
            .unwrap()
            .canonical_export()
            .unwrap()
    };
    let first = build();
    assert!(first.contains("Dependency value: first."));
    assert_eq!(
        fs::read_dir(
            root.path()
                .join(".diplodocus/cache/incremental/python-native")
        )
        .unwrap()
        .count(),
        1
    );
    fs::remove_file(&search_path).unwrap();
    link("other");
    let changed = build();
    assert!(changed.contains("Dependency value: other."));
    assert!(!changed.contains("Dependency value: first."));
}

#[test]
#[ignore = "native Python and Sphinx acceptance"]
fn runtime_exports_generated_declarations_decorators_and_attached_docs_are_authoritative() {
    let root = workspace(
        r#"
"""Module overview.

Details
-------
Module-level section content.
"""
def _decorate(function):
    def wrapped(value: int, *, scale: int = 2) -> int:
        return value * scale
    wrapped.__name__ = function.__name__
    wrapped.__qualname__ = function.__qualname__
    wrapped.__doc__ = "Runtime wrapper documentation."
    return wrapped
@_decorate
def updated(value: str) -> str:
    """Superseded implementation documentation."""
    return value
exec('def generated(value: int = 1) -> int:\n    return value')
generated.__doc__ = "Attached documentation referencing :func:`updated`."
alias = generated
Generated = type('Generated', (), {'__module__': __name__, '__doc__': 'A generated class.'})
__all__ = [name for name in ('generated', 'alias', 'updated', 'Generated')]
"#,
    );
    let snapshot = source_snapshot(&root);
    let package = &snapshot.workspace().packages["demo"];
    let module = resolve_item(snapshot.workspace(), Some("demo"), "generated_demo").unwrap();
    let module_document = serde_json::to_string(
        &package.items[&module.item]
            .documentation
            .as_ref()
            .unwrap()
            .document,
    )
    .unwrap();
    assert!(module_document.contains("Module-level section content."));
    assert!(!module_document.contains("Runtime wrapper documentation."));
    let generated = resolve_item(
        snapshot.workspace(),
        Some("demo"),
        "generated_demo.generated",
    )
    .unwrap();
    assert_eq!(
        resolve_item(snapshot.workspace(), Some("demo"), "generated_demo.alias").unwrap(),
        generated
    );
    let item = &package.items[&generated.item];
    assert!(item.source_location.as_ref().unwrap().span.is_none());
    assert!(
        item.documentation
            .as_ref()
            .unwrap()
            .raw_source
            .as_ref()
            .unwrap()
            .starts_with("Attached documentation")
    );
    let updated =
        resolve_item(snapshot.workspace(), Some("demo"), "generated_demo.updated").unwrap();
    let item = &package.items[&updated.item];
    let Signature::Callable {
        parameters,
        returns,
    } = &item.signatures[0].signature
    else {
        panic!("callable")
    };
    assert_eq!(
        parameters
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["value", "scale"]
    );
    assert!(matches!(returns, Some(SignatureExpression::Name { name, .. }) if name == "int"));
    assert_eq!(
        item.documentation.as_ref().unwrap().raw_source.as_deref(),
        Some("Runtime wrapper documentation.")
    );
    let class = resolve_item(
        snapshot.workspace(),
        Some("demo"),
        "generated_demo.Generated",
    )
    .unwrap();
    assert_eq!(package.items[&class.item].kind, ItemKind::Class);
    assert!(
        snapshot
            .workspace()
            .provenance
            .iter()
            .any(|p| matches!(p.activity, ProvenanceActivity::PythonRuntime { .. }))
    );
    assert!(
        !snapshot
            .canonical_export()
            .unwrap()
            .contains(root.path().to_str().unwrap())
    );
}

#[test]
#[ignore = "native Python and Sphinx acceptance"]
fn unsupported_generics_are_checked_after_public_reexports_are_resolved() {
    let root = workspace("def answer(): return 42\n__all__ = ['answer']\n");
    root.write(
        "generated_demo/_helpers.py",
        "def identity[T](value: T) -> T: return value\n",
    );
    let module = "generated_demo/__init__.py";
    let source = format!(
        "from ._helpers import identity as _identity\n{}",
        root.read(module)
    );
    root.write(module, &source);
    successful(cli(&root).args(["check", "--no-cache"]).output().unwrap());

    root.write(module, format!("{source}\n__all__.append('_identity')\n"));
    let output = cli(&root).args(["check", "--no-cache"]).output().unwrap();
    assert!(!output.status.success(), "accepted a public generic");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Generic type parameters are not yet represented"),
        "{stderr}"
    );
}

#[test]
#[ignore = "native Python and Sphinx acceptance"]
fn maintained_stubs_select_signatures_and_stub_only_modules_remain_explicit() {
    let root = workspace(
        "def value(number: int) -> int:\n    \"\"\"Runtime prose.\"\"\"\n    return number\ndef label(number):\n    return str(number)\ndef hidden():\n    pass\n__all__ = ['value', 'label']\n",
    );
    root.write(
        "generated_demo/__init__.pyi",
        "def value(number: str) -> str:\n    \"\"\"Superseded stub prose.\"\"\"\n    ...\ndef label(number: str) -> str:\n    \"\"\"Maintained fallback prose.\"\"\"\n    ...\ndef hidden(): ...\n__all__ = ['value', 'hidden']\n",
    );
    root.write("generated_demo/types_only.pyi", "\"\"\"A maintained stub-only module.\"\"\"\nfrom typing import overload\n@overload\ndef choose(value: int) -> int: ...\n@overload\ndef choose(value: str) -> str: ...\n");
    let snapshot = source_snapshot(&root);
    let items = &snapshot.workspace().packages["demo"].items;
    assert!(resolve_item(snapshot.workspace(), Some("demo"), "generated_demo.hidden").is_err());
    assert!(matches!(
        &item(&snapshot, "generated_demo").language_data,
        Some(ItemLanguageData::Python(PythonItemData {
            declaration: PythonDeclaration::Module {
                exports: PythonExports::Runtime { names },
                ..
            },
            ..
        })) if names == &["value", "label"]
    ));
    let value = resolve_item(snapshot.workspace(), Some("demo"), "generated_demo.value").unwrap();
    let item = &items[&value.item];
    assert!(
        matches!(&item.signatures[0].signature, Signature::Callable { returns: Some(SignatureExpression::Name { name, .. }), .. } if name == "str")
    );
    assert!(
        item.signatures[0].sources.iter().any(|source| source
            .source
            .path
            .as_str()
            .ends_with(".pyi"))
    );
    assert_eq!(
        item.documentation.as_ref().unwrap().raw_source.as_deref(),
        Some("Runtime prose.")
    );
    let label = resolve_item(snapshot.workspace(), Some("demo"), "generated_demo.label").unwrap();
    assert_eq!(
        items[&label.item]
            .documentation
            .as_ref()
            .unwrap()
            .raw_source
            .as_deref(),
        Some("Maintained fallback prose.")
    );
    let module = items
        .values()
        .find(|item| item.qualified_name == "generated_demo.types_only")
        .unwrap();
    assert_eq!(
        module.documentation.as_ref().unwrap().raw_source.as_deref(),
        Some("A maintained stub-only module.")
    );
    assert!(matches!(
        &module.language_data,
        Some(ItemLanguageData::Python(PythonItemData {
            declaration: PythonDeclaration::Module {
                source: PythonModuleSource::StubOnly,
                ..
            },
            ..
        }))
    ));
    let choose = resolve_item(
        snapshot.workspace(),
        Some("demo"),
        "generated_demo.types_only.choose",
    )
    .unwrap();
    assert_eq!(items[&choose.item].signatures.len(), 2);
    root.write(
        "generated_demo/__init__.py",
        "from .types_only import choose\n",
    );
    let error = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap_err();
    assert!(
        matches!(error, diplodocus::assembly::AssemblyError::Diagnostics(ref diagnostics) if diagnostics.iter().any(|d| d.message.contains("types_only")))
    );
}

#[test]
#[ignore = "native Python and Sphinx acceptance"]
fn matching_stub_reexports_keep_their_intermediate_binding() {
    let root = workspace("from .facade import choose as choose\n");
    let stub = "generated_demo/__init__.pyi";
    root.write(stub, "from .facade import choose as choose\n");
    root.write("generated_demo/facade.py", "from .impl import choose\n");
    root.write(
        "generated_demo/impl.py",
        "def choose() -> int:\n    return 1\ndef other() -> int:\n    return 2\n",
    );
    let snapshot = source_snapshot(&root);
    let public = item(&snapshot, "generated_demo.choose");
    assert_eq!(public.qualified_name, "generated_demo.impl.choose");
    let alias = public
        .aliases
        .iter()
        .find(|alias| alias.qualified_name == "generated_demo.choose")
        .unwrap();
    assert!(
        alias
            .sources
            .iter()
            .any(|source| source.source.path.as_str() == stub)
    );
    assert!(alias.sources.iter().any(|source| {
        source.source.path.as_str() == "generated_demo/__init__.py" && source.source.span.is_some()
    }));
    assert!(
        resolve_item(
            snapshot.workspace(),
            Some("demo"),
            "generated_demo.facade.choose"
        )
        .is_err()
    );
    root.write(stub, "from .impl import other as choose\n");
    let error = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap_err();
    assert!(
        matches!(error, diplodocus::assembly::AssemblyError::Diagnostics(diagnostics)
        if diagnostics.iter().any(|d| d.code == diplodocus::diagnostics::DiagnosticCode::PythonConflictingStub))
    );
}

#[test]
#[ignore = "native Python 3.12+ type alias and Sphinx acceptance"]
fn maintained_lazy_type_aliases_preserve_symbolic_targets() {
    let version = std::process::Command::new("python")
        .args(["-c", "import sys; print(sys.version_info >= (3, 12))"])
        .output()
        .unwrap();
    assert!(version.status.success());
    if version.stdout == b"False\n" {
        return;
    }
    let root = workspace(
        r#"
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from ._types import TypeOnly

type Text = TypeOnly
"""Public alias documentation."""

def identity(value: Text) -> Text:
    return value
"#,
    );
    root.write("generated_demo/_types.py", "TypeOnly = str\n");
    let snapshot = source_snapshot(&root);
    let alias = item(&snapshot, "generated_demo.Text");
    assert!(matches!(
        &alias.language_data,
        Some(ItemLanguageData::Python(PythonItemData {
            declaration: PythonDeclaration::TypeAlias {
                target: SignatureExpression::Name { name, .. },
            },
            ..
        })) if name.ends_with("TypeOnly")
    ));
    assert_eq!(
        alias.documentation.as_ref().unwrap().raw_source.as_deref(),
        Some("Public alias documentation.")
    );
}

#[test]
#[ignore = "native Python signatures and descriptors acceptance"]
fn observed_defaults_and_property_descriptors_define_the_runtime_surface() {
    fn build() -> String {
        let root = workspace(
            r#"
from pathlib import Path

def updated(value=1, *, scale=3):
    return value * scale
updated.__defaults__ = (2,)
updated.__kwdefaults__ = {"scale": 4}
exec("def generated(value=Path(__file__).parent):\n    return value")

class Box:
    def read(self):
        """Read the assigned property."""
    def write(self, value):
        pass
    def delete(self):
        pass
    value = property(read, write, delete)
"#,
        );
        let snapshot = source_snapshot(&root);
        let updated = item(&snapshot, "generated_demo.updated");
        let Signature::Callable { parameters, .. } = &updated.signatures[0].signature else {
            panic!("expected callable");
        };
        for (parameter, expected) in parameters.iter().zip(["2", "4"]) {
            assert!(
                matches!(&parameter.default, Some(SignatureExpression::Literal { text }) if text == expected)
            );
        }
        let generated = item(&snapshot, "generated_demo.generated");
        assert!(generated.signatures.is_empty());
        assert!(snapshot.workspace().diagnostics.iter().any(|d| {
            d.message
                .contains("`generated_demo.generated` has no representable signature")
        }));
        let property = item(&snapshot, "generated_demo.Box.value");
        assert!(matches!(
            &property.language_data,
            Some(ItemLanguageData::Python(PythonItemData {
                declaration: PythonDeclaration::Property {
                    has_setter: true,
                    has_deleter: true,
                },
                ..
            }))
        ));
        assert_eq!(
            property
                .documentation
                .as_ref()
                .unwrap()
                .raw_source
                .as_deref(),
            Some("Read the assigned property.")
        );
        let canonical = snapshot.canonical_export().unwrap();
        assert!(!canonical.contains(root.path().to_str().unwrap()));
        canonical
    }
    assert_eq!(build(), build());
}

#[test]
#[ignore = "native Python signatures and descriptors acceptance"]
fn maintained_property_signatures_describe_slotted_attributes() {
    let root = workspace(
        r#"
from dataclasses import dataclass

class Box:
    __slots__ = ('value', 'other')

@dataclass(slots=True)
class Record:
    value: int
"#,
    );
    root.write(
        "generated_demo/__init__.pyi",
        "class Box:\n    @property\n    def value(self) -> int: ...\n    other: str\n\nclass Record:\n    @property\n    def value(self) -> int: ...\n",
    );
    let snapshot = source_snapshot(&root);
    let property = item(&snapshot, "generated_demo.Box.value");
    assert!(matches!(
        &property.language_data,
        Some(ItemLanguageData::Python(PythonItemData {
            declaration: PythonDeclaration::Property { .. },
            ..
        }))
    ));
    let signature = &property.signatures[0];
    assert!(matches!(
        &signature.signature,
        Signature::Callable { returns: Some(SignatureExpression::Name { name, .. }), .. } if name == "int"
    ));
    assert_eq!(
        signature.sources[0].source.path.as_str(),
        "generated_demo/__init__.pyi"
    );
    assert_eq!(
        item(&snapshot, "generated_demo.Record.value").signatures[0].signature,
        signature.signature
    );
    assert!(matches!(
        &item(&snapshot, "generated_demo.Box.other").language_data,
        Some(ItemLanguageData::Python(PythonItemData {
            declaration: PythonDeclaration::Field,
            ..
        }))
    ));
    root.write("generated_demo/__init__.py", "class Box:\n    value = 42\n");
    assert!(matches!(
        assemble_workspace(root.path().join("diplodocus.yaml")),
        Err(diplodocus::assembly::AssemblyError::Diagnostics(diagnostics))
            if diagnostics.iter().any(|d| d.code == diplodocus::diagnostics::DiagnosticCode::PythonConflictingStub)
    ));
}

#[test]
#[ignore = "native Python overload source evidence acceptance"]
fn maintained_overloads_link_to_their_definitions_and_implementation() {
    let source = r#"
from typing import overload

@overload
def choose(value: int) -> int: ...
@overload
def choose(value: str) -> str: ...
def choose(value):
    """Implementation prose."""
    return value
"#;
    let root = workspace(source);
    let snapshot = source_snapshot(&root);
    let family = item(&snapshot, "generated_demo.choose");
    let source_for = |item: &Item| {
        let span = item.source_location.as_ref().unwrap().span.unwrap();
        &source[span.start..span.end]
    };
    assert!(source_for(family).starts_with("def choose(value):"));
    let Some(ItemLanguageData::Python(PythonItemData {
        declaration:
            PythonDeclaration::Callable {
                role: PythonCallableRole::Family { overloads },
                ..
            },
        ..
    })) = &family.language_data
    else {
        panic!("expected callable family");
    };
    assert_eq!(overloads.len(), 2);
    for (reference, expected) in overloads.iter().zip(["int", "str"]) {
        let overload = &snapshot.workspace().packages["demo"].items[&reference.item];
        assert!(
            source_for(overload).contains(&format!("def choose(value: {expected}) -> {expected}"))
        );
        assert_eq!(
            overload.source_location.as_ref(),
            Some(&overload.signatures[0].sources[0].source)
        );
    }
}

#[test]
#[ignore = "native Python and Sphinx acceptance"]
fn file_targets_keep_imported_support_modules_out_of_the_public_surface() {
    let root = workspace("def root_only():\n    pass\n");
    let configuration = root
        .read("diplodocus.yaml")
        .replace("path: generated_demo\n", "path: generated_demo/api.py\n");
    let configuration = configuration
        + concat!(
            "      - id: sibling\n",
            "        extractor: python\n",
            "        path: generated_demo/sibling.py\n",
            "        role: public-api\n",
        );
    root.write("diplodocus.yaml", &configuration);
    root.write("generated_demo/sibling.py", "def another():\n    pass\n");
    root.write("generated_demo/api.py", "from ._implementation import selected as selected\nfrom . import unrelated\n__all__ = ['selected']\n");
    root.write("generated_demo/_implementation.py", "def selected(value: int) -> int:\n    \"\"\"Selected implementation.\"\"\"\n    return value\n");
    root.write(
        "generated_demo/unrelated.py",
        "def sibling_only():\n    pass\n",
    );
    let snapshot = source_snapshot(&root);
    let selected = resolve_item(
        snapshot.workspace(),
        Some("demo"),
        "generated_demo.api.selected",
    )
    .unwrap();
    assert_eq!(
        snapshot.workspace().packages["demo"].items[&selected.item].qualified_name,
        "generated_demo._implementation.selected"
    );
    assert_eq!(
        item(&snapshot, "generated_demo.sibling.another").kind,
        ItemKind::Function
    );
    let parent = item(&snapshot, "generated_demo");
    assert!(!parent.discoverable());
    assert!(
        snapshot.workspace().packages["demo"]
            .items
            .values()
            .all(|item| {
                !item.qualified_name.ends_with("root_only")
                    && !item.qualified_name.ends_with("sibling_only")
            })
    );
    let configuration = configuration
        + concat!(
            "      - id: parent\n",
            "        extractor: python\n",
            "        path: generated_demo/__init__.py\n",
            "        role: public-api\n",
        );
    root.write("diplodocus.yaml", &configuration);
    let snapshot = source_snapshot(&root);
    assert!(item(&snapshot, "generated_demo").discoverable());
    assert_eq!(
        item(&snapshot, "generated_demo.root_only").kind,
        ItemKind::Function
    );
    root.write(
        "diplodocus.yaml",
        configuration
            + concat!(
                "      - id: parent-again\n",
                "        extractor: python\n",
                "        path: generated_demo/__init__.py\n",
                "        role: public-api\n",
            ),
    );
    let error = assemble_workspace(root.path().join("diplodocus.yaml")).unwrap_err();
    assert!(
        matches!(error, diplodocus::assembly::AssemblyError::Diagnostics(diagnostics)
        if diagnostics.iter().any(|d| d.code == diplodocus::diagnostics::DiagnosticCode::ConflictingExtraction))
    );
}

#[test]
#[ignore = "native Python file target acceptance"]
fn disjoint_file_targets_merge_reexports_into_the_selected_definition() {
    let root = workspace("");
    root.write("generated_demo/api/__init__.py", "");
    root.write(
        "generated_demo/api/public.py",
        "from ..impl import greet as hello\n",
    );
    root.write(
        "generated_demo/impl.py",
        "def greet(name: str) -> str:\n    \"\"\"Greet by name.\"\"\"\n    return name\n\ndef unrelated(): pass\n",
    );
    let config = root.read("diplodocus.yaml");
    let (header, _) = config.split_once("    targets:\n").unwrap();
    let target = |id, file| {
        format!(
            "      - id: {id}\n        extractor: python\n        path: generated_demo/{file}.py\n        role: public-api\n"
        )
    };
    let api = target("api", "api/public");
    let defining = target("impl", "impl");
    root.write("diplodocus.yaml", format!("{header}    targets:\n{api}"));
    let snapshot = source_snapshot(&root);
    let canonical = item(&snapshot, "generated_demo.impl.greet");
    assert_eq!(
        item(&snapshot, "generated_demo.api.public.hello"),
        canonical
    );
    assert_eq!(
        canonical
            .documentation
            .as_ref()
            .unwrap()
            .source_location
            .as_ref()
            .unwrap()
            .path
            .as_str(),
        "generated_demo/impl.py"
    );
    assert!(
        resolve_item(
            snapshot.workspace(),
            Some("demo"),
            "generated_demo.impl.unrelated"
        )
        .is_err()
    );
    for (first, second) in [(&api, &defining), (&defining, &api)] {
        root.write(
            "diplodocus.yaml",
            format!("{header}    targets:\n{first}{second}"),
        );
        let snapshot = source_snapshot(&root);
        let canonical = item(&snapshot, "generated_demo.impl.greet");
        assert_eq!(
            item(&snapshot, "generated_demo.api.public.hello"),
            canonical
        );
        let source = canonical.source_location.as_ref().unwrap();
        assert_eq!(source.path.as_str(), "generated_demo/impl.py");
        assert!(
            source.span.is_some(),
            "definition lost its selected source span"
        );
        assert_eq!(
            canonical
                .documentation
                .as_ref()
                .unwrap()
                .raw_source
                .as_deref(),
            Some("Greet by name.")
        );
    }
    root.write(
        "diplodocus.yaml",
        format!(
            "{header}    targets:\n{api}{defining}{}",
            target("duplicate", "impl")
        ),
    );
    assert!(matches!(
        assemble_workspace(root.path().join("diplodocus.yaml")),
        Err(diplodocus::assembly::AssemblyError::Diagnostics(diagnostics))
            if diagnostics.iter().any(|d| d.code == diplodocus::diagnostics::DiagnosticCode::ConflictingExtraction)
    ));
}

#[test]
#[ignore = "native Python generated module and doctest acceptance"]
fn generated_children_use_their_own_exports_and_the_parent_source_file() {
    let root = workspace(
        r#"
import sys as _sys
from types import ModuleType as _ModuleType

runtime = _ModuleType(__name__ + '.runtime')
exec('''
_value = 42

def answer() -> int:
    """Return the generated answer.

    >>> _value
    42
    """
    return _value

__all__ = ['answer']
''', vars(runtime))
_sys.modules[runtime.__name__] = runtime
_sys.modules[__name__ + '.unbound'] = _ModuleType(__name__ + '.unbound')
__all__ = ['runtime']
"#,
    );
    successful(cli(&root).arg("extract").output().unwrap());
    let snapshot = Snapshot::load(root.path().join(".diplodocus/documentation.sqlite")).unwrap();
    let runtime = item(&snapshot, "generated_demo.runtime");
    assert!(matches!(
        &runtime.language_data,
        Some(ItemLanguageData::Python(PythonItemData {
            declaration: PythonDeclaration::Module {
                source: PythonModuleSource::Runtime,
                ..
            },
            ..
        }))
    ));
    assert_eq!(
        runtime.source_location.as_ref().unwrap().path.as_str(),
        "generated_demo/__init__.py"
    );
    let answer = item(&snapshot, "generated_demo.runtime.answer");
    assert!(
        answer
            .documentation
            .as_ref()
            .unwrap()
            .provenance
            .iter()
            .any(|record| matches!(
                &record.activity,
                ProvenanceActivity::ApiExamples { examples, .. } if examples[0].actual == "42\n"
            ))
    );
    assert!(resolve_item(snapshot.workspace(), Some("demo"), "generated_demo.unbound").is_err());
}

#[test]
#[ignore = "native Python and Sphinx acceptance"]
fn namespace_packages_support_native_discovery_and_doctests() {
    let root = workspace("");
    root.remove("generated_demo/__init__.py");
    root.write("generated_demo/values.py", "def answer() -> int:\n    \"\"\"Return the answer.\n\n    >>> answer()\n    42\n    \"\"\"\n    return 42\n");
    CommandRunner::default()
        .build(BuildOptions {
            config: root.path().join("diplodocus.yaml"),
            output: root.path().join("site"),
        })
        .unwrap();
    let result = Snapshot::load(root.path().join(".diplodocus/documentation.sqlite")).unwrap();
    assert!(
        result.workspace().packages["demo"]
            .items
            .values()
            .any(|item| {
                item.documentation.as_ref().is_some_and(|doc| {
                    doc.provenance.iter().any(|record| {
                        matches!(&record.activity, ProvenanceActivity::ApiExamples { .. })
                    })
                })
            })
    );
}

#[test]
#[ignore = "prepared Python virtual environment and Sphinx cache acceptance"]
fn dependency_changes_inside_a_checkout_virtual_environment_invalidate_discovery() {
    let root = workspace(
        "import prepared_dependency as _dependency\ndef answer():\n    return _dependency.value\nanswer.__doc__ = f'Dependency value: {_dependency.value}.'\n__all__ = ['answer']\n",
    );
    let environment = root.path().join(".venv");
    let prepared = std::process::Command::new("python")
        .args(["-m", "venv", "--without-pip", "--system-site-packages"])
        .arg(&environment)
        .output()
        .unwrap();
    assert!(
        prepared.status.success(),
        "{}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    let python = environment.join("bin/python");
    let library = std::process::Command::new(&python)
        .args([
            "-c",
            "import sysconfig; print(sysconfig.get_path('purelib'))",
        ])
        .output()
        .unwrap();
    assert!(library.status.success());
    let library = std::path::PathBuf::from(String::from_utf8(library.stdout).unwrap().trim());
    assert!(library.starts_with(&environment));
    let dependency = library.join("prepared_dependency.py");
    fs::write(&dependency, "__version__ = '1.0.0'\nvalue = 'first'\n").unwrap();
    let paths = std::env::join_paths(std::iter::once(environment.join("bin")).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    let build = || {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_diplodocus"))
            .current_dir(root.path())
            .env("PATH", &paths)
            .args(["build", "--output", "site"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Snapshot::load(root.path().join(".diplodocus/documentation.sqlite"))
            .unwrap()
            .canonical_export()
            .unwrap()
    };
    let first = build();
    assert!(first.contains("Dependency value: first."));
    assert_eq!(first, build());
    // Keep the advertised version and file length unchanged.
    fs::write(&dependency, "__version__ = '1.0.0'\nvalue = 'other'\n").unwrap();
    let changed = build();
    assert!(changed.contains("Dependency value: other."));
    assert!(!changed.contains("Dependency value: first."));
    assert!(!changed.contains("prepared_dependency.py"));
}

#[derive(Default)]
struct CacheEvents(Mutex<Vec<(String, bool)>>);

impl Reporter for CacheEvents {
    fn report(&self, event: Event<'_>) {
        if let Event::Cache { stage, hit, .. } = event {
            self.0.lock().unwrap().push((stage.into(), hit));
        }
    }
}

#[test]
#[ignore = "native Python optimization cache acceptance"]
fn discovery_cache_respects_python_optimization() {
    let root = workspace("def answer():\n    \"\"\"Return an answer.\"\"\"\n    return 42\n");
    let database = root.path().join(".diplodocus/documentation.sqlite");
    let build = |optimize: &str, no_cache| {
        let mut command = cli(&root);
        command
            .env("PYTHONOPTIMIZE", optimize)
            .args(["build", "--output", "site"]);
        if no_cache {
            command.arg("--no-cache");
        }
        successful(command.output().unwrap());
        Snapshot::load(&database).unwrap()
    };
    let standard = build("0", false);
    assert!(
        item(&standard, "generated_demo.answer")
            .documentation
            .is_some()
    );
    let optimized = build("2", false);
    assert!(
        item(&optimized, "generated_demo.answer")
            .documentation
            .is_none()
    );
    let uncached = build("2", true);
    assert_eq!(
        optimized.canonical_export().unwrap(),
        uncached.canonical_export().unwrap()
    );
}

#[test]
#[ignore = "native Python and Sphinx incremental acceptance"]
fn discovery_and_doctests_reuse_independently_and_failures_preserve_the_site() {
    const SOURCE: &str = "__all__ = ['answer']\ndef answer(value: int = 1) -> int:\n    \"\"\"Return a value.\n\n    >>> answer(2)\n    2\n    \"\"\"\n    return value\n";
    let root = workspace(SOURCE);
    let events = Arc::new(CacheEvents::default());
    let runner = CommandRunner::with_reporter(events.clone());
    let config = root.path().join("diplodocus.yaml");
    let options = BuildOptions {
        config: config.clone(),
        output: root.path().join("site"),
    };
    runner.check(CheckOptions { config }).unwrap();
    assert!(
        !events
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|(stage, _)| stage == "python-api-examples")
    );
    runner.build(options.clone()).unwrap();
    events.0.lock().unwrap().clear();
    runner.build(options.clone()).unwrap();
    let observed = events.0.lock().unwrap().clone();
    assert!(
        observed.contains(&("python-native".into(), true)),
        "{observed:?}"
    );
    assert!(
        observed.contains(&("python-api-examples".into(), true)),
        "{observed:?}"
    );
    let database = root.path().join(".diplodocus/documentation.sqlite");
    let original = Snapshot::load(&database)
        .unwrap()
        .canonical_export()
        .unwrap();
    let site = fs::read(root.path().join("site/index.html")).unwrap();
    root.write(
        "generated_demo/__init__.py",
        "raise RuntimeError('discovery failed')\n",
    );
    assert!(runner.build(options.clone()).is_err());
    assert_eq!(
        original,
        Snapshot::load(&database)
            .unwrap()
            .canonical_export()
            .unwrap()
    );
    assert_eq!(site, fs::read(root.path().join("site/index.html")).unwrap());
    root.write(
        "generated_demo/__init__.py",
        SOURCE.replace("Return a value.", "Updated source documentation."),
    );
    events.0.lock().unwrap().clear();
    runner.build(options).unwrap();
    let observed = events.0.lock().unwrap().clone();
    assert!(
        observed.contains(&("python-native".into(), false)),
        "{observed:?}"
    );
    let published = Snapshot::load(&database).unwrap();
    assert!(
        published
            .canonical_export()
            .unwrap()
            .contains("Updated source documentation.")
    );
    fs::remove_dir_all(root.path().join("generated_demo")).unwrap();
    generate(GenerateOptions {
        input: database,
        output: root.path().join("detached"),
    })
    .unwrap();
    assert_eq!(
        fs::read(root.path().join("site/index.html")).unwrap(),
        fs::read(root.path().join("detached/index.html")).unwrap()
    );
}

#[test]
#[ignore = "prepared compiled Python extension and Sphinx acceptance"]
fn compiled_extensions_keep_runtime_documentation_and_maintained_stub_signatures() {
    let root = support::prepared_acceptance_workspace();
    root.write("diplodocus.yaml", "project:\n  name: Native Python\nrepositories:\n  - id: repo\n    path: .\npackages:\n  - id: python\n    name: Foo\n    slug: python\n    ecosystem: python\n    repository: repo\n    path: python\n    metadata-path: pyproject.toml\n    targets:\n      - id: api\n        extractor: python\n        path: python/foo\n        role: public-api\n");
    let snapshot = source_snapshot(&root);
    let item = resolve_item(snapshot.workspace(), Some("python"), "foo.native_mean").unwrap();
    let item = &snapshot.workspace().packages["python"].items[&item.item];
    assert!(!item.signatures.is_empty());
    assert!(
        item.signatures[0].sources.iter().any(|source| source
            .source
            .path
            .as_str()
            .ends_with(".pyi"))
    );
    assert!(
        item.documentation
            .as_ref()
            .is_some_and(|doc| doc.raw_source.as_ref().is_some_and(|text| !text.is_empty()))
    );
    assert!(snapshot.workspace().provenance.iter().any(|record| matches!(&record.activity,
        ProvenanceActivity::PythonRuntime { inputs, .. } if inputs.iter().any(|input| input.source.path.as_str().contains("_native") && input.source.path.as_str().ends_with(".so")))));
}
