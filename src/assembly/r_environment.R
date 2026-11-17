verified_checkout_dlls <- new.env(parent = emptyenv())

native_mappings <- function() {
  if (!file.exists("/proc/self/maps")) return(NULL)
  lines <- tryCatch(readLines("/proc/self/maps", warn = FALSE), error = function(e) NULL)
  if (is.null(lines)) return(NULL)
  fields <- regmatches(lines, regexec(
    "^\\S+\\s+\\S+\\s+\\S+\\s+(\\S+)\\s+(\\S+)\\s+(.*)$", lines, perl = TRUE))
  mappings <- Filter(Negate(is.null), lapply(fields, function(field) {
    if (length(field) != 4L || !startsWith(field[[4L]], "/")) return(NULL)
    list(device = field[[2L]], inode = field[[3L]], path = field[[4L]])
  }))
  mappings[!duplicated(vapply(mappings, native_mapping_key, character(1L)))]
}

native_mapping_key <- function(mapping) {
  paste(mapping$device, mapping$inode, sub(" \\(deleted\\)$", "", mapping$path), sep = "\n")
}

record_verified_checkout_dll <- function(root, dll, source) {
  root <- normalizePath(root, mustWork = TRUE)
  source <- normalizePath(source, mustWork = TRUE)
  if (!startsWith(source, paste0(root, .Platform$file.sep))) stop("Verified package extension escaped the supplied checkout")
  bytes <- readBin(source, "raw", n = file.info(source)$size)
  if (!identical(bytes, readBin(dll, "raw", n = file.info(dll)$size))) {
    stop("Verified package extension changed during native observation")
  }
  # GNU UNIQUE symbols can retain a pkgload copy after unload deletes its file.
  # Keep the exact mapping identity and verified bytes only for this R process.
  for (mapping in native_mappings()) {
    if (identical(mapping$path, dll)) {
      assign(native_mapping_key(mapping), list(root = root, source = source, bytes = bytes),
             envir = verified_checkout_dlls)
    }
  }
  invisible(NULL)
}

observe_native_environment <- function(root = NULL) {
  mappings <- native_mappings()
  complete <- !is.null(mappings)
  reasons <- if (complete) character() else "native process mappings are unavailable"
  refuse <- function(reason) {
    complete <<- FALSE
    reasons <<- c(reasons, reason)
  }
  mapped <- character()
  for (mapping in mappings) {
    path <- mapping$path
    proof <- get0(native_mapping_key(mapping), envir = verified_checkout_dlls, inherits = FALSE)
    if (!is.null(proof) && !is.null(root) &&
        identical(proof$root, normalizePath(root, mustWork = TRUE)) &&
        file.exists(proof$source) &&
        identical(normalizePath(proof$source, mustWork = TRUE), proof$source) &&
        identical(readBin(proof$source, "raw", n = file.info(proof$source)$size), proof$bytes)) {
      path <- proof$source
    } else if (endsWith(path, " (deleted)")) {
      refuse("unrecognized deleted native mapping")
      next
    } else if (!file.exists(path)) {
      refuse("a mapped native file is unavailable")
      next
    } else if (startsWith(path, paste0(tempdir(), .Platform$file.sep))) {
      refuse("unverified temporary native mapping")
      next
    }
    mapped <- c(mapped, normalizePath(path, mustWork = TRUE))
  }
  loader_paths <- character()
  for (name in c("LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH")) {
    value <- Sys.getenv(name, unset = "")
    if (!nzchar(value)) next
    parts <- strsplit(paste0(value, .Platform$path.sep, "__end__"), .Platform$path.sep, fixed = TRUE)[[1L]]
    for (path in utils::head(parts, -1L)) {
      if (!nzchar(path)) path <- getwd()
      if (!grepl("^(/|[A-Za-z]:)", path)) path <- file.path(getwd(), path)
      loader_paths <- c(loader_paths, path)
    }
  }
  namespaces <- list()
  libraries <- normalizePath(.libPaths(), mustWork = TRUE)
  for (name in sort(loadedNamespaces())) {
    path <- if (identical(name, "base")) file.path(R.home("library"), "base") else
      tryCatch(getNamespaceInfo(asNamespace(name), "path"), error = function(e) NULL)
    if (is.null(path) || !dir.exists(path)) {
      refuse("a loaded namespace path is unavailable")
      next
    }
    path <- normalizePath(path, mustWork = TRUE)
    if (!is.null(root) && identical(path, normalizePath(root, mustWork = TRUE))) next
    namespaces[[name]] <- path
  }
  list(complete = complete, reasons = I(unique(reasons)), mapped = I(sort(unique(mapped))),
       loader_paths = I(unique(loader_paths)), libraries = I(libraries), namespaces = namespaces)
}

write_native_environment <- function(output, root = NULL) {
  tryCatch({
    writeLines(jsonlite::toJSON(observe_native_environment(root), auto_unbox = TRUE),
               paste0(output, ".environment"), useBytes = TRUE)
  }, error = function(e) invisible(NULL))
}

observe_cache_environment <- function(output) {
  if (!requireNamespace("jsonlite", quietly = TRUE)) stop("R cache observation requires jsonlite")
  libraries <- normalizePath(.libPaths(), mustWork = TRUE)
  packages <- list()
  for (library in libraries) {
    for (path in sort(list.dirs(library, full.names = TRUE, recursive = FALSE))) {
      name <- basename(path)
      if (is.null(packages[[name]]) && file.exists(file.path(path, "DESCRIPTION"))) {
        packages[[name]] <- normalizePath(path, mustWork = TRUE)
      }
    }
  }
  result <- list(version = as.character(getRversion()),
                 home = normalizePath(R.home(), mustWork = TRUE),
                 launcher = unname(Sys.which("R")),
                 libraries = I(libraries), packages = packages,
                 native = observe_native_environment())
  writeLines(jsonlite::toJSON(result, auto_unbox = TRUE), output, useBytes = TRUE)
}
