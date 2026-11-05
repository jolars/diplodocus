export Sample

"""
A sample with [stored values](@ref TinyStats.Sample.values).
"""
mutable struct Sample
    "The observations in this sample."
    values::Vector{Float64}
    "The weight assigned to the observations."
    weight::Float64
    label
    Sample(values, weight, label) = new(values, weight, label)
end
