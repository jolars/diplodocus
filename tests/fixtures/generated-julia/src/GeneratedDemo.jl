module GeneratedDemo
export generated

"""
Return a value through [the integer method](@ref generated(::Int64)).
"""
function generated end

macro define_api()
    quote
        """
        Return the generated value.

        ```jldoctest
        julia> write("generated-doctest-ran", "yes") > 0
        true

        julia> generated(3)
        3
        ```
        """
        $(esc(:generated))(value::Int) = value
    end
end

@define_api
@eval generated(value::AbstractFloat) = value
@eval const runtime_alias = generated
@eval export runtime_alias

function __init__()
    write("generated-checkout-loaded", "yes")
end
end
