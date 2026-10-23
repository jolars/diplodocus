use std::net::IpAddr;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "diplodocus", version, about = env!("CARGO_PKG_DESCRIPTION"))]
pub(super) struct Cli {
    #[command(subcommand)]
    pub(super) command: Command,
}

#[derive(Debug, Subcommand)]
pub(super) enum Command {
    /// Build a static documentation site.
    Build(BuildArgs),
    /// Check documentation without writing a site.
    Check(CheckArgs),
    /// Extract a portable SQLite documentation snapshot.
    Extract(ExtractArgs),
    /// Generate a site using only a completed snapshot.
    Generate(GenerateArgs),
    /// Build and serve a site, rebuilding when declared inputs change.
    Serve(ServeArgs),
}

#[derive(Debug, Args)]
pub(super) struct BuildArgs {
    /// Path to the workspace configuration file.
    #[arg(long, value_name = "PATH", default_value = "./diplodocus.yaml")]
    pub(super) config: PathBuf,
    /// Directory in which to write the generated site.
    #[arg(long, value_name = "PATH", default_value = "./site")]
    pub(super) output: PathBuf,
}

#[derive(Debug, Args)]
pub(super) struct CheckArgs {
    /// Path to the workspace configuration file.
    #[arg(long, value_name = "PATH", default_value = "./diplodocus.yaml")]
    pub(super) config: PathBuf,
}

#[derive(Debug, Args)]
pub(super) struct ExtractArgs {
    /// Path to the workspace configuration file.
    #[arg(long, value_name = "PATH", default_value = "./diplodocus.yaml")]
    pub(super) config: PathBuf,
    /// Snapshot destination (defaults to .diplodocus/documentation.sqlite beside the configuration).
    #[arg(long, value_name = "PATH")]
    pub(super) output: Option<PathBuf>,
    /// Re-extract changed inputs and emit a JSON line after each published snapshot.
    #[arg(long)]
    pub(super) watch: bool,
}

#[derive(Debug, Args)]
pub(super) struct GenerateArgs {
    /// Completed SQLite documentation snapshot.
    #[arg(long, value_name = "PATH")]
    pub(super) input: PathBuf,
    /// Directory in which to write the generated site.
    #[arg(long, value_name = "PATH", default_value = "./site")]
    pub(super) output: PathBuf,
    /// Output complete site pages or embeddable HTML and Markdown fragments.
    #[arg(long, value_enum, default_value = "site")]
    pub(super) format: GenerateFormat,
    /// Public document and asset URL map (requires --format fragments).
    #[arg(long, value_name = "PATH")]
    pub(super) url_map: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(super) enum GenerateFormat {
    Site,
    Fragments,
}

#[derive(Debug, Args)]
pub(super) struct ServeArgs {
    /// Path to the workspace configuration file.
    #[arg(long, value_name = "PATH", default_value = "./diplodocus.yaml")]
    pub(super) config: PathBuf,
    /// Directory in which to write the generated site.
    #[arg(long, value_name = "PATH", default_value = "./site")]
    pub(super) output: PathBuf,
    /// Address on which to serve the generated site.
    #[arg(long, value_name = "ADDRESS", default_value = "127.0.0.1")]
    pub(super) host: IpAddr,
    /// Port on which to serve the generated site.
    #[arg(long, value_name = "PORT", default_value_t = 8000)]
    pub(super) port: u16,
    /// Refresh open browser pages after successful rebuilds.
    #[arg(long)]
    pub(super) live_reload: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_defaults_match_the_command_contract() {
        let cli = Cli::try_parse_from(["diplodocus", "build"]).unwrap();
        let Command::Build(args) = cli.command else {
            panic!("expected the build command");
        };
        assert_eq!(args.config, PathBuf::from("./diplodocus.yaml"));
        assert_eq!(args.output, PathBuf::from("./site"));
    }

    #[test]
    fn check_defaults_match_the_command_contract() {
        let cli = Cli::try_parse_from(["diplodocus", "check"]).unwrap();
        let Command::Check(args) = cli.command else {
            panic!("expected the check command");
        };
        assert_eq!(args.config, PathBuf::from("./diplodocus.yaml"));
    }

    #[test]
    fn extract_defaults_match_the_command_contract() {
        let cli = Cli::try_parse_from(["diplodocus", "extract"]).unwrap();
        let Command::Extract(args) = cli.command else {
            panic!("expected the extract command");
        };
        assert_eq!(args.config, PathBuf::from("./diplodocus.yaml"));
        assert!(args.output.is_none());
        assert!(!args.watch);
    }

    #[test]
    fn serve_defaults_match_the_command_contract() {
        let cli = Cli::try_parse_from(["diplodocus", "serve"]).unwrap();
        let Command::Serve(args) = cli.command else {
            panic!("expected the serve command");
        };
        assert_eq!(args.config, PathBuf::from("./diplodocus.yaml"));
        assert_eq!(args.output, PathBuf::from("./site"));
        assert_eq!(args.host, IpAddr::from([127, 0, 0, 1]));
        assert_eq!(args.port, 8000);
        assert!(!args.live_reload);
    }
}
