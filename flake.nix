{
  description = "Native macOS menu bar app for Codex usage limits";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { nixpkgs, ... }:
    let
      system = "aarch64-darwin";
      pkgs = nixpkgs.legacyPackages.${system};
      package = pkgs.rustPlatform.buildRustPackage {
        pname = "codex-menubar";
        version = "0.1.0";
        src = pkgs.lib.fileset.toSource {
          root = ./.;
          fileset = pkgs.lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./Info.plist ./src ];
        };
        cargoLock.lockFile = ./Cargo.lock;
        nativeBuildInputs = [ pkgs.rcodesign ];

        postInstall = ''
          bundle="$out/Applications/Codex Menu Bar.app/Contents"
          mkdir -p "$bundle/MacOS"
          install -m 644 Info.plist "$bundle/Info.plist"
          install -m 755 "$out/bin/codex-menubar" "$bundle/MacOS/codex-menubar"
        '';
        postFixup = ''
          rcodesign sign "$out/Applications/Codex Menu Bar.app"
        '';

        meta.mainProgram = "codex-menubar";
      };
    in
    {
      packages.${system}.default = package;
      apps.${system}.default = {
        type = "app";
        program = "${package}/bin/codex-menubar";
        meta.description = "Show Codex rate limits in the macOS menu bar";
      };
      devShells.${system}.default = pkgs.mkShell {
        packages = with pkgs; [ cargo rustc rustfmt ];
      };
    };
}
