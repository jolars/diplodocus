"""Experimental APIs with exports that require runtime evaluation."""

from collections.abc import Sequence


def experimental_rank(values: Sequence[float]) -> list[int]:
    """Return stable zero-based ranks for a sequence."""

    order = sorted(range(len(values)), key=values.__getitem__)
    ranks = [0] * len(values)
    for rank, position in enumerate(order):
        ranks[position] = rank
    return ranks


def _exported_names() -> list[str]:
    return [name for name in globals() if name.startswith("experimental_")]


__all__ = _exported_names()
