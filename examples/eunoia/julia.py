"""Validate Eunoia.jl's API and generate documentation from a detached snapshot."""

import argparse
import hashlib
import json
import os
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import tomllib
from collections import Counter
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urljoin, urlsplit

FUNCTIONS = {
    "euler",
    "venn",
    "version",
    "eunoiaplot",
    "eunoiaplot!",
    "place_labels",
    "label_boxes",
    "place_set_labels",
    "place_glyphs",
    "place_glyph_boxes",
}
TYPES = {
    "EulerFit",
    "VennFit",
    "Circle",
    "Ellipse",
    "Square",
    "Rectangle",
    "RotatedRectangle",
    "Point",
    "Container",
    "BoundingBox",
    "LabelPlacement",
    "GlyphPlacements",
    "GlyphBoxPlacements",
}
EXPORTS = FUNCTIONS | TYPES
RESULT_FIELDS = {
    "shapes",
    "original_values",
    "fitted_values",
    "residuals",
    "region_error",
    "diag_error",
    "stress",
    "loss",
    "iterations",
    "container",
    "plot_data",
}


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def run(*arguments, **options):
    subprocess.run([str(argument) for argument in arguments], check=True, **options)


class FragmentHTML(HTMLParser):
    def __init__(self, body):
        super().__init__()
        self.anchors = set()
        self.links = []
        self.list_items = []
        self.lists = []
        self.list_code_blocks = []
        self.code_block = None
        self.feed(body)

    def handle_starttag(self, tag, attributes):
        attributes = dict(attributes)
        if identifier := attributes.get("id"):
            self.anchors.add(identifier)
        for name in ("href", "src"):
            if value := attributes.get(name):
                self.links.append(value)
        if tag == "li":
            self.lists.append([])
        if tag == "pre" and self.lists:
            self.code_block = []

    def handle_endtag(self, tag):
        if tag == "li":
            self.list_items.append(" ".join("".join(self.lists.pop()).split()))
        if tag == "pre" and self.code_block is not None:
            self.list_code_blocks.append("".join(self.code_block).rstrip("\r\n"))
            self.code_block = None

    def handle_data(self, data):
        for item in self.lists:
            item.append(data)
        if self.code_block is not None:
            self.code_block.append(data)


def api_coverage(items):
    exported = {
        item["name"]: (identity, item)
        for identity, item in items.items()
        if item["kind"] != "method"
        and item["language_data"]["data"]["visibility"] == "exported"
    }
    require(
        not (missing := EXPORTS - exported.keys()),
        f"Missing Eunoia exports: {sorted(missing)}",
    )
    members = set()
    methods = set()
    for name in sorted(EXPORTS):
        identity, item = exported[name]
        declaration = item["language_data"]["data"]["declaration"]
        require(
            item["qualified_name"] == f"Eunoia.{name}",
            f"Wrong defining module for {name}",
        )
        require(
            item["kind"] == ("function" if name in FUNCTIONS else "type"),
            f"Wrong API kind for {name}: {item['kind']}",
        )
        if item["kind"] == "function":
            references = declaration["role"]["methods"]
            require(references, f"Missing core methods for {name}")
            for reference in references:
                method = items[reference["item"]]
                require(method["kind"] == "method", f"Invalid method of {name}")
                require(
                    method["language_data"]["data"]["declaration"]["role"]["family"]
                    == {
                        "package": "eunoia",
                        "item": identity,
                    },
                    f"Wrong callable family for {name}",
                )
                methods.add(reference["item"])
        if item["kind"] == "type":
            fields = {
                items[child]["name"]: child
                for child in item["children"]
                if items[child]["kind"] == "field"
            }
            require(fields, f"Missing fields of {name}")
            if name in {"EulerFit", "VennFit"}:
                require(
                    RESULT_FIELDS <= fields.keys(),
                    f"Incomplete result fields of {name}",
                )
            for field, child in fields.items():
                require(
                    items[child]["qualified_name"] == f"Eunoia.{name}.{field}",
                    f"Wrong owner of {name}.{field}",
                )
                members.add(child)
    required = {exported[name][0] for name in EXPORTS} | members | methods
    return required, {
        "exported_bindings": len(exported),
        "fields": len(members),
        "methods": len(methods),
    }


