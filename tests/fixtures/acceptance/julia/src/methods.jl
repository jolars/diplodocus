"""
Calculate a distance from the origin.

See [`Point`](@ref) or the [point method](@ref norm(::Point)).

```@example geometry
error("this fence is display-only")
```
"""
function norm end

"Calculate the Euclidean distance of a point."
@inline norm(point::Point) = sqrt(point.x^2 + point.y^2)

"Calculate distance for a vector of real coordinates."
norm(values::AbstractVector{T}) where T<:Real = sqrt(sum(abs2, values))

"Describe a point through an explicitly supplied external extension."
Base.show(io::IO, point::Point) = print(io, "Point(", point.x, ", ", point.y, ")")
