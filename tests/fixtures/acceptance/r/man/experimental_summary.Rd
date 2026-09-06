% This dynamic fragment must produce an unsupported-Rd diagnostic in Polydoc.
\name{experimental_summary}
\alias{experimental_summary}
\title{Experimental Numeric Summary}
\usage{
experimental_summary(x)
}
\arguments{
  \item{x}{A nonempty numeric vector.}
}
\value{
A named numeric vector containing the minimum, median, and maximum.
}
\description{
Compute three summary statistics for a numeric vector.
}
\details{
This page deliberately includes dynamic documentation that static extraction
must not evaluate:
\Sexpr[stage=render,results=text]{paste("computed", "documentation")}
}
\examples{
experimental_summary(c(3, 1, 2))
}
\keyword{univar}
