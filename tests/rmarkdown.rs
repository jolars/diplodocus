mod support;

use diplodocus::assembly::assemble_workspace;
use diplodocus::ir::{DocumentFormat, ProvenanceActivity};
use std::fs;
use std::process::{Command, Output};

fn workspace(article: &str) -> support::TestWorkspace {
    let root = support::TestWorkspace::new();
    root.write("diplodocus.toml", "[project]\nname='Articles'\n[[repository]]\nid='r'\npath='r'\n[[package]]\nid='demo'\nname='Demo'\nslug='demo'\necosystem='r'\nrepository='r'\npath='.'\nmetadata-path='DESCRIPTION'\ntargets=[{id='api',extractor='r',path='.',role='public-api'}]\n");
    root.write("r/DESCRIPTION", "Package: demo\nVersion: 0.1.0\nTitle: Demo\nDescription: An article fixture.\nLicense: MIT\nAuthor: Example Author\nMaintainer: Example Author <author@example.com>\nEncoding: UTF-8\n");
    root.write("r/NAMESPACE", "export(answer)\n");
    root.write("r/R/api.R", "answer <- function() 42L\n");
    root.write(
        "r/man/answer.Rd",
        "\\name{answer}\\alias{answer}\\title{Answer}\\usage{answer()}\\description{Answer.}\n",
    );
    root.write("r/vignettes/article.Rmd", article);
    root
}

fn run(root: &support::TestWorkspace, command: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .current_dir(root.path())
        .args([command, "--config", "diplodocus.toml"])
        .output()
        .unwrap()
}
fn success(output: Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn package_data_articles_execute_and_generate_without_the_source_package() {
    let root = workspace(
        r#"---
title: Offline fixture
---
```{r setup, include=FALSE}
library(demo)
input <- system.file("extdata", "mixed.csv", package="demo", mustWork=TRUE)
values <- read.csv(input, colClasses=c("numeric", "character", "character", "logical"), na.strings="")
stopifnot(identical(dim(values), c(5L, 4L)), identical(values$measurement, c(1.5, -0.0, NA_real_, 20, 0)))
```
Retrieved `r nrow(values)` rows and `r ncol(values)` columns from the bundled fixture.

```{r values, results='asis'}
knitr::kable(values)
```
```{r measurements, echo=FALSE, fig.cap='Four observed measurements; the third row is missing.', fig.alt='Four measurements at rows one, two, four, and five.'}
plot(seq_len(nrow(values)), values$measurement)
```
"#,
    );
    root.write(
        "r/inst/extdata/mixed.csv",
        fs::read(support::fixture_path("r-vignettes/offline/mixed.csv")).unwrap(),
    );
    success(run(&root, "check"));
    success(run(&root, "build"));
    let path = root
        .path()
        .join("site/packages/demo/vignettes/article.html");
    let html = fs::read_to_string(&path).unwrap();
    for expected in [
        "Retrieved 5 rows and 4 columns",
        "quoted, value",
        "<table",
        "<figure ",
        "Four observed measurements; the third row is missing.",
    ] {
        assert!(html.contains(expected), "missing {expected}");
    }
    let database = root.path().join(".diplodocus/documentation.sqlite");
    let snapshot = fs::read(&database).unwrap();
    root.write(
        "r/inst/extdata/mixed.csv",
        "measurement,count,label,enabled\n99,1,changed,true\n",
    );
    let output = run(&root, "build");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("R Markdown execution failed"));
    assert_eq!(fs::read(&database).unwrap(), snapshot);
    assert_eq!(fs::read_to_string(&path).unwrap(), html);
    fs::remove_dir_all(root.path().join("r")).unwrap();
    success(
        Command::new(env!("CARGO_BIN_EXE_diplodocus"))
            .current_dir(root.path())
            .env("PATH", "")
            .args([
                "generate",
                "--input",
                ".diplodocus/documentation.sqlite",
                "--output",
                "portable-site",
            ])
            .output()
            .unwrap(),
    );
    assert_eq!(
        fs::read_to_string(
            root.path()
                .join("portable-site/packages/demo/vignettes/article.html")
        )
        .unwrap(),
        html
    );
}

