from typing import overload


@overload
def native_value(value: int) -> int: ...


@overload
def native_value(value: str) -> str: ...
