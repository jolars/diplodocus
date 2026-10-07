"Static geometry with module-scoped includes."
module TinyGeometry

export Point, AbstractPoint, Marker, norm, magnitude, ORIGIN, @twice
public coordinates

include("types.jl")
include("methods.jl")

const magnitude = norm
const ORIGIN = Point(0.0, 0.0)

"Return the two coordinates of a point."
function coordinates end
coordinates(point::Point) = (point.x, point.y)

"Repeat a supplied expression, without expanding it during extraction."
macro twice(ex)
    :($ex + $ex)
end

baremodule Units
public SCALE
const SCALE = 1
end

# These sentinels fail if extraction accidentally evaluates the module.
write("julia-was-executed", "bad")
error("Julia extraction must remain static")

end
