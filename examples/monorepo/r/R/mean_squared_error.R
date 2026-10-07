mean_squared_error <- function(actual, predicted) {
  mean(residuals(actual, predicted)^2)
}
