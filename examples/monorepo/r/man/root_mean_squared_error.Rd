\name{root_mean_squared_error}
\alias{root_mean_squared_error}
\title{Root mean squared error}
\description{Compute the square root of mean squared error.}
\usage{root_mean_squared_error(actual, predicted)}
\arguments{
  \item{actual}{A nonempty numeric vector of observed responses.}
  \item{predicted}{Predictions with the same length as \code{actual}.}
}
\value{Prediction error in the units of the response, with greater weight on large errors.}
\examples{
root_mean_squared_error(c(1, 2), c(1, 3))
}
