use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;

use polydoc::commands::{self, BuildOptions, CheckOptions, ServeOptions};

#[test]
fn placeholder_commands_return_structured_errors() {
    let cases = [
        commands::build(BuildOptions {
            config: PathBuf::from("polydoc.toml"),
            output: PathBuf::from("site"),
        }),
        commands::check(CheckOptions {
            config: PathBuf::from("polydoc.toml"),
        }),
        commands::serve(ServeOptions {
            config: PathBuf::from("polydoc.toml"),
            output: PathBuf::from("site"),
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8000,
        }),
    ];

    let messages = cases.map(|result| result.unwrap_err().to_string());
    assert_eq!(
        messages,
        [
            "`polydoc build` is not implemented yet",
            "`polydoc check` is not implemented yet",
            "`polydoc serve` is not implemented yet",
        ]
    );
}
