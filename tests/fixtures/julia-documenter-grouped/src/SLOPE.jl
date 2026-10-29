"A small fixture for an authored, grouped API reference."
module SLOPE
export slope, SlopeFit, predict, coef, slopecv, best_α, best_model, refit
export SlopeGridResult, SlopeCvResult, regweights, plot_path, plot_cv

include("fit.jl")
include("cv.jl")
include("plots.jl")
include("utilities.jl")
end
