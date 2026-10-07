# Quick start

Suppose the observed responses are `[1, 2, 3, 4]`. Model A predicts
`[1, 2, 3, 6]`; model B predicts `[2, 3, 4, 5]`.

## Compare average error

In Python:

```python
from tinystats import mean_absolute_error, root_mean_squared_error

actual = [1.0, 2.0, 3.0, 4.0]
model_a = [1.0, 2.0, 3.0, 6.0]
model_b = [2.0, 3.0, 4.0, 5.0]
mean_absolute_error(actual, model_a)  # 0.5
mean_absolute_error(actual, model_b)  # 1.0
root_mean_squared_error(actual, model_a)  # 1.0
root_mean_squared_error(actual, model_b)  # 1.0
```

In R:

```r
library(tinystats)

actual <- c(1, 2, 3, 4)
model_a <- c(1, 2, 3, 6)
model_b <- c(2, 3, 4, 5)
mean_absolute_error(actual, model_a)  # 0.5
mean_absolute_error(actual, model_b)  # 1
root_mean_squared_error(actual, model_a)  # 1
root_mean_squared_error(actual, model_b)  # 1
```

## Understand the difference

Model A has one large error. Model B has four smaller errors. Mean absolute
error favors A, while root mean squared error gives them the same score.
[Choosing a metric](../guides/choosing-a-metric.md) explains how to interpret
that difference.

## Look up the functions

- Mean absolute error: [`pystats::tinystats.mean_absolute_error`] and [`rstats::mean_absolute_error`].
- Root mean squared error: [`pystats::tinystats.root_mean_squared_error`] and [`rstats::root_mean_squared_error`].
