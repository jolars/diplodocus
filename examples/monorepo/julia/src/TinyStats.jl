"""
Prediction metrics for numeric vectors.

Start with [`mean_squared_error`](@ref), then inspect [`residuals`](@ref).
"""
module TinyStats

export mean_squared_error, mean_absolute_error, root_mean_squared_error,
       mean_error, r_squared, residuals

include("metrics.jl")

end
