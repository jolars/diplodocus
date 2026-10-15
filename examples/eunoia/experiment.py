"""Embed a real package snapshot in a disposable copy of Eunoia's website."""

import argparse
import html
import json
import os
import re
import shlex
import shutil
import sqlite3
import subprocess
import tempfile
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urljoin, urlsplit, urlunsplit

PROJECT = Path(__file__).resolve().parents[2]


def run(*command, cwd=PROJECT):
    print("+", *map(str, command), flush=True)
    environment = os.environ.copy()
    if Path(command[0]).name == "pnpm":
        # The copied site uses its existing dependencies; downloading another
        # pnpm executable breaks NixOS without changing this build's inputs.
        environment["npm_config_manage_package_manager_versions"] = "false"
    subprocess.run(list(map(str, command)), cwd=cwd, env=environment, check=True)


def prepare(output, python, r):
    sources = output / "sources"
    sources.mkdir()
    py = sources / "python"
    py.mkdir()
    shutil.copy2(python / "pyproject.toml", py)
    (py / "python/eunoia").mkdir(parents=True)
    shutil.copy2(python / "python/eunoia/_fit.py", py / "python/eunoia/_fit.py")
    # Limit the public export list in this disposable package slice. The real
    # fitting module and its docstrings remain unchanged and keep their paths.
    version = re.search(
        r"^__version__ = (.+)$",
        (python / "python/eunoia/__init__.py").read_text(),
        re.MULTILINE,
    )[1]
    (py / "python/eunoia/__init__.py").write_text(
        f'from ._fit import euler\n__all__ = ["euler"]\n__version__ = {version}\n'
    )
    shutil.copytree(
        r,
        sources / "r",
        ignore=shutil.ignore_patterns(
            ".git", ".devenv*", ".direnv", "target", "docs", "vignettes", "__pycache__"
        ),
    )
    # Keep this experiment on API references; the upstream README's remote
    # badges require an asset policy that the snapshot reader does not support.
    (sources / "r/README.md").unlink(missing_ok=True)
    revisions = {
        name: subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=root, text=True
        ).strip()
        for name, root in [("python", python), ("r", r)]
    }
    (output / "diplodocus.toml").write_text(f'''[project]
name = "Eunoia"

[[repository]]
id = "python"
path = "sources/python"
url = "https://github.com/jolars/eunoia-py"
revision = "{revisions["python"]}"

[[repository]]
id = "r"
path = "sources/r"
url = "https://github.com/jolars/eulerr"
revision = "{revisions["r"]}"

[[package]]
id = "python"
name = "Eunoia for Python"
slug = "python"
ecosystem = "python"
repository = "python"
path = "."
metadata-path = "pyproject.toml"
targets = [{{ id = "api", extractor = "python", path = "python/eunoia", role = "public-api" }}]

[[package]]
id = "r"
name = "eulerr"
slug = "r"
ecosystem = "r"
repository = "r"
path = "."
metadata-path = "DESCRIPTION"
targets = [{{ id = "api", extractor = "r", path = ".", role = "public-api" }}]

[[concept]]
id = "euler"
kind = "analogous"
members = [{{ package = "python", item = "eunoia.euler" }}, {{ package = "r", item = "euler" }}]
''')


def document_urls(documents):
    urls = {}
    for document in documents:
        path = document["path"]
        if document["qualified_name"]:
            name = (
                document["title"]
                if document["package"] == "python"
                else document["qualified_name"]
            ).removeprefix("eunoia.")
            name = re.sub(
                r"[^a-zA-Z0-9_.-]", lambda match: f"u{ord(match[0]):04x}-", name
            ).strip("-.")
            url = f"/docs/reference/{document['package']}/{name}/"
        elif path.startswith("packages/"):
            url = (
                "/docs/reference/"
                + path.removeprefix("packages/")
                .removesuffix("index.html")
                .removesuffix(".html")
                .rstrip("/")
                + "/"
            )
        else:
            url = "/docs/reference/snapshot/" + path.removesuffix(".html") + "/"
        if url in urls.values():
            raise ValueError(f"Duplicate host route: {url}")
        urls[path] = url
    return urls


