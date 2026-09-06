from collections.abc import Sequence
from typing import Final, Literal, overload

DEFAULT_TOLERANCE: Final[float]
SUPPORTED_SOLVERS: Final[tuple[str, ...]]

class FitDiagnostics:
    iterations: int
    converged: bool

class FooModel:
    def __init__(self, coefficients: Sequence[float], intercept: float) -> None: ...
    @property
    def coefficients(self) -> tuple[float, ...]: ...
    @property
    def intercept(self) -> float: ...
    def fit(
        self,
        features: Sequence[Sequence[float]],
        target: Sequence[float],
        *,
        tolerance: float = ...,
    ) -> FooModel: ...
    @overload
    def predict(self, features: Sequence[float], /) -> float: ...
    @overload
    def predict(self, features: Sequence[Sequence[float]], /) -> list[float]: ...

@overload
def fit(
    features: Sequence[Sequence[float]],
    target: Sequence[float],
    *,
    solver: Literal["normal", "qr"] = "normal",
    tolerance: float = ...,
    return_diagnostics: Literal[False] = False,
) -> FooModel: ...
@overload
def fit(
    features: Sequence[Sequence[float]],
    target: Sequence[float],
    *,
    solver: Literal["normal", "qr"] = "normal",
    tolerance: float = ...,
    return_diagnostics: Literal[True],
) -> tuple[FooModel, FitDiagnostics]: ...
def mean_squared_error(
    actual: Sequence[float], predicted: Sequence[float]
) -> float: ...
