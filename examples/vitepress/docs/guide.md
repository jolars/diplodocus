# Comparing predictions

Mean squared error measures the average squared difference between observations
and predictions. Smaller values indicate a closer fit.

```python
from tinystats import mean_squared_error

mean_squared_error([1.0, 2.0], [1.0, 3.0])
```

The result is `0.5`. See the [Python reference](reference/python/tinystats.mean_squared_error)
and the [R reference](reference/r/mean_squared_error) for signatures and examples.

## Choosing a metric

Use mean absolute error when you want errors in the original units. Root mean
squared error also uses the original units and gives greater weight to large
errors.
