{
  config,
  lib,
  pkgs,
  ...
}:
let
  browserTools = builtins.fromJSON (builtins.readFile ./package.json);
  browserBinaries =
    assert lib.assertMsg
      (browserTools.devDependencies."@playwright/test" == pkgs.playwright-driver.version)
      "Update @playwright/test and its CLI overrides to match nixpkgs playwright-driver, then regenerate package-lock.json.";
    pkgs.playwright-driver.browsers-chromium;
  browserFonts = pkgs.writeText "browser-fonts.conf" ''
    <?xml version="1.0"?>
    <!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
    <fontconfig>
      <dir>${pkgs.dejavu_fonts}/share/fonts/truetype</dir>
      <cachedir prefix="xdg">diplodocus/fontconfig</cachedir>
      <include>${pkgs.fontconfig.out}/share/fontconfig/conf.avail/49-sansserif.conf</include>
      <alias><family>system-ui</family><prefer><family>DejaVu Sans</family></prefer></alias>
      <alias><family>sans-serif</family><prefer><family>DejaVu Sans</family></prefer></alias>
      <alias><family>serif</family><prefer><family>DejaVu Serif</family></prefer></alias>
      <alias><family>monospace</family><prefer><family>DejaVu Sans Mono</family></prefer></alias>
    </fontconfig>
  '';
  python = pkgs.python3.withPackages (pythonPackages: [
    pythonPackages.ipykernel
  ]);
  r = pkgs.rWrapper.override {
    packages = [ pkgs.rPackages.IRkernel ];
  };
  jupyterKernels = pkgs.symlinkJoin {
    name = "diplodocus-jupyter-kernels";
    paths = [
      (pkgs.writeTextDir "kernels/python3/kernel.json" (
        builtins.toJSON {
          argv = [
            "${python}/bin/python"
            "-m"
            "ipykernel_launcher"
            "-f"
            "{connection_file}"
          ];
          display_name = "Python 3";
          language = "python";
          interrupt_mode = "signal";
        }
      ))
      (pkgs.writeTextDir "kernels/ir/kernel.json" (
        builtins.toJSON {
          argv = [
            "${r}/bin/R"
            "--slave"
            "-e"
            "IRkernel::main()"
            "--args"
            "{connection_file}"
          ];
          display_name = "R";
          language = "R";
          interrupt_mode = "signal";
        }
      ))
    ];
  };
in
{
  packages = with pkgs; [
    actionlint
    cargo-audit
    cargo-deny
    cargo-llvm-cov
    cargo-msrv
    nodejs
    go-task
  ];

  languages = {
    rust = {
      enable = true;
      toolchainFile = ./rust-toolchain.toml;
    };

    python = {
      enable = true;
      package = python;
    };

    r = {
      enable = true;
      package = r;
    };
  };

  env = {
    JUPYTER_PATH = jupyterKernels;
    PLAYWRIGHT_BROWSERS_PATH = browserBinaries;
    PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD = "1";
    PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS = "true";
    DIPLODOCUS_BROWSER_FONTCONFIG_FILE = browserFonts;
  };

  scripts =
    lib.genAttrs [ "site-dev" "site-test" "site-capture" ] (name: {
      exec = ''
        cd ${lib.escapeShellArg config.devenv.root}
        exec npm run --silent ${name} -- "$@"
      '';
    })
    // {
      playwright-cli.exec = ''
        cd ${lib.escapeShellArg config.devenv.root}
        exec node scripts/browser.mjs "$@"
      '';
    };

  git-hooks.hooks = {
    clippy = {
      enable = true;
      settings = {
        allFeatures = true;
        denyWarnings = true;
        extraArgs = "--all-targets";
      };
    };

    rustfmt.enable = true;
  };
}
