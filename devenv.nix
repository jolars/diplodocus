{
  pkgs,
  ...
}:
let
  python = pkgs.python3.withPackages (pythonPackages: [
    pythonPackages.ipykernel
  ]);
  r = pkgs.rWrapper.override {
    packages = [ pkgs.rPackages.IRkernel ];
  };
  jupyterKernels = pkgs.symlinkJoin {
    name = "diplodocus-jupyter-kernels";
    paths = [
      (pkgs.writeTextDir "kernels/python3/kernel.json" (builtins.toJSON {
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
      }))
      (pkgs.writeTextDir "kernels/ir/kernel.json" (builtins.toJSON {
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
      }))
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

  env.JUPYTER_PATH = jupyterKernels;

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
