local({
  run_native <- function() {
  args <- commandArgs(trailingOnly = TRUE)
  root <- normalizePath(args[[1L]], mustWork = TRUE)
  output <- args[[2L]]
  expected_name <- args[[3L]]
  expected_version <- args[[4L]]
  generate <- identical(args[[5L]], "roxygen2")
  options(cli.unicode = FALSE, cli.num_colors = 1L, cli.hyperlink = FALSE,
          crayon.enabled = FALSE, width = 1000L)
  if (!requireNamespace("jsonlite", quietly = TRUE)) stop("Native R discovery requires jsonlite")
  if (!requireNamespace("pkgload", quietly = TRUE)) stop("Native R discovery requires pkgload")
  metadata <- read.dcf(file.path(root, "DESCRIPTION"))[1L, ]
  if (!identical(unname(metadata[["Package"]]), expected_name) ||
      !identical(unname(metadata[["Version"]]), expected_version)) {
    stop("Supplied DESCRIPTION changed before native R discovery")
  }
  checked <- NULL
  on.exit({
    if (expected_name %in% loadedNamespaces()) pkgload::unload(expected_name)
  }, add = TRUE)
  generation <- NULL
  diagnostics <- character()
  runtime_warnings <- character()
  runtime <- function(expr) withCallingHandlers(expr, warning = function(w) {
    runtime_warnings <<- c(runtime_warnings, conditionMessage(w))
    invokeRestart("muffleWarning")
  })
  strict <- function(expr) {
    value <- withCallingHandlers(expr,
      warning = function(w) {
        diagnostics <<- c(diagnostics, conditionMessage(w))
        invokeRestart("muffleWarning")
      },
      message = function(m) {
        text <- conditionMessage(m)
        if (grepl("(^|\n)[[:space:]]*[x!] ", text)) diagnostics <<- c(diagnostics, text)
      })
    if (length(diagnostics)) stop(paste(unique(diagnostics), collapse = "\n"))
    value
  }
  if (generate) {
    if (!requireNamespace("roxygen2", quietly = TRUE)) stop("Roxygen generation requires roxygen2")
    version <- utils::packageVersion("roxygen2")
    if (version < "7.3.2" || version >= "9.0.0") {
      stop("Supported roxygen2 versions are >= 7.3.2 and < 9.0.0")
    }
    opts <- strict(roxygen2::load_options(root))
    loader <- opts$load
    if (!is.character(loader) || length(loader) != 1L || !loader %in% c("pkgload", "source")) {
      stop("Supported roxygen2 loading strategies are pkgload and source; installed packages do not prove checkout freshness")
    }
    if (!is.character(opts$roclets) || !length(opts$roclets) || !"rd" %in% opts$roclets) {
      stop("Roxygen generation must include the rd roclet")
    }
    generation <- list(roxygen2_version = as.character(version), loader = loader,
                       roclets = I(opts$roclets))
    writeLines(jsonlite::toJSON(generation, auto_unbox = TRUE), paste0(output, ".generation"))
    load_code <- if (identical(loader, "pkgload")) function(path) {
      checked <<- withCallingHandlers(
        load_checkout(path, expected_name, expected_version, export_all = TRUE),
        warning = function(w) {
          # Namespace generation repairs exports removed from the maintained source.
          if (startsWith(conditionMessage(w), "Objects listed as exports, but not present in namespace:")) invokeRestart("muffleWarning")
        })
      asNamespace(checked$name)
    } else roxygen2::load_source
    started <- proc.time()[["elapsed"]]
    strict(roxygen2::roxygenize(root, roclets = NULL, load_code = load_code, clean = FALSE))
    if (!is.null(checked)) checked$verify()
    cat("documentation-generation", proc.time()[["elapsed"]] - started,
        sep = "\t", file = paste0(output, ".timings"))
    cat("\n", file = paste0(output, ".timings"), append = TRUE)
    # Reload after namespace generation so the observed exports use the completed output.
    if (expected_name %in% loadedNamespaces()) strict(pkgload::unload(expected_name))
  }
  started <- proc.time()[["elapsed"]]
  checked <- runtime(load_checkout(root, expected_name, expected_version))
  ns <- asNamespace(checked$name)
  cat("package-loading", proc.time()[["elapsed"]] - started,
      sep = "\t", file = paste0(output, ".timings"), append = TRUE)
  cat("\n", file = paste0(output, ".timings"), append = TRUE)
  generic_name <- function(value) {
    if (!is.function(value) || is.primitive(value)) return(NULL)
    binds <- function(x) {
      if (!is.call(x) || identical(x[[1L]], as.name("function"))) return(FALSE)
      head <- paste(deparse(x[[1L]]), collapse = "")
      if (head %in% c("<-", "<<-", "=") &&
          length(x) >= 2L && identical(x[[2L]], as.name("UseMethod"))) return(TRUE)
      if (head %in% c("assign", "base::assign", "delayedAssign", "base::delayedAssign")) {
        if (length(x) < 2L || !is.character(x[[2L]]) || identical(x[[2L]], "UseMethod")) return(TRUE)
      }
      if (length(x) > 1L) for (i in seq.int(2L, length(x))) {
        if (!identical(x[[i]], quote(expr = )) && binds(x[[i]])) return(TRUE)
      }
      FALSE
    }
    terminal <- body(value)
    while (is.call(terminal) && length(terminal) > 1L &&
           paste(deparse(terminal[[1L]]), collapse = "") %in% c("{", "return")) {
      terminal <- terminal[[if (identical(terminal[[1L]], as.name("{"))) length(terminal) else 2L]]
    }
    if (!is.call(terminal) || length(terminal) < 2L) return(NULL)
    head <- paste(deparse(terminal[[1L]]), collapse = "")
    if (!head %in% c("UseMethod", "base::UseMethod")) return(NULL)
    if (head == "UseMethod" && ("UseMethod" %in% names(formals(value)) || binds(body(value)) ||
        !identical(get0("UseMethod", envir = environment(value), inherits = TRUE), base::UseMethod))) return(NULL)
    parameters <- as.list(terminal)[-1L]
    names <- names(parameters)
    argument <- if (!is.null(names) && "generic" %in% names) parameters[[which(names == "generic")[[1L]]]] else parameters[[1L]]
    if (is.character(argument) && length(argument) == 1L && nzchar(argument)) argument else NULL
  }
  internal_binding <- function(name) {
    any(startsWith(name, c(".__T__", ".__C__", ".__M__"))) ||
      name %in% c(".__NAMESPACE__.", ".__S3MethodsTable__.")
  }
  default_source <- function(value) {
    # Deparse inserts addition calls for imaginary constants. Preserve literal
    # syntax inside defaults without evaluating their expressions.
    control <- c("keepNA", "keepInteger", "niceNames", "showAttributes", "digits17")
    original <- paste(deparse(value, width.cutoff = 500L, control = control), collapse = "\n")
    prefix <- ".__diplodocus_complex_literal_"
    while (grepl(prefix, original, fixed = TRUE)) prefix <- paste0(prefix, "_")
    replacements <- list()
    replace_literals <- function(value) {
      if (is.complex(value) && length(value) == 1L && is.null(attributes(value)) &&
          isTRUE(Re(value) == 0) && is.finite(Im(value))) {
        marker <- paste0(prefix, length(replacements) + 1L, "_end")
        imaginary <- paste0(paste(deparse(Im(value), control = control), collapse = "\n"), "i")
        if (Im(value) < 0) imaginary <- paste0("(", imaginary, ")")
        replacements[[marker]] <<- imaginary
        return(as.name(marker))
      }
      if (is.call(value) || is.pairlist(value) || is.expression(value) || is.list(value)) {
        parts <- as.list(value)
        for (i in seq_along(parts)) {
          if (!identical(parts[[i]], quote(expr = ))) parts[i] <- list(replace_literals(parts[[i]]))
        }
        result <- if (is.call(value)) as.call(parts) else if (is.pairlist(value)) as.pairlist(parts) else if (is.expression(value)) as.expression(parts) else parts
        attributes(result) <- attributes(value)
        return(result)
      }
      value
    }
    replaced <- replace_literals(value)
    if (!length(replacements)) return(original)
    # The chosen prefix is absent from the original expression, including strings.
    # Only inserted symbols can match these substitutions.
    text <- paste(deparse(replaced, width.cutoff = 500L, control = control), collapse = "\n")
    for (marker in names(replacements)) text <- gsub(marker, replacements[[marker]], text, fixed = TRUE)
    text
  }
  exports <- sort(Filter(function(name) !internal_binding(name), getNamespaceExports(ns)))
  binding_values <- list()
  observe_binding <- function(name) {
    if (internal_binding(name)) return(NULL)
    value <- if (exists(name, envir = ns, inherits = FALSE)) {
      get(name, envir = ns, inherits = FALSE)
    } else getExportedValue(expected_name, name)
    kind <- if (isS4(value)) "unsupported-s4" else if (inherits(value, "R6ClassGenerator")) "unsupported-r6" else if (inherits(value, "S7_class") || inherits(value, "S7_object")) "unsupported-s7" else if (is.function(value)) "function" else "value"
    signature <- list()
    if (kind == "function") {
      formals <- formals(if (is.primitive(value)) base::args(value) else value)
      signature <- lapply(seq_along(formals), function(i) {
        required <- identical(formals[[i]], quote(expr = ))
        default <- if (required) NULL else default_source(formals[[i]])
        list(name = names(formals)[[i]], required = required,
             default = default)
      })
    }
    binding <- list(name = name, kind = kind,
                    formals = signature, generic = if (kind == "function") generic_name(value) else NULL)
    binding_values[name] <<- list(value)
    binding
  }
  bindings <- lapply(sort(unique(c(ls(ns, all.names = TRUE), exports))), function(name) {
    tryCatch(observe_binding(name), error = function(e) {
      # Reconciliation decides whether a private binding belongs to the API.
      list(name = name, kind = "unavailable", formals = list(), generic = NULL,
           same_as = I(character()), error = conditionMessage(e))
    })
  })
  bindings <- Filter(Negate(is.null), bindings)
  addresses <- vapply(binding_values, rlang::obj_address, character(1L))
  groups <- split(names(addresses), addresses)
  bindings <- lapply(bindings, function(binding) {
    if (identical(binding$kind, "unavailable")) return(binding)
    binding$same_as <- I(setdiff(groups[[addresses[[binding$name]]]], binding$name))
    binding
  })
  registrations <- getNamespaceInfo(ns, "S3methods")
  methods <- if (is.null(registrations) || !nrow(registrations)) list() else lapply(seq_len(nrow(registrations)), function(i) {
    row <- registrations[i, ]
    owner <- if (length(row) >= 4L && !is.na(row[[4L]]) && nzchar(row[[4L]])) row[[4L]] else NULL
    if (is.null(owner)) {
      generic <- get0(row[[1L]], envir = ns, inherits = TRUE)
      if (is.function(generic) && !is.primitive(generic) && isNamespace(environment(generic))) {
        owner <- getNamespaceName(environment(generic))
      }
    }
    list(generic = row[[1L]], class = row[[2L]],
         binding = if (length(row) >= 3L && !is.na(row[[3L]]) && nzchar(row[[3L]])) row[[3L]] else paste(row[[1L]], row[[2L]], sep = "."),
         package = owner)
  })
  checked$verify()
  dependencies <- lapply(sort(loadedNamespaces()), function(name) as.character(utils::packageVersion(name)))
  names(dependencies) <- sort(loadedNamespaces())
  observation <- list(r_version = as.character(getRversion()),
                      pkgload_version = as.character(utils::packageVersion("pkgload")),
                      package_name = expected_name, package_version = expected_version,
                      dependencies = dependencies, exports = I(exports),
                      bindings = bindings, methods = methods)
  strict(pkgload::unload(expected_name))
  writeLines(jsonlite::toJSON(list(complete = "r-native-v1", observation = observation,
                                  generation = generation, warnings = I(unique(runtime_warnings))), auto_unbox = TRUE, null = "null"),
             output, useBytes = TRUE)
  }
  run_native()
})
