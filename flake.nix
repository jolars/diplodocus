{
  description = "Documentation websites for polyglot software projects";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
    }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      packageFor =
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          manifest = builtins.fromTOML (builtins.readFile ./Cargo.toml);
          toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = toolchain;
            rustc = toolchain;
          };
        in
        rustPlatform.buildRustPackage {
          pname = manifest.package.name;
          inherit (manifest.package) version;

          src = pkgs.lib.fileset.toSource {
            root = ./.;
            fileset = pkgs.lib.fileset.unions [
              ./Cargo.lock
              ./Cargo.toml
              ./build.rs
              ./docs/cli-intro.md
              ./src
            ];
          };
          cargoLock.lockFile = ./Cargo.lock;

          # Execution tests need the prepared language environments supplied by devenv.
          doCheck = false;
          doInstallCheck = true;
          installCheckPhase = ''
            runHook preInstallCheck
            "$out/bin/diplodocus" --version
            "$out/bin/diplodocus" --help > /dev/null
            runHook postInstallCheck
          '';

          meta = {
            inherit (manifest.package) description;
            homepage = manifest.package.homepage;
            license = with pkgs.lib.licenses; [
              asl20
              mit
            ];
            mainProgram = "diplodocus";
            platforms = systems;
          };
        };
    in
    {
      packages = forAllSystems (
        system:
        let
          diplodocus = packageFor system;
        in
        {
          inherit diplodocus;
          default = diplodocus;
        }
      );

      apps = forAllSystems (
        system:
        let
          app = {
            type = "app";
            program = "${self.packages.${system}.diplodocus}/bin/diplodocus";
            meta.description = "Run Diplodocus";
          };
        in
        {
          diplodocus = app;
          default = app;
        }
      );

      checks = forAllSystems (system: {
        default = self.packages.${system}.diplodocus;
      });

      formatter = forAllSystems (system: nixpkgs.legacyPackages.${system}.nixfmt);
    };
}
