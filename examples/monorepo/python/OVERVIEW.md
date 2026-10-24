# Tiny Stats for Python

Compare predictions with observed values using six small functions. Measure
accuracy with error magnitudes, inspect bias with mean error, and use residuals
to find predictions that need attention.

## Installation

From the Diplodocus repository root, install the example package:

```console
python -m pip install examples/monorepo/python
```

## Compare predictions

```python
from tinystats import mean_squared_error

observed = [1, 2, 3, 4]
predicted = [1, 2, 3, 6]
mean_squared_error(observed, predicted)
# 1.0
```

Read the [shared metric guide](../docs/guides/choosing-a-metric.md) to choose a
score, or follow the [Python workflow](docs/evaluating-models.md) for a longer
example.
