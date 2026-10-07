abstract type AbstractPoint end
primitive type Marker 8 end

"""
A point with two coordinates of a common real type.

| Field | Meaning |
|:------|:--------|
| `x` | Horizontal coordinate |
| `y` | Vertical coordinate |

!!! note "Construction"
    Only explicit constructors appear in this reference.
"""
struct Point{T<:Real} <: AbstractPoint
    "Horizontal coordinate α."
    x::T
    "Vertical coordinate."
    y::T
    Point(x::T, y::T) where T<:Real = new{T}(x, y)
end
