\name{fit}
\alias{fit}
\alias{fit.default}
\alias{fit.model}
\alias{model-fitting}
\title{Fit a Model}
\usage{
fit(x, ...)
\method{fit}{default}(x, y, ...)
\method{fit}{model}(x, weights = NULL, ...)
}
\arguments{
\item{x}{Data or an existing model.}
\item{y}{Response values for the default method.}
\item{weights}{Observation weights for the model method.}
\item{...}{Additional method arguments.}
}
\description{Fit a model with class-specific arguments.}
\seealso{\link{model-fitting}, \link{eulergram-compose}}
