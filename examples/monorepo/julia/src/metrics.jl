function checked_residuals(observed, predicted)
    isempty(observed) && throw(ArgumentError("inputs must be nonempty"))
    length(observed) == length(predicted) || throw(DimensionMismatch("input lengths differ"))
    observed .- predicted
end

"""
Calculate the mean squared difference between observed and predicted values.

Both inputs must have the same nonzero length. See the
[vector method](@ref mean_squared_error(::AbstractVector, ::AbstractVector)).

# Examples

```jldoctest
julia> mean_squared_error([1.0, 2.0], [1.0, 3.0])
0.5
```
"""
function mean_squared_error end

"Calculate mean squared error for numeric vectors."
function mean_squared_error(observed::AbstractVector, predicted::AbstractVector)
    errors = checked_residuals(observed, predicted)
    sum(abs2, errors) / length(errors)
end

"Calculate mean squared error for tuples by converting them to vectors."
mean_squared_error(observed::Tuple, predicted::Tuple) = mean_squared_error(collect(observed), collect(predicted))

"""
Calculate the mean absolute difference between observed and predicted values.

This metric gives each error equal weight. Compare [`mean_squared_error`](@ref)
when large errors matter more.
"""
function mean_absolute_error end

mean_absolute_error(observed::AbstractVector, predicted::AbstractVector) = sum(abs, checked_residuals(observed, predicted)) / length(observed)

"Calculate the square root of [`mean_squared_error`](@ref), in the response's units."
function root_mean_squared_error end

root_mean_squared_error(observed::AbstractVector, predicted::AbstractVector) = sqrt(mean_squared_error(observed, predicted))

"Calculate the mean signed error, with positive values indicating underprediction."
function mean_error end

mean_error(observed::AbstractVector, predicted::AbstractVector) = sum(checked_residuals(observed, predicted)) / length(observed)

"Calculate R squared relative to predicting the mean of the observed values."
function r_squared end

function r_squared(observed::AbstractVector, predicted::AbstractVector)
    errors = checked_residuals(observed, predicted)
    center = sum(observed) / length(observed)
    total = sum(abs2, observed .- center)
    iszero(total) && throw(ArgumentError("observations must vary"))
    1 - sum(abs2, errors) / total
end

"Calculate observed minus predicted values, preserving the individual errors."
function residuals end

residuals(observed::AbstractVector, predicted::AbstractVector) = checked_residuals(observed, predicted)
