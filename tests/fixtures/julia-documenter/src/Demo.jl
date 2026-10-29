module Demo
export fit, Shape
public qualified

"""
Fit a diagram with [`Shape`](@ref).

# Details

See [details](#details) and [notes](notes.txt).
"""
function fit end

"Fit integer inputs with precise areas."
fit(x::Int) = x

"A shape with a retained field."
struct Shape
    "The shape's area."
    area::Float64
end

"A public qualified helper."
qualified(x) = x

"Private implementation details."
internal(x) = x

module Nested
export nested
"Documentation from a nested module."
nested(x) = x
end
end
