\name{mean_absolute_error}
\alias{mean_absolute_error}
\title{Mean absolute error}
\description{Compute the mean absolute difference between observations and predictions.}
\usage{mean_absolute_error(actual, predicted)}
\arguments{
  \item{actual}{A nonempty numeric vector of observed responses.}
  \item{predicted}{Predictions with the same length as \code{actual}.}
}
\value{The average absolute prediction error, in the units of the response.}
\examples{
mean_absolute_error(c(1, 2), c(1, 3))
}
