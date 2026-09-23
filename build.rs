#[path = "src/execution/identity/build_metadata.rs"]
mod build_metadata;

fn main() {
    for path in [
        "Cargo.lock",
        "Cargo.toml",
        "build.rs",
        "src/execution/identity/build_metadata.rs",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    let lock = std::fs::read_to_string("Cargo.lock").expect("read locked component graph");
    let version = std::env::var("CARGO_PKG_VERSION").expect("Cargo package version");
    let components = build_metadata::components(&lock, &version)
        .expect("unambiguous locked execution components");
    let mut generated = String::from("pub const COMPONENTS: &[(&str, &str, &str)] = &[\n");
    for (role, name, version) in components {
        generated.push_str(&format!("({role:?}, {name:?}, {version:?}),\n"));
    }
    generated.push_str("];\n");
    for (constant, variable) in [
        ("TARGET", "TARGET"),
        ("OS", "CARGO_CFG_TARGET_OS"),
        ("ARCH", "CARGO_CFG_TARGET_ARCH"),
    ] {
        let value = std::env::var(variable).expect("Cargo target metadata");
        assert!(!value.is_empty(), "empty Cargo target metadata");
        generated.push_str(&format!("pub const {constant}: &str = {value:?};\n"));
    }
    let destination =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"));
    std::fs::write(destination.join("execution_build.rs"), generated)
        .expect("write execution build metadata");
}
