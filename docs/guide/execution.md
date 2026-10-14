# QMD execution and security

QMD execution is disabled by default. To run cells, declare a QMD collection with
`mode = "execute"`, `engine = "jupyter"`, and a kernel name in
[`diplodocus.toml`](configuration.md). The kernel must already be installed.
Document metadata may turn execution off for a page, but cannot grant it to a
collection where it is disabled. `check` never starts a kernel; `extract`,
`build`, and `serve` may run authorized cells.

Each page uses one kernel session, so later cells can use values defined by
earlier cells on that page. Supported options control whether a cell runs, shows
its source or output, permits an ordinary language error, and labels a figure.
See the [authored-pages guide](authoring.md) for the option names and the
[working example](../examples/stateful.qmd) for a complete QMD page.

**Authorized cells run with the privileges of the user or CI job that builds the
site.** They can read and change files and use the network. Output sanitization
protects generated HTML; it does not sandbox code execution. Review a
workspace's source and execution settings before running a build, especially for
contributions from another person. Run `check` first when you need source
validation without execution.

Diplodocus validates cell output before including it in a site. Ordinary stdout,
stderr, and tracebacks remain text. Supported images become local assets; unsafe
HTML or SVG is rejected. A page cache can reuse a successful execution when its
source, effective options, selected kernel, toolchain, and declared environment
inputs still match. The configured kernel is still started to verify its
identity on a cache hit.

R help-topic examples execute during `extract`, `build`, and `serve` independently
of QMD configuration. See [R extraction](extraction.md) for the prepared
environment, illustrative regions, conditional skips, results, and figures.

Python doctest transcripts and Julia `jldoctest` fences also execute during
extraction, using the prepared checkout and native interpreter. Expected-output
mismatches, unexpected errors, missing dependencies, and unsupported executable
syntax prevent publication. Snapshots retain checked outcomes and literal
results. See the [API extraction guide](extraction.md) for comparison, setup,
scope, and explicit skip rules. These adapters do not use Jupyter or an execution
cache.


## Native R Markdown execution

R package vignettes execute by default through knitr during extraction. They use
one native R process per article, with shared chunk and inline state and the
article's directory as the working directory. Rmd collections default to
`mode = "execute"` and `engine = "knitr"`; a kernel selector or disabled
collection is an error. Source options such as `eval = FALSE` still retain their
native meaning. Declared environment inputs can supplement the automatically
observed article and support files.

The caller supplies the prepared checkout and dependencies. Diplodocus does
not install packages, compile extensions, or use persistent knitr caches. Native
processes have the same privileges as the caller. Failures, cancellation,
timeouts, changed inputs, and invalid generated content prevent publication.
Snapshots retain evaluated Markdown, chunk outcomes, inline results, validated
PNG figures, and actual runtime and dependency versions. Generation needs no R
runtime. See [R Markdown authoring](authoring.md).
