module Demo
export fit, Shape, LIMIT, @twice, PublicModule
public qualified, PUBLIC_VALUE

"""
Fit supplied inputs.

See [private helper](@ref helper_alias), [stored value](@ref Demo.Shape.value), and
[integer method](@ref fit(::Int)).
"""
function fit end

"Fit integer inputs precisely."
fit(x::Int) = x
fit(x::Float64) = x

"A public qualified helper."
qualified(x) = x

const LIMIT = 10
const PUBLIC_VALUE = 20

"Repeat an expression."
macro twice(ex)
    :($ex + $ex)
end

"A public shape."
struct Shape
    "The stored value."
    value::Float64
    "Construct from a real value."
    Shape(x::Real) = new(Float64(x))
end
Shape(x::Int, y::Int) = Shape(x + y)

"Private implementation details."
helper(x) = x
const helper_alias = helper

"A private type retained for its documentation."
struct PrivateType
    "A private field."
    value::Int
    PrivateType(x::Int) = new(x)
end

module Containers
module Nested
public score
"Score from an unexported nested module."
score(x) = x
end
end

"A documented module without public descendants."
module Hidden
"A hidden secret selected by authored autodocs."
secret(x) = x
end

module PublicModule
"A private child of a public module."
private_child(x) = x
end

module Impl
internal(x::Int) = x
other(x::Int) = x

struct InternalType
    value::Int
    InternalType(x::Int) = new(x)
end

module Shared
"A private child of an aliased public module."
private_child(x) = x
end
end

using .Impl: internal as train
export train
const public_train = Impl.other
public public_train
const AliasedType = Impl.InternalType
export AliasedType
const SharedModule = Impl.Shared
public SharedModule
const local_fit = fit

"Display a public shape through a private external family."
Base.show(io::IO, shape::Shape) = nothing

"An explicitly exposed external family."
Base.length(shape::Shape) = 1
const shape_length = Base.length
public shape_length
end
