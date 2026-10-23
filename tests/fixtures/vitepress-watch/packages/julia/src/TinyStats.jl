"""
Prediction metrics for numeric vectors.
"""
module TinyStats

export mean_squared_error

"Calculate the mean squared difference between observations and predictions."
mean_squared_error(actual, predicted) = sum(abs2, actual .- predicted) / length(actual)

end
