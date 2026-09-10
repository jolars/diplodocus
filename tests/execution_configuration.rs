use std::path::PathBuf;

use diplodocus::configuration::{
    ConfigurationError, ContentConfiguration, ExecutionConfigurationError, ExecutionMode,
    load_configuration, parse_configuration,
};
use diplodocus::documents::AuthoredFormat;

mod support;

fn configuration(format: &str, execution: &str) -> String {
    format!(
        "[project]\nname = 'Execution tests'\n\n\
         [[repository]]\nid = 'source'\npath = '../unavailable-checkout'\n\n\
         [[content]]\nid = 'examples'\nowner = 'project'\nrepository = 'source'\n\
         path = 'docs'\nmount = 'examples'\nformat = '{format}'\n{execution}"
    )
}

fn execution(settings: &str) -> String {
    format!("[content.execution]\n{settings}")
}

fn assert_rejected(source: &str, message: &str) {
    let error = parse_configuration(source).unwrap_err();
    assert!(error.message().contains(message), "{error}");
    assert!(error.message().contains("examples"), "{error}");
    let span = error.span().expect("configuration source range");
    assert!(
        span.start < span.end && span.end <= source.len(),
        "{span:?}"
    );
}

#[test]
fn every_collection_requires_a_supported_explicit_format() {
    for format in ["gfm", "qmd"] {
        parse_configuration(&configuration(format, "")).unwrap();
    }
    let missing = configuration("qmd", "").replace("format = 'qmd'\n", "");
    assert!(
        parse_configuration(&missing)
            .unwrap_err()
            .message()
            .contains("missing field `format`")
    );
    for format in ["", "markdown", "quarto", "GFM", "QMD"] {
        assert!(
            parse_configuration(&configuration(format, "")).is_err(),
            "{format}"
        );
    }
}

#[test]
fn omitted_empty_and_explicit_never_settings_have_the_same_serialized_defaults() {
    for format in ["gfm", "qmd"] {
        let sources = [
            "",
            "[content.execution]\n",
            "[content.execution]\nmode = 'never'\n",
            "[content.execution]\nmode = 'never'\ndeclared_environment_inputs = []\n",
        ];
        let expected = parse_configuration(&configuration(format, "")).unwrap();
        for settings in sources {
            let config = parse_configuration(&configuration(format, settings)).unwrap();
            assert_eq!(config, expected);
            let content = &config.content[0];
            assert_eq!(content.execution.mode, ExecutionMode::Never);
            assert!(content.execution.engine.is_none());
            assert!(content.execution.kernel.is_none());
            assert!(content.execution.declared_environment_inputs.is_empty());
            let serialized = toml::to_string(&config).unwrap();
            let value: toml::Value = toml::from_str(&serialized).unwrap();
            assert_eq!(
                value["content"][0]["execution"],
                toml::Value::Table(toml::from_str("mode = 'never'").unwrap())
            );
            assert_eq!(parse_configuration(&serialized).unwrap(), expected);
        }
    }
}

#[test]
fn execute_requires_both_an_explicit_jupyter_engine_and_kernel() {
    for (settings, message) in [
        ("mode = 'execute'\n", "requires `engine = \"jupyter\"`"),
        (
            "mode = 'execute'\nkernel = 'python3'\n",
            "requires `engine = \"jupyter\"`",
        ),
        (
            "mode = 'execute'\nengine = 'jupyter'\n",
            "requires an explicit kernel",
        ),
    ] {
        assert_rejected(&configuration("qmd", &execution(settings)), message);
    }
    for (key, value) in [("mode", "auto"), ("engine", "knitr"), ("engine", "Jupyter")] {
        let settings = "mode = 'execute'\nengine = 'jupyter'\nkernel = 'python3'\n".replace(
            &format!(
                "{key} = '{}'",
                if key == "mode" { "execute" } else { "jupyter" }
            ),
            &format!("{key} = '{value}'"),
        );
        assert!(parse_configuration(&configuration("qmd", &execution(&settings))).is_err());
    }
}

