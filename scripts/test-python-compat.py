#!/usr/bin/env python3
"""Check the exporter using caller-prepared Python/Sphinx profiles.

Run with ``compute run -- python scripts/test-python-compat.py`` or repeat
``--profile SPHINX_VERSION=PYTHON``. No packages are installed by this script.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


SOURCE = '''"""Module documentation.

Details
-------
A module section with native markup.
"""
from __future__ import annotations
from dataclasses import dataclass
from pathlib import Path as _Path
from typing import TypeAlias as TA, overload

Payload: TA = int | str

@dataclass(frozen=True)
class Result:
    """A result with native fields."""
    value: int

@overload
def convert(value: int) -> int: ...
@overload
def convert(value: str) -> str: ...
def convert(value):
    """Convert a value.

    Parameters
    ----------
    value : int or str
        The conversion input.

    Returns
    -------
    int or str
        The converted value.
    """
    return value

def google(value: int) -> Result:
    """Create a result.

    Args:
        value (int): The result input.

    Returns:
        Result: The constructed result.
    """
    return Result(value)

def undocumented(value: int) -> int:
    return value

exec('def generated(value: int = 1) -> int:\\n    return value')
generated.__doc__ = """Attached reStructuredText documentation.

See :func:`convert`.

:param value: The generated input.
:returns: The generated value.

.. code-block:: pycon

   >>> raise RuntimeError("Illustrative example.")

