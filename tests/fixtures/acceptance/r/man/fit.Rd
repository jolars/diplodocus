\name{fit}
\alias{fit}
\alias{fit.default}
\alias{fit.foo_model}
\title{Fit a Foo Model}
\usage{
fit(x, ...)

\method{fit}{default}(x, y, solver = c("normal", "qr"),
  tolerance = 1e-8, ...)

\method{fit}{foo_model}(x, features, target, tolerance = 1e-8, ...)
}
\arguments{
  \item{x}{For \code{fit.default()}, a numeric feature matrix. For
    \code{fit.foo_model()}, an existing \code{foo_model} object.}
  \item{...}{Additional arguments passed to an S3 method.}
  \item{y}{A numeric response vector with one value per row of \code{x}.}
  \item{solver}{The fitting algorithm, either \code{"normal"} or \code{"qr"}.}
  \item{tolerance}{A positive convergence tolerance.}
  \item{features}{A numeric feature matrix used to refit \code{x}.}
  \item{target}{A numeric response vector used to refit \code{x}.}
}
\value{
An object of class \code{"foo_model"}. The \code{foo_model} method returns the
supplied model with its fitted values replaced.
}
\description{
Fit a deterministic baseline model through an S3 generic.
}
\details{
The default method records a zero coefficient for each feature and uses the
mean response as the intercept. The simple estimator keeps this acceptance
fixture deterministic while exercising generic and method extraction.
}
\references{
A. Researcher (2024). A Small Model. \emph{Journal of Fixtures}, 1, 1--4.
}
\examples{
features <- matrix(c(0, 1), ncol = 1)
model <- fit(features, c(1, 3))
model$intercept

fit(model, features, c(2, 4), tolerance = 1e-6)
}
\keyword{models}
