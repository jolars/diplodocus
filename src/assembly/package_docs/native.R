args <- commandArgs(trailingOnly = TRUE)
output <- file(args[[4L]], open = "wt", encoding = "UTF-8")
emit <- function(key, value) {
  value <- paste(enc2utf8(value), collapse = "\n")
  bytes <- paste(sprintf("%02x", as.integer(charToRaw(value))), collapse = "")
  writeLines(paste(key, bytes, sep = "\t"), output, useBytes = TRUE)
}
meta <- as.list(read.dcf(args[[1L]])[1L, ])
people <- if (!is.null(meta$`Authors@R`)) {
  eval(str2expression(meta$`Authors@R`), envir = new.env(parent = asNamespace("utils")))
} else if (!is.null(meta$Author)) {
  utils::as.person(meta$Author)
} else NULL
if (!is.null(people)) {
  if (!inherits(people, "person")) stop("Authors@R must produce a person object")
  for (person in format(people)) emit("author", person)
  creators <- Filter(function(person) "cre" %in% person$role, people)
  for (person in format(creators, include = c("given", "family", "email"))) emit("maintainer", person)
}
cit <- if (nzchar(args[[2L]])) {
  utils::readCitationFile(args[[2L]], meta = meta)
} else if (!is.null(people)) {
  withCallingHandlers(utils::citation(auto = meta), warning = function(w) invokeRestart("muffleWarning"))
} else NULL
if (!is.null(cit)) {
  if (!inherits(cit, "bibentry") || !length(cit)) stop("CITATION must produce bibliography entries")
  emit("citation", c(attr(cit, "mheader"), format(cit, style = "text"), attr(cit, "mfooter")))
  emit("bibtex", utils::toBibtex(cit))
}
if (nzchar(args[[3L]])) {
  emit("news", capture.output(tools::Rd2txt(tools::parse_Rd(args[[3L]]))))
}
emit("version", as.character(getRversion()))
emit("complete", "r-package-metadata-v1")
close(output)