>>> _Path('example-ran').write_text('must remain inert')
"""
alias = generated
__all__ = [name for name in ('Result', 'convert', 'google', 'generated', 'alias', 'undocumented', 'Payload')]
'''

DEFERRED = '''from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from nonexistent import TypeOnly

SETTING: TypeOnly = None

class Result:
    value: TypeOnly

def create() -> TypeOnly:
    return SETTING
'''


def run_profile(expected: str | None, python: str) -> None:
    python = str(Path(shutil.which(python) or python).absolute())
    environment = os.environ.copy()
    for name in ("PYTHONPATH", "PYTHONHOME", "NIX_PYTHONPATH"):
        environment.pop(name, None)
    environment["PATH"] = str(Path(python).parent) + os.pathsep + environment.get("PATH", "")
    version, python_version = json.loads(subprocess.run(
        [python, "-c", "import json, sphinx, sys; print(json.dumps([sphinx.__version__, sys.version_info[:2]]))"],
        env=environment, check=True, capture_output=True, text=True,
    ).stdout)
    if expected is not None:
        assert version == expected, (expected, version)
    repository = Path(__file__).resolve().parents[1]
    with tempfile.TemporaryDirectory(prefix="diplodocus-python-compat-") as directory:
        root = Path(directory).resolve()
        package = root / "package"
        module = package / "compat_api"
        module.mkdir(parents=True)
        (module / "__init__.py").write_text(SOURCE)
        (module / "types_only.pyi").write_text("def choose(value: int) -> int: ...\n")
        (package / "pyproject.toml").write_text("[project]\nname='compat-api'\nversion='1.0.0'\n")
        (package / "conf.py").write_text("raise RuntimeError('project configuration must stay inert')\n")

        def export(*, in_package=False):
            stage = Path(tempfile.mkdtemp(dir=package if in_package else root))
            for name in ("python_native.py", "python_environment.py"):
                shutil.copy2(repository / "src/assembly" / name, stage / name)
            request = {
                "package_root": str(package), "repository_root": str(package),
                "target_path": str(module), "package_version": "1.0.0",
                "import_roots": [str(package)], "tracked_source_paths": [],
                "modules": [
                    {"name": "compat_api", "path": str(module / "__init__.py"), "stub_only": False},
                    {"name": "compat_api.types_only", "path": str(module / "types_only.pyi"), "stub_only": True},
                ],
                "stub_docs": [
                    {"module": "compat_api.types_only", "name": "compat_api.types_only.choose",
                     "kind": "function", "text": "A maintained stub-only declaration."},
                    {"module": "compat_api.types_only", "name": "compat_api.types_only",
                     "kind": "module", "text": "A maintained stub-only module."},
                    {"module": "compat_api", "name": "compat_api.undocumented",
                     "kind": "function", "text": "Maintained fallback documentation."},
                    {"module": "compat_api", "name": "compat_api.generated",
                     "kind": "function", "text": "Superseded stub documentation."},
                ],
            }
            (stage / "request.json").write_text(json.dumps(request))
            output = stage / "observations.json"
            result = subprocess.run([python, str(stage / "python_native.py"), str(stage / "request.json"), str(output)],
                                    cwd=package, env=environment, capture_output=True, text=True)
            return result, output

        # Even a TMPDIR inside the checkout must never contribute helper files.
        result, output = export(in_package=True)
        assert result.returncode == 0, result.stderr
        observed = json.loads(output.read_text())
        assert observed["protocol"] == 4 and observed["sphinx_version"] == version
        assert all(not Path(path).is_relative_to(output.parent) for path in observed["inputs"])
        assert observed["inputs"][str(module / "__init__.py")] == {
            "algorithm": "sha256", "value": hashlib.sha256((module / "__init__.py").read_bytes()).hexdigest(),
        }
        assert len(observed["doctest_lines"]["compat_api.generated"]) == 1
        declarations = {entry["name"]: entry for entry in observed["modules"][0]["declarations"]}
        assert declarations["alias"]["alias"] == "compat_api.generated"
        assert declarations["Payload"]["kind"] == "type-alias"
        assert declarations["generated"]["signatures"]
        assert len(declarations["convert"]["signatures"]) == 2
        assert declarations["google"]["signatures"] == ["(value: int) -> Result"]
        assert declarations["Result"]["members"][0]["name"] == "value"
        for name in ("convert", "google", "generated"):
            assert observed["docs"]["compat_api." + name]["children"]
        assert "The conversion input." in json.dumps(observed["docs"])
        assert "The result input." in json.dumps(observed["docs"])
        assert "A module section with native markup." in json.dumps(observed["docs"]["compat_api"])
        assert "The conversion input." not in json.dumps(observed["docs"]["compat_api"])
        assert observed["raw_docs"]["compat_api.generated"].startswith("Attached reStructuredText")
        assert "compat_api.types_only.choose" in observed["docs"]
        assert observed["raw_docs"]["compat_api.undocumented"] == "Maintained fallback documentation."
        assert "A maintained stub-only module." in json.dumps(observed["docs"]["compat_api.types_only"])
        assert not (package / "example-ran").exists()
        if python_version >= [3, 14]:
            (module / "__init__.py").write_text(DEFERRED)
            result, output = export()
            assert result.returncode == 0, result.stderr
            observed = json.loads(output.read_text())
            declarations = {entry["name"]: entry for entry in observed["modules"][0]["declarations"]}
            assert declarations["SETTING"]["annotation"] == "TypeOnly"
            assert declarations["SETTING"]["value"] == "None"
            assert declarations["Result"]["members"][0]["annotation"] == "TypeOnly"
            assert declarations["create"]["signatures"] == ["() -> TypeOnly"]
        (module / "__init__.py").write_text("def replacement(value: float) -> float:\n    return value\n__all__ = ['replacement']\n")
        result, output = export()
        assert result.returncode == 0, result.stderr
        observed = json.loads(output.read_text())
        assert observed["modules"][0]["exports"] == ["replacement"]
        if python_version >= [3, 12]:
            (module / "__init__.py").write_text(
                "from __future__ import annotations\n" + DEFERRED + "\ntype Text = TypeOnly\n"
            )
            result, output = export()
            assert result.returncode == 0, result.stderr
            observed = json.loads(output.read_text())
            declarations = {entry["name"]: entry for entry in observed["modules"][0]["declarations"]}
            assert declarations["Text"]["kind"] == "type-alias"
            assert declarations["Text"]["value"] == "TypeOnly"
            (module / "__init__.py").write_text("exec('def generic[T](value: T) -> T: return value')\n")
            result, output = export()
            assert result.returncode != 0 and not output.exists()
            assert "Generic type parameters are not yet represented" in result.stderr
        (module / "__init__.py").write_text("raise RuntimeError('native import failure')\n")
        result, output = export()
        assert result.returncode != 0 and not output.exists()
        assert "native import failure" in result.stderr
    print(f"Sphinx {version}: native exports, overloads, docstring formats, stubs, inert examples, changed inputs, and import rejection passed")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", action="append", default=[], metavar="SPHINX_VERSION=PYTHON")
    arguments = parser.parse_args()
    if not arguments.profile:
        run_profile(None, sys.executable)
    for profile in arguments.profile:
        expected, separator, python = profile.partition("=")
        if not separator:
            parser.error("Profiles use SPHINX_VERSION=PYTHON")
        run_profile(expected, python)


if __name__ == "__main__":
    main()
