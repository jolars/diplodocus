# Portable snapshots

`extract` writes a self-contained SQLite snapshot containing the structured
documentation, presentation settings, provenance, diagnostics, and validated
local assets. `generate` reads a completed snapshot and writes the site without
the original source checkout, language runtimes, or Jupyter kernels. `build`
combines these two stages. See the [command reference](cli.md) for paths and
options.

The snapshot manifest has separate storage, documentation IR, and record
encoding versions. The current reader accepts storage version 2, IR version 5,
and encoding version 1. It rejects other combinations rather than migrating
them. If a newer Diplodocus cannot read an older snapshot, extract a new one
from the original sources before generating the site. The producer's package
version is recorded, but is not itself the compatibility rule.

Extraction replaces the complete snapshot. It keeps stable semantic IDs for
retained entities and drops removed entities and unused assets. A failed
extraction or publication leaves the previous completed snapshot in place. Treat
a copied snapshot as a build artifact; do not edit its records by hand. The
[storage
schema](https://github.com/jolars/diplodocus/blob/main/docs/design/snapshot-schema.md)
in the source repository describes its tables and validation rules.

## Use a snapshot in an existing site

SQLite is a supported consumption boundary. Read the manifest's storage, IR,
and encoding versions before interpreting records. Package and item records
contain structured JSON; documents retain semantic link targets rather than
site URLs. The `assets` table carries retained image and download bytes. A
consumer can query these records directly and choose its own presentation.

The fragment renderer reuses the built-in API presentation without its page
shell. Export HTML and Markdown with the normal command-line interface:

```console
diplodocus generate --format fragments \
  --input documentation.sqlite --output fragments
```

The output includes HTML and Markdown bodies, a version 2 `documents.json`
manifest, search records, syntax colors, and retained assets. Each document
descriptor records its artifact paths, preferred public API name, package
version, semantic identities, visibility, and heading anchors. Python class
pages include constructors, fields, and methods. Inherited members link to
their defining class. The Rust equivalents are
`diplodocus::rendering::render_fragments` and its typed `FragmentIndex`.

Links remain relative by default. A host can read the first manifest, assign
public URLs, and render again using a version 1 URL map:

```json
{
  "format_version": 1,
  "documents": {
    "packages/example/reference/item.html": "/docs/reference/example/fit/"
  },
  "assets": {
    "assets/content.css": "/_reference/content.css"
  }
}
```

```console
diplodocus generate --format fragments --input documentation.sqlite \
  --output fragments --url-map urls.json
```

Keys name the original artifact paths. Mapping changes links in HTML, Markdown,
and search without changing artifact filenames. Unknown keys, unsafe URLs, and
colliding destinations fail before publication. Member and overload anchors
remain intact, as do external and repository source links. Unmapped paths keep
their default relative URLs; hosts should map every local destination used by
the pages they publish.

The host supplies layout, navigation, and content typography. Wrap HTML bodies
in `.diplodocus-content` to use the scoped syntax stylesheet. A `.dark` ancestor
selects its dark colors. Retain the exported assets and load the KaTeX stylesheet
when rendering math. Filter public navigation using each descriptor's
`visible` flag and `public_name`; internal support documents are not public API
pages.

Fragment format versions are independent of the SQLite storage contract. The
[Eunoia experiment](https://github.com/jolars/diplodocus/tree/main/examples/eunoia)
also demonstrates direct SQL consumption inside an existing SvelteKit site.
The [VitePress example](https://github.com/jolars/diplodocus/tree/main/examples/vitepress)
uses `extract --watch` to update generated reference pages while VitePress watches
its authored guides. It also demonstrates building from a completed snapshot.

## Markdown for agents

Every full site build includes a `.md` sibling for each HTML page, rendered
from the same validated content without the navigation or page chrome. HTML
pages advertise this representation with a `rel="alternate"` link.
`diplodocus serve` returns it at the HTML URL when a client requests
`Accept: text/markdown`. HTML remains the default, and both representations
include `Vary: Accept`. Explicit quality values are honored: Markdown must have
a positive quality at least as high as an explicitly requested HTML type.

Static hosting needs a server or edge handler to negotiate HTTP responses.
Builds include a Cloudflare Worker entry point, so Workers deployments can use
negotiation without writing application code. For a build in `site/`, configure
Wrangler as follows, alongside your account and domain settings:

```toml
main = "./site/__diplodocus/worker.js"
compatibility_date = "2026-10-06"

[assets]
directory = "./site"
binding = "ASSETS"
run_worker_first = true
```

Other hosts can map HTML requests to the corresponding `.md` file when the
client accepts Markdown. Set `Content-Type: text/markdown; charset=utf-8` and
`Vary: Accept` on negotiated responses, including the HTML variant. Hosts
without negotiation can still serve the advertised `.md` URLs directly.
No token estimate is emitted because Diplodocus does not select a tokenizer.

```console
curl -H 'Accept: text/markdown' https://your-site.example/guide/index.html
```
