"""Run standard doctests with module globals from the verified checkout."""

import contextlib
import doctest
import importlib
import importlib.metadata
import io
import json
import pathlib
import platform
import shutil
import sys
import sysconfig
import time

import tomllib


def run(request_path, output_path):
    with open(request_path, "rb") as stream:
        request = tomllib.load(stream)
    timings = pathlib.Path(output_path).with_suffix(".timings")
    started = time.monotonic()
    root = pathlib.Path(request["package_root"]).resolve()
    project = tomllib.loads(pathlib.Path(request["metadata_path"]).read_text())[
        "project"
    ]
    if str(project.get("version", "")) != request["package_version"]:
        raise ValueError("Checkout package version does not match extraction metadata.")
    # Prefer the selected checkout just as the native R and Julia loaders do.
    sys.path[:0] = request["import_roots"]
    # An old timestamp-based bytecode entry must not conceal changed example code.
    sys.pycache_prefix = str(pathlib.Path(request_path).parent / "bytecode")
    sys.dont_write_bytecode = True
    package = importlib.import_module(request["package_name"])
    if not pathlib.Path(package.__file__).resolve().is_relative_to(root):
        raise ValueError("Loaded package does not match the supplied checkout.")
    advertised_version = getattr(package, "__version__", None)
    if advertised_version is not None and str(advertised_version) != str(
        project["version"]
    ):
        raise ValueError("Loaded package version conflicts with checkout metadata.")
    components = request["qualified_name"].split(".")
    while components:
        name = ".".join(components)
        try:
            module = importlib.import_module(name)
            break
        except ModuleNotFoundError as error:
            if error.name is None or not (
                name == error.name or name.startswith(error.name + ".")
            ):
                raise
            components.pop()
    if not pathlib.Path(module.__file__).resolve().is_relative_to(root):
        raise ValueError("Example module does not match the supplied checkout.")
    timings.write_text(f"package-loading\t{time.monotonic() - started}\n")
    started = time.monotonic()
    source = request["source"]
    test = doctest.DocTestParser().get_doctest(
        source, vars(module).copy(), request["qualified_name"], "<api-document>", 0
    )
    lines = source.splitlines(keepends=True)
    offsets = [0]
    for line in lines:
        offsets.append(offsets[-1] + len(line.encode("utf-8")))
    outcomes = []
    indexes = {}
    for example in test.examples:
        indexes[id(example)] = len(outcomes)
        skipped = example.options.get(doctest.SKIP, False)
        outcomes.append(
            {
                "span": {
                    "start": offsets[example.lineno],
                    "end": offsets[
                        min(
                            len(lines),
                            example.lineno
                            + example.source.count("\n")
                            + example.want.count("\n"),
                        )
                    ],
                },
                "source": example.source,
                "expected": example.want,
                "actual": "",
                "outcome": "explicit-skip" if skipped else "executed",
                "reason": "doctest +SKIP directive" if skipped else None,
                "options": {
                    name: str(enabled).lower()
                    for name, flag in doctest.OPTIONFLAGS_BY_NAME.items()
                    if (enabled := example.options.get(flag)) is not None
                },
            }
        )

    class Checker(doctest.OutputChecker):
        current = None
        exception_actual = ""

        def check_output(self, want, got, optionflags):
            if self.current is not None and self.current.exc_msg == want:
                self.exception_actual = got
            return super().check_output(want, got, optionflags)

    checker = Checker()

    class Runner(doctest.DocTestRunner):
        def report_start(self, out, test, example):
            checker.current = example
            checker.exception_actual = ""
            super().report_start(out, test, example)

        def report_success(self, out, test, example, got):
            record = outcomes[indexes[id(example)]]
            record["actual"] = got + checker.exception_actual
            if example.exc_msg is not None:
                record["outcome"] = "expected-error"

    runner = Runner(checker=checker, verbose=True)
    messages = io.StringIO()
    with contextlib.redirect_stderr(messages):
        result = runner.run(test, out=messages.write)
    if result.failed:
        raise ValueError(messages.getvalue())
    with timings.open("a") as stream:
        stream.write(f"examples\t{time.monotonic() - started}\n")
    dependencies = {}
    distributions = importlib.metadata.packages_distributions()
    for name in sorted({name.split(".")[0] for name in sys.modules}):
        for distribution in distributions.get(name, []):
            dependencies[distribution] = importlib.metadata.version(distribution)
    try:
        environment = observe_environment(
            request_path,
            root,
            pathlib.Path(request.get("repository_root", root)).resolve(),
            request.get("tracked_source_paths", []),
        )
    except (OSError, RuntimeError, TypeError, ValueError):
        environment = {"search_paths": [], "runtime_paths": [], "flags": [], "eligible": False}
    result = {
        "runtime_version": platform.python_version(),
        "package_name": request["package_name"],
        "package_version": str(project["version"]),
        "dependencies": dependencies,
        "examples": outcomes,
        "environment": environment,
    }
    pathlib.Path(output_path).write_text(json.dumps(result, ensure_ascii=False))


def observe_environment(request_path, root, repository_root, tracked_source_paths):
    staging_root = pathlib.Path(request_path).parent.resolve()
    search_paths = [
        str(pathlib.Path(path or root).resolve())
        for path in sys.path
        if pathlib.Path(path or root).resolve() != staging_root
    ]
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
    runtime_paths = sorted(
        {
            str(pathlib.Path(path).resolve())
            for path in [
                sys.executable,
                shutil.which("python"),
                sysconfig.get_path("stdlib"),
                sysconfig.get_path("platstdlib"),
                sysconfig.get_config_var("LIBDIR"),
                *search_paths,
                *mapped_libraries,
            ]
            if path
            and not pathlib.Path(path).resolve().is_relative_to(repository_root)
            and not repository_root.is_relative_to(pathlib.Path(path).resolve())
        }
    )
    runtime_paths = [
        path
        for path in runtime_paths
        if not any(
            path != parent
            and pathlib.Path(parent).is_dir()
            and pathlib.Path(path).is_relative_to(parent)
            for parent in runtime_paths
        )
    ]
    covered_roots = [
        root,
        staging_root,
        *map(pathlib.Path, runtime_paths),
        *[pathlib.Path(path).resolve() for path in tracked_source_paths],
    ]
    eligible = maps_path.is_file() and all(
        pathlib.Path(path).is_file() for path in mapped_libraries
    )
    for module in list(sys.modules.values()):
        module_path = getattr(module, "__file__", None)
        if module_path is not None:
            module_path = pathlib.Path(module_path).resolve()
            if not any(module_path.is_relative_to(base) for base in covered_roots):
                eligible = False
        elif getattr(getattr(module, "__spec__", None), "origin", None) not in (
            "built-in", "frozen", None
        ):
            eligible = False
    return {
        "search_paths": search_paths,
        "runtime_paths": runtime_paths,
        "flags": [int(flag) for flag in sys.flags],
        "eligible": eligible,
    }


if __name__ == "__main__":
    run(*sys.argv[1:])
