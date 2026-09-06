"""Fit and evaluate statistical models with Foo."""

from . import experimental as experimental
from ._native import NativeWorkspace, native_mean
from .model import (
    DEFAULT_TOLERANCE,
    SUPPORTED_SOLVERS,
    FitDiagnostics,
    FooModel,
    fit,
    mean_squared_error,
)

__version__ = "1.9.0"

__all__ = [
    "DEFAULT_TOLERANCE",
    "SUPPORTED_SOLVERS",
    "FitDiagnostics",
    "FooModel",
    "NativeWorkspace",
    "experimental",
    "fit",
    "mean_squared_error",
    "native_mean",
]