def snapshot_coverage(snapshot, forbidden_paths):
    with sqlite3.connect(f"{snapshot.as_uri()}?mode=ro", uri=True) as database:
        records = list(database.execute("SELECT kind, owner, id, content FROM records"))
    for _, _, _, content in records:
        for path in forbidden_paths:
            require(
                str(path) not in content, f"Snapshot contains a checkout path: {path}"
            )
    items = {
        identity: json.loads(content)
        for kind, owner, identity, content in records
        if kind == "item" and owner == "eunoia"
    }
    workspace = next(
        json.loads(content) for kind, _, _, content in records if kind == "workspace"
    )
    diagnostics = workspace["diagnostics"]
    require(
        not any(d["severity"] == "error" for d in diagnostics),
        "Snapshot contains errors",
    )
    for diagnostic in diagnostics:
        if diagnostic["code"] == "julia-unsupported-markdown":
            require(
                not re.search(
                    r"Markdown (INLINE_CODE|EMPHASIS|STRONG|LINK|IMAGE|AUTOLINK) ",
                    diagnostic["message"],
                ),
                f"Supported inline markup became a block: {diagnostic}",
            )
    required, coverage = api_coverage(items)
    return required, coverage | {"api_items": len(items)}, diagnostics


def documenter_coverage(snapshot):
    with sqlite3.connect(f"{snapshot.as_uri()}?mode=ro", uri=True) as database:
        pages = [
            json.loads(content)
            for (content,) in database.execute(
                "SELECT content FROM records WHERE kind = 'page'"
            )
        ]
        assets = {
            digest: bytes(content)
            for digest, content in database.execute("SELECT digest, bytes FROM assets")
        }
    counts = {}
    figures = {}
    for page in pages:
        document = page["document"]
        for record in document["provenance"]:
            activity = record["activity"]
            if activity["kind"] != "documenter-examples":
                continue
            require(
                activity["policy"] == "documenter-examples-v1",
                "Unexpected Documenter policy",
            )
            require(activity["julia_version"], "Missing observed Julia version")
            require(
                "13f3f980-e62b-5c42-98c6-ff1f3baf88f0" in activity["dependencies"],
                "CairoMakie was not loaded",
            )
            inputs = {item["source"]["path"] for item in activity["environment_inputs"]}
            require(
                {"docs/Project.toml", "docs/Manifest.toml"} <= inputs,
                "Missing prepared environment fingerprints",
            )
            counts[document["source_location"]["path"]] = len(activity["examples"])
            for example in activity["examples"]:
                for output in example["outputs"]:
                    if output["type"] == "image":
                        digest = output["asset"]["fingerprint"]["value"]
                        require(digest in assets, f"Missing native figure {digest}")
                        require(
                            hashlib.sha256(assets[digest]).hexdigest() == digest,
                            "Native figure digest mismatch",
                        )
                        figures[digest] = assets[digest]
    require(
        counts == {"docs/src/index.md": 1, "docs/src/gallery.md": 30},
        f"Incomplete Documenter execution: {counts}",
    )
    require(len(figures) == 30, f"Expected 30 native figures, found {len(figures)}")
    return figures


def validate_retained_figures(root, figures):
    found = {}
    for path in root.rglob("*"):
        if path.is_file():
            content = path.read_bytes()
            digest = hashlib.sha256(content).hexdigest()
            if digest in figures:
                require(
                    content == figures[digest], f"Changed native figure bytes: {path}"
                )
                found[digest] = path
    require(
        found.keys() == figures.keys(), f"Missing detached native figures in {root}"
    )


