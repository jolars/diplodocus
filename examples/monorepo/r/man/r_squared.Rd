\name{r_squared}
\alias{r_squared}
\title{R squared}
\description{Compare squared prediction error with a constant prediction at the observed mean.}
\usage{r_squared(actual, predicted)}
\arguments{
  \item{actual}{A nonempty numeric vector of observed responses.}
  \item{predicted}{Predictions with the same length as \code{actual}.}
}
\value{One minus the ratio of residual to total sums of squares. Values can be negative.}
\details{Constant observations produce an error because the total variation is zero.}
\examples{
r_squared(c(1, 2), c(1, 3))
}
