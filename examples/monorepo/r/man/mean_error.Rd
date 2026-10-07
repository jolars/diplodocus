\name{mean_error}
\alias{mean_error}
\title{Mean error}
\description{Compute the average signed prediction error.}
\usage{mean_error(actual, predicted)}
\arguments{
  \item{actual}{A nonempty numeric vector of observed responses.}
  \item{predicted}{Predictions with the same length as \code{actual}.}
}
\value{The mean of actual minus predicted. Positive values indicate underprediction.}
\examples{
mean_error(c(1, 2), c(1, 3))
}
