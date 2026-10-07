"""Evaluate numeric predictions with the same metrics in Python and R."""

from math import sqrt as _sqrt

__all__ = [
    "mean_squared_error",
    "mean_absolute_error",
    "root_mean_squared_error",
    "mean_error",
    "r_squared",
    "residuals",
]


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

    Raises
    ------
    ValueError
        If the inputs are empty or have different lengths.

    Examples
    --------
    >>> mean_squared_error([1.0, 2.0], [1.0, 3.0])
    0.5
    """
    return sum(error ** 2 for error in residuals(actual, predicted)) / len(actual)


def mean_absolute_error(actual: list[float], predicted: list[float]) -> float:
    """Compute the mean absolute difference between observations and predictions.

    Parameters
    ----------
    actual : list of float
        A nonempty list of observed responses.
    predicted : list of float
        Predictions with the same length as actual.

    Returns
    -------
    float
        The average absolute prediction error, in the units of the response.

    Raises
    ------
    ValueError
        If the inputs are empty or have different lengths.

    Examples
    --------
    >>> mean_absolute_error([1.0, 2.0], [1.0, 3.0])
    0.5
    """
    return sum(abs(error) for error in residuals(actual, predicted)) / len(actual)


def root_mean_squared_error(actual: list[float], predicted: list[float]) -> float:
    """Compute the square root of mean squared error.

    Parameters
    ----------
    actual : list of float
        A nonempty list of observed responses.
    predicted : list of float
        Predictions with the same length as actual.

    Returns
    -------
    float
        Prediction error in the units of the response, with greater weight on large errors.

    Raises
    ------
    ValueError
        If the inputs are empty or have different lengths.

    Examples
    --------
    >>> root_mean_squared_error([1.0, 2.0], [1.0, 3.0])
    0.7071067811865476
    """
    return _sqrt(mean_squared_error(actual, predicted))


def mean_error(actual: list[float], predicted: list[float]) -> float:
    """Compute the average signed prediction error.

    Parameters
    ----------
    actual : list of float
        A nonempty list of observed responses.
    predicted : list of float
        Predictions with the same length as actual.

    Returns
    -------
    float
        The mean of actual minus predicted. Positive values indicate underprediction.

    Raises
    ------
    ValueError
        If the inputs are empty or have different lengths.

    Examples
    --------
    >>> mean_error([1.0, 2.0], [1.0, 3.0])
    -0.5
    """
    return sum(residuals(actual, predicted)) / len(actual)


def r_squared(actual: list[float], predicted: list[float]) -> float:
    """Compare squared prediction error with a constant prediction at the observed mean.

    Constant observations raise ValueError because the total variation is zero.

    Parameters
    ----------
    actual : list of float
        A nonempty list of observed responses.
    predicted : list of float
        Predictions with the same length as actual.

    Returns
    -------
    float
        One minus the ratio of residual to total sums of squares. Values can be negative.

    Raises
    ------
    ValueError
        If the inputs are empty or have different lengths.

    Examples
    --------
    >>> r_squared([1.0, 2.0], [1.0, 3.0])
    -1.0
    """
    errors = residuals(actual, predicted)
    observed_mean = sum(actual) / len(actual)
    total = sum((value - observed_mean) ** 2 for value in actual)
    if total == 0:
        raise ValueError("r_squared requires nonconstant observations")
    return 1 - sum(error ** 2 for error in errors) / total


def residuals(actual: list[float], predicted: list[float]) -> list[float]:
    """Subtract each prediction from its corresponding observation.

    Parameters
    ----------
    actual : list of float
        A nonempty list of observed responses.
    predicted : list of float
        Predictions with the same length as actual.

    Returns
    -------
    list[float]
        Signed errors in input order. Positive values indicate underprediction.

    Raises
    ------
    ValueError
        If the inputs are empty or have different lengths.

    Examples
    --------
    >>> residuals([1.0, 2.0], [1.0, 3.0])
    [0.0, -1.0]
    """
    if not actual or len(actual) != len(predicted):
        raise ValueError("actual and predicted must have the same nonzero length")
    return [a - p for a, p in zip(actual, predicted)]