class HostLinks(HTMLParser):
    """Resolve fragment URLs against their original paths before remounting."""

    def __init__(self, source, urls):
        super().__init__(convert_charrefs=False)
        self.source = source
        self.urls = urls
        self.parts = []

    def link(self, value):
        parsed = urlsplit(value)
        if parsed.scheme or parsed.netloc or not parsed.path:
            return value
        resolved = unquote(urlsplit(urljoin("/" + self.source, value)).path).lstrip("/")
        if resolved in self.urls:
            path = self.urls[resolved]
        elif resolved.startswith("assets/"):
            path = "/_diplodocus/" + resolved.removeprefix("assets/")
        else:
            raise ValueError(f"Unmapped local URL in {self.source}: {value}")
        return urlunsplit(("", "", path, parsed.query, parsed.fragment))

    def handle_starttag(self, tag, attrs):
        if tag == "table":
            self.parts.append('<div class="table-scroll">')
        attributes = []
        for key, value in attrs:
            if value is not None and key in ("href", "src", "poster"):
                value = self.link(value)
            attributes.append(
                key if value is None else f'{key}="{html.escape(value, quote=True)}"'
            )
        self.parts.append(
            "<" + tag + (" " + " ".join(attributes) if attributes else "") + ">"
        )

    def handle_startendtag(self, tag, attrs):
        self.handle_starttag(tag, attrs)

    def handle_endtag(self, tag):
        self.parts.append(f"</{tag}>")
        if tag == "table":
            self.parts.append("</div>")

    def handle_data(self, data):
        self.parts.append(data)

    def handle_entityref(self, name):
        self.parts.append(f"&{name};")

    def handle_charref(self, name):
        self.parts.append(f"&#{name};")


def write_page(web, url, title, body):
    route = web / "src/routes" / url.strip("/")
    route.mkdir(parents=True, exist_ok=True)
    if list(route.glob("+page.*")):
        raise ValueError(f"Host page already exists: {url}")
    # JSON strings preserve literal braces and backticks in the generated HTML.
    content = json.dumps(body, ensure_ascii=True).replace("</", "<\\/")
    (route / "+page.svelte").write_text(f"""<script>
  const content = {content};
</script>

<svelte:head>
  <link rel="stylesheet" href="/_diplodocus/content.css" />
</svelte:head>

<div class="diplodocus-content">{{@html content}}</div>
""")
    (route / "+page.ts").write_text(
        f"export const load = () => ({{ title: {json.dumps(title)} }});\n"
    )


