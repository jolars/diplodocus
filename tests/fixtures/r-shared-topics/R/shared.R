fit <- function(x, ...) UseMethod("fit")
fit.default <- function(x, y, ...) x
fit.model <- function(x, weights = NULL, ...) x
"|.eulergram" <- function(e1, e2) e1
"/.eulergram" <- function(e1, e2) e1
plot.euler <- function(x, ...) x
eulerr_options <- function(...) NULL