def validate_fragments(root, required):
    manifest = json.loads((root / "documents.json").read_text())
    require(manifest["format_version"] == 2, "Expected fragment manifest version 2")
    documents = manifest["documents"]
    mappings = json.loads((root.parent / "urls.json").read_text())
    require(
        {d["path"] for d in documents} == mappings["documents"].keys(),
        "Mapped fragments changed the document inventory",
    )
    for document in documents:
        require(
            document["url"] == mappings["documents"][document["path"]],
            f"Document URL mapping was not applied: {document['path']}",
        )
    covered = {
        identity["item"]["item"]
        for document in documents
        for identity in document["identities"]
        if identity["kind"] == "item"
    }
    require(
        required <= covered,
        f"Missing API fragment identities: {sorted(required - covered)}",
    )
    html = {d["url"]: FragmentHTML((root / d["path"]).read_text()) for d in documents}
    markdown = {d["url"]: (root / d["markdown_path"]).read_text() for d in documents}
    markdown_anchors = {
        url: FragmentHTML(body).anchors for url, body in markdown.items()
    }
    assets = set(mappings["assets"].values())
    for artifact, url in mappings["assets"].items():
        require((root / artifact).is_file(), f"Missing retained asset: {url}")
    links = 0
    for document in documents:
        url = document["url"]
        for _, anchor, _ in document["headings"]:
            require(
                anchor in html[url].anchors and anchor in markdown_anchors[url],
                f"HTML and Markdown anchors disagree at {url}#{anchor}",
            )
        targets = html[url].links + re.findall(r"\]\(<([^>\n]+)>\)", markdown[url])
        for target in targets:
            destination = urlsplit(urljoin(url, target))
            if destination.scheme or destination.netloc:
                continue
            path = destination.path
            require(
                path in html or path in assets,
                f"Unmapped local link in {url}: {target}",
            )
            if path in html and destination.fragment:
                anchor = unquote(destination.fragment)
                require(
                    anchor in html[path].anchors and anchor in markdown_anchors[path],
                    f"Missing HTML or Markdown anchor in {url}: {target}",
                )
            links += 1

    for name, snippets in {
        "Eunoia.EulerFit": [
            (
                'original_values: the requested exclusive area per region (keyed by combination string, e.g. "A&B").',
                '`original_values`: the requested exclusive area per region (keyed by combination string, e.g. `"A&B"`).',
            ),
        ],
        "Eunoia.eunoiaplot": [
            (
                "colors: per-set colors — a vector (shape order) or a Dict(name => color); omitted uses a built-in categorical palette. Region fills blend the member colors perceptually (OKLab).",
                "`colors`: per-set colors — a vector (shape order) or a `Dict(name => color)`; omitted uses a built-in categorical palette. Region fills blend the member colors perceptually (OKLab).",
            ),
        ],
    }.items():
        document = next(d for d in documents if d["public_name"] == name)
        for plain, formatted in snippets:
            require(
                html[document["url"]].list_items.count(plain) == 1,
                f"Duplicated or damaged HTML list text in {name}",
            )
            require(
                markdown[document["url"]].count(formatted) == 1,
                f"Duplicated or damaged Markdown list text in {name}",
            )
    euler = next(d for d in documents if d["public_name"] == "Eunoia.euler")
    examples = [
        'euler(Dict("A" => 5, "B" => 3, "A&B" => 1.5))',
        'euler(Dict("A" => ["x", "y"], "B" => ["y", "z"]))',
    ]
    code_examples = sum(
        html[euler["url"]].list_code_blocks.count(example) == 1
        and bool(
            re.search(
                r"(?m)^([ ]*)```julia\n\1" + re.escape(example) + r"\n\1```$",
                markdown[euler["url"]],
            )
        )
        for example in examples
    )
    literal_fences = sum(
        "```" in item for body in html.values() for item in body.list_items
    )
    require(
        code_examples == 2,
        "Eunoia.euler list examples must render as code in HTML and Markdown",
    )
    require(literal_fences == 0, "List items still contain literal fences")
    return {
        "fragment_documents": len(documents),
        "checked_local_links": links,
        "euler_list_code_examples": code_examples,
        "list_items_with_literal_fences": literal_fences,
    }


