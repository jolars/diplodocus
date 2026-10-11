use std::process::ExitCode;

use clap::Parser;
use diplodocus::commands::{
    self, BuildOptions, CheckOptions, CommandError, ExtractOptions, GenerateOptions, ServeOptions,
};

mod cli;

use cli::{Cli, Command};

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
            live_reload: args.live_reload,
        }),
    }
}
