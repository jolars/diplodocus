from contextlib import contextmanager as manage
from collections.abc import Generator, Iterator

from ._cache import verified_lease


@manage
def public_lease(path: str) -> Iterator[str]:
    with verified_lease(path) as result:
        yield result


@manage
def public_generator(path: str) -> Generator[str, None, None]:
    yield path


@manage
def _private_lease() -> Iterator[None]:
    yield None


@other_decorator
def _private_helper(value: str) -> str:
    return value


@other_decorator
def public_unknown(value: str) -> str:
    return value
