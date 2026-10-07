# Choosing a metric

A prediction score answers a particular question. Choose the question before
choosing the score, and compare models on the same held-out observations.

## Measure typical error

**Mean absolute error** reports average error magnitude in the units of the
response. It gives each error weight proportional to its size.

**Mean squared error** squares each error before averaging. Large errors
therefore contribute disproportionately. **Root mean squared error** takes the
square root to return the score to the response's units.

| Metric | Python | R |
| --- | --- | --- |
| Mean absolute error | [`pystats::tinystats.mean_absolute_error`] | [`rstats::mean_absolute_error`] |
| Mean squared error | [`pystats::tinystats.mean_squared_error`] | [`rstats::mean_squared_error`] |
| Root mean squared error | [`pystats::tinystats.root_mean_squared_error`] | [`rstats::root_mean_squared_error`] |

The [quick start](../getting-started/quick-start.md) shows two models whose mean
absolute errors differ even though their root mean squared errors match.

## Check systematic bias

**Mean error** averages signed residuals: observations minus predictions.
Positive values indicate underprediction; negative values indicate
overprediction. Errors of opposite signs can cancel, so a mean error near zero
does not imply accurate predictions.

Use [`pystats::tinystats.mean_error`] or [`rstats::mean_error`], alongside a
measure of error magnitude.

## Compare with a constant baseline

**R squared** compares squared error with the total squared deviation from the
observed mean. A perfect prediction scores one. A prediction at the observed
mean scores zero. A worse prediction can score below zero.

Use [`pystats::tinystats.r_squared`] or [`rstats::r_squared`]. Tiny Stats raises
an error when observations are constant, because the denominator is zero.

## Inspect individual errors

A single score can hide a large error or a recurring pattern. Follow
[Inspecting residuals](inspecting-residuals.md) before deciding that one model
is adequate.
