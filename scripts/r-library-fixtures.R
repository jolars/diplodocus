# Regenerate metadata-only installed-package fixtures without installing a package.
root <- "tests/fixtures/r-installed-library"
for (package in c("direct", "patterns", "malformed")) {
  path <- file.path(root, package)
  dir.create(file.path(path, "Meta"), recursive = TRUE, showWarnings = FALSE)
  dir.create(file.path(path, "R"), recursive = TRUE, showWarnings = FALSE)
  writeLines(c(paste0("Package: ", package), "Version: 1.0.0"), file.path(path, "DESCRIPTION"))
  exports <- if (package == "malformed") 1L else "foreignGeneric"
  pattern <- if (package == "patterns") "^public" else character()
  saveRDS(list(exports = exports, exportPatterns = pattern), file.path(path, "Meta", "nsInfo.rds"), version = 3)
  index <- list(
    variables = list(.onLoad = c(0L, 1L), publicGeneric = c(1L, 1L), privateGeneric = c(2L, 1L)),
    references = list(), compressed = TRUE
  )
  saveRDS(index, file.path(path, "R", paste0(package, ".rdx")), version = 3)
}
