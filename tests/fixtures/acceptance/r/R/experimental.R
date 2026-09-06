experimental_summary <- function(x) {
  x <- as.double(x)
  if (length(x) == 0L) {
    stop("`x` must not be empty.", call. = FALSE)
  }

  c(minimum = min(x), median = stats::median(x), maximum = max(x))
}
