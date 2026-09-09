"""Experimental APIs with an explicit static export list."""

from collections.abc import Sequence


def experimental_rank(values: Sequence[float]) -> list[int]:
    """Return stable zero-based ranks for a sequence."""

    order = sorted(range(len(values)), key=values.__getitem__)
    ranks = [0] * len(values)
    for rank, position in enumerate(order):
        ranks[position] = rank
    return ranks


__all__ = ["experimental_rank"]
