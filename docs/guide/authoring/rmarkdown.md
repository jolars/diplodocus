# R Markdown articles

Diplodocus discovers `.Rmd` and `.rmd` files under an R package's `vignettes/`
directory and mounts them beneath `vignettes`. To choose a different mount,
declare a package-owned collection with `format: "rmd"`. Explicit collections
replace automatic discovery for the same source files.

R Markdown uses native knitr execution with no kernel or execution opt-in.
Prepare R, pkgload, evaluate, knitr, package dependencies, and prebuilt native
extensions before building. Each article loads the supplied checkout in a fresh
R process. Chunks and inline R share state within that article. Relative data
paths resolve from the article's directory. `check` validates sources and links
without loading the package or evaluating R.

Fence options and `#|` options retain knitr semantics, including setup defaults,
computed `eval`, hidden chunks, `results = "asis"`, `knitr::kable()`, warnings,
messages, explicit permitted errors, and multiple captioned figures. Hidden
chunks still execute; `eval = FALSE` remains illustrative, and computed false
conditions retain skip outcomes. Unexpected errors stop publication. Local
support files participate in input validation and preview watching.

Console output appears separately from source code, without a shaded background,
syntax highlighting, or a copy button. This also applies to warnings, messages,
and permitted errors, including when `collapse = TRUE`. Source code keeps its
usual highlighting and copy button. `results = "asis"` retains its Markdown
formatting.

Title, author, and evaluated date metadata appear in the article header.
`bibliography` names one local BibTeX file or a list of files; optional `csl`
names a local independent CSL style. Without a style, citations use Chicago
author-date. Citations link to a formatted reference list beneath References.
The CSL style and locale archive comes from Hayagriva and the upstream
[Citation Style Language project](https://citationstyles.org/), with its
CC BY-SA 3.0 attribution retained in the archive.

Diplodocus renders the evaluated document itself. The `output` and `vignette`
metadata do not select an rmarkdown HTML template, and neither rmarkdown nor
Pandoc is required. This release supports R chunks, PNG figures, and static
Markdown content. Other chunk engines, child documents, persistent knitr caches,
interactive widgets, and README.Rmd rendering are unsupported. Runtime failures
leave the previous snapshot and site available. A completed snapshot can be
rendered with `generate` after removing the checkout and R.


See [R extraction](../extraction/r.md) for package APIs and help-topic examples.
