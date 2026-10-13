load_checkout <- function(root, expected_name, expected_version) {
  if (!requireNamespace("pkgload", quietly = TRUE)) stop("Native R execution requires pkgload")
  root <- normalizePath(root, mustWork = TRUE)
  meta <- read.dcf(file.path(root, "DESCRIPTION"))[1L, ]
  name <- unname(meta[["Package"]])
  if (!identical(name, expected_name) || !isTRUE(base::package_version(meta[["Version"]]) == base::package_version(expected_version))) {
    stop("Supplied checkout package identity does not match the configured DESCRIPTION")
  }
  pkgload::load_all(root, compile = FALSE, quiet = TRUE, export_all = FALSE, helpers = FALSE, attach_testthat = FALSE)
  verify <- function() {
    ns <- asNamespace(name)
    if (!identical(normalizePath(getNamespaceInfo(ns, "path"), mustWork = TRUE), root)) stop("Loaded R package does not match the supplied checkout")
    for (library in getNamespaceInfo(ns, "dynlibs")) {
      dll <- getNamespaceInfo(ns, "DLLs")[[library]]
      if (is.null(dll)) stop(paste("Missing prebuilt package extension:", library))
      dll_path <- normalizePath(dll[["path"]], mustWork = TRUE)
      # pkgload copies development DLLs to its temporary directory before loading them.
      candidates <- c(file.path(root, "src", paste0(library, .Platform$dynlib.ext)), file.path(root, "src", .Platform$r_arch, paste0(library, .Platform$dynlib.ext)))
      candidates <- candidates[file.exists(candidates)]
      matches <- vapply(candidates, function(path) {
        path <- normalizePath(path, mustWork = TRUE)
        startsWith(path, paste0(root, .Platform$file.sep)) &&
          identical(readBin(path, "raw", n = file.info(path)$size), readBin(dll_path, "raw", n = file.info(dll_path)$size))
      }, logical(1L))
      if (!any(matches)) stop("Loaded package extension does not belong to the supplied checkout")
    }
    if (!isTRUE(utils::packageVersion(name) == base::package_version(meta[["Version"]]))) stop("Loaded R package version does not match DESCRIPTION")
  }
  verify()
  list(name = name, verify = verify)
}
