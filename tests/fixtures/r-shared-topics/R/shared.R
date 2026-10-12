fit <- function(x, ...) UseMethod("fit")
fit.default <- function(x, y, ...) x
fit.model <- function(x, weights = NULL, ...) x
"|.eulergram" <- function(e1, e2) e1
"/.eulergram" <- function(e1, e2) e1
example_euler <- function(x) structure(list(values = x), class = "euler")
plot.euler <- function(x, ...) structure(x, class = "eulergram")
eulerr_options <- function(...) NULL
