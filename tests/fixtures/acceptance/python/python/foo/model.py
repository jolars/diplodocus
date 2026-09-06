"""Model fitting and evaluation."""

from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass
from math import fsum
from typing import Final

DEFAULT_TOLERANCE: Final = 1e-8
"""Default convergence tolerance used by :func:`fit`."""

SUPPORTED_SOLVERS: Final = ("normal", "qr")
"""Solver names accepted by :func:`fit`."""


@dataclass(frozen=True)
class FitDiagnostics:
    """Information collected while fitting a model.

    Parameters
    ----------
    iterations : int
        Number of solver iterations.
    converged : bool
        Whether the solver met its convergence criterion.
    """

    iterations: int
    converged: bool


class FooModel:
    """A fitted linear model.

    Parameters
    ----------
    coefficients : sequence of float
        Coefficients ordered like the columns in the feature matrix.
    intercept : float
        Constant added to every prediction.
    """

    def __init__(self, coefficients: Sequence[float], intercept: float) -> None:
        self._coefficients = tuple(float(value) for value in coefficients)
        self._intercept = float(intercept)

    @property
    def coefficients(self) -> tuple[float, ...]:
        """The fitted coefficients."""

        return self._coefficients

    @property
    def intercept(self) -> float:
        """The fitted intercept."""

        return self._intercept

    def fit(
        self,
        features: Sequence[Sequence[float]],
        target: Sequence[float],
        *,
        tolerance: float = DEFAULT_TOLERANCE,
    ) -> FooModel:
        """Refit the model in place.

        Parameters
        ----------
        features : sequence of sequence of float
            Rectangular feature matrix.
        target : sequence of float
            Response value for every row.
        tolerance : float, default=1e-8
            Positive convergence tolerance.

        Returns
        -------
        FooModel
            This model after replacing its fitted parameters.
        """

        fitted = fit(features, target, tolerance=tolerance)
        self._coefficients = fitted.coefficients
        self._intercept = fitted.intercept
        return self

    def predict(
        self, features: Sequence[float] | Sequence[Sequence[float]], /
    ) -> float | list[float]:
        """Predict one response or a list of responses.

        Parameters
        ----------
        features : sequence of float or sequence of sequence of float
            One feature row or a matrix of feature rows.

        Returns
        -------
        float or list of float
            A scalar for one row, or one prediction per matrix row.
        """

        if not features:
            return []
        if isinstance(features[0], Sequence):
            return [self._predict_one(row) for row in features]
        return self._predict_one(features)

    def _predict_one(self, features: Sequence[float]) -> float:
        if len(features) != len(self._coefficients):
            raise ValueError("feature width must match the fitted model")
        return self._intercept + fsum(
            coefficient * float(value)
            for coefficient, value in zip(self._coefficients, features, strict=True)
        )


def fit(
    features: Sequence[Sequence[float]],
    target: Sequence[float],
    *,
    solver: str = "normal",
    tolerance: float = DEFAULT_TOLERANCE,
    return_diagnostics: bool = False,
) -> FooModel | tuple[FooModel, FitDiagnostics]:
    """Fit a linear model.

    Parameters
    ----------
    features : sequence of sequence of float
        Rectangular feature matrix with observations in rows.
    target : sequence of float
        Response value for every observation.
    solver : {"normal", "qr"}, default="normal"
        Solver used to estimate the coefficients.
    tolerance : float, default=1e-8
        Positive convergence tolerance.
    return_diagnostics : bool, default=False
        Return solver diagnostics together with the model when true.

    Returns
    -------
    FooModel or tuple of FooModel and FitDiagnostics
        The fitted model, optionally paired with solver diagnostics.

    Raises
    ------
    ValueError
        If the inputs are empty, incompatible, or unsupported.

    Notes
    -----
    This fixture uses the mean response as a deterministic baseline estimator;
    production bindings delegate fitting to the native Foo library.

    References
    ----------
    .. [1] A. Researcher, "A Small Model," Journal of Fixtures, 1, 1-4, 2024.

    Examples
    --------
    >>> model = fit([[0.0], [1.0]], [1.0, 3.0])
    >>> model.intercept
    2.0
    >>> model.predict([2.0])
    2.0
    """

    if solver not in SUPPORTED_SOLVERS:
        raise ValueError(f"unsupported solver: {solver}")
    if tolerance <= 0:
        raise ValueError("tolerance must be positive")

    rows = [tuple(float(value) for value in row) for row in features]
    responses = [float(value) for value in target]
    if not rows or not responses:
        raise ValueError("features and target must not be empty")
    if len(rows) != len(responses):
        raise ValueError("features and target must have the same length")

    width = len(rows[0])
    if width == 0 or any(len(row) != width for row in rows):
        raise ValueError("features must be a nonempty rectangular matrix")

    model = FooModel((0.0,) * width, fsum(responses) / len(responses))
    diagnostics = FitDiagnostics(iterations=1, converged=True)
    if return_diagnostics:
        return model, diagnostics
    return model


def mean_squared_error(actual: Sequence[float], predicted: Sequence[float]) -> float:
    """Compute the mean squared prediction error.

    Parameters
    ----------
    actual : sequence of float
        Observed response values.
    predicted : sequence of float
        Predicted response values.

    Returns
    -------
    float
        Mean squared difference between observed and predicted values.

    Examples
    --------
    >>> mean_squared_error([1.0, 2.0], [1.0, 3.0])
    0.5
    """

    if not actual or len(actual) != len(predicted):
        raise ValueError("actual and predicted must have the same nonzero length")
    squared_errors = (
        (float(observed) - float(estimate)) ** 2
        for observed, estimate in zip(actual, predicted, strict=True)
    )
    return fsum(squared_errors) / len(actual)
