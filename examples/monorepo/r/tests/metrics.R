library(tinystats)

actual <- c(1, 2, 3, 4)
predicted <- c(1, 2, 3, 6)
stopifnot(
  identical(residuals(actual, predicted), c(0, 0, 0, -2)),
  mean_absolute_error(actual, predicted) == 0.5,
  mean_squared_error(actual, predicted) == 1,
  root_mean_squared_error(actual, predicted) == 1,
  mean_error(actual, predicted) == -0.5,
  isTRUE(all.equal(r_squared(actual, predicted), 0.2)),
  r_squared(actual, actual) == 1,
  r_squared(actual, rep(mean(actual), length(actual))) == 0,
  is.na(mean_squared_error(c(1, NA_real_), c(1, 2)))
)

for (name in getNamespaceExports("tinystats")) {
  metric <- getExportedValue("tinystats", name)
  stopifnot(
    inherits(try(metric(numeric(), numeric()), silent = TRUE), "try-error"),
    inherits(try(metric(1, c(1, 2)), silent = TRUE), "try-error")
  )
}
stopifnot(inherits(try(r_squared(c(1, 1), c(1, 2)), silent = TRUE), "try-error"))
