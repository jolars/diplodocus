local({
  staging <- commandArgs(trailingOnly = TRUE)[[1L]]
  request <- jsonlite::fromJSON(file.path(staging, "request.json"))
  checkout <- load_checkout(request$root, request$name, request$version)
  env <- new.env(parent = globalenv())
  results <- lapply(seq_along(request$blocks), function(i) {
    index <- i - 1L
    progress <- file.path(staging, "progress.tmp")
    cat(index, file = progress)
    if (!file.rename(progress, file.path(staging, "progress"))) stop("Could not report guide progress")
    events <- evaluate::evaluate(request$blocks[[i]], envir = env, stop_on_error = 2L, new_device = TRUE)
    outputs <- list()
    for (event in events) {
      if (inherits(event, "source")) next
      if (inherits(event, "recordedplot")) {
        file <- sprintf("figure-%d-%d.png", index, length(outputs))
        grDevices::png(file.path(staging, file), width = 1000L, height = 700L)
        grDevices::replayPlot(event)
        grDevices::dev.off()
        output <- list(type = "image", file = file)
      } else if (inherits(event, "condition")) {
        output <- list(type = "text", text = conditionMessage(event))
      } else if (is.character(event)) {
        output <- list(type = "text", text = paste(event, collapse = ""))
      } else {
        stop("Unsupported R guide output")
      }
      outputs[[length(outputs) + 1L]] <- output
    }
    outputs
  })
  checkout$verify()
  pkgload::unload(checkout$name)
  jsonlite::write_json(list(version = as.character(getRversion()), blocks = results),
                       file.path(staging, "results.json"), auto_unbox = TRUE)
})
