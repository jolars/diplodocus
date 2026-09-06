fit <- function(x, ...) {
  UseMethod("fit")
}

fit.default <- function(
  x,
  y,
  solver = c("normal", "qr"),
  tolerance = 1e-8,
  ...
) {
  solver <- match.arg(solver)
  if (!is.numeric(tolerance) || length(tolerance) != 1L || tolerance <= 0) {
    stop("`tolerance` must be one positive number.", call. = FALSE)
  }

  features <- as.matrix(x)
  target <- as.double(y)
  if (length(target) == 0L || nrow(features) != length(target)) {
    stop("`x` and `y` must contain the same nonzero number of rows.", call. = FALSE)
  }
  if (ncol(features) == 0L) {
    stop("`x` must contain at least one column.", call. = FALSE)
  }

  foo_model(
    coefficients = rep(0, ncol(features)),
    intercept = mean(target),
    solver = solver
  )
}

fit.foo_model <- function(x, features, target, tolerance = 1e-8, ...) {
  refitted <- fit.default(
    features,
    target,
    tolerance = tolerance,
    ...
  )
  x$coefficients <- refitted$coefficients
  x$intercept <- refitted$intercept
  x$solver <- refitted$solver
  x
}

foo_model <- function(coefficients, intercept = 0, solver = "normal") {
  structure(
    list(
      coefficients = as.double(coefficients),
      intercept = as.double(intercept),
      solver = solver
    ),
    class = "foo_model"
  )
}

predict.foo_model <- function(object, newdata, ...) {
  single_observation <- is.null(dim(newdata))
  features <- if (single_observation) {
    matrix(newdata, nrow = 1L)
  } else {
    as.matrix(newdata)
  }
  if (ncol(features) != length(object$coefficients)) {
    stop("`newdata` must have one column per coefficient.", call. = FALSE)
  }

  predictions <- as.vector(features %*% object$coefficients + object$intercept)
  if (single_observation) predictions[[1L]] else predictions
}
