"""Export prepared Python APIs through public Sphinx extension interfaces.

The exchange is private and versioned. Object addresses and temporary Sphinx
paths never cross the portable adapter boundary. No examples run in this build.
"""

import ast
import contextlib
import dataclasses
import difflib
import hashlib
import importlib
import importlib.metadata
import importlib.util
import inspect
import io
import itertools
import json
import keyword
import os
import platform
import re
import runpy
import sys
import threading
import time
import types
import typing
from pathlib import Path

environment_helper = runpy.run_path(str(Path(__file__).with_name("python_environment.py")))
observe_environment = environment_helper["observe_environment"]
checkout_module_file = environment_helper["checkout_module_file"]

if sys.version_info >= (3, 14):
    import annotationlib


MISSING = object()
TYPE_ALIAS_TYPE = getattr(typing, "TypeAliasType", ())


def annotations(obj):
    if sys.version_info >= (3, 14):
        return annotationlib.get_annotations(obj, format=annotationlib.Format.STRING)
    return inspect.get_annotations(obj, eval_str=False)


def annotation(value):
    if isinstance(value, str):
        return value
    if isinstance(value, typing.ForwardRef):
        return value.__forward_arg__
    return inspect.formatannotation(value).replace("~", "")


def class_base(base):
    # Protocol can be a class, and NamedTuple and TypedDict can be factories.
    # inspect.formatannotation may render their repr instead of a reference.
    owner = getattr(base, "__module__", None)
    name = getattr(base, "__name__", None)
    if owner in ("typing", "typing_extensions") and name:
        module = sys.modules.get(owner)
        if module is not None and vars(module).get(name) is base:
            return f"{owner}.{name}"
    return annotation(base)


@dataclasses.dataclass(frozen=True)
class Expression:
    text: str

    def __repr__(self):
        return self.text


def signature(obj, local_paths):
    try:
        options = {"eval_str": False}
        if sys.version_info >= (3, 14):
            options["annotation_format"] = annotationlib.Format.STRING
        value = inspect.signature(obj, **options)
    except (ValueError, TypeError):
        return MISSING
    try:
        if sys.version_info >= (3, 14):
            rendered = value.format(quote_annotation_strings=False)
        else:
            # Older inspect versions quote postponed annotations in str().
            def unquote(value):
                return Expression(value) if isinstance(value, str) else value

            rendered = str(value.replace(
                parameters=[parameter.replace(annotation=unquote(parameter.annotation))
                            for parameter in value.parameters.values()],
                return_annotation=unquote(value.return_annotation),
            ))
        return rendered if portable_signature(rendered, local_paths) else None
    except (ValueError, TypeError):
        return None


def portable_signature(rendered, local_paths):
    try:
        tree = ast.parse("def _signature" + rendered + ": pass")
    except SyntaxError:
        return False
    return not contains_local_path(tree, local_paths)


def contains_local_path(tree, local_paths):
    # Check decoded constants so escaping or nesting a path cannot hide it.
    return any(
        isinstance(node, ast.Constant) and isinstance(node.value, (str, bytes))
        and any(path in os.fsdecode(node.value) for path in local_paths)
        for node in ast.walk(tree)
    )


def source_line(obj, path):
    code = getattr(inspect.unwrap(obj), "__code__", None)
    if code and Path(code.co_filename).resolve() == path:
        return code.co_firstlineno
    if inspect.isclass(obj):
        return vars(obj).get("__firstlineno__")
    return None


def export_names(module):
    names = getattr(module, "__all__", MISSING)
    if names is MISSING:
        return None
    message = f"{module.__name__}.__all__ must be a sequence of names"
    if isinstance(names, dict):
        raise TypeError(message)
    result = []
    # Star imports use the sequence's indexed access, stopping at IndexError.
    for index in itertools.count():
        try:
            member = names[index]
        except IndexError:
            return result
        except (KeyError, TypeError) as error:
            raise TypeError(message) from error
        if not isinstance(member, str):
            raise TypeError(message)
        result.append(member)


def literal(value, local_paths):
    try:
        spelling = repr(value)
        tree = ast.parse(spelling, mode="eval")
        ast.literal_eval(tree)
        return None if contains_local_path(tree, local_paths) else spelling
    except (ValueError, SyntaxError, TypeError):
        return None


def dotted_name(expression):
    if isinstance(expression, ast.Name):
        return (expression.id,)
    if isinstance(expression, ast.Attribute) and (parent := dotted_name(expression.value)):
        return (*parent, expression.attr)
    return None


@dataclasses.dataclass(frozen=True)
class Binding:
    module: str
    members: tuple[str, ...]
    kind: str
    line: int
    explicit: bool = True


def branch_statements(statement):
    for node in ast.iter_child_nodes(statement):
        if isinstance(node, (ast.stmt, ast.ExceptHandler, ast.match_case)):
            yield node
            if not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
                yield from branch_statements(node)


