# R API extraction

R extraction combines native exports, function formals, and S3 registrations
from the configured checkout with `DESCRIPTION`, `NAMESPACE`, maintained `R/`
source, and `man/` Rd topics. Native discovery includes generated declarations;
source parsers retain attribution and supported supplemental evidence.
`check`, `extract`, `build`, and `serve` use the same API discovery rules and may
load the package. `extract`, `build`, and `serve` also execute Rd examples as a
required step. `check` never executes documentation examples.
R Markdown vignettes are also discovered and executed; see
[authored articles](../authoring/rmarkdown.md).

Prepare a working checkout with R, `pkgload`, `jsonlite`, `evaluate`, the package dependencies,
and the dependencies used by its examples. Compile native extensions beforehand;
Diplodocus never compiles or installs the package. Each help topic runs once in a
fresh R process, including topics shared by several declarations. Expressions
within a topic share state and a graphics device. The package loads from the
configured checkout, and extraction verifies its namespace path and version.
Native helpers use `R --vanilla`, which skips `.Rprofile` and automatic renv
activation. Expose prepared libraries through `R_LIBS`, `R_LIBS_USER`, or the R
launcher on `PATH`.

Keep snapshots and generated sites in `.diplodocus/` or outside the package's
`R/`, `man/`, and `data/` trees. CLI publication rejects output in those trees
regardless of filename extension, and also protects `DESCRIPTION`, `NAMESPACE`,
and `.Rbuildignore`. If a recognized snapshot already occupies a native input
location, native commands report the conflict before loading the package. Move
the artifact before retrying.

## Roxygen2 or maintained Rd {#roxygen2-or-maintained-rd}

By default, packages with `Roxygen`, `RoxygenNote`, or `Config/roxygen2/*`
DESCRIPTION fields, or `man/roxygen/meta.R`, use roxygen2. Other packages use
maintained Rd files. Override this choice on the package:

```yaml
r:
  documentation: roxygen2 # auto (the default), roxygen2, or rd
  extra-inputs: [tools/documentation.R]
  generated-outputs: [generated-help]
```

Normal commands run generation automatically. Saving roxygen prose, inherited
documentation, formals, or export tags during `diplodocus serve` regenerates the
documentation, rebuilds the site, and refreshes open browser pages. Use
`--no-live-reload` to disable browser refresh. A generation error keeps the
previous successful site; correcting the source lets the next rebuild recover.
No separate `devtools::document()` call is needed.

Generation can evaluate `@evalRd`, `@evalNamespace`, included R Markdown, custom
roclets, and package loading hooks. `check` permits this native preparation but
does not run Rd examples or authored articles. Generation warnings and roxygen
diagnostics about dropped or incomplete documentation prevent publication.

Generation writes to the supplied checkout. Roxygen2 owns generated `man/*.Rd`,
`NAMESPACE`, and its DESCRIPTION version and `Collate` fields. These writes are
not rolled back after a later failure. Diplodocus acknowledges its generated
changes without hiding concurrent edits to maintained sources. A failed
generation never falls back to existing Rd files.

The supported roxygen2 range is `>= 7.3.2, < 9`. Compatibility checks pass with
7.3.2, 8.0.0, and 8.1.1 using both supported loaders. Run
`compute run -- python scripts/test-roxygen-compat.py` to repeat these checks
with the prepared R environment. Add `--profile VERSION=LIBRARY` for each
separately prepared version's R library.

