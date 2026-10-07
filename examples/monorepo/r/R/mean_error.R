mean_error <- function(actual, predicted) {
  mean(residuals(actual, predicted))
}