def main():
    repository = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=repository.parent / "Eunoia.jl")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--diplodocus-bin", type=Path)
    parser.add_argument(
        "--execute-documenter",
        action="store_true",
        help="Execute the original guides with an already prepared docs/Project.toml and Manifest.toml",
    )
    options = parser.parse_args()
    source = options.source.resolve(strict=True)
    metadata = tomllib.loads((source / "Project.toml").read_text())
    if metadata.get("name") != "Eunoia" or not (source / "src/Eunoia.jl").is_file():
        parser.error("--source must name the Eunoia.jl package root")
    output = (
        options.output.resolve()
        if options.output
        else Path(tempfile.mkdtemp(prefix="diplodocus-eunoia-julia-"))
    )
    if output == source or source in output.parents:
        parser.error("--output must be outside the source checkout")
    if options.output:
        output.mkdir(parents=True, exist_ok=False)
    print(f"Acceptance artifacts: {output}", file=sys.stderr, flush=True)
    binary = options.diplodocus_bin
    if binary is None:
        run(
            "cargo",
            "build",
            "--locked",
            "--manifest-path",
            repository / "Cargo.toml",
            "--bin",
            "diplodocus",
        )
        cargo_metadata = json.loads(
            subprocess.check_output(
                [
                    "cargo",
                    "metadata",
                    "--locked",
                    "--no-deps",
                    "--format-version=1",
                    "--manifest-path",
                    str(repository / "Cargo.toml"),
                ]
            )
        )
        binary = Path(cargo_metadata["target_directory"]) / "debug/diplodocus"
    binary = binary.resolve(strict=True)
    revision = subprocess.run(
        ["git", "-C", str(source), "rev-parse", "HEAD"],
        check=False,
        capture_output=True,
        text=True,
    )
    revision = revision.stdout.strip() if revision.returncode == 0 else None

    extraction = output / "extraction"
    checkout = extraction / "sources/Eunoia.jl"
    shutil.copytree(
        source,
        checkout,
        ignore=shutil.ignore_patterns(
            ".git",
            ".devenv",
            ".direnv",
            ".diplodocus",
            "node_modules",
            "build",
            "site",
            "__pycache__",
        ),
    )
    if options.execute_documenter:
        manifest = checkout / "docs/Manifest.toml"
        require(
            manifest.is_file(),
            "Prepare Eunoia's documentation dependencies before running --execute-documenter",
        )
        text = manifest.read_text()
        text, replaced = re.subn(
            r"(?ms)(^\[\[deps\.Eunoia\]\]\n(?:(?!^\[).)*?^path\s*=\s*)[^\n]+",
            r'\g<1>".."',
            text,
        )
        require(
            replaced == 1,
            "The prepared docs manifest must select the Eunoia checkout by path",
        )
        manifest.write_text(text)
    (checkout / "diplodocus-overview.md").write_text(
        "# Eunoia.jl API\n\n"
        "This acceptance run covers Julia API documentation.\n\n"
        "Start with [`Eunoia.euler`], [`Eunoia.venn`], [`Eunoia.eunoiaplot!`], "
        "[`Eunoia.EulerFit`], and [`Eunoia.VennFit`].\n"
    )
    configuration = extraction / "diplodocus.yaml"
    configuration.write_text(
        """project:
  name: "Eunoia Julia API"
repositories:
  - id: "eunoia"
    path: "sources/Eunoia.jl"
    url: "https://github.com/jolars/Eunoia.jl"
REVISION
packages:
  - id: "eunoia"
    name: "Eunoia.jl"
    slug: "julia"
    ecosystem: "julia"
    repository: "eunoia"
    path: "."
    metadata-path: "Project.toml"
    overview: "diplodocus-overview.md"
    targets:
      - id: "api"
        extractor: "julia"
        path: "src/Eunoia.jl"
        role: "public-api"
""".replace("REVISION", f"    revision: {json.dumps(revision)}" if revision else "")
    )
    if options.execute_documenter:
        configuration.write_text(
            configuration.read_text().replace(
                '    overview: "diplodocus-overview.md"\n', ""
            )
            + """content:
  - id: "julia-manual"
    owner: "eunoia"
    repository: "eunoia"
    path: "docs/src"
    mount: ""
    format: "documenter"
    execution:
      mode: "execute"
      engine: "julia"
      julia-project: "docs/Project.toml"
"""
        )
    snapshot = extraction / "documentation.sqlite"
    run(binary, "check", "--config", configuration)
    run(binary, "extract", "--config", configuration, "--output", snapshot)
    required, coverage, diagnostics = snapshot_coverage(snapshot, [source, extraction])
    figures = documenter_coverage(snapshot) if options.execute_documenter else {}
    (output / "diagnostics.json").write_text(json.dumps(diagnostics, indent=2) + "\n")

    detached = output / "detached"
    detached.mkdir()
    snapshot = snapshot.rename(detached / "documentation.sqlite")
    shutil.rmtree(extraction)
    environment = {
        key: value
        for key, value in os.environ.items()
        if not key.startswith("JULIA_") and key != "EUNOIA_CAPI_LIB"
    }
    environment["PATH"] = ""
    run(
        binary,
        "generate",
        "--input",
        snapshot,
        "--output",
        detached / "site",
        cwd=detached,
        env=environment,
    )
    fragments = detached / "fragments"
    run(
        binary,
        "generate",
        "--format",
        "fragments",
        "--input",
        snapshot,
        "--output",
        fragments,
        cwd=detached,
        env=environment,
    )
    manifest = json.loads((fragments / "documents.json").read_text())
    mappings = {
        "format_version": 1,
        "documents": {
            d["path"]: f"/docs/reference/julia/{i}/"
            for i, d in enumerate(manifest["documents"])
        },
        "assets": {
            p.relative_to(
                fragments
            ).as_posix(): f"/_diplodocus/julia/{p.relative_to(fragments / 'assets').as_posix()}"
            for p in sorted((fragments / "assets").rglob("*"))
            if p.is_file()
        },
    }
    urls = detached / "urls.json"
    urls.write_text(json.dumps(mappings, indent=2) + "\n")
    mapped = detached / "mapped-fragments"
    run(
        binary,
        "generate",
        "--format",
        "fragments",
        "--input",
        snapshot,
        "--url-map",
        urls,
        "--output",
        mapped,
        cwd=detached,
        env=environment,
    )
    coverage |= validate_fragments(mapped, required)
    if figures:
        for root in [detached / "site", fragments, mapped]:
            validate_retained_figures(root, figures)
        coverage |= {
            "executed_documenter_examples": 31,
            "retained_native_figures": len(figures),
        }
    for path in (fragments / "assets").rglob("*"):
        # Search records contain mapped URLs; styles and retained assets keep their bytes.
        if (
            path.is_file()
            and path.relative_to(fragments).as_posix() != "assets/search.json"
        ):
            require(
                path.read_bytes()
                == (mapped / path.relative_to(fragments)).read_bytes(),
                f"URL mapping changed retained asset bytes: {path}",
            )
    summary = {
        "output": str(output),
        "package_version": metadata.get("version"),
        "source_revision": revision,
        **coverage,
        "diagnostics": dict(Counter(d["code"] for d in diagnostics)),
        "source_removed": not extraction.exists(),
        "generation_path": environment["PATH"],
        "scope": "core-api-and-executed-documenter-guides"
        if options.execute_documenter
        else "core-api-without-guides-or-gallery",
    }
    (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
