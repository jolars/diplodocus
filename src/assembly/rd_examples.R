local({
  args <- commandArgs(trailingOnly = TRUE)
  output <- file(args[[3L]], open = "wt", encoding = "UTF-8")
  emit <- function(key, value) {
    value <- paste(enc2utf8(value), collapse = "\n")
    writeLines(paste(key, paste(sprintf("%02x", as.integer(charToRaw(value))), collapse = ""), sep = "\t"), output, useBytes = TRUE)
  }
  if (!requireNamespace("pkgload", quietly = TRUE) || !requireNamespace("evaluate", quietly = TRUE)) {
    stop("Rd example execution requires pkgload and evaluate")
  }
  root <- normalizePath(args[[1L]], mustWork = TRUE)
  meta <- read.dcf(file.path(root, "DESCRIPTION"))[1L, ]
  name <- unname(meta[["Package"]])
  if (!identical(name, args[[4L]]) || !isTRUE(base::package_version(meta[["Version"]]) == base::package_version(args[[5L]]))) {
    stop("Supplied checkout package identity does not match the configured DESCRIPTION")
  }
  pkgload::load_all(root, compile = FALSE, quiet = TRUE, export_all = FALSE, helpers = FALSE, attach_testthat = FALSE)
  verify_checkout <- function() {
    ns <- asNamespace(name)
    if (!identical(normalizePath(getNamespaceInfo(ns, "path"), mustWork = TRUE), root)) {
      stop("Loaded R package does not match the supplied checkout")
    }
    for (library in getNamespaceInfo(ns, "dynlibs")) {
      dll <- getNamespaceInfo(ns, "DLLs")[[library]]
      if (is.null(dll)) stop(paste("Missing prebuilt package extension:", library))
      dll_path <- normalizePath(dll[["path"]], mustWork = TRUE)
      if (!startsWith(dll_path, paste0(root, .Platform$file.sep))) stop("Loaded package extension does not belong to the supplied checkout")
    }
    if (!isTRUE(utils::packageVersion(name) == base::package_version(meta[["Version"]]))) {
      stop("Loaded R package version does not match DESCRIPTION")
    }
  }
  verify_checkout()
  rd <- tools::parse_Rd(args[[2L]], encoding = "UTF-8")
  codefile <- file.path(dirname(args[[3L]]), "examples.R")
  tools::Rd2ex(rd, out = codefile, commentDontrun = TRUE, commentDonttest = FALSE)
  code <- paste(readLines(codefile, warn = FALSE), collapse = "\n")
  # The native converter keeps dontrun as comments and makes donttest executable.
  for (node in rd) {
    if (identical(attr(node, "Rd_tag"), "\\examples")) {
      visit <- function(nodes) {
        for (node in nodes) {
          if (identical(attr(node, "Rd_tag"), "\\dontrun")) emit("illustrative", paste(unlist(node), collapse = ""))
          else if (is.list(node)) visit(node)
        }
      }
      visit(node)
    }
  }
  env <- new.env(parent = globalenv())
  # Test dependency availability once and fail instead of hiding it in a conditional skip.
  env$.diplodocus_dependency <- function(value, expression) {
    if (!isTRUE(value)) stop(paste("Missing example dependency:", expression))
    value
  }
  env$.diplodocus_condition <- function(value, expression, has_else) {
    decision <- if (value) TRUE else FALSE
    if (!decision && !has_else) emit("skipped", paste("Condition is false:", expression))
    decision
  }
  rewrite <- function(x) {
    if (!is.call(x)) return(x)
    head <- as.character(x[[1L]])
    if (length(head) == 3L && head[[1L]] %in% c("::", ":::") && head[[2L]] == "base") head <- head[[3L]]
    if (length(head) == 1L && head %in% c("quote", "substitute", "expression", "bquote")) return(x)
    if (length(head) == 1L && head %in% c("require", "requireNamespace")) {
      return(as.call(list(as.name(".diplodocus_dependency"), x, paste(deparse(x), collapse = " "))))
    }
    if (length(head) == 1L && head == "if") {
      predicate <- paste(deparse(x[[2L]]), collapse = " ")
      x[[2L]] <- as.call(list(as.name(".diplodocus_condition"), rewrite(x[[2L]]), predicate, length(x) == 4L))
      for (i in seq.int(3L, length(x))) x[[i]] <- rewrite(x[[i]])
      return(x)
    }
    if (length(x) > 1L) for (i in seq.int(2L, length(x))) {
      # Missing arguments must remain missing when traversing calls.
      if (!identical(x[[i]], quote(expr = ))) x[[i]] <- rewrite(x[[i]])
    }
    x
  }
  expressions <- parse(text = code, keep.source = FALSE)
  figure <- 0L
  rewritten <- vapply(expressions, function(x) paste(deparse(rewrite(x)), collapse = "\n"), character(1L))
  results <- evaluate::evaluate(paste(rewritten, collapse = "\n"), envir = env, stop_on_error = 2L, new_device = TRUE)
  for (expression in expressions) emit("executed", paste(deparse(expression), collapse = "\n"))
  for (result in results) {
      if (inherits(result, "source")) next
      if (inherits(result, "recordedplot")) {
        figure <- figure + 1L
        filename <- sprintf("figure-%d.png", figure)
        grDevices::png(file.path(dirname(args[[3L]]), filename), width = 1000L, height = 700L)
        replayPlot(result)
        grDevices::dev.off()
        emit("figure", filename)
      } else if (inherits(result, "warning")) emit("text", paste("Warning:", conditionMessage(result)))
      else if (inherits(result, "message")) emit("text", conditionMessage(result))
      else if (is.character(result)) emit("text", result)
      else stop("Unsupported Rd example output")
  }
  verify_checkout()
  emit("r-version", as.character(getRversion()))
  emit("package-version", as.character(utils::packageVersion(name)))
  for (dependency in sort(loadedNamespaces())) emit("dependency", paste(dependency, as.character(utils::packageVersion(dependency)), sep = "="))
  withCallingHandlers(pkgload::unload(name), warning = function(w) stop(conditionMessage(w)))
  emit("complete", "rd-examples-v1")
  close(output)
})
