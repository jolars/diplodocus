mean_squared_error <- function(actual, predicted) {
  actual <- as.double(actual)
  predicted <- as.double(predicted)
  if (length(actual) == 0L || length(actual) != length(predicted)) {
    stop(
      "`actual` and `predicted` must have the same nonzero length.",
      call. = FALSE
    )
  }

  mean((actual - predicted)^2)
}