#[test]
fn never_rejects_execution_settings_even_when_mode_is_omitted() {
    for mode in ["", "mode = 'never'\n"] {
        for (setting, field) in [
            ("engine = 'jupyter'\n", "engine"),
            ("kernel = 'python3'\n", "kernel"),
            (
                "declared_environment_inputs = ['uv.lock']\n",
                "declared_environment_inputs",
            ),
        ] {
            for format in ["gfm", "qmd"] {
                assert_rejected(
                    &configuration(format, &execution(&format!("{mode}{setting}"))),
                    &format!("`{field}` requires `mode = \"execute\"`"),
                );
            }
        }
    }
}

#[test]
fn gfm_cannot_select_execute_even_with_a_complete_jupyter_configuration() {
    assert_rejected(
        &configuration(
            "gfm",
            &execution("mode = 'execute'\nengine = 'jupyter'\nkernel = 'python3'\n"),
        ),
        "requires `format = \"qmd\"`",
    );
}

#[test]
fn kernel_selectors_follow_the_contract_without_requiring_an_installed_kernel() {
    for kernel in [
        "python3",
        "ir",
        "Python3",
        "MY_kernel-4.2",
        "uninstalled-kernel",
        "...",
        "_",
    ] {
        let source = configuration(
            "qmd",
            &execution(&format!(
                "mode = 'execute'\nengine = 'jupyter'\nkernel = '{kernel}'\n"
            )),
        );
        let config = parse_configuration(&source).unwrap();
        assert_eq!(config.content[0].execution.kernel.as_deref(), Some(kernel));
        assert_eq!(
            parse_configuration(&toml::to_string(&config).unwrap()).unwrap(),
            config
        );
    }
    for kernel in [
        "",
        " ",
        ".",
        "..",
        "../python3",
        "/python3",
        "kernels/python3",
        "C:\\python3",
        "python 3",
        "python3\n",
        "pythön",
        "python$3",
        "python:3",
    ] {
        let kernel = toml::Value::String(kernel.to_owned());
        let source = configuration(
            "qmd",
            &execution(&format!(
                "mode = 'execute'\nengine = 'jupyter'\nkernel = {kernel}\n"
            )),
        );
        assert_rejected(&source, "invalid kernel selector");
    }
}

#[test]
fn environment_inputs_are_optional_explicit_file_declarations() {
    for inputs in [
        "",
        "declared_environment_inputs = []\n",
        "declared_environment_inputs = ['uv.lock', './environments//docs.toml', 'locks/../renv.lock', 'environment manifests/研究.lock']\n",
    ] {
        let source = configuration(
            "qmd",
            &execution(&format!(
                "mode = 'execute'\nengine = 'jupyter'\nkernel = 'python3'\n{inputs}"
            )),
        );
        let config = parse_configuration(&source).unwrap();
        assert_eq!(
            parse_configuration(&toml::to_string(&config).unwrap()).unwrap(),
            config
        );
        if !config.content[0]
            .execution
            .declared_environment_inputs
            .is_empty()
        {
            assert_eq!(
                config.content[0].execution.declared_environment_inputs[1],
                PathBuf::from("./environments//docs.toml")
            );
        }
    }
}

