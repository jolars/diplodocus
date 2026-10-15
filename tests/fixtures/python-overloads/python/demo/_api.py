from typing import Literal, overload


@overload
def fetch_data(name: str, *, return_info: Literal[False] = False) -> str: ...


@overload
def fetch_data(name: str, *, return_info: Literal[True]) -> tuple[str, str]: ...


@overload
def fetch_data(name: str, *, return_info: bool) -> str | tuple[str, str]: ...


def fetch_data(name: str, *, return_info: bool = False) -> str | tuple[str, str]:
    """Retrieve one dataset, optionally returning its metadata."""
    return (name, "metadata") if return_info else name


class Reader:
    """Read text or binary dataset artifacts."""

    @overload
    def read(self, value: str) -> str: ...

    @overload
    def read(self, value: bytes) -> bytes: ...

    def read(self, value: str | bytes) -> str | bytes:
        """Return the artifact in its original representation."""
        return value