Diplodocus respects package options from DESCRIPTION and
`man/roxygen/meta.R`, including Markdown, roclets, and the `pkgload` or `source`
loader. Generation requires the `rd` roclet. The `installed` loader is rejected
because it cannot establish the configured checkout as the documentation
authority. See roxygen2's
[generation reference](https://roxygen2.r-lib.org/reference/roxygenize.html) and
[option reference](https://roxygen2.r-lib.org/reference/load_options.html).

Custom roclets run in the caller's prepared environment. Declare their
supplemental input files and exclusively generated output paths with
`extra-inputs` and `generated-outputs`, relative to the package root. Declared
paths must remain inside the package and cannot overlap maintained inputs.
Custom outputs cannot occupy `R/`, `src/`, `inst/`, `data/`, DESCRIPTION,
NAMESPACE, or roxygen's maintained configuration, templates, and macros.
Diplodocus does not install extensions or sandbox their side effects.

Generation and native discovery have separate persistent caches in
`.diplodocus/cache/incremental/`. Reuse requires matching maintained inputs,
generated output bytes and membership, configuration, supplemental inputs, and
the prepared R environment. Diplodocus checks the selected R runtime, library
precedence, dependency contents, mapped native libraries, and loader search paths.
Replacing an installed dependency without changing its version invalidates
affected results. Missing optional dependencies remain tracked as missing;
installing one invalidates the affected entry. Stage helpers and dependencies
actually loaded during native work must be available. Native-library reuse
requires observable loaded libraries. Unsupported environments run afresh. A
missing generated Rd file triggers generation again. `documentation: rd` skips
generation while keeping native API discovery.

Use `--no-cache` with `check`, `extract`, `build`, or `serve` to recompute results.
Declare additional files or directories and environment variables consumed by
evaluated documentation or package hooks through the package's `incremental`
settings. These paths are relative to its repository, while `r.extra-inputs`
remains relative to the package. Use `incremental.cache: off` for untracked
external state such as network services. Dependency coverage that cannot be
established causes fresh preparation instead of cache reuse.

The current API model supports functions, constants, datasets, and S3 dispatch.
Selected S4, R6, and S7 declarations receive explicit unsupported-surface
diagnostics.

`\dontrun` regions remain explicitly illustrative. `\donttest` regions execute.
False `if` conditions without an `else`, including `interactive()` and platform
checks, record a skip reason. Conditions run once. Missing dependencies requested
through `require()` or `requireNamespace()` cause an error rather than a skip.
Normal R error handling such as `tryCatch()` remains available to examples.

Results, messages, and warnings render as literal text, and recorded plots become
validated PNG assets. Snapshots retain executed expressions, illustrative regions,
skip reasons, the Rd source fingerprint, R and package versions, and loaded
namespace versions. Native example reuse requires the package source,
documentation, dependency, and execution environment inputs to remain valid.
Any package source change, including comments, invalidates its examples.
Generation uses the completed snapshot without R or the
checkout. Errors, missing prerequisites, timeouts, invalid figures, and cleanup
failures prevent publication and preserve the previous snapshot and site.

Shared help topics retain all documented usages and arguments on each
declaration's page. Topic aliases resolve to a primary declaration, while
method aliases resolve to their own pages. Links between declarations and
methods grouped by S3 class make the complete topic navigable.

R reference pages use the Rd title as their heading and show the description
before usage. URLs use readable API names, or the topic name for a shared
topic's primary declaration. The reference overview lists each topic with its
title and introductory summary. Code blocks have syntax highlighting and copy
buttons. Usage appears once, including the complete shared topic's signatures.
Source links point to the recorded R and Rd files when the repository has a
revision and a GitHub URL or a supported `source-link-template`. These links
remain available when generating from a snapshot without the checkout.

Rd content supports labeled local and package-qualified help links, equations,
aligned tables, custom sections, named return entries, HTML and platform
conditionals, and DOI links. Equations render natively with `katex-rs` and bundled
fonts, so they work with JavaScript disabled. External help links use installed
help aliases to select rdrr.io destinations; snapshots retain the results so
generation needs neither R nor installed help databases.

Dataset topics marked `\docType{data}` create documented data objects without
requiring source bindings or exports. Their usage lists bare object names, and
their Format and Source sections retain structured prose, lists, and links.

Documented constants retain literal values and assignment aliases from maintained
source bindings. Their usage lists bare object names. Standalone help topics
named `*_deprecated` retain prose and aliases without requiring a function or
inventing a callable signature. Nested Rd lists and prose between described
entries retain their content and order.

Standard S3 generics from default R packages resolve without explicit imports.
R must be available on `PATH`. Native discovery loads the checkout and its
dependencies; supplemental import analysis uses installed metadata. Documented helpers
receive native signatures without requiring exports. Topics marked
`\keyword{internal}` have addressable pages but stay out of navigation and search.
See the R extraction contract below for discovery and import limits.

## Package landing pages {#package-landing-pages}

Every package receives a [standard landing page](../authoring.md#package-landing-pages).
Diplodocus uses an authored package index, a configured `overview` file, or the
package-root `README.md`, in that order. Without those sources, package metadata
supplies an introduction when available. The compact information block shows
available details from DESCRIPTION, pyproject.toml, Project.toml, or Cargo
metadata. R overviews also link to available release notes, authors and citation
information, licenses, and community guidelines.

Release notes use `NEWS.md`, then `NEWS.Rd`, then plain `NEWS`. Licenses include
the DESCRIPTION declaration, its local `file` references, and conventional
`LICENSE`, `LICENSE.md`, `LICENCE`, `LICENCE.md`, `LICENSE-MIT`, `LICENSE-APACHE`,
`COPYING`, and `COPYING.md` files. Contribution guidelines
and a code of conduct use `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, and `CONDUCT.md`, preferring
package files over files at the repository root. Missing optional files do not
create empty pages. A missing file explicitly named by License is an error.

Extraction evaluates `Authors@R` and `inst/CITATION` with native R helpers. When
CITATION is absent, R generates a citation from DESCRIPTION. Author supplies a
fallback when Authors@R is absent. R also converts NEWS.Rd to readable text. These
operations use the checkout's files and supplied environment, and do not install
the documented package. Native errors and timeouts prevent publication. `check`
validates source syntax and links without evaluating this information.

An explicitly authored page at the package root or a supporting page's route
takes precedence. Diplodocus resolves these overrides before parsing the
automatically discovered documents, so replaced content does not produce syntax
diagnostics. Links to the original files resolve to the authored replacement.
Files explicitly included by authored collections still receive diagnostics and
are not duplicated automatically. README links, heading anchors, and local
images use the normal reference and asset pipeline. Snapshots retain the
metadata, native results, documents, and assets so `generate` works without the
checkout or R.
README.Rmd rendering remains outside this workflow.

Automatically discovered package pages can also link to existing repository
files outside the declared content, such as `spec/README.md`, `TRUST.md`, and
`OPERATIONS.md` in a root contributor guide. Declared pages and their source
aliases resolve to site pages first. Other local documents and directories use
a repository URL, as do files outside package and content asset boundaries.
This fallback requires a [repository URL or source-link template](../../reference/configuration.md#repository-links)
and a declared or observed revision. Missing targets, invalid site anchors,
and paths that leave the source repository remain errors, including symlinks
that leave and reenter it. Diplodocus checks the target's existence and type
without parsing it, reading its contents, or including it in the site.
Fragments on repository links pass to the host without local anchor validation.
Local images still require asset validation; authored HTTP and HTTPS images
retain their external URLs. Explicitly authored pages, including package
overrides and R Markdown articles, keep the usual link rules.

Unsupported static supplemental analysis remains diagnostic rather than
inventing source declarations for runtime objects. Unsupported Rd markup
stays visible with a diagnostic. For exact parser and identity rules, see the
[R extraction
contract](https://github.com/jolars/diplodocus/blob/main/docs/ir/r-extraction.md)
in the source repository.