#[test]
fn default_execution_collects_content_and_survives_without_sources_or_r() {
    let root = workspace(
        r#"---
title: An article
author: Example Author
date: "`r 'today'`"
bibliography: references.bib
---
```{r setup, include=FALSE}
library(demo)
x <- answer()
have_results <- FALSE
knitr::opts_chunk$set(collapse=TRUE, comment='#>')
```
The answer is `r x`. Empty `r NULL`. Small `r 1e-20`. Math $x^2$, citation [@smith], and a note^[Useful note.].

$$
x = 42
$$

```{r table, results='asis'}
knitr::kable(data.frame(answer=x))
```
```{r figure, fig.cap='A caption', fig.alt='A useful description'}
plot(1:3)
plot(3:1)
```
```{r illustrative, eval=FALSE}
stop('must not run')
```
```{r conditional, eval=have_results}
stop('must not run either')
```
```{r expected, error=TRUE}
stop('expected example error')
```
- A matrix, $$
  \begin{bmatrix}
  1 & 2 \\
  3 & 4
  \end{bmatrix}
  $$ after the matrix.

## References
"#,
    );
    root.write("r/vignettes/references.bib", "@article{smith, title={Useful research}, author={Smith, Jane}, year={2020}, journal={Research}, doi={10.1234/test}}\n");
    success(run(&root, "build"));
    let snapshot =
        diplodocus::snapshots::Snapshot::load(root.path().join(".diplodocus/documentation.sqlite"))
            .unwrap();
    let article = snapshot
        .workspace()
        .pages
        .values()
        .find(|p| {
            matches!(
                p.document.source_format,
                DocumentFormat::Authored {
                    format: diplodocus::documents::AuthoredFormat::Rmd
                }
            )
        })
        .unwrap();
    let chunks = article
        .document
        .provenance
        .iter()
        .find_map(|p| match &p.activity {
            ProvenanceActivity::RMarkdown { chunks, .. } => Some(chunks),
            _ => None,
        })
        .unwrap();
    assert_eq!(chunks.len(), 6);
    assert_eq!(chunks[0].options["include"], "FALSE");
    assert_eq!(chunks[3].outcome, "illustrative");
    assert_eq!(chunks[4].outcome, "conditional-skip");
    assert_eq!(chunks[5].outcome, "allowed-error");
    let site = root
        .path()
        .join("site/packages/demo/vignettes/article.html");
    let html = fs::read_to_string(&site).unwrap();
    for expected in [
        "The answer is 42",
        "<table",
        "<figcaption>",
        "A caption",
        "A useful description",
        "Useful research",
        "Useful note",
        "katex",
        "Example Author",
        "today",
    ] {
        assert!(
            html.to_lowercase().contains(&expected.to_lowercase()),
            "missing {expected}"
        );
    }
    assert_eq!(html.matches("<figure ").count(), 2);
    assert!(!html.contains("$$"));
    assert!(!html.contains("knitr::opts_chunk"));
    fs::rename(root.path().join("r"), root.path().join("removed-r")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_diplodocus"))
        .current_dir(root.path())
        .env("PATH", "")
        .args([
            "generate",
            "--input",
            ".diplodocus/documentation.sqlite",
            "--output",
            "portable-site",
        ])
        .output()
        .unwrap();
    success(output);
    assert_eq!(
        html,
        fs::read_to_string(
            root.path()
                .join("portable-site/packages/demo/vignettes/article.html")
        )
        .unwrap()
    );
}

#[test]
fn console_output_stays_separate_when_knitr_collapses_chunks() {
    let root = workspace(&support::load_fixture("code-output.Rmd"));
    success(run(&root, "build"));
    let html = fs::read_to_string(
        root.path()
            .join("site/packages/demo/vignettes/article.html"),
    )
    .unwrap();
    let outputs: Vec<_> = html.split("<div class=\"code-output\">").skip(1).collect();
    assert!(outputs.len() > 3);
    for output in &outputs {
        let output = output.split_once("</div>").unwrap().0;
        assert!(output.contains("<pre tabindex=\"0\"><code>"));
        assert!(!output.contains("syntax-"));
    }
    assert!(html.contains("  &lt;result&gt;&amp; &quot;quoted&quot;\n    second line"));
    assert!(html.contains("<pre class=\"syntax-code\""));
    assert!(html.contains("Output from hidden source"));
    let markdown =
        fs::read_to_string(root.path().join("site/packages/demo/vignettes/article.md")).unwrap();
    assert!(markdown.contains("```text\n  <result>& \"quoted\"\n    second line\n```"));
}

#[test]
fn check_is_static_and_explicit_collections_override_discovery() {
    let root =
        workspace("---\ntitle: Custom\n---\n```{r}\nwriteLines('bad','execution-marker')\n```\n");
    let config = fs::read_to_string(root.path().join("diplodocus.toml")).unwrap();
    root.write("diplodocus.toml",format!("{config}\n[[content]]\nid='articles'\nowner='demo'\nrepository='r'\npath='vignettes'\nmount='articles'\nformat='rmd'\n"));
    success(run(&root, "check"));
    assert!(!root.path().join("r/vignettes/execution-marker").exists());
    let sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    assert_eq!(
        sources
            .workspace()
            .pages
            .values()
            .filter(|p| matches!(
                p.document.source_format,
                DocumentFormat::Authored {
                    format: diplodocus::documents::AuthoredFormat::Rmd
                }
            ))
            .count(),
        1
    );
    assert_eq!(
        sources.configuration().content[0].execution.engine,
        Some(diplodocus::configuration::ExecutionEngine::Knitr)
    );
    let resolved = diplodocus::validation::resolve_workspace(&sources).unwrap();
    assert!(diplodocus::snapshots::Snapshot::from_sources(&sources, &resolved).is_err());
    root.write("r/vignettes/support.rds", "changed");
    assert!(sources.revalidate().is_err());
}

#[test]
fn failures_preserve_the_previous_snapshot_and_site() {
    let root = workspace("---\ntitle: Successful\n---\n```{r}\nanswer()\n```\n");
    success(run(&root, "build"));
    let snapshot = fs::read(root.path().join(".diplodocus/documentation.sqlite")).unwrap();
    let site = fs::read(
        root.path()
            .join("site/packages/demo/vignettes/article.html"),
    )
    .unwrap();
    for source in [
        "```{r}\nstop('unexpected')\n```",
        "```{r}\nlibrary(a_missing_vignette_dependency)\n```",
        "```{python}\nprint(1)\n```",
        "```{r child='other.Rmd'}\n```",
        "```{r cache=TRUE}\n1\n```",
        "```{r results='asis'}\ncat('<script>bad()</script>')\n```",
    ] {
        root.write("r/vignettes/article.Rmd", source);
        let output = run(&root, "build");
        assert!(!output.status.success(), "unexpected success for {source}");
        assert_eq!(
            snapshot,
            fs::read(root.path().join(".diplodocus/documentation.sqlite")).unwrap()
        );
        assert_eq!(
            site,
            fs::read(
                root.path()
                    .join("site/packages/demo/vignettes/article.html")
            )
            .unwrap()
        );
    }
}

#[test]
fn local_csl_and_bibliography_errors_are_checked_without_r_execution() {
    let root = workspace(
        "---\ntitle: Citations\nbibliography: references.bib\ncsl: numeric.csl\n---\nCitation [@smith, p. 12].\n\nTerm\n: A definition with $x^2$.\n\n  Value  Meaning\n  -----  -------\n  one    First\n\n  : A simple table caption\n\nA named note[^n].\n\n[^n]: A reusable note.\n",
    );
    root.write(
        "r/vignettes/references.bib",
        "@book{smith,title={Useful research},author={Smith, Jane},year={2020}}\n",
    );
    root.write("r/vignettes/numeric.csl", r#"<style xmlns="http://purl.org/net/xbiblio/csl" version="1.0" class="in-text"><info><title>Test</title><id>https://example.org/test</id></info><citation><layout prefix="[" suffix="]"><text variable="citation-number"/></layout></citation><bibliography><layout><text variable="title"/></layout></bibliography></style>"#);
    success(run(&root, "check"));
    success(run(&root, "build"));
    let html = fs::read_to_string(
        root.path()
            .join("site/packages/demo/vignettes/article.html"),
    )
    .unwrap();
    assert!(html.contains("["));
    assert!(html.contains("Useful research"));
    assert!(html.contains("<dl"));
    assert!(html.contains("reusable note"));
    root.write(
        "r/vignettes/references.bib",
        "@book{other,title={Other},year={2020}}\n",
    );
    let output = run(&root, "check");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Unknown bibliography key"));
    root.write(
        "r/vignettes/references.bib",
        "@book{smith,title={Useful},year={2020}}\n",
    );
    root.write("r/vignettes/numeric.csl", "<invalid>");
    assert!(!run(&root, "check").status.success());
}

#[tokio::test]
async fn native_articles_are_bounded_and_cancellation_stops_the_helper() {
    use diplodocus::execution::{ExecutionCancellation, ExecutionDeadlines};
    let root = workspace("```{r}\nSys.sleep(60)\n```\n");
    let mut sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let mut cancellation: ExecutionCancellation<'_> = Box::pin(std::future::pending());
    let error = sources
        .execute_rmarkdown(
            ExecutionDeadlines {
                cell: 10,
                ..ExecutionDeadlines::default()
            },
            &mut cancellation,
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("diagnostics"));
    let mut sources = assemble_workspace(root.path().join("diplodocus.toml")).unwrap();
    let mut cancellation: ExecutionCancellation<'_> = Box::pin(async {});
    assert!(matches!(
        sources
            .execute_rmarkdown(ExecutionDeadlines::default(), &mut cancellation)
            .await,
        Err(diplodocus::assembly::AssemblyError::Cancelled)
    ));
}

#[test]
fn generated_code_is_inert_and_repeated_figures_keep_their_captions() {
    let root = workspace(
        r####"---
title: Output boundaries
---
## Shared heading

```{r output, results='asis', echo=FALSE}
cat("## Shared heading\n\n```{r}\nwriteLines('bad','generated-marker')\n```\n")
```
```{r figures, echo=FALSE, fig.cap=c('First caption','Second caption')}
image <- tempfile(fileext=".png")
png(image)
plot(1:3)
dev.off()
knitr::include_graphics(c(image,image))
```
"####,
    );
    success(run(&root, "build"));
    assert!(!root.path().join("r/vignettes/generated-marker").exists());
    let html = fs::read_to_string(
        root.path()
            .join("site/packages/demo/vignettes/article.html"),
    )
    .unwrap();
    assert!(html.contains("generated-marker"));
    assert!(!html.contains("id=\"shared-heading-1\""));
    assert!(html.contains("First caption"));
    assert!(html.contains("Second caption"));
    assert_eq!(html.matches("<figure ").count(), 2);
}
