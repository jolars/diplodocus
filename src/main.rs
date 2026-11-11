use std::process::ExitCode;

use clap::{CommandFactory, FromArgMatches};
use diplodocus::commands::{
    BuildOptions, CheckOptions, CommandError, CommandRunner, ExtractOptions, GenerateOptions,
    ServeOptions,
};
use std::sync::Arc;

mod cli;
mod terminal;

use cli::{Cli, Command, GenerateFormat};

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().collect();
    let color = terminal::early_color(&arguments);
    let matches = Cli::command().color(color).get_matches_from(arguments);
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    let reporter = Arc::new(terminal::ConsoleReporter::new(
        cli.color,
        cli.no_progress,
        cli.quiet,
    ));
    let runner = CommandRunner::with_reporter(reporter);
    match dispatch(cli, &runner) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            runner.report_error(&error);
            ExitCode::FAILURE
        }
    }
}

fn dispatch(cli: Cli, runner: &CommandRunner) -> Result<(), CommandError> {
    match cli.command {
        Command::Build(args) => runner.build(BuildOptions {
            config: args.config,
            output: args.output,
        }),
        Command::Check(args) => runner
            .check(CheckOptions {
                config: args.config,
            })
            .map(|_| ()),
        Command::Extract(args) => {
            let options = ExtractOptions {
                config: args.config,
                output: args.output,
            };
            if args.watch {
                runner.watch_extract(options)
            } else {
                runner.extract(options)
            }
        }
        Command::Generate(args) => {
            let options = GenerateOptions {
                input: args.input,
                output: args.output,
            };
            match args.format {
                GenerateFormat::Site if args.url_map.is_some() => {
                    Err(CommandError::Site(diplodocus::site::SiteError::Route(
                        "--url-map requires --format fragments".into(),
                    )))
                }
                GenerateFormat::Site => {
                    runner.generate_with_components(options, args.allow_components)
                }
                GenerateFormat::Fragments => runner.generate_fragments(options, args.url_map),
            }
        }
        Command::Serve(args) => runner.serve(ServeOptions {
            config: args.config,
            output: args.output,
            host: args.host,
            port: args.port,
            live_reload: args.live_reload,
            open: args.open,
        }),
    }
}
