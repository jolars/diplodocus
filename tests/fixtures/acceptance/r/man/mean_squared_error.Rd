\name{mean_squared_error}
\alias{mean_squared_error}
\title{Mean Squared Error}
\usage{
mean_squared_error(actual, predicted)
}
\arguments{
  \item{actual}{A nonempty numeric vector of observed responses.}
  \item{predicted}{A numeric vector of predictions with the same length as
    \code{actual}.}
}
\value{
One numeric mean squared error.
}
\description{
Compute the mean squared difference between observed and predicted responses.
}
\examples{
mean_squared_error(c(1, 2), c(1, 3))
}
\keyword{models}
