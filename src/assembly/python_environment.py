"""Conservative prepared-environment observations shared by Python adapters."""

import pathlib
import sys
import sysconfig
import types


def checkout_module_file(module, root):
    """Locate a checkout module or the file backing its generated child."""
    if not isinstance(module, types.ModuleType):
        return None
    if path := getattr(module, "__file__", None):
        path = pathlib.Path(path).resolve()
        return path if path.is_relative_to(root) else None
    name = getattr(module, "__name__", None)
    if not isinstance(name, str):
        return None
    parent_name, _, member = name.rpartition(".")
    parent = sys.modules.get(parent_name)
    if (getattr(parent, "__name__", None) != parent_name
            or sys.modules.get(name) is not module):
        return None
    path = checkout_module_file(parent, root)
    if path is not None and path.is_file() and getattr(parent, member, None) is module:
        return path
    return None


def verify_module(module, root):
    valid = checkout_module_file(module, root) is not None
    if not valid and not getattr(module, "__file__", None):
        valid = any(pathlib.Path(path).resolve().is_relative_to(root)
                    for path in getattr(module, "__path__", ()))
    if not valid:
        raise ValueError("Loaded module does not match the supplied checkout.")


def runtime_library_roots():
    return {pathlib.Path(sysconfig.get_path(name)).resolve()
            for name in ("stdlib", "platstdlib", "purelib", "platlib")}


def observe_environment(request_path, root, repository_root, tracked_source_paths):
    staging_root = pathlib.Path(request_path).parent.resolve()
    # A virtual environment can live inside the checkout. Its libraries and
    # configuration remain runtime inputs rather than package declarations.
    libraries = runtime_library_roots()
    environment_config = pathlib.Path(sys.prefix).resolve() / "pyvenv.cfg"

    def runtime_path(path):
        return (not path.is_relative_to(repository_root)
                or path == environment_config
                or any(path.is_relative_to(library) for library in libraries))

    cwd = pathlib.Path.cwd()
    search_paths = [cwd / path for path in sys.path]
    search_paths = [path for path in search_paths
                    if not path.resolve().is_relative_to(staging_root)]
    # Native library replacements must invalidate results even at the same version.
    maps_path = pathlib.Path("/proc/self/maps")
    mapped_libraries = []
    if maps_path.is_file():
        mapped_libraries = [
            line.split(maxsplit=5)[5]
            for line in maps_path.read_text().splitlines()
            if len(line.split(maxsplit=5)) == 6
            and line.split(maxsplit=5)[5].startswith("/")
        ]
    linked_roots = set()
    visited = set()

    def linked_imports(directory):
        if directory in visited:
            return
        visited.add(directory)
        for child in directory.iterdir():
            if child.is_symlink():
                target = child.resolve()
                if target.exists():
                    linked_roots.add(str(target))
            elif child.is_dir() and child.name.isidentifier() and child.name != "__pycache__" and not (child / "__init__.py").is_file():
                # Namespace packages can merge symlinked children in a Nix
                # profile. Ordinary packages are covered by their own root.
                linked_imports(child)

    for path in search_paths:
        directory = path.resolve()
        if directory.is_dir() and runtime_path(directory) and not directory.is_relative_to(staging_root):
            linked_imports(directory)
    runtime_paths = set()
    for path in [sys.executable, *libraries, environment_config,
                 sysconfig.get_config_var("LIBDIR"), *search_paths,
                 *mapped_libraries, *linked_roots]:
        if not path:
            continue
        logical = cwd / path
        resolved = logical.resolve()
        if resolved.is_relative_to(staging_root) or repository_root.is_relative_to(resolved):
            continue
        if runtime_path(resolved) or runtime_path(logical) or logical != resolved:
            # The logical entry observes retargeting even when the old
            # physical directory and PYTHONPATH still have the same bytes.
            runtime_paths.update((logical, resolved))
    runtime_paths = [
        path
        for path in sorted(runtime_paths)
        if not any(
            path != parent
            and path.is_relative_to(parent)
            and parent.is_dir()
            for parent in runtime_paths
        )
    ]
    covered_roots = [
        root,
        staging_root,
        *runtime_paths,
        *[pathlib.Path(path).resolve() for path in tracked_source_paths],
    ]
    eligible = maps_path.is_file() and all(
        pathlib.Path(path).is_file() for path in mapped_libraries
    )
    used_paths = set()
    for module in list(sys.modules.values()):
        module_path = getattr(module, "__file__", None)
        if module_path is not None:
            module_path = pathlib.Path(module_path).resolve()
            if not module_path.is_relative_to(staging_root):
                used_paths.add(str(module_path))
            if not any(module_path.is_relative_to(base) for base in covered_roots):
                eligible = False
        elif getattr(getattr(module, "__spec__", None), "origin", None) not in (
            "built-in", "frozen", None
        ):
            eligible = False
    return {
        "search_paths": list(map(str, search_paths)),
        "runtime_paths": list(map(str, runtime_paths)),
        "used_paths": sorted(used_paths),
        "flags": [int(flag) for flag in sys.flags],
        "eligible": eligible,
    }

