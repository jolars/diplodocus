"""Prediction metrics for the VitePress watcher fixture."""

__all__ = [
    "mean_squared_error",
]


def mean_squared_error(actual: list[float], predicted: list[float]) -> float:
    """Compute the mean squared difference between observations and predictions.

    Parameters
    ----------
    actual : list of float
        Observed responses.
    predicted : list of float
        Predicted responses.

    Returns
    -------
    float
        The average squared prediction error.
    """
    return sum((a - p) ** 2 for a, p in zip(actual, predicted)) / len(actual)