def maintained(module):
    """Collect possible aliases for verification against the final runtime bindings."""
    bindings = {}
    aliases = {}
    path = getattr(module, "__file__", None)
    if not path or not path.endswith(".py"):
        return bindings, aliases

    def type_alias_marker(expression):
        if isinstance(expression, ast.Name):
            return vars(module).get(expression.id) is typing.TypeAlias
        if isinstance(expression, ast.Attribute) and isinstance(expression.value, ast.Name):
            owner = vars(module).get(expression.value.id)
            return inspect.getattr_static(owner, expression.attr, None) is typing.TypeAlias
        return False

    def forget(name):
        bindings.pop(name, None)
        aliases.pop(name, None)

    def record_import(statement, conditional=False):
        for name in statement.names:
            if name.name == "*":
                continue
            local = name.asname or name.name.split(".")[0]
            if not conditional:
                forget(local)
            if isinstance(statement, ast.ImportFrom):
                prefix = "." * statement.level + (statement.module or "")
                if statement.level:
                    try:
                        prefix = importlib.util.resolve_name(prefix, module.__package__)
                    except ImportError:
                        if conditional:
                            continue
                        raise
                target, members = prefix, (name.name,)
            else:
                target, members = (name.name if name.asname else local), ()
            bindings.setdefault(local, []).append(
                Binding(target, members, "import", statement.lineno, name.asname is not None)
            )

    for statement in ast.parse(Path(path).read_bytes()).body:
        if isinstance(statement, (ast.Import, ast.ImportFrom)):
            record_import(statement)
        elif isinstance(statement, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            forget(statement.name)
        elif isinstance(statement, (ast.Assign, ast.AugAssign, ast.Delete)):
            targets = (statement.target,) if isinstance(statement, ast.AugAssign) else statement.targets
            for target in targets:
                for node in ast.walk(target):
                    if isinstance(node, ast.Name) and isinstance(node.ctx, (ast.Store, ast.Del)):
                        forget(node.id)
                if (isinstance(statement, ast.Assign) and isinstance(target, ast.Name)
                        and (members := dotted_name(statement.value))):
                    bindings[target.id] = [Binding(module.__name__, members, "assignment", statement.lineno)]
        elif isinstance(statement, ast.AnnAssign) and isinstance(statement.target, ast.Name):
            if statement.value is not None:
                forget(statement.target.id)
            if type_alias_marker(statement.annotation):
                aliases[statement.target.id] = None
        elif isinstance(statement, getattr(ast, "TypeAlias", ())):
            forget(statement.name.id)
            aliases[statement.name.id] = (ast.unparse(statement.value), statement.lineno)
        else:
            for branch in branch_statements(statement):
                if isinstance(branch, (ast.Import, ast.ImportFrom)):
                    record_import(branch, conditional=True)
    return bindings, aliases


class InputObservations:
    """Fingerprint checkout files before import or Sphinx reads them."""

    def __init__(self, root, stage):
        self.root = root
        self.stage = stage
        self.fingerprints = {}
        self.targets = {}
        self.changed = set()
        self.reading = threading.local()

    def audit(self, event, args):
        if event == "open" and not args[2] & os.O_WRONLY:
            path = args[0]
        elif event == "import":
            # Extension modules are opened by the native loader.
            path = args[1]
        else:
            return
        if not isinstance(path, (str, bytes)) or getattr(self.reading, "active", False):
            return
        try:
            logical = Path(os.fsdecode(path))
            if not logical.is_absolute():
                logical = Path.cwd() / logical
            path = logical.resolve()
            if not path.is_relative_to(self.root) or path.is_relative_to(self.stage) or not path.is_file():
                return
            self.reading.active = True
            with path.open("rb") as stream:
                digest = hashlib.file_digest(stream, "sha256").hexdigest()
        except (OSError, RuntimeError, ValueError):
            # Optional or failed reads need no evidence unless they contribute.
            return
        finally:
            self.reading.active = False
        if self.fingerprints.setdefault(path, digest) != digest:
            self.changed.add(path)
        previous = self.targets.setdefault(logical, path)
        if previous != path:
            self.changed.add(previous)


def run(request_path, output_path, observe_only=False):
    request = json.loads(Path(request_path).read_bytes())
    root = Path(request["package_root"]).resolve()
    repository = Path(request["repository_root"]).resolve()
    stage = Path(request_path).parent.resolve()
    sys.path[:0] = request["import_roots"]
    sys.pycache_prefix = str(stage / "bytecode")
    sys.dont_write_bytecode = True
    checkout_inputs = InputObservations(root, stage)
    startup_modules = set(sys.modules)
    if not observe_only:
        sys.addaudithook(checkout_inputs.audit)
    # Native documentation and maintained stub signatures are collected separately.
    os.environ["SPHINX_AUTODOC_IGNORE_NATIVE_MODULE_TYPE_STUBS"] = "1"
    import docutils
    import sphinx
    from docutils import nodes
    from sphinx import addnodes
    from sphinx.application import Sphinx
    from sphinx.builders import Builder

    version = tuple(int(part) for part in sphinx.__version__.split(".")[:3])
    if not (version >= (9, 0, 4) and version < (10,)):
        raise ValueError("Supported Sphinx versions are >=9.0.4 and <10")

    def environment_observation():
        try:
            return observe_environment(request_path, root, repository, request["tracked_source_paths"])
        except (OSError, RuntimeError, TypeError, ValueError):
            return {"search_paths": [], "runtime_paths": [], "used_paths": [], "flags": [], "eligible": False}

    if observe_only:
        # This process starts separately from discovery. Rust fingerprints the
        # available inputs before the process performing discovery imports them.
        # Sphinx and doctest load these optional standard-library extensions
        # after startup. Their linked libraries can live outside sys.path.
        for name in ("readline", "ssl", "uuid", "xml.parsers.expat"):
            try:
                importlib.import_module(name)
            except ImportError:
                pass
        Path(output_path).write_text(json.dumps(environment_observation()), encoding="utf-8")
        return
    started = time.monotonic()
    modules = {}
    paths = {}
    syntax = {}
    exports = {}
    explicit_bindings = {}

    def add_module(name, module, path):
        modules[name] = module
        paths[name] = str(path)
        syntax[name] = maintained(module)
        names = export_names(module)
        if names is None:
            return
        exports[name] = names
        for member in dict.fromkeys(names):
            if not member.isidentifier() or keyword.iskeyword(member):
                raise ValueError(f"Export {name}.{member} is not a Python identifier")
            obj = getattr(module, member, MISSING)
            if obj is MISSING:
                raise ValueError(f"Export {name}.{member} has no runtime binding")
            explicit_bindings[name, member] = obj

    selected = {entry["name"] for entry in request["modules"]}
    for entry in request["modules"]:
        name = entry["name"]
        try:
            module = importlib.import_module(name)
        except ModuleNotFoundError as error:
            if entry["stub_only"] and error.name and (error.name == name or name.startswith(error.name + ".")):
                continue
            raise
        path = getattr(module, "__file__", None)
        if path:
            path = Path(path).resolve()
            if not path.is_relative_to(root):
                raise ValueError(f"Loaded module {name} does not match the supplied checkout")
            expected_path = Path(entry["path"]).resolve()
            if not entry["stub_only"] and path != expected_path:
                raise ValueError(f"Loaded module {name} does not match the selected module file")
            if entry["stub_only"] and path.parent != expected_path.parent:
                raise ValueError(f"Loaded module {name} does not match its maintained stub directory")
        else:
            path = checkout_module_file(module, root) if entry["stub_only"] else None
            if path is None:
                locations = getattr(module, "__path__", ())
                if not any(Path(p).resolve().is_relative_to(root) for p in locations):
                    raise ValueError(f"Module {name} does not match the supplied checkout")
                path = entry["path"]
        advertised = getattr(module, "__version__", None)
        if name.count(".") == 0 and advertised is not None and str(advertised) != request["package_version"]:
            raise ValueError("Loaded package version conflicts with checkout metadata")
        add_module(name, module, path)
    # Native extensions and modules created by the package may have no .py or
    # .pyi input. Retain loaded modules inside the explicit target boundary.
    target = Path(request["target_path"]).resolve()
    boundary = target if target.is_dir() else target.parent
    for name, module in sorted(sys.modules.items()):
        if name in modules or getattr(module, "__name__", None) != name:
            continue
        path = checkout_module_file(module, root)
        if (path is not None and path.is_file() and path.is_relative_to(boundary)
                and not path.is_relative_to(stage)):
            add_module(name, module, path)
    def lookup(module, members):
        value = sys.modules.get(module, MISSING)
        for index, member in enumerate(members):
            if index == 0 and (module, member) in explicit_bindings:
                value = explicit_bindings[module, member]
            else:
                value = inspect.getattr_static(value, member, MISSING)
        return value

    library_roots = environment_helper["runtime_library_roots"]()

    def add_support(name):
        if not name or name in modules:
            return
        module = sys.modules.get(name)
        path = checkout_module_file(module, root)
        if (getattr(module, "__name__", None) == name and path is not None
                and path.is_file() and not path.is_relative_to(stage)
                and not any(path.is_relative_to(library) for library in library_roots)):
            add_module(name, module, path)
            pending.append(name)

    # Follow verified runtime bindings into checkout modules outside the target.
    # The queue also visits intermediate modules in a chain of reexports.
    pending = list(modules)
    verified = {}
    for name in pending:
        bindings, _ = syntax[name]
        for member, candidates in bindings.items():
            obj = lookup(name, (member,))
            for binding in reversed(candidates):
                if obj is not MISSING and obj is lookup(binding.module, binding.members):
                    verified[name, member] = binding
                    add_support(binding.module)
                    for index in range(1, len(binding.members) + 1):
                        owner = lookup(binding.module, binding.members[:index])
                        if isinstance(owner, types.ModuleType):
                            add_support(owner.__name__)
                    break
        members = exports.get(name, vars(modules[name]))
        for member in members:
            obj = lookup(name, (member,))
            if inspect.isclass(obj) or inspect.isroutine(obj):
                add_support(getattr(obj, "__module__", None))
                if inspect.isclass(obj) and dataclasses.is_dataclass(obj):
                    for base in obj.__mro__[1:]:
                        if dataclasses.is_dataclass(base):
                            add_support(base.__module__)
    for name in {name.split(".")[0] for name in modules}:
        package = sys.modules[name]
        path = getattr(package, "__file__", None)
        if path and not Path(path).resolve().is_relative_to(root):
            raise ValueError("Loaded package does not match the supplied checkout")
        advertised = getattr(package, "__version__", None)
        if advertised is not None and str(advertised) != request["package_version"]:
            raise ValueError("Loaded package version conflicts with checkout metadata")

    def implicit_import(module, member):
        binding = verified.get((module, member))
        return binding is not None and binding.kind == "import" and not binding.explicit

    def alias_target(binding):
        module, members = binding.module, binding.members
        seen = set()
        while module in modules and (module, members) not in seen:
            seen.add((module, members))
            if not members:
                return module
            if len(members) > 1:
                owner = lookup(module, members[:-1])
                if not isinstance(owner, types.ModuleType):
                    # A callable or class attribute can still be resolved via
                    # its runtime qualified name below.
                    return None
                module, members = owner.__name__, members[-1:]
                continue
            edge = verified.get((module, members[0]))
            if edge is None:
                return module + "." + members[0]
            module, members = edge.module, edge.members
        return None

    alias_targets = {}
    for key, binding in verified.items():
        target_name = alias_target(binding)
        if target_name and binding.kind == "import":
            # Keep the verified import edge so a stub spelling the same import
            # reconciles even when its target is itself a reexport.
            target_name = ".".join((binding.module, *binding.members))
        alias_targets[key] = target_name
    support_names = {}
    for target_name in filter(None, alias_targets.values()):
        owner, _, member = target_name.rpartition(".")
        if owner in modules and member.startswith("__"):
            # An explicit export can name a dunder from a private module.
            support_names.setdefault(owner, set()).add(member)

    timings = Path(output_path).with_suffix(".timings")
    timings.write_text(f"package-loading\t{time.monotonic() - started}\n", encoding="utf-8")
    started = time.monotonic()
    docs = {}
    raw_docs = {}
    doctest_modules = {}
    processed_docs = {}
    doctest_lines = {}
    local_paths = {str(repository), repository.as_posix(), str(stage), stage.as_posix()}
    sphinx_signatures = {}

    def inherited_docstring_module(name, obj, text):
        if not text or getattr(obj, "__doc__", None) is not None:
            return
        owner, _, member = name.rpartition(".")
        module = owner
        while module and module not in modules:
            module, _, _ = module.rpartition(".")
        if not module or owner == module:
            return
        cls = lookup(module, owner[len(module) + 1:].split("."))
        if not inspect.isclass(cls):
            return
        for base in cls.__mro__[1:]:
            method = vars(base).get(member)
            if isinstance(method, (classmethod, staticmethod)):
                method = method.__func__
            doc = getattr(method, "__doc__", None)
            if isinstance(doc, str) and inspect.cleandoc(doc).rstrip() == text:
                defining_module = getattr(method, "__module__", base.__module__)
                # A transcript from outside the verified checkout is display-only.
                doctest_modules[name] = (defining_module if defining_module in modules or
                                         checkout_module_file(sys.modules.get(defining_module), root) else None)
                return

    def capture_raw(app, what, name, obj, options, lines):
        text = "\n".join(lines).rstrip()
        raw_docs[name] = text
        doctest_modules.pop(name, None)
        if obj is not None:
            defining = obj.fget if isinstance(obj, property) else obj
            if text and (inspect.isroutine(defining) or inspect.isclass(defining)):
                module = getattr(defining, "__module__", None)
                if module in modules or checkout_module_file(sys.modules.get(module), root):
                    doctest_modules[name] = module
            inherited_docstring_module(name, obj, text)

    def capture_processed(app, what, name, obj, options, lines):
        processed_docs[name] = list(lines)

    def capture_signature(app, what, name, obj, options, sig, returns):
        if sig and name in sphinx_signatures:
            signatures = [line + (" -> " + returns if returns and "->" not in line else "")
                          for line in sig.splitlines() if line.strip()]
            if all(portable_signature(line, local_paths) for line in signatures):
                record = sphinx_signatures[name]
                record["signatures"] = signatures
                if len(signatures) > 1:
                    record["overload_lines"] = [None] * len(signatures)

    def portable(text):
        return text.replace(str(stage), "<python-helper>").replace(str(repository), "<repository>")

    def node_record(node):
        if isinstance(node, nodes.system_message):
            # The message children carry the detail. The node's astext() also
            # adds the absolute Sphinx or Python source location.
            return {"kind": node.tagname, "text": portable("\n\n".join(child.astext() for child in node.children)), "children": []}
        result = {"kind": node.tagname, "text": portable(node.astext()), "children": []}
        if isinstance(node, nodes.Text):
            result["kind"] = "text"
        else:
            result["children"] = [node_record(child) for child in node.children]
            for key in ("reftarget", "refuri", "refdomain", "reftype", "language"):
                if key in node:
                    result[key] = portable(str(node[key]))
        return result

    def record_doctests(name, module, content, source, first_line):
        doctest_lines.pop(name, None)
        blocks = list(content.findall(nodes.doctest_block))
        if not blocks:
            return
        raw = raw_docs.get(name, "").splitlines()
        processed = processed_docs.get(name, [])
        if not raw or not processed:
            return
        # Docutils locates doctests in the processed text. Napoleon may have
        # inserted or reindented lines, so map verified ranges back to the raw
        # docstring. The example runner uses the original indentation and text.
        raw = [line.strip() for line in raw]
        processed = [line.strip() for line in processed]
        matches = difflib.SequenceMatcher(None, processed, raw, autojunk=False)
        raw_lines = {display + i: original + i
                     for display, original, size in matches.get_matching_blocks()
                     for i in range(size)}
        leading = next((i for i, line in enumerate(raw) if line), 0)
        docstring_source = "docstring of " + name
        ranges = []
        for node in blocks:
            if node.line is None:
                continue
            if (node.source and node.source not in (source, docstring_source)
                    and not node.source.endswith(":" + docstring_source)):
                continue
            transcript = [line.strip() for line in node.rawsource.splitlines()]
            if not any(line.startswith(">>> ") and line[4:].strip() and
                       not line[4:].lstrip().startswith("#") for line in transcript):
                continue
            start = node.line - first_line
            end = start + len(transcript)
            if start < 0 or processed[start:end] != transcript:
                continue
            original = raw_lines.get(start)
            if original is not None and all(raw_lines.get(start + i) == original + i
                                            for i in range(len(transcript))):
                ranges.append((original - leading, original - leading + len(transcript)))
        if ranges:
            doctest_lines[name] = sorted(set(ranges))
            doctest_modules.setdefault(name, module)

    def collect(app, tree):
        # Container text can also include the message's location via astext().
        for message in tree.findall(nodes.system_message):
            message["source"] = "<docstring>"
        # Capture before Sphinx resolves Python references into its own site URLs.
        source = tree["source"]
        docname = Path(source).stem
        module = module_documents.get(docname)
        first_line = docstring_starts.get(docname, 1)
        for desc in tree.findall(addnodes.desc):
            sig = next(iter(desc.findall(addnodes.desc_signature)), None)
            content = next((child for child in desc.children if isinstance(child, addnodes.desc_content)), None)
            if sig is None or content is None:
                continue
            name = ".".join(filter(None, (sig.get("module"), sig.get("fullname"))))
            prose = content.copy()
            prose.children = [child for child in content.children if not isinstance(child, (addnodes.desc, addnodes.index))]
            docs[name] = node_record(prose)
            record_doctests(name, sig.get("module") or module, prose, source, first_line)
        # A module's prose sits outside its child object descriptions.
        if module:
            prose = tree.deepcopy()
            for child in list(prose.findall()):
                if isinstance(child, (addnodes.desc, addnodes.index)):
                    child.parent.remove(child)
            docs[module] = node_record(prose)
            docs[module]["kind"] = "desc_content"
            record_doctests(module, module, prose, source, first_line)

    class ExportBuilder(Builder):
        name = "diplodocus"
        format = "diplodocus"

        def get_outdated_docs(self):
            return "all documents"

        def get_target_uri(self, docname, typ=None):
            return docname

        def write_doc(self, docname, doctree):
            pass

    def autodoc_getattr(module, member, *default):
        value = explicit_bindings.get((module.__name__, member), MISSING)
        return getattr(module, member, *default) if value is MISSING else value

    def skip_type_alias(app, what, name, obj, skip, options):
        # Autodoc's automatic type member handler evaluates the lazy RHS.
        if isinstance(obj, TYPE_ALIAS_TYPE):
            return True
        return None

    def setup(app):
        app.add_builder(ExportBuilder)
        app.add_autodoc_attrgetter(types.ModuleType, autodoc_getattr)
        app.connect("autodoc-process-docstring", capture_raw, priority=100)
        app.connect("autodoc-process-docstring", capture_processed, priority=999)
        app.connect("autodoc-process-signature", capture_signature, priority=999)
        app.connect("autodoc-skip-member", skip_type_alias)
        app.connect("doctree-read", collect, priority=999)

    sources = stage / "sphinx"
    sources.mkdir()
    (sources / "conf.py").write_text("", encoding="utf-8")
    module_documents = {}
    docstring_starts = {}
    for index, (name, module) in enumerate(sorted(modules.items())):
        docname = f"module-{index}"
        module_documents[docname] = name
        # Listing explicitly public imports prevents annotation helpers from
        # appearing merely because imported-members is enabled.
        names = exports.get(name)
        if names is None:
            names = [n for n in vars(module) if not n.startswith("_") and not implicit_import(name, n)]
        discoverable = set(dir(module))
        # Private defining modules are needed for public reexports. Their
        # presentation visibility is decided after canonical item reconciliation.
        members = sorted(discoverable & (set(names) | support_names.get(name, set()) | {
            n for n, obj in vars(module).items()
            if not n.startswith("__") and getattr(obj, "__module__", None) == name
        }))
        source = f":orphan:\n\n.. automodule:: {name}\n   :no-index:\n"
        if members:
            source += f"   :members: {', '.join(members)}\n   :undoc-members:\n   :imported-members:\n   :special-members: __init__, __new__\n"
        # Autodoc's member discovery starts with dir(module), which can omit
        # exports resolved by __getattr__. Address those objects directly.
        direct = set(exports.get(name, ())) - discoverable
        direct.update(member for member in members if isinstance(lookup(name, (member,)), TYPE_ALIAS_TYPE))
        for member in sorted(direct):
            obj = lookup(name, (member,))
            if isinstance(obj, types.ModuleType):
                continue
            directive = ("class" if inspect.isclass(obj) else
                         "function" if callable(obj) else "data")
            source += f"\n.. auto{directive}:: {name}::{member}\n   :no-index:\n"
            if directive == "class":
                source += "   :members:\n   :undoc-members:\n   :special-members: __init__, __new__\n"
        (sources / (docname + ".rst")).write_text(source, encoding="utf-8")
    (sources / "index.rst").write_text("API\n===\n", encoding="utf-8")
    warnings = io.StringIO()
    extension = types.ModuleType("diplodocus_export")
    extension.setup = setup
    sys.modules[extension.__name__] = extension
    def canonical(obj, binding):
        if isinstance(obj, types.ModuleType):
            return obj.__name__
        module = getattr(obj, "__module__", "")
        qualname = getattr(obj, "__qualname__", "")
        if module in modules and qualname and "<" not in qualname:
            target = modules[module]
            try:
                for part in qualname.split("."):
                    target = inspect.getattr_static(target, part)
                if isinstance(target, (classmethod, staticmethod)):
                    target = target.__func__
                if target is obj:
                    return module + "." + qualname
            except AttributeError:
                pass
        return binding

    unsupported_defaults = []
    unsupported_generics = set()
    dataclass_fields = {}
    module_annotations = {name: annotations(module) for name, module in modules.items()}

    def declaration(name, obj, qualified, module, in_class=False, descriptor=None, seen=None):
        seen = set() if seen is None else seen
        generic = bool(getattr(obj, "__type_params__", ()))
        if generic:
            unsupported_generics.add(qualified)
        path = Path(paths[module.__name__])
        line = source_line(obj, path)
        result = {"name": name, "kind": "value", "signatures": [], "overload_lines": [], "decorators": [], "bases": [], "members": [], "is_async": False, "alias": None, "assignment_alias": False, "implicit_alias": False, "annotation": None, "value": None, "has_setter": False, "has_deleter": False, "source_line": line}
        if not in_class:
            aliases = syntax[module.__name__][1]
            own_alias = name in aliases
            key = (module.__name__, name)
            binding = verified.get(key)
            if binding is not None and not own_alias:
                target = alias_targets[key]
                if target is None and (isinstance(obj, types.ModuleType) or inspect.isclass(obj) or inspect.isroutine(obj)):
                    target = canonical(obj, qualified)
                if target and target != qualified:
                    result["alias"] = target
                    result["assignment_alias"] = binding.kind == "assignment"
                    result["implicit_alias"] = implicit_import(*key)
                    result["source_line"] = binding.line
                    return result
            if own_alias or isinstance(obj, TYPE_ALIAS_TYPE):
                result["kind"] = "type-alias"
                if generic:
                    result["value"] = "object"
                elif (symbolic := aliases.get(name)) is not None and isinstance(obj, TYPE_ALIAS_TYPE):
                    result["value"], result["source_line"] = symbolic
                elif isinstance(obj, TYPE_ALIAS_TYPE) and sys.version_info >= (3, 14):
                    result["value"] = annotation(annotationlib.call_evaluate_function(
                        obj.evaluate_value, annotationlib.Format.STRING
                    ))
                else:
                    result["value"] = annotation(getattr(obj, "__value__", obj))
                return result
        if (not isinstance(descriptor, property)
                and (isinstance(obj, types.ModuleType) or inspect.isclass(obj) or inspect.isroutine(obj))):
            target = canonical(obj, qualified)
            if target != qualified:
                result["alias"] = target
                return result
        # Keep an identity for reconciliation without rendering a signature
        # that may rely on type parameters the IR cannot represent.
        if generic:
            result["kind"] = "class" if inspect.isclass(obj) else "function"
            return result
        if isinstance(descriptor, property):
            result["kind"] = "function"
            result["decorators"] = ["property"]
            sig = signature(descriptor.fget, local_paths)
            result["signatures"] = [sig] if isinstance(sig, str) else []
            result["has_setter"] = descriptor.fset is not None
            result["has_deleter"] = descriptor.fdel is not None
        elif inspect.isclass(obj):
            result["kind"] = "class"
            result["bases"] = [class_base(base) for base in vars(obj).get("__orig_bases__", obj.__bases__) if base is not object]
            if id(obj) in seen:
                raise ValueError(f"Cyclic class ownership at {qualified}")
            seen = seen | {id(obj)}
            fields = {}
            class_annotations = annotations(obj)
            is_dataclass = dataclasses.is_dataclass(obj) and "__dataclass_fields__" in vars(obj)
            inherited_fields = set()
            if is_dataclass:
                options = obj.__dataclass_params__
                result["decorators"] = [f"dataclasses.dataclass(init={options.init}, frozen={options.frozen})"]
                constructor_fields = []
                for field in dataclasses.fields(obj):
                    if field.name in class_annotations:
                        fields[field.name] = field
                        owner = obj
                    else:
                        inherited_fields.add(field.name)
                        owner = next(base for base in obj.__mro__[1:]
                                     if vars(base).get("__dataclass_fields__", {}).get(field.name) is field
                                     and field.name in annotations(base))
                    if field.init:
                        constructor_fields.append(f"{owner.__module__}.{owner.__qualname__}.{field.name}")
                if any(dataclasses.is_dataclass(base) for base in obj.__mro__[1:]):
                    dataclass_fields[qualified] = constructor_fields
            # Dataclasses preserve definition order even when a required field
            # exists only in __annotations__ and has no entry in vars(obj).
            for member in dict.fromkeys([*fields, *vars(obj), *class_annotations]):
                if member.startswith("_") and member not in ("__init__", "__new__") and member not in fields:
                    continue
                value = inspect.getattr_static(obj, member, MISSING)
                if member in inherited_fields and isinstance(value, types.MemberDescriptorType):
                    continue
                if member == "__init__" and is_dataclass and getattr(getattr(value, "__code__", None), "co_filename", None) == "<string>":
                    continue
                if member in fields:
                    field = fields[member]
                    descriptor_value = value if inspect.isdatadescriptor(value) else MISSING
                    child = declaration(member, descriptor_value, qualified + "." + member, module, True, seen=seen)
                    child["annotation"] = annotation(class_annotations.get(member, field.type))
                    args = [f"init={field.init}"]
                    if field.kw_only:
                        args.append("kw_only=True")
                    if field.default is not dataclasses.MISSING:
                        default = literal(field.default, local_paths)
                        if default is None:
                            unsupported_defaults.append(qualified + "." + member)
                        args.append("default=" + (default if default is not None else "..."))
                    if field.default_factory is not dataclasses.MISSING:
                        factory = getattr(field.default_factory, "__qualname__", "")
                        if not isinstance(factory, str) or not all(
                            part.isidentifier() and not keyword.iskeyword(part)
                            for part in factory.split(".")
                        ):
                            factory = "..."
                        args.append("default_factory=" + factory)
                    child["value"] = "dataclasses.field(" + ", ".join(args) + ")"
                elif isinstance(value, (classmethod, staticmethod)):
                    child = declaration(member, value.__func__, qualified + "." + member, module, True, value, seen)
                    child["decorators"] = ["classmethod" if isinstance(value, classmethod) else "staticmethod"]
                elif isinstance(value, property):
                    child = declaration(member, value.fget, qualified + "." + member, module, True, value, seen)
                elif inspect.isclass(value) or inspect.isroutine(value):
                    child = declaration(member, value, qualified + "." + member, module, True, value, seen)
                elif member in class_annotations or member in vars(obj):
                    child = declaration(member, value, qualified + "." + member, module, True, seen=seen)
                    if member in class_annotations:
                        child["annotation"] = annotation(class_annotations[member])
                else:
                    continue
                result["members"].append(child)
        elif in_class and inspect.isdatadescriptor(obj):
            result["kind"] = "data-descriptor"
        elif inspect.isroutine(obj) or (callable(obj) and not isinstance(obj, types.UnionType)):
            result["kind"] = "function"
            result["is_async"] = inspect.iscoroutinefunction(obj)
            overloads = typing.get_overloads(obj) if inspect.isfunction(obj) else []
            if overloads:
                signatures = [signature(overload, local_paths) for overload in overloads]
                if all(isinstance(sig, str) for sig in signatures):
                    result["signatures"] = signatures
                    result["overload_lines"] = [source_line(overload, path) for overload in overloads]
            else:
                observed_signature = signature(obj, local_paths)
                if isinstance(observed_signature, str):
                    result["signatures"] = [observed_signature]
                elif observed_signature is MISSING and not in_class:
                    sphinx_signatures[qualified] = result
            code = getattr(obj, "__code__", None)
            if code and code.co_filename == contextlib.__file__ and inspect.isgeneratorfunction(inspect.unwrap(obj)):
                result["decorators"] = ["contextlib.contextmanager"]
        else:
            if not in_class and name in module_annotations[module.__name__]:
                result["annotation"] = annotation(module_annotations[module.__name__][name])
            if obj is None or isinstance(obj, (str, bytes, int, float, bool, tuple, list, dict, set, frozenset)):
                result["value"] = literal(obj, local_paths)
        return result

    exported = []
    for name, module in sorted(modules.items()):
        names = exports.get(name)
        members = {
            member: obj for member, obj in vars(module).items()
            if member in support_names.get(name, ())
            or (not member.startswith("__") and
                (not implicit_import(name, member) or alias_targets.get((name, member))))
        }
        if names is not None:
            members.update((member, explicit_bindings[name, member]) for member in names)
        declarations = []
        for member, obj in members.items():
            if isinstance(obj, types.ModuleType) and obj.__name__ == name + "." + member:
                continue
            declarations.append(declaration(member, obj, name + "." + member, module))
        support_only = name not in selected and (
            not target.is_dir() or not Path(paths[name]).is_relative_to(boundary)
        )
        exported.append({"name": name, "path": paths[name], "exports": names,
                         "declarations": declarations, "support_only": support_only})

    app = Sphinx(str(sources), str(sources), str(stage / "out"), str(stage / "doctrees"), "diplodocus", confoverrides={
        "extensions": ["sphinx.ext.autodoc", "sphinx.ext.napoleon", extension.__name__],
        "autodoc_use_legacy_class_based": False,
        "autodoc_typehints": "signature",
        "autodoc_inherit_docstrings": True,
        "napoleon_numpy_docstring": True,
        "napoleon_google_docstring": True,
        "napoleon_use_param": False,
        "napoleon_use_rtype": False,
        "napoleon_use_ivar": True,
        "nitpicky": False,
        "primary_domain": "py",
    }, status=None, warning=warnings, freshenv=True, warningiserror=False)
    def build_docs(force_all):
        # Autodoc expects a concrete list or tuple. Adapt the validated
        # sequence after collecting live declarations and restore it afterward.
        original = {}
        for name, names in exports.items():
            namespace = vars(modules[name])
            value = namespace.get("__all__", MISSING)
            if type(value) not in (list, tuple):
                original[name] = value
                namespace["__all__"] = names
        try:
            app.build(force_all=force_all)
        finally:
            for name, value in original.items():
                namespace = vars(modules[name])
                if value is MISSING:
                    namespace.pop("__all__", None)
                else:
                    namespace["__all__"] = value

    build_docs(force_all=True)
    fallback_raw = {}
    for index, entry in enumerate(request["stub_docs"]):
        if raw_docs.get(entry["name"], "").strip():
            continue
        lines = inspect.cleandoc(entry["text"]).splitlines()
        app.emit("autodoc-process-docstring", entry["kind"], entry["name"], None, {}, lines)
        doctest_modules[entry["name"]] = entry["module"]
        fallback_raw[entry["name"]] = entry["text"]
        docname = f"stub-{index}"
        if entry["kind"] == "module":
            module_documents[docname] = entry["module"]
            docstring_starts[docname] = 3
            (sources / (docname + ".rst")).write_text(":orphan:\n\n" + "\n".join(lines) + "\n", encoding="utf-8")
            continue
        local = entry["name"][len(entry["module"]) + 1:]
        directive = "class" if entry["kind"] == "class" else "function"
        source = f":orphan:\n\n.. py:currentmodule:: {entry['module']}\n\n.. py:{directive}:: {local}"
        if directive == "function":
            source += "()"
        source += "\n   :no-index:\n\n"
        docstring_starts[docname] = source.count("\n") + 1
        source += "\n".join("   " + line for line in lines) + "\n"
        (sources / (docname + ".rst")).write_text(source, encoding="utf-8")
    if fallback_raw:
        raw_docs.update(fallback_raw)
        build_docs(force_all=False)

    dependencies = {"sphinx": sphinx.__version__, "docutils": docutils.__version__}
    distributions = importlib.metadata.packages_distributions()
    for name in sorted({name.split(".")[0] for name in sys.modules}):
        for distribution in distributions.get(name, []):
            dependencies[distribution] = importlib.metadata.version(distribution)
    environment = environment_observation()
    with timings.open("a") as stream:
        stream.write(f"documentation-generation\t{time.monotonic() - started}\n")
    inputs = set()
    runtime_roots = [Path(base) for base in environment["runtime_paths"]]
    for name, module in list(sys.modules.items()):
        path = getattr(module, "__file__", None)
        if path:
            path = Path(path).resolve()
            if ((path.is_file() or path in checkout_inputs.fingerprints)
                    and path.is_relative_to(root) and not path.is_relative_to(stage)
                    and not any(path.is_relative_to(base) for base in runtime_roots)):
                if name in startup_modules:
                    raise ValueError(f"Cannot verify the bytes of checkout module {name} loaded before discovery")
                inputs.add(path)
    for dependencies_read in app.env.dependencies.values():
        for dependency in dependencies_read:
            path = Path(dependency)
            path = path.resolve() if path.is_absolute() else (sources / path).resolve()
            if path.is_relative_to(stage) or (not path.is_file() and path not in checkout_inputs.fingerprints):
                continue
            if any(path.is_relative_to(base) for base in runtime_roots):
                continue
            if not path.is_relative_to(root):
                raise ValueError(f"A Sphinx documentation input escapes the selected package: {path}")
            inputs.add(path)
    missing = inputs - checkout_inputs.fingerprints.keys()
    if missing:
        raise ValueError(f"Cannot verify a consumed checkout input: {min(missing)}")
    # Import-time data reads can affect the surface without loading a module
    # or creating a Sphinx dependency. They need the same publication checks.
    inputs.update(path for path in checkout_inputs.fingerprints
                  if not any(path.is_relative_to(base) for base in runtime_roots))
    fingerprints = {str(path): {"algorithm": "sha256", "value": checkout_inputs.fingerprints[path]}
                    for path in sorted(inputs)}
    environment["used_paths"] = sorted(fingerprints.keys() | set(environment["used_paths"]))
    warning_text = re.sub(r"\x1b\[[0-9;]*m", "", warnings.getvalue())
    result = {
        "protocol": 6,
        "python_version": platform.python_version(),
        "sphinx_version": sphinx.__version__,
        "docutils_version": docutils.__version__,
        "dependencies": dependencies,
        "modules": exported,
        "docs": docs,
        "raw_docs": {name: portable(text) for name, text in raw_docs.items()},
        "doctest_sources": {name: raw_docs[name] for name in doctest_lines},
        "doctest_modules": doctest_modules,
        "doctest_lines": doctest_lines,
        "dataclass_fields": dataclass_fields,
        "unsupported_defaults": unsupported_defaults,
        "unsupported_generics": sorted(unsupported_generics),
        "warnings": portable(warning_text),
        "environment": environment,
        "inputs": fingerprints,
        "input_aliases": {str(logical): str(path) for logical, path in sorted(checkout_inputs.targets.items())
                          if logical != path and path in inputs},
        "inputs_changed": bool(inputs & checkout_inputs.changed),
    }
    Path(output_path).write_text(json.dumps(result, ensure_ascii=False), encoding="utf-8")


if __name__ == "__main__":
    run(sys.argv[1], sys.argv[2], observe_only="--observe" in sys.argv[3:])
