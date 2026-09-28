\name{mean_squared_error}
\alias{mean_squared_error}
\title{Mean Squared Error}
\description{
Compute the mean squared difference between observations and predictions.
}
\usage{
mean_squared_error(actual, predicted)
}
\arguments{
  \item{actual}{A nonempty numeric vector of observed responses.}
  \item{predicted}{Predictions with the same length as \code{actual}.}
}
\value{
The average squared prediction error. Zero indicates a perfect match.
}
\examples{
mean_squared_error(c(1, 2), c(1, 3))
}
