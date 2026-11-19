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


def export(binary, root, name, repo, output, dll=None):
    source = root / name
    revision = stage(repo, source)
    if name == "qualpal":
        (source / "Doxyfile").write_text(
            "PROJECT_NAME = Qualpal\nINPUT = include\nRECURSIVE = YES\n"
            "FILE_PATTERNS = *.h\nEXTRACT_ALL = YES\nQUIET = YES\n"
        )
        (source / "BUNDLE.md").write_text("# Qualpal\n\nSee [`qualpalr::qualpal`].\n")
        config = producer(root, name, revision, "cpp", "Doxyfile", "doxygen", "Doxyfile")
    else:
        (source / "BUNDLE.md").write_text("# qualpalr\n\nSee [`qualpal::Qualpal`].\n")
        patch = ROOT / "docs/development/benchmarks/qualpalr-description.patch"
        applicable = subprocess.run(
            ["git", "apply", "--check", patch], cwd=source, capture_output=True
        ).returncode == 0
        if applicable:
            run("git", "apply", patch, cwd=source)
        # Keep the mixed build on the same API-only profile as the export.
        if (source / "vignettes").exists():
            shutil.rmtree(source / "vignettes")
        if dll:
            shutil.copyfile(dll, source / "src/qualpalr.so")
        else:
            run("Rscript", "--vanilla", "-e",
                "pkgbuild::compile_dll('.', force=TRUE, compile_attributes=FALSE, debug=FALSE)",
                cwd=source)
        config = producer(root, name, revision, "r", "DESCRIPTION", "r", ".")
    config_path = root / f"{name}.yaml"
    write_config(config_path, **config)
    run(binary, "bundle", "export", "--config", config_path, "--output", output, cwd=root)
    with sqlite3.connect(output) as conn:
        revisions = [
            json.loads(content)["revision"]
            for (content,) in conn.execute("select content from records where kind = 'repository'")
        ]
    assert revisions == [revision], f"{output} should record source revision {revision}"
    return config


CPP_IMPORT = {"package": "qualpal", "path": "qualpal.sqlite"}
COMPOSITION = {
    "concepts": [{"id": "palette", "kind": "related", "members": [
        {"package": "qualpal", "item": "qualpal::Qualpal"},
        {"package": "qualpalr", "item": "qualpal"},
    ]}],
    "relationships": [{"from": "qualpalr", "to": "qualpal", "kind": "binds"}],
}


def assemble(binary, root, cpp_bundle, r_bundle):
    consumer = root / "consumer"
    consumer.mkdir()
    shutil.copyfile(cpp_bundle, consumer / "qualpal.sqlite")
    shutil.copyfile(r_bundle, consumer / "qualpalr.sqlite")
    config = consumer / "diplodocus.yaml"
    write_config(config, **COMPOSITION, **{"package-bundles": [
        CPP_IMPORT, {"package": "qualpalr", "path": "qualpalr.sqlite"},
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
        for package, target in (("qualpal", "qualpalr"), ("qualpalr", "qualpal")):
            overview = (site / f"packages/{package}/index.{suffix}").read_text()
            assert f"../{target}/reference/" in overview
        pages = (site / "packages/qualpalr").rglob(f"*.{suffix}")
        assert any(figure in page for page in map(Path.read_text, pages) for figure in figures)
    print(f"Generated site: {site}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qualpal", type=Path, default=ROOT.parent / "qualpal")
    parser.add_argument("--qualpalr", type=Path, default=ROOT.parent / "qualpalr")
    parser.add_argument("--diplodocus-bin", type=Path, default=ROOT / "target/debug/diplodocus")
    parser.add_argument("--qualpalr-dll", type=Path)
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--export", choices=("qualpal", "qualpalr"))
    modes.add_argument("--assemble", nargs=2, type=Path, metavar=("CPP_BUNDLE", "R_BUNDLE"))
    parser.add_argument("--output", type=Path, help="bundle file for --export or directory for --assemble")
    args = parser.parse_args()
    if bool(args.output) != bool(args.export or args.assemble):
        parser.error("--output is required with --export or --assemble")

    binary = args.diplodocus_bin.resolve(strict=True)
    if args.export:
        with tempfile.TemporaryDirectory(prefix="qualpal-export-") as directory:
            export(binary, Path(directory), args.export, getattr(args, args.export),
                   args.output.resolve(), args.qualpalr_dll)
        return

    if args.assemble:
        args.output.mkdir(parents=True)
        assemble(binary, args.output.resolve(), *args.assemble)
        return

    root = Path(tempfile.mkdtemp(prefix="qualpal-bundles-"))
    print(root, flush=True)
    cpp_bundle = root / "qualpal.sqlite"
    r_bundle = root / "qualpalr.sqlite"
    export(binary, root, "qualpal", args.qualpal, cpp_bundle)
    r_config = export(binary, root, "qualpalr", args.qualpalr, r_bundle, args.qualpalr_dll)
    shutil.rmtree(root / "qualpal")
    mixed = root / "mixed.yaml"
    write_config(mixed, **r_config, **COMPOSITION, **{"package-bundles": [CPP_IMPORT]})
    shim = root / "without-doxygen"
    shim.mkdir()
    (shim / "doxygen").write_text("#!/bin/sh\nexit 71\n")
    (shim / "doxygen").chmod(0o755)
    run(binary, "build", "--config", mixed, "--output", root / "mixed",
        cwd=root, env={**os.environ, "PATH": f"{shim}{os.pathsep}{os.environ['PATH']}"})
    shutil.rmtree(root / "qualpalr")
    assemble(binary, root, cpp_bundle, r_bundle)


if __name__ == "__main__":
    main()
