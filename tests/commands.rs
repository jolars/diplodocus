use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;

use diplodocus::commands::{self, BuildOptions, CheckOptions, ServeOptions};

#[test]
fn placeholder_commands_return_structured_errors() {
    let cases = [
        commands::build(BuildOptions {
            config: PathBuf::from("diplodocus.toml"),
            output: PathBuf::from("site"),
        }),
        commands::check(CheckOptions {
            config: PathBuf::from("diplodocus.toml"),
        }),
        commands::serve(ServeOptions {
            config: PathBuf::from("diplodocus.toml"),
            output: PathBuf::from("site"),
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 8000,
        }),
    ];

    let messages = cases.map(|result| result.unwrap_err().to_string());
    assert_eq!(
        messages,
        [
            "`diplodocus build` is not implemented yet",
            "`diplodocus check` is not implemented yet",
            "`diplodocus serve` is not implemented yet",
        ]
    );
}