def embed(output, eunoia, snapshot, pnpm):
    fragments = output / "fragments"
    run(
        PROJECT / "target/debug/examples/export-fragments",
        "--input",
        snapshot,
        "--output",
        fragments,
    )
    documents = json.loads((fragments / "documents.json").read_text())["documents"]
    urls = document_urls(documents)
    web = output / "web"
    shutil.copytree(
        eunoia / "web",
        web,
        ignore=shutil.ignore_patterns(
            "node_modules", "dist", ".svelte-kit", ".wrangler"
        ),
    )
    (web / "node_modules").symlink_to(
        eunoia / "web/node_modules", target_is_directory=True
    )
    (output / "npm").symlink_to(eunoia / "npm", target_is_directory=True)
    (output / "ts").mkdir()
    shutil.copy2(eunoia / "ts/package.json", output / "ts/package.json")
    assets = web / "static/_diplodocus"
    shutil.copytree(fragments / "assets", assets)
    shutil.copy2(snapshot, assets / "documentation.sqlite")
    if (assets / "katex/katex.min.css").exists():
        css = assets / "content.css"
        css.write_text(
            '@import url("/_diplodocus/katex/katex.min.css");\n' + css.read_text()
        )
    # The host owns spacing and layout; Diplodocus supplies only token colors.
    with (assets / "content.css").open("a") as css:
        css.write("""
.diplodocus-content { min-width: 0; }
.diplodocus-content pre { max-width: 100%; overflow-x: auto; white-space: pre; }
.diplodocus-content .table-scroll { max-width: 100%; overflow-x: auto; }
.diplodocus-content img { max-width: 100%; height: auto; }
.diplodocus-content .package { font-size: .875rem; opacity: .7; }
.diplodocus-content dt { font-weight: 600; margin-top: 1rem; }
.diplodocus-content dd { margin-left: 1rem; }
""")
    for document in documents:
        parser = HostLinks(document["path"], urls)
        parser.feed((fragments / document["path"]).read_text())
        write_page(
            web, urls[document["path"]], document["title"], "".join(parser.parts)
        )
    search = json.loads((assets / "search.json").read_text())
    for entry in search:
        entry["path"] = urls[unquote(entry["path"])]
    (assets / "search.json").write_text(json.dumps(search) + "\n")

    # This custom index consumes SQLite directly; it does not parse rendered HTML.
    with sqlite3.connect(
        snapshot.as_uri() + "?mode=ro&immutable=1", uri=True
    ) as database:
        versions = database.execute(
            "SELECT storage_version, ir_version, encoding_version FROM manifest WHERE id = 1"
        ).fetchone()
        if versions != (2, 5, 1):
            raise ValueError(f"Unsupported snapshot versions: {versions}")
        packages = database.execute("""
            SELECT id, json_extract(content, '$.name'), json_extract(content, '$.version'),
                   (SELECT count(*) FROM records i WHERE i.kind = 'item' AND i.owner = p.id)
            FROM records p WHERE kind = 'package' ORDER BY id
        """).fetchall()
    rows = "".join(
        f'<tr><td><a href="/docs/reference/{html.escape(id)}/">{html.escape(name)}</a></td>'
        f"<td>{html.escape(version or '')}</td><td>{count}</td></tr>"
        for id, name, version, count in packages
    )
    write_page(
        web,
        "/docs/reference/packages/",
        "Package API",
        f"""<h1>Package API</h1>
<p>Browse the Python and R interfaces to Eunoia alongside the shared guides.</p>
<p><a href="/docs/reference/python/euler/">Python: euler()</a> ·
<a href="/docs/reference/r/euler/">R: euler()</a> ·
<a href="/docs/reference/r/plot.euler/">R: plot.euler()</a></p>
<table><thead><tr><th>Package</th><th>Version</th><th>API items</th></tr></thead><tbody>{rows}</tbody></table>
<p><a href="/_diplodocus/documentation.sqlite" download>Download the documentation as SQLite</a></p>""",
    )
    summary = web / "src/lib/docs/SUMMARY.ts"
    summary.write_text(
        summary.read_text().replace(
            '{ title: "Overview", slug: "/docs/reference/" },',
            '{ title: "Overview", slug: "/docs/reference/" },\n'
            '      { title: "Package API", slug: "/docs/reference/packages/" },',
        )
    )
    llms = web / "static/llms.txt"
    llms.write_text(
        llms.read_text()
        + "\n- [Package API](https://eunoia.bz/docs/reference/packages/): Python and R reference documentation, with a downloadable SQLite snapshot.\n"
    )
    (output / "routes.json").write_text(json.dumps(urls, indent=2) + "\n")
    print(
        f"Embedded {len(documents)} documents; SQL package summaries: {packages}",
        flush=True,
    )
    run(pnpm, "check", cwd=web)
    run(pnpm, "build", cwd=web)
    print(
        f"Preview: cd {shlex.quote(str(web))} && env npm_config_manage_package_manager_versions=false "
        f"{shlex.quote(pnpm)} preview --host 127.0.0.1",
        flush=True,
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--eunoia", type=Path, default=PROJECT.parent / "eunoia")
    parser.add_argument("--python", type=Path, default=PROJECT.parent / "eunoia-py")
    parser.add_argument("--r", type=Path, default=PROJECT.parent / "eulerr")
    parser.add_argument(
        "--snapshot", type=Path, help="Use an existing snapshot without extraction"
    )
    parser.add_argument("--pnpm", default="pnpm", help="Installed pnpm 10 executable")
    parser.add_argument("--prepare-only", action="store_true")
    args = parser.parse_args()
    output = (
        args.output or Path(tempfile.mkdtemp(prefix="diplodocus-eunoia-"))
    ).resolve()
    output.mkdir(parents=True, exist_ok=True)
    print(f"Experiment output: {output}", flush=True)
    if not args.snapshot:
        prepare(output, args.python.resolve(), args.r.resolve())
        if args.prepare_only:
            return
    run(
        "cargo",
        "build",
        "--locked",
        "--bin",
        "diplodocus",
        "--example",
        "export-fragments",
    )
    snapshot = (
        args.snapshot.resolve() if args.snapshot else output / "documentation.sqlite"
    )
    if not args.snapshot:
        run(
            PROJECT / "target/debug/diplodocus",
            "extract",
            "--config",
            output / "diplodocus.toml",
            "--output",
            snapshot,
        )
    embed(output, args.eunoia.resolve(), snapshot, args.pnpm)


if __name__ == "__main__":
    main()
