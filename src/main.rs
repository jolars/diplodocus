use std::net::IpAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use diplodocus::commands::{
    self, BuildOptions, CheckOptions, CommandError, ExtractOptions, GenerateOptions, ServeOptions,
};

#[derive(Debug, Parser)]
#[command(name = "diplodocus", version, about = env!("CARGO_PKG_DESCRIPTION"))]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
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
struct BuildArgs {
    /// Path to the workspace configuration file.
    #[arg(long, value_name = "PATH", default_value = "./diplodocus.toml")]
    config: PathBuf,
    /// Directory in which to write the generated site.
    #[arg(long, value_name = "PATH", default_value = "./site")]
    output: PathBuf,
}

#[derive(Debug, Args)]
struct CheckArgs {
    /// Path to the workspace configuration file.
    #[arg(long, value_name = "PATH", default_value = "./diplodocus.toml")]
    config: PathBuf,
}

#[derive(Debug, Args)]
struct ExtractArgs {
    /// Path to the workspace configuration file.
    #[arg(long, value_name = "PATH", default_value = "./diplodocus.toml")]
    config: PathBuf,
    /// Snapshot destination (defaults to .diplodocus/documentation.sqlite beside the configuration).
    #[arg(long, value_name = "PATH")]
    output: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct GenerateArgs {
    /// Completed SQLite documentation snapshot.
    #[arg(long, value_name = "PATH")]
    input: PathBuf,
    /// Directory in which to write the generated site.
    #[arg(long, value_name = "PATH", default_value = "./site")]
    output: PathBuf,
}

#[derive(Debug, Args)]
struct ServeArgs {
    /// Path to the workspace configuration file.
    #[arg(long, value_name = "PATH", default_value = "./diplodocus.toml")]
    config: PathBuf,
    /// Directory in which to write the generated site.
    #[arg(long, value_name = "PATH", default_value = "./site")]
    output: PathBuf,
    /// Address on which to serve the generated site.
    #[arg(long, value_name = "ADDRESS", default_value = "127.0.0.1")]
    host: IpAddr,
    /// Port on which to serve the generated site.
    #[arg(long, value_name = "PORT", default_value_t = 8000)]
    port: u16,
}

fn main() -> ExitCode {
    match dispatch(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            for diagnostic in error
                .diagnostics()
                .iter()
                .chain(error.cleanup_diagnostics())
            {
                eprintln!("{}", commands::format_diagnostic(diagnostic));
            }
            ExitCode::FAILURE
        }
    }
}

fn dispatch(cli: Cli) -> Result<(), CommandError> {
    match cli.command {
        Command::Build(args) => commands::build(BuildOptions {
            config: args.config,
            output: args.output,
        }),
        Command::Check(args) => commands::check(CheckOptions {
            config: args.config,
        })
        .map(|report| {
            for diagnostic in &report.diagnostics {
                eprintln!("{}", commands::format_diagnostic(diagnostic));
            }
        }),
        Command::Extract(args) => commands::extract(ExtractOptions {
            config: args.config,
            output: args.output,
        }),
        Command::Generate(args) => commands::generate(GenerateOptions {
            input: args.input,
            output: args.output,
        }),
        Command::Serve(args) => commands::serve(ServeOptions {
            config: args.config,
            output: args.output,
            host: args.host,
            port: args.port,
        }),
    }
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
        assert_eq!(args.config, PathBuf::from("./diplodocus.toml"));
        assert_eq!(args.output, PathBuf::from("./site"));
    }

    #[test]
    fn check_defaults_match_the_command_contract() {
        let cli = Cli::try_parse_from(["diplodocus", "check"]).unwrap();
        let Command::Check(args) = cli.command else {
            panic!("expected the check command");
        };
        assert_eq!(args.config, PathBuf::from("./diplodocus.toml"));
    }

    #[test]
    fn serve_defaults_match_the_command_contract() {
        let cli = Cli::try_parse_from(["diplodocus", "serve"]).unwrap();
        let Command::Serve(args) = cli.command else {
            panic!("expected the serve command");
        };
        assert_eq!(args.config, PathBuf::from("./diplodocus.toml"));
        assert_eq!(args.output, PathBuf::from("./site"));
        assert_eq!(args.host, IpAddr::from([127, 0, 0, 1]));
        assert_eq!(args.port, 8000);
    }
}
