# Markdown include contract

This document defines the include contract. Whole-file and nested expansion,
cycle detection, and parser diagnostic attribution are implemented for static
GFM and QMD collections. Prepared pages retain source ranges and include chains.
Relative links and images resolve from their original source files, including
partials outside the collection directory. Preview watching tracks nested
partials even after failed resolution and retries when missing files appear.
Portable source-map retention remains pending, so includes are not yet
documented as a complete authoring feature.
Executable QMD pages with includes produce an explicit error until the execution
engine can verify expanded inputs. The remaining slices are tracked in
[TODO.md](../../TODO.md).

## Authoring syntax

GFM collections use VitePress's directive:

```markdown
<!--@include: ./parts/basics.md-->
```

QMD collections use Quarto's shortcode:

```markdown
{{< include ./parts/basics.qmd >}}
```

Both forms feed the same resolver. Each form is recognized only in its declared
collection format. R Markdown, Documenter, package overviews, and extracted API
documentation are outside the first implementation. Includes do not introduce a
general shortcode system.

An include occupies its own unindented line, with blank lines before and after
it unless it is at the start or end of the file. Trailing whitespace is allowed.
The initial implementation supports top-level includes only. Directives inside
code fences, indented code, block quotes, and lists remain literal content.
Verbatim inclusion inside code fences is a separate, later slice.

The directive takes one nonempty path. Double quotes delimit paths containing
spaces. Remote URLs, query strings, fragments, and partial-file selectors are
rejected in the first implementation. Malformed top-level include directives
produce errors rather than silently disappearing as HTML comments.

## Files and paths

Relative include paths resolve from the file containing the directive. Relative
links, images, and nested includes inside a partial resolve from that partial's
directory, regardless of which page includes it. Reference-style links and images
use the directory containing their reference definition, even when the reference
appears in another file. Fragment-only links refer to anchors in the composed
page. This rule applies to both GFM and QMD. Local resources may be outside the
collection directory but must remain inside the declared repository.

GFM's `@/parts/basics.md` and QMD's `/parts/basics.qmd` resolve from the content
collection's source root. For a directory collection, that root is its declared
directory. For a single-file collection, it is the file's parent directory.
Nested includes retain the including page's collection root. Root paths never
refer to the host filesystem root.

Includes may reach outside the collection directory through relative paths, but
must remain inside the collection's declared repository. Use the existing
repository path resolver and its boundary checks, including symlink checks.
Reject reserved generated-storage paths. Targets must be regular UTF-8 files:
GFM accepts `.md`, and QMD accepts `.md` or `.qmd`. Parse included content using
the including collection's format, including nested directives.

Including a file does not create a route or remove an existing route. A partial
that collection discovery also selects remains a standalone page. Authors can
keep partials outside the collection directory when they should have no route;
underscore prefixes do not acquire special discovery behavior in this slice.

## Composition and source attribution

Insert the included body at the directive's position before document preparation
and reference resolution. Included files cannot declare frontmatter: reject a
leading metadata block rather than letting a partial change page metadata or
execution defaults. The main page owns its title, metadata, collection
authority, and execution session. Included QMD cells follow the page's existing
execution rules and never select another engine or kernel.

Detect cycles using the active include chain and resolved file identities.
Including the same file in separate branches is allowed. Report a cycle at the
directive that closes it, with the chain of source files and directive
locations.

Retain a mapping from expanded byte ranges to original repository paths and byte
ranges, together with the include chain. Parser spans must not be presented as
offsets into the main file when they originate in a partial. Diagnostics
identify the original file and location, and provide the include chain as
context. Relative resource resolution uses the same mapping. Content that spans
an include boundary must retain all contributing source ranges.

Missing, unreadable, invalid UTF-8, disallowed, or out-of-boundary targets fail
assembly with a diagnostic at the requesting directive. There is no silent-skip
mode. Failed builds preserve the previous preview and published output through
the existing pipeline behavior.

## Dependencies and snapshots

Track every included input for fingerprints and preview watching, including
files outside the collection directory. A failed resolution must leave enough
watch information to retry when a missing target is created. Editing, deleting,
or restoring a nested partial must rebuild the affected pages.

Portable snapshots retain the expanded document, original source attribution,
and referenced assets. Generation must not reopen the original source tree.
Included content must participate in execution cache invalidation just as main
page content does. These requirements need explicit integration coverage before
includes are described as supported in the authoring guide.

## Compatibility

The syntax follows [VitePress
includes](https://vitepress.dev/guide/markdown#markdown-file-inclusion) for GFM
and [Quarto includes](https://quarto.org/docs/authoring/includes.html) for QMD.
Quarto resolves references in partials from the main document; Diplodocus
deliberately resolves them from the included file. Collection roots also replace
the upstream tools' project roots.

Line ranges, named regions, heading selection, and verbatim code inclusion are
later slices. Supporting the whole-file syntax does not imply support for those
extensions or full compatibility with either tool.
