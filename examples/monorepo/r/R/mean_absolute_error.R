mean_absolute_error <- function(actual, predicted) {
  mean(abs(residuals(actual, predicted)))
}
