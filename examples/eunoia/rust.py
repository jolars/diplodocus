#!/usr/bin/env python3
"""Validate Rust extraction and detached generation against Eunoia's core crate."""

import argparse
import json
from pathlib import Path
import shutil
import sqlite3
import subprocess
import tempfile


def run(*arguments):
    subprocess.run([str(argument) for argument in arguments], check=True)


def main():
    repository = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=repository.parent / "eunoia")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--diplodocus-bin", type=Path)
    options = parser.parse_args()
    source = options.source.resolve(strict=True)
    if not (source / "crates/eunoia/src/lib.rs").is_file():
        parser.error("--source must name Eunoia's workspace root")
    output = options.output.resolve() if options.output else Path(tempfile.mkdtemp(prefix="diplodocus-eunoia-rust-"))
    if output == source or source in output.parents:
        parser.error("--output must be outside the source checkout")
    if options.output:
        output.mkdir(parents=True, exist_ok=False)
    binary = options.diplodocus_bin
    if binary is None:
        run("cargo", "build", "--locked", "--manifest-path", repository / "Cargo.toml", "--bin", "diplodocus")
        metadata = json.loads(subprocess.check_output([
            "cargo", "metadata", "--locked", "--no-deps", "--format-version=1",
            "--manifest-path", str(repository / "Cargo.toml"),
        ]))
        binary = Path(metadata["target_directory"]) / "debug/diplodocus"
    binary = binary.resolve(strict=True)

    checkout = output / "sources/eunoia"
    shutil.copytree(source, checkout, ignore=shutil.ignore_patterns(
        ".git", "target", "node_modules", ".devenv", ".direnv", ".diplodocus",
        ".svelte-kit", ".docs-sources", "site", "dist", "__pycache__",
    ))
    configuration = output / "diplodocus.toml"
    revision = subprocess.run(["git", "-C", str(source), "rev-parse", "HEAD"], capture_output=True, text=True)
    revision = revision.stdout.strip() if revision.returncode == 0 else "main"
    configuration.write_text('''[project]
name = "Eunoia Rust API"

[[repository]]
id = "eunoia"
path = "sources/eunoia"
url = "https://github.com/jolars/eunoia"
revision = "REVISION"

[[package]]
id = "eunoia"
name = "Eunoia"
slug = "rust"
ecosystem = "rust"
repository = "eunoia"
path = "crates/eunoia"
metadata-path = "Cargo.toml"
targets = [{ id = "api", extractor = "rust", path = "src/lib.rs", role = "public-api" }]
'''.replace("REVISION", revision))
    snapshot = output / "documentation.sqlite"
    run(binary, "check", "--config", configuration)
    run(binary, "extract", "--config", configuration, "--output", snapshot)
    with sqlite3.connect(f"file:{snapshot}?mode=ro", uri=True) as database:
        items = [json.loads(row[0]) for row in database.execute(
            "SELECT content FROM records WHERE kind = 'item' AND owner = 'eunoia'"
        )]
        names = {item["name"] for item in items}
        required = {"Fitter", "Layout", "Circle", "Ellipse", "DiagramSpecBuilder", "DiagramShape", "GlyphOptions"}
        if missing := required - names:
            raise RuntimeError(f"Missing Eunoia API: {sorted(missing)}")
        if "test_utils" in names:
            raise RuntimeError("The default API unexpectedly includes corpus test utilities")
        workspace = json.loads(database.execute(
            "SELECT content FROM records WHERE kind = 'workspace'"
        ).fetchone()[0])
        outcomes = [example for record in workspace["provenance"]
                    if record["activity"]["kind"] == "rust-doctests"
                    for example in record["activity"]["examples"]]
    shutil.rmtree(output / "sources")
    run(binary, "generate", "--input", snapshot, "--output", output / "site")
    run(binary, "generate", "--format", "fragments", "--input", snapshot, "--output", output / "fragments")
    manifest = json.loads((output / "fragments/documents.json").read_text())
    print(json.dumps({
        "output": str(output), "api_items": len(items), "doctests": len(outcomes),
        "explicit_skips": sum(example["outcome"] == "explicit-skip" for example in outcomes),
        "fragment_documents": len(manifest["documents"]), "source_removed": True,
    }, indent=2))


if __name__ == "__main__":
    main()
