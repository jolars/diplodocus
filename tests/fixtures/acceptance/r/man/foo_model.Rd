\name{foo_model}
\alias{foo_model}
\alias{predict.foo_model}
\title{Create and Use a Foo Model}
\usage{
foo_model(coefficients, intercept = 0, solver = "normal")

\method{predict}{foo_model}(object, newdata, ...)
}
\arguments{
  \item{coefficients}{A numeric vector of fitted coefficients.}
  \item{intercept}{A numeric intercept.}
  \item{solver}{The name of the fitting algorithm.}
  \item{object}{An object of class \code{"foo_model"}.}
  \item{newdata}{A numeric feature vector or matrix.}
  \item{...}{Additional arguments reserved for method compatibility.}
}
\value{
\code{foo_model()} returns an object of class \code{"foo_model"}.
\code{predict.foo_model()} returns one prediction or a numeric vector of
predictions.
}
\description{
Create a fitted model directly, or predict responses from one.
}
\examples{
model <- foo_model(c(0.5, -0.25), intercept = 1)
predict(model, c(2, 4))
predict(model, matrix(c(2, 4, 3, 5), ncol = 2, byrow = TRUE))
}
\keyword{models}
