use super::*;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use crate::reporting::{Event, Reporter, Reporting};

/// Command operations with an explicitly scoped progress and diagnostic reporter.
///
/// The default retains the free functions' plain diagnostics and preview notices.
/// Supply a reporter to observe work without changing command options or artifacts.
#[derive(Debug, Clone, Default)]
pub struct CommandRunner {
    pub(super) reporting: Reporting,
}

impl CommandRunner {
    /// Use this reporter for all operations and watch attempts on this runner.
    pub fn with_reporter(reporter: Arc<dyn Reporter>) -> Self {
        Self {
            reporting: Reporting::new(reporter),
        }
    }

    /// Build and atomically publish a documentation site.
    pub fn build(&self, options: BuildOptions) -> Result<(), CommandError> {
        let command = self.reporting.command("build");
        let site = pipeline::runtime()?.block_on(pipeline::build_attempt(
            &options,
            ExecutionDeadlines::default(),
            Box::pin(watching::termination()),
            &self.reporting,
        ))?;
        command.finish(Some(&options.output), Some(site.page_count()));
        Ok(())
    }

    /// Check sources and report successful diagnostics as well as returning them.
    pub fn check(&self, options: CheckOptions) -> Result<CheckReport, CommandError> {
        let command = self.reporting.command("check");
        let report = pipeline::check_attempt(options, &self.reporting)?;
        for diagnostic in &report.diagnostics {
            self.reporting.emit(Event::Diagnostic(diagnostic));
        }
        command.finish(None, None);
        Ok(report)
    }

    /// Extract a snapshot using default execution limits and signal handling.
    pub fn extract(&self, options: ExtractOptions) -> Result<(), CommandError> {
        pipeline::runtime()?.block_on(self.extract_with(
            options,
            ExecutionDeadlines::default(),
            Box::pin(watching::termination()),
        ))?;
        Ok(())
    }

    /// Extract with explicit execution limits and cooperative cancellation.
    pub async fn extract_with(
        &self,
        options: ExtractOptions,
        deadlines: ExecutionDeadlines,
        cancellation: ExecutionCancellation<'_>,
    ) -> Result<PathBuf, CommandError> {
        let command = self.reporting.command("extract");
        let output =
            pipeline::extract_attempt(options, None, deadlines, cancellation, &self.reporting)
                .await?;
        command.finish(Some(&output), None);
        Ok(output)
    }

    /// Watch inputs and write one flushed JSON line per published snapshot.
    pub fn watch_extract(&self, options: ExtractOptions) -> Result<(), CommandError> {
        pipeline::runtime()?.block_on(self.watch_extract_with(
            options,
            ExecutionDeadlines::default(),
            Box::pin(watching::termination()),
            |path| {
                let event =
                    serde_json::json!({"format_version": 1, "event": "snapshot", "path": path});
                let mut output = std::io::stdout().lock();
                writeln!(output, "{event}")?;
                output.flush()
            },
        ))
    }

    /// Watch inputs with explicit limits, cancellation, and publication callback.
    pub async fn watch_extract_with(
        &self,
        options: ExtractOptions,
        deadlines: ExecutionDeadlines,
        cancellation: ExecutionCancellation<'_>,
        on_snapshot: impl FnMut(&Path) -> std::io::Result<()>,
    ) -> Result<(), CommandError> {
        watching::watch_extract_reported(
            options,
            deadlines,
            cancellation,
            on_snapshot,
            &self.reporting,
        )
        .await
    }

    /// Generate a site from a completed snapshot with components disabled.
    pub fn generate(&self, options: GenerateOptions) -> Result<(), CommandError> {
        self.generate_with_components(options, false)
    }

    /// Generate a site with explicitly selected component trust.
    pub fn generate_with_components(
        &self,
        options: GenerateOptions,
        allow_components: bool,
    ) -> Result<(), CommandError> {
        let command = self.reporting.command("generate");
        let site = pipeline::generate_attempt(
            &options.input,
            &options.output,
            true,
            allow_components,
            &self.reporting,
        )?;
        command.finish(Some(&options.output), Some(site.page_count()));
        Ok(())
    }

    /// Generate embeddable fragments with an optional public URL map.
    pub fn generate_fragments(
        &self,
        options: GenerateOptions,
        url_map: Option<PathBuf>,
    ) -> Result<(), CommandError> {
        let command = self.reporting.command("generate");
        let site = pipeline::generate_fragments_attempt(
            &options.input,
            &options.output,
            url_map.as_deref(),
            &self.reporting,
        )?;
        command.finish(Some(&options.output), Some(site.page_count()));
        Ok(())
    }

    /// Build and serve a preview with default limits and signal handling.
    pub fn serve(&self, options: ServeOptions) -> Result<(), CommandError> {
        pipeline::runtime()?.block_on(self.serve_with(
            options,
            ExecutionDeadlines::default(),
            Box::pin(watching::termination()),
        ))
    }

    /// Serve a preview with explicit execution limits and cooperative shutdown.
    pub async fn serve_with(
        &self,
        options: ServeOptions,
        deadlines: ExecutionDeadlines,
        cancellation: ExecutionCancellation<'_>,
    ) -> Result<(), CommandError> {
        preview::serve_reported(options, deadlines, cancellation, &self.reporting).await
    }

    /// Report an error returned to the caller, including cleanup diagnostics.
    ///
    /// Returned errors are not otherwise printed by finite runner operations.
    pub fn report_error(&self, error: &CommandError) {
        self.reporting.error(error);
    }
}
