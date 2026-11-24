"""Check an installed Diplodocus distribution from a clean working directory."""

from importlib.metadata import version
from pathlib import Path
import os
import subprocess
import sys
import sysconfig
from tempfile import TemporaryDirectory


expected = sys.argv[1]
metadata = version("diplodocus")
if metadata != expected:
    raise SystemExit(f"Expected package version {expected}, got {metadata}")

name = "diplodocus.exe" if os.name == "nt" else "diplodocus"
cli = Path(sysconfig.get_path("scripts")) / name
with TemporaryDirectory() as directory:
    workspace = Path(directory)
    reported = subprocess.check_output([cli, "--version"], cwd=workspace, text=True)
    if reported.strip() != f"diplodocus {expected}":
        raise SystemExit(f"Unexpected CLI version: {reported.strip()}")

    (workspace / "diplodocus.yaml").write_text(
        """project:
  name: Packaging smoke test
repositories:
  - id: project
    path: .
content:
  - id: guide
    owner: project
    repository: project
    path: docs
    mount: ""
    format: gfm
""",
        encoding="utf-8",
    )
    (workspace / "docs").mkdir()
    (workspace / "docs/index.md").write_text(
        "# Home\n\nBuilt with the installed CLI.\n", encoding="utf-8"
    )
    subprocess.run([cli, "build"], cwd=workspace, check=True)
    html = (workspace / "site/index.html").read_text(encoding="utf-8")
    if "Built with the installed CLI." not in html:
        raise SystemExit("The built site is missing the Markdown content")
