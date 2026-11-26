"""Execute ordinary Python source in one namespace for a guide and package."""

import contextlib
import importlib
import json
import pathlib
import platform
import sys

from python_environment import verify_module

staging = pathlib.Path(sys.argv[1])
request = json.loads((staging / "request.json").read_text(encoding="utf-8"))
root = pathlib.Path(request["root"])
sys.path[:0] = request["import_roots"]
modules = request["modules"]


def verify_checkout():
    for name in modules:
        verify_module(importlib.import_module(name), root)


verify_checkout()
namespace = {"__name__": "__main__"}
results = []
for index, code in enumerate(request["blocks"]):
    progress = staging / "progress.tmp"
    progress.write_text(str(index), encoding="ascii")
    progress.replace(staging / "progress")
    output = staging / f"text-{index}"
    with output.open("w", encoding="utf-8") as stream:
        with contextlib.redirect_stdout(stream), contextlib.redirect_stderr(stream):
            exec(compile(code, f"<guide block {index + 1}>", "exec"), namespace)
    results.append([{"type": "text-file", "file": output.name}])

verify_checkout()
(staging / "results.json").write_text(
    json.dumps({"version": platform.python_version(), "blocks": results}),
    encoding="utf-8",
)
