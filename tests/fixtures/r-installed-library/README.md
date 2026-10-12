These synthetic installed packages contain namespace metadata and lazy-load
indexes, but no code databases. Tests can therefore prove that discovering
exports never loads package hooks or reads code records. `direct` declares one
export, `patterns` also expands an export pattern, and `malformed` has invalid
export metadata.

Regenerate them with `devenv shell -- Rscript scripts/r-library-fixtures.R`.
