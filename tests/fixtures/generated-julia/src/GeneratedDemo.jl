module GeneratedDemo
export generated

macro define_api()
    quote
        "Return the generated value."
        $(esc(:generated))(value::Int) = value
    end
end

@define_api
end
