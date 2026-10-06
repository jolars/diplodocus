from collections.abc import Iterator
from contextlib import contextmanager


@contextmanager
def _lease(path: str) -> Iterator[str]:
    yield path


@contextmanager
def verified_lease(path: str) -> Iterator[str]:
    with _lease(path) as result:
        yield result
