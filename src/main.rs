use std::process::ExitCode;

use clap::{CommandFactory, FromArgMatches};
use diplodocus::commands::{
    BuildOptions, BundleExportOptions, CheckOptions, CommandError, CommandRunner, ExtractOptions,
    GenerateOptions, ServeOptions,
};
use std::sync::Arc;

mod cli;
mod terminal;

use cli::{BundleCommand, Cli, Command, GenerateFormat};

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
    let no_cache = match &cli.command {
        Command::Build(args) => args.no_cache,
        Command::Check(args) => args.no_cache,
        Command::Extract(args) => args.no_cache,
        Command::Serve(args) => args.no_cache,
        Command::Generate(_) => false,
        Command::Bundle {
            command: BundleCommand::Export { no_cache, .. },
        } => *no_cache,
    };
    let runner = runner.clone().with_cache_policy(if no_cache {
        diplodocus::incremental::CachePolicy::Bypass
    } else {
        diplodocus::incremental::CachePolicy::Auto
    });
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
        Command::Bundle {
            command: BundleCommand::Export { config, output, .. },
        } => runner.export_bundle(BundleExportOptions { config, output }),
        Command::Generate(args) => {
            if matches!(args.format, GenerateFormat::Inventory) {
                if args.output.is_some()
                    || args.url_map.is_some()
                    || args.allow_components
                    || args.allow_site_scripts
                {
                    return Err(diplodocus::site::SiteError::Route(
                        "--format inventory writes to stdout and does not accept --output, --url-map, --allow-components, or --allow-site-scripts".into(),
                    ).into());
                }
                return runner.write_fragment_inventory(&args.input, std::io::stdout().lock());
            }
            let options = GenerateOptions {
                input: args.input,
                output: args.output.unwrap_or_else(|| "./site".into()),
            };
            match args.format {
                GenerateFormat::Inventory => unreachable!(),
                GenerateFormat::Site if args.url_map.is_some() => {
                    Err(CommandError::Site(diplodocus::site::SiteError::Route(
                        "--url-map requires --format fragments".into(),
                    )))
                }
                GenerateFormat::Site => runner.generate_with_trust(
                    options,
                    diplodocus::commands::GenerateTrust {
                        allow_components: args.allow_components,
                        allow_site_scripts: args.allow_site_scripts,
                    },
                ),
                GenerateFormat::Fragments if args.allow_site_scripts => Err(CommandError::Site(
                    diplodocus::site::SiteError::Presentation(
                        "--allow-site-scripts requires --format site".into(),
                    ),
                )),
                GenerateFormat::Fragments => runner.generate_fragments(options, args.url_map),
            }
        }
        Command::Serve(args) => runner.serve(ServeOptions {
            config: args.config,
            output: args.output,
            host: args.host,
            port: args.port,
            live_reload: args.live_reload && !args.no_live_reload,
            open: args.open,
        }),
    }
}
