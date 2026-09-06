{
  pkgs,
  ...
}:
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

    python.enable = true;
    r.enable = true;
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
