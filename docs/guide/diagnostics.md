# Diagnostics and failed builds

Run `diplodocus check` to validate configuration, declared sources, authored
syntax, and links without executing cells or publishing output. Errors cause a
nonzero exit status. Warnings identify incomplete interpretations and potential
documentation problems; a command can succeed with warnings.

Each diagnostic includes a stable code, severity, message, and, when known, a
repository-relative source path and byte range. For example,
`unresolved-document-reference` points to a missing local page, asset, or
anchor. `unsupported-authored-syntax` marks syntax outside the selected GFM or
QMD profile. `python-dynamic-export` means a computed Python export list cannot
establish a certain public surface. A warning about Rd source attribution means
the parsed documentation has file-level, rather than exact byte-level, location
evidence.

`check` also validates generated routes and redirect destinations. A
`duplicate-page-title` warning lists visible pages that share an HTML title,
including the site title suffix. Give those pages distinct titles to make
browser tabs and search results easier to distinguish. Hidden pages do not
participate in this check.

When a build fails, fix the first source error and rerun `check`. If the error
only appears during `build` or `serve`, inspect the execution code and kernel
configuration in the reported collection. `serve` keeps the last successful site
available after a failed rebuild. Snapshot or output publication failures also
preserve the previous completed artifact.

See [authored pages](authoring.md) for supported syntax,
[extraction](extraction.md) for static API limits, and [execution](execution.md)
for kernel failures and permissions.
