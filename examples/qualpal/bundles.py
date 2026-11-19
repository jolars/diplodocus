#!/usr/bin/env python3
"""Exercise independent C++ and R exports and detached site assembly."""

import argparse
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def run(*args, cwd=None, env=None):
    subprocess.run(args, cwd=cwd, env=env, check=True)


def write_config(path, **values):
    path.write_text(json.dumps({"project": {"name": "Qualpal"}, **values}))


def stage(repo, dest):
    revision = subprocess.check_output(
        ["git", "-C", repo, "rev-parse", "HEAD"], text=True
    ).strip()
    archive = subprocess.check_output(["git", "-C", repo, "archive", revision])
    dest.mkdir()
    subprocess.run(["tar", "-xf", "-"], cwd=dest, input=archive, check=True)
    return revision


def producer(root, name, source, ecosystem, metadata, extractor, target):
    return {
        "repositories": [{
            "id": name, "path": str(root / name),
            "url": f"https://github.com/jolars/{name}", "revision": source,
        }],
        "packages": [{
            "id": name, "name": name, "slug": name, "ecosystem": ecosystem,
            "repository": name, "path": ".", "metadata-path": metadata,
            "overview": "BUNDLE.md",
            "targets": [{"id": "api", "extractor": extractor, "path": target,
                         "role": "public-api"}],
        }],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qualpal", type=Path, default=ROOT.parent / "qualpal")
    parser.add_argument("--qualpalr", type=Path, default=ROOT.parent / "qualpalr")
    parser.add_argument("--diplodocus-bin", type=Path, default=ROOT / "target/debug/diplodocus")
    parser.add_argument("--qualpalr-dll", type=Path)
    args = parser.parse_args()
    binary = args.diplodocus_bin.resolve(strict=True)
    root = Path(tempfile.mkdtemp(prefix="qualpal-bundles-"))
    print(root, flush=True)
    cpp, r = root / "qualpal", root / "qualpalr"
    cpp_revision = stage(args.qualpal, cpp)
    r_revision = stage(args.qualpalr, r)
    (cpp / "Doxyfile").write_text(
        "PROJECT_NAME = Qualpal\nINPUT = include\nRECURSIVE = YES\n"
        "FILE_PATTERNS = *.h\nEXTRACT_ALL = YES\nQUIET = YES\n"
    )
    (cpp / "BUNDLE.md").write_text("# Qualpal\n\nSee [`qualpalr::qualpal`].\n")
    (r / "BUNDLE.md").write_text("# qualpalr\n\nSee [`qualpal::Qualpal`].\n")
    patch = ROOT / "docs/development/benchmarks/qualpalr-description.patch"
    applicable = subprocess.run(
        ["git", "apply", "--check", patch], cwd=r, capture_output=True
    ).returncode == 0
    if applicable:
        run("git", "apply", patch, cwd=r)
    # Keep the mixed build on the same API-only profile as the exports.
    if (r / "vignettes").exists():
        shutil.rmtree(r / "vignettes")
    if args.qualpalr_dll:
        shutil.copyfile(args.qualpalr_dll, r / "src/qualpalr.so")
    else:
        run("Rscript", "--vanilla", "-e",
            "pkgbuild::compile_dll('.', force=TRUE, compile_attributes=FALSE, debug=FALSE)", cwd=r)

    cpp_config = producer(root, "qualpal", cpp_revision, "cpp", "Doxyfile", "doxygen", "Doxyfile")
    r_config = producer(root, "qualpalr", r_revision, "r", "DESCRIPTION", "r", ".")
    for name, config in [("qualpal", cpp_config), ("qualpalr", r_config)]:
        config_path = root / f"{name}.yaml"
        write_config(config_path, **config)
        run(binary, "bundle", "export", "--config", config_path,
            "--output", root / f"{name}.sqlite", cwd=root)

    cpp_import = {"package": "qualpal", "path": "qualpal.sqlite"}
    composition = {
        "concepts": [{"id": "palette", "kind": "related", "members": [
            {"package": "qualpal", "item": "qualpal::Qualpal"},
            {"package": "qualpalr", "item": "qualpal"},
        ]}],
        "relationships": [{"from": "qualpalr", "to": "qualpal", "kind": "binds"}],
    }
    shutil.rmtree(cpp)
    mixed = root / "mixed.yaml"
    write_config(mixed, **r_config, **composition, **{"package-bundles": [cpp_import]})
    shim = root / "without-doxygen"
    shim.mkdir()
    (shim / "doxygen").write_text("#!/bin/sh\nexit 71\n")
    (shim / "doxygen").chmod(0o755)
    run(binary, "build", "--config", mixed, "--output", root / "mixed",
        cwd=root, env={**os.environ, "PATH": f"{shim}{os.pathsep}{os.environ['PATH']}"})
    shutil.rmtree(r)

    consumer = root / "consumer"
    consumer.mkdir()
    for name in ("qualpal", "qualpalr"):
        (root / f"{name}.sqlite").rename(consumer / f"{name}.sqlite")
    config = consumer / "diplodocus.yaml"
    write_config(config, **composition, **{"package-bundles": [
        cpp_import, {"package": "qualpalr", "path": "qualpalr.sqlite"},
    ]})
    detached = root / "detached"
    detached.mkdir()
    snapshot = consumer / "documentation.sqlite"
    no_tools = {**os.environ, "PATH": ""}
    run(binary, "extract", "--config", config, "--output", snapshot, cwd=consumer, env=no_tools)
    final_snapshot = detached / snapshot.name
    snapshot.rename(final_snapshot)
    shutil.rmtree(consumer)
    site = detached / "site"
    run(binary, "generate", "--input", final_snapshot, "--output", site, cwd=detached, env=no_tools)

    with sqlite3.connect(final_snapshot) as conn:
        records = [(kind, owner, json.loads(value)) for kind, owner, value in
                   conn.execute("select kind, owner, content from records")]
        assert conn.execute("select count(*) from assets where media_type = 'image/png'").fetchone()[0]
    assert any(kind == "item" and owner == "qualpal" and
               (value.get("language_data") or {}).get("data", {}).get("overloads") for kind, owner, value in records)
    assert any(kind == "item" and owner == "qualpalr" and value["name"] == "plot.qualpal"
               for kind, owner, value in records)
    assert any(kind == "concept" for kind, _, _ in records)
    assert any(kind == "workspace" and value["relationships"][0]["kind"] == "binds"
               for kind, _, value in records)
    assert any(kind == "item" and owner == "qualpalr"
               and any(receipt["activity"]["kind"] == "rd-examples"
                       for receipt in (value.get("documentation") or {}).get("provenance", []))
               for kind, owner, value in records)
    figures = {path.name for path in site.rglob("*.png")}
    assert figures
    for suffix in ("html", "md"):
        overview = (site / f"packages/qualpal/index.{suffix}").read_text()
        assert "../qualpalr/" in overview and "qualpal" in overview
        pages = (site / "packages/qualpalr").rglob(f"*.{suffix}")
        assert any(figure in page for page in map(Path.read_text, pages) for figure in figures)
    print(f"Generated site: {site}")


if __name__ == "__main__":
    main()
