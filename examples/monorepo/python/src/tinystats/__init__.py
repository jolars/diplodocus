"""Compare observed responses and predictions with Tiny Stats."""

__all__ = ["mean_squared_error"]


def mean_squared_error(actual: list[float], predicted: list[float]) -> float:
    """Compute the mean squared difference between observations and predictions.

    Parameters
    ----------
    actual : list of float
        A nonempty list of observed responses.
    predicted : list of float
        Predictions with the same length as actual.

    Returns
    -------
    float
        The average squared prediction error. Zero indicates a perfect match.

    Examples
    --------
    >>> mean_squared_error([1.0, 2.0], [1.0, 3.0])
    0.5
    """
    if not actual or len(actual) != len(predicted):
        raise ValueError("actual and predicted must have the same nonzero length")
    return sum((a - p) ** 2 for a, p in zip(actual, predicted)) / len(actual)