#[test]
fn environment_inputs_reject_invalid_paths_and_duplicate_normalized_declarations() {
    for path in [
        "",
        ".",
        "./",
        "locks/..",
        "../uv.lock",
        "locks/../../uv.lock",
        "/etc/environment",
        "C:/environment.lock",
        "C:\\environment.lock",
        "\\\\server\\environment.lock",
        "*.lock",
        "locks/?.lock",
        "locks/[ab].lock",
        "locks/{a,b}.lock",
        "locks/",
        "locks/.",
        "bad\0.lock",
    ] {
        let path = toml::Value::String(path.to_owned());
        let source = configuration(
            "qmd",
            &execution(&format!(
                "mode = 'execute'\nengine = 'jupyter'\nkernel = 'python3'\ndeclared_environment_inputs = [{path}]\n"
            )),
        );
        assert_rejected(&source, "invalid `declared_environment_inputs[0]`");
    }
    for inputs in [
        "['uv.lock', 'uv.lock']",
        "['./uv.lock', 'uv.lock']",
        "['env//uv.lock', 'env/./uv.lock']",
        "['env/../uv.lock', 'uv.lock']",
    ] {
        let source = configuration(
            "qmd",
            &execution(&format!(
                "mode = 'execute'\nengine = 'jupyter'\nkernel = 'python3'\ndeclared_environment_inputs = {inputs}\n"
            )),
        );
        assert_rejected(
            &source,
            "`declared_environment_inputs[1]` duplicates input 0",
        );
    }
}

#[test]
fn every_content_collection_is_validated_in_source_order() {
    let source = configuration("qmd", "");
    let second = configuration("qmd", &execution("mode = 'execute'\n"));
    let second = second
        .split_once("[[content]]")
        .unwrap()
        .1
        .replace("id = 'examples'", "id = 'second'");
    let error = parse_configuration(&format!("{source}\n[[content]]{second}")).unwrap_err();
    assert!(error.message().contains("second"), "{error}");
    assert!(error.message().contains("requires `engine"), "{error}");
}

#[test]
fn direct_deserialization_and_loading_enforce_the_same_collection_rules() {
    let source = configuration("qmd", &execution("mode = 'execute'\nengine = 'jupyter'\n"));
    let content_source = source
        .split_once("[[content]]")
        .unwrap()
        .1
        .replace("[content.execution]", "[execution]");
    assert!(toml::from_str::<ContentConfiguration>(&content_source).is_err());

    let workspace = support::TestWorkspace::new();
    workspace.write("workspace.toml", source);
    let path = workspace.path().join("workspace.toml");
    match load_configuration(&path).unwrap_err() {
        ConfigurationError::Parse {
            path: error_path,
            source,
        } => {
            assert_eq!(error_path, path);
            assert!(source.message().contains("requires an explicit kernel"));
            assert!(source.span().is_some());
        }
        error => panic!("expected configuration error, got {error}"),
    }
    workspace.write("workspace.toml", configuration("qmd", &execution("mode = 'execute'\nengine = 'jupyter'\nkernel = 'uninstalled-kernel'\ndeclared_environment_inputs = ['missing.lock']\n")));
    let parsed = load_configuration(&path).unwrap();
    assert_eq!(parsed.content[0].format, AuthoredFormat::Qmd);
    assert_eq!(
        support::files_under(workspace.path()),
        [PathBuf::from("workspace.toml")]
    );
}

#[test]
fn programmatically_modified_collections_can_be_revalidated() {
    let mut config = parse_configuration(&configuration(
        "qmd",
        &execution("mode = 'execute'\nengine = 'jupyter'\nkernel = 'python3'\n"),
    ))
    .unwrap();
    let collection = &mut config.content[0];
    assert_eq!(collection.validate_execution(), Ok(()));
    collection.execution.kernel = None;
    assert_eq!(
        collection.validate_execution(),
        Err(ExecutionConfigurationError::MissingKernel)
    );
    collection.format = AuthoredFormat::Gfm;
    assert_eq!(
        collection.validate_execution(),
        Err(ExecutionConfigurationError::GfmExecution)
    );
}

#[cfg(unix)]
#[test]
fn programmatic_environment_inputs_must_be_utf8() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let mut config = parse_configuration(&configuration(
        "qmd",
        &execution("mode = 'execute'\nengine = 'jupyter'\nkernel = 'python3'\n"),
    ))
    .unwrap();
    config.content[0].execution.declared_environment_inputs =
        vec![PathBuf::from(OsString::from_vec(vec![0xff]))];
    assert!(matches!(
        config.content[0].validate_execution(),
        Err(ExecutionConfigurationError::InvalidEnvironmentInput { index: 0, .. })
    ));
}
