use std::fmt::Write;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

// Devicon v2.17.0, with its license included in the emitted stylesheet.
const COLORED: &[(&str, &[u8])] = &[
    ("cpp", include_bytes!("assets/tab-icons/cpp.svg")),
    ("julia", include_bytes!("assets/tab-icons/julia.svg")),
    ("python", include_bytes!("assets/tab-icons/python.svg")),
    ("r", include_bytes!("assets/tab-icons/r.svg")),
    ("windows", include_bytes!("assets/tab-icons/windows.svg")),
];

const MONOCHROME: &[(&str, &[u8])] = &[
    ("linux", include_bytes!("assets/tab-icons/linux.svg")),
    ("macos", include_bytes!("assets/tab-icons/macos.svg")),
    ("rust", include_bytes!("assets/tab-icons/rust.svg")),
];

pub(super) fn stylesheet() -> String {
    let mut css = String::from(concat!(
        "\n/*! Devicon v2.17.0 (https://github.com/devicons/devicon)\n",
        include_str!("assets/tab-icons/LICENSE"),
        "*/\n",
        include_str!("assets/styles/tab-icons.css"),
    ));
    for (icons, property) in [
        (COLORED, "--tab-icon-background"),
        (MONOCHROME, "--tab-icon-mask"),
    ] {
        for (name, svg) in icons {
            writeln!(
                css,
                r#".tabset-controls > button[data-tab-icon="{name}"]::before {{ content: ""; {property}: url("data:image/svg+xml;base64,{}"); }}"#,
                STANDARD.encode(svg),
            )
            .unwrap();
        }
    }
    css
}
