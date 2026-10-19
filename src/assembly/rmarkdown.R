local({
  args <- commandArgs(trailingOnly = TRUE)
  if (!requireNamespace("knitr", quietly = TRUE) || !requireNamespace("evaluate", quietly = TRUE)) stop("R Markdown execution requires knitr and evaluate")
  checkout <- load_checkout(args[[1L]], args[[4L]], args[[5L]])
  output <- file(args[[3L]], open = "wt", encoding = "UTF-8")
  emit <- function(key, value) {
    value <- paste(enc2utf8(value), collapse = "\n")
    writeLines(paste(key, paste(sprintf("%02x", as.integer(charToRaw(value))), collapse = ""), sep = "\t"), output, useBytes = TRUE)
    flush(output)
  }
  staging <- dirname(args[[3L]])
  knitr::render_markdown()
  knitr::opts_knit$set(root.dir = dirname(normalizePath(args[[2L]])), rmarkdown.pandoc.to = "markdown")
  knitr::opts_chunk$set(error = FALSE, cache = FALSE, dev = "png", fig.path = paste0(staging, "/knitr-figure-"))
  hooks <- knitr::knit_hooks$get()
  console_output <- function(name) {
    function(x, options) {
      result <- hooks[[name]](x, options)
      if (!nzchar(result) || (name == "output" && identical(options$results, "asis"))) return(result)
      # Keep console output separate even when the article requests collapsed chunks.
      paste0("\n\n::: {.diplodocus-rmd-output}\n", result, "\n:::\n\n")
    }
  }
  chunk <- 0L
  figure <- 0L
  knitr::opts_hooks$set(cache = function(options) {
    if (!identical(options$engine, "R")) stop("R Markdown supports only R chunks")
    if (!identical(options$cache, FALSE) && !identical(options$cache, 0)) stop("Persistent knitr caching is not supported")
    if (!is.null(options$child)) stop("Child documents are not supported")
    if (!identical(options$dev, "png")) stop("R Markdown figures require the png device")
    if (!startsWith(options$fig.path, paste0(staging, "/"))) stop("Figure paths must remain in native staging")
    chunk <<- chunk + 1L
    emit("chunk", options$label)
    for (key in c("eval", "echo", "include", "error", "results", "warning", "message", "collapse", "comment", "fig.width", "fig.height", "fig.show", "fig.cap", "fig.alt")) {
      emit("option", paste(key, paste(deparse(options[[key]]), collapse = " "), sep = "\t"))
    }
    emit("outcome", if (isFALSE(options$eval)) "skipped" else "executed")
    options
  })
  knitr::knit_hooks$set(
    output = console_output("output"),
    warning = console_output("warning"),
    message = console_output("message"),
    error = console_output("error"),
    evaluate = function(...) {
      result <- evaluate::evaluate(...)
      if (any(vapply(result, inherits, logical(1L), "error"))) emit("outcome", "allowed-error")
      result
    },
    evaluate.inline = function(...) {
      result <- hooks$evaluate.inline(...)
      if (!length(result)) emit("inline", "")
      result
    },
    inline = function(x) {
      result <- hooks$inline(x)
      if (is.numeric(x) && grepl("\\\\times|\\^\\{", result)) result <- paste0("$", result, "$")
      emit("inline", result)
      result
    },
    chunk = function(x, options) {
      result <- hooks$chunk(x, options)
      if (!nzchar(result)) return("")
      paste0("\n\n:::: {.diplodocus-rmd-chunk}\n", result, "\n::::\n\n")
    },
    plot = function(x, options) {
      result <- character()
      for (i in seq_along(x)) {
        path <- x[[i]]
        current <- options$fig.cur + i - 1L
        figure <<- figure + 1L
        filename <- sprintf("figure-%d.png", figure)
        if (!file.copy(path, file.path(staging, filename))) stop("Could not retain generated figure")
        emit("figure", filename)
        caption <- options$fig.cap
        alt <- options$fig.alt
        if (length(caption)) caption <- caption[[min(length(caption), current)]] else caption <- ""
        if (length(alt)) alt <- alt[[min(length(alt), current)]] else alt <- caption
        emit("caption", caption)
        emit("alt", alt)
        result <- c(result, paste0("\n\n![](rmd-assets/", filename, ")\n\n"))
      }
      paste(result, collapse = "\n")
    }
  )
  env <- new.env(parent = globalenv())
  markdown <- knitr::knit(input = args[[2L]], output = file.path(staging, "article.md"), envir = env, quiet = TRUE)
  checkout$verify()
  emit("r-version", as.character(getRversion()))
  emit("knitr-version", as.character(utils::packageVersion("knitr")))
  emit("package-version", as.character(utils::packageVersion(checkout$name)))
  for (dependency in sort(loadedNamespaces())) emit("dependency", paste(dependency, as.character(utils::packageVersion(dependency)), sep = "="))
  withCallingHandlers(pkgload::unload(checkout$name), warning = function(w) stop(conditionMessage(w)))
  emit("complete", "rmd-knitr-v1")
  close(output)
})
