\name{residuals}
\alias{residuals}
\title{Residuals}
\description{Subtract each prediction from its corresponding observation.}
\usage{residuals(actual, predicted)}
\arguments{
  \item{actual}{A nonempty numeric vector of observed responses.}
  \item{predicted}{Predictions with the same length as \code{actual}.}
}
\value{Signed errors in input order. Positive values indicate underprediction.}
\examples{
residuals(c(1, 2), c(1, 3))
}
