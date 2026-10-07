# Evaluating models in Python

Store each model's predictions in a dictionary and evaluate them against the
same observations. This keeps the comparison explicit and requires no dataframe
library.

## Prepare the predictions

```python
from tinystats import mean_absolute_error, mean_error, root_mean_squared_error

actual = [1.0, 2.0, 3.0, 4.0]
models = {
    "A": [1.0, 2.0, 3.0, 6.0],
    "B": [2.0, 3.0, 4.0, 5.0],
}
```

## Compare the scores

```python
for name, predicted in models.items():
    print(name, {
        "MAE": mean_absolute_error(actual, predicted),
        "RMSE": root_mean_squared_error(actual, predicted),
        "bias": mean_error(actual, predicted),
    })
# A {'MAE': 0.5, 'RMSE': 1.0, 'bias': -0.5}
# B {'MAE': 1.0, 'RMSE': 1.0, 'bias': -1.0}
```

## Explore the reference

Look up [`pystats::tinystats.mean_absolute_error`],
[`pystats::tinystats.root_mean_squared_error`], and
[`pystats::tinystats.mean_error`] for the definitions. Each page also links to
the corresponding R function.

Read [Choosing a metric](../../docs/guides/choosing-a-metric.md) for the shared
explanation of these scores.
