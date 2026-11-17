"""Run standard doctests with module globals from the verified checkout."""

import contextlib
import doctest
import importlib
import importlib.metadata
import inspect
import io
import json
import pathlib
import platform
import runpy
import sys
import time
import tomllib

environment_helper = runpy.run_path(str(pathlib.Path(__file__).with_name("python_environment.py")))
observe_environment = environment_helper["observe_environment"]
verify_module = environment_helper["verify_module"]


def first_content_line(lines):
    return next((index for index, line in enumerate(lines) if line.strip()), len(lines))


def collect_examples(source, ranges):
    parser = doctest.DocTestParser()
    if ranges is None:
        return parser.get_examples(source)

    cleaned = inspect.cleandoc(source).splitlines()
    cleaned = cleaned[first_content_line(cleaned):]
    leading = first_content_line(source.splitlines())
    examples = []
    previous = 0
    for start, end in ranges:
        if not previous <= start < end <= len(cleaned):
            raise ValueError("Invalid doctest range in the original docstring.")
        for example in parser.get_examples("\n".join(cleaned[start:end])):
            example.lineno += leading + start
            examples.append(example)
        previous = end
    return examples


def run(request_path, output_path):
    with open(request_path, "rb") as stream:
        request = tomllib.load(stream)
    timings = pathlib.Path(output_path).with_suffix(".timings")
    started = time.monotonic()
    root = pathlib.Path(request["package_root"]).resolve()
    project = tomllib.loads(pathlib.Path(request["metadata_path"]).read_text(encoding="utf-8"))[
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
    verify_module(package, root)
    advertised_version = getattr(package, "__version__", None)
    if advertised_version is not None and str(advertised_version) != str(
        project["version"]
    ):
        raise ValueError("Loaded package version conflicts with checkout metadata.")
    module = importlib.import_module(request["docstring_module"])
    verify_module(module, root)
    timings.write_text(f"package-loading\t{time.monotonic() - started}\n", encoding="utf-8")
    started = time.monotonic()
    source = request["source"]
    test = doctest.DocTest(
        collect_examples(source, request.get("doctest_lines")),
        vars(module), request["qualified_name"], "<api-document>", 0, source,
    )
    lines = request.get("portable_source", source).splitlines(keepends=True)
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
        environment = {
            "search_paths": [], "runtime_paths": [], "used_paths": [], "flags": [], "eligible": False
        }
    result = {
        "runtime_version": platform.python_version(),
        "package_name": request["package_name"],
        "package_version": str(project["version"]),
        "dependencies": dependencies,
        "examples": outcomes,
        "environment": environment,
    }
    pathlib.Path(output_path).write_text(json.dumps(result, ensure_ascii=False), encoding="utf-8")



if __name__ == "__main__":
    run(*sys.argv[1:])
