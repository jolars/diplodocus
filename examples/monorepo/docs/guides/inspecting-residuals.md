# Inspecting residuals

A residual is an observation minus its prediction. Positive residuals mean
the model predicted too little; negative residuals mean it predicted too much.

## Calculate the residuals

In Python:

```python
from tinystats import residuals

residuals([1.0, 2.0, 3.0, 4.0], [1.0, 2.0, 3.0, 6.0])
# [0.0, 0.0, 0.0, -2.0]
```

In R:

```r
library(tinystats)

residuals(c(1, 2, 3, 4), c(1, 2, 3, 6))
# [1] 0 0 0 -2
```

See [`pystats::tinystats.residuals`] and [`rstats::residuals`] for input
requirements and return types.

## Look for patterns

Keep residuals in the same order as the observations. Compare them with the
predicted values, relevant input variables, or time. A cluster of positive
residuals may reveal a group for which the model consistently underpredicts.

Tiny Stats returns the numbers; use your preferred plotting tools to inspect
them. It does not fit models or create plots.

## Handle inputs deliberately

Both inputs must be nonempty and have equal lengths. Tiny Stats does not drop
missing observations. Filter paired observations and predictions together
before calculating a score, and record why you excluded them.

## Return to the aggregate scores

Use [Choosing a metric](choosing-a-metric.md) to summarize what you found,
while retaining the residuals for further inspection.
