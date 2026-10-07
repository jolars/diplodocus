r_squared <- function(actual, predicted) {
  actual <- as.double(actual)
  errors <- residuals(actual, predicted)
  total <- sum((actual - mean(actual))^2)
  if (total == 0) {
    stop("r_squared requires nonconstant observations", call. = FALSE)
  }
  1 - sum(errors^2) / total
}
