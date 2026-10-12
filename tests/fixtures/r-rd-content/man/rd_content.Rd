\name{rd_content}
\alias{rd_content}
\alias{euler}
\title{Structured R documentation}
\usage{rd_content(x)}
\arguments{\item{x}{A value.}}
\description{Local \code{\link[=rd_content]{rd_content()}} and \link{euler} links,
\link[base]{data.frame}, \link[grid:gpar]{graphical parameters}, and \link{as.numeric}.
\if{html}{HTML-only content.}
\ifelse{text}{Text-only content.}{HTML alternative.}
}
\value{
A list object of class \code{'euler'} with the following parameters.
\item{shapes}{a data frame of fitted shape parameters. One row per set
with a \code{type} column (one of \code{"circle"}, \code{"ellipse"}, \code{"rectangle"},
\code{"square"}, \code{"rotated_rectangle"}), the center coordinates \code{h} and \code{k},
and the shape-specific columns: \code{a}, \code{b}, \code{phi} for ellipses/circles;
\code{width} and \code{height} for rectangles; \code{side} (plus mirrored
\code{width}/\code{height}) for squares; \code{width}, \code{height}, and \code{phi} (rotation,
in radians) for rotated rectangles. Columns that don't apply to the
chosen shape are \code{NA}.}
\item{ellipses}{for \code{shape = "circle"} and \code{shape = "ellipse"} fits,
the legacy 5-column data frame of \code{h}, \code{k}, \code{a}, \code{b}, \code{phi}. This
slot is deprecated in favour of \code{shapes} and is not populated for
rectangle/square fits.}
\item{original.values}{set relationships in the input}
\item{fitted.values}{set relationships in the solution}
\item{residuals}{residuals}
\item{regionError}{the difference in percentage points between each
disjoint subset in the input and the respective area in the output}
\item{diagError}{the largest \code{regionError}}
\item{stress}{normalized residual sums of squares}
}
\details{
If the input is a matrix or data frame and argument \code{by} is specified,
the function returns a list of euler diagrams.

The function minimizes the residual sums of squares,
\deqn{
  \sum_{i=1}^n (A_i - \omega_i)^2,
}{
  \sum (A_i - \omega_i)^2,
}
by default, where \eqn{\omega_i} the size of the ith disjoint subset, and
\eqn{A_i} the corresponding area in the diagram, that is, the unique
contribution to the total area from this overlap. The loss function
can, however, be controlled via the \code{loss} argument.

\code{\link[=euler]{euler()}} also returns \code{stress} (from \pkg{venneuler}), as well as
\code{diagError}, and \code{regionError} from \pkg{eulerAPE}.

The \emph{stress} statistic is computed as

\deqn{
  \frac{\sum_{i=1}^n (A_i - \beta\omega_i)^2}{\sum_{i=1}^n A_i^2},
}{
  \sum (A_i - \beta\omega_i)^2 / \sum A_i^2,
}
where
\deqn{
  \beta = \sum_{i=1}^n A_i\omega_i / \sum_{i=1}^n \omega_i^2.
}{
  \beta = \sum A_i\omega_i / \sum \omega_i^2.
}

\code{regionError} is computed as

\deqn{
  \left| \frac{A_i}{\sum_{i=1}^n A_i} - \frac{\omega_i}{\sum_{i=1}^n \omega_i}\right|.
}{
  max|A_i / \sum A  - \omega_i / \sum \omega|.
}

\code{diagError} is simply the maximum of regionError.
}
\section{Graphical parameters}{
\tabular{lcccccccc}{
\tab fills \tab edges \tab labels \tab quantities  \tab annotations \tab strips \tab legend \tab main \cr
col        \tab       \tab x     \tab x      \tab x           \tab x           \tab x      \tab x      \tab x    \cr
fill       \tab x     \tab       \tab        \tab             \tab             \tab        \tab        \tab      \cr
alpha      \tab x     \tab x     \tab x      \tab x           \tab x           \tab x      \tab x      \tab x    \cr
lty        \tab       \tab x     \tab        \tab             \tab             \tab        \tab        \tab      \cr
lwd        \tab       \tab x     \tab        \tab             \tab             \tab        \tab        \tab      \cr
lex        \tab       \tab x     \tab        \tab             \tab             \tab        \tab        \tab      \cr
fontsize   \tab       \tab       \tab x      \tab x           \tab x           \tab x      \tab x      \tab x    \cr
cex        \tab       \tab       \tab x      \tab x           \tab x           \tab x      \tab x      \tab x    \cr
fontfamily \tab       \tab       \tab x      \tab x           \tab x           \tab x      \tab x      \tab x    \cr
lineheight \tab       \tab       \tab x      \tab x           \tab x           \tab x      \tab x      \tab x    \cr
font       \tab       \tab       \tab x      \tab x           \tab x           \tab x      \tab x      \tab x    \cr
}
\subsection{Links}{See \code{\link[grid:unit]{grid::unit()}}.}
}
\references{\doi{10.1109/TVCG.2011.56} and \doi{10.1371/journal.pone.0101717}.}
