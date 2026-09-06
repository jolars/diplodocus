"""Public declarations for the compiled ``foo._native`` extension."""

from collections.abc import Sequence

class NativeWorkspace:
    """Reusable workspace allocated by the native Foo library."""

    dimension: int
    def __init__(self, dimension: int) -> None: ...

def native_mean(values: Sequence[float], /) -> float: ...
