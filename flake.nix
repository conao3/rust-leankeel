{
  description = "Leankeel: a linter and verified fixer for Lean 4";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    leanfran.url = "github:conao3/lean-leanfran/1bb464c6d419da8037698053f52dd23476a042d9";
    leanfran.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    inputs:
    inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [ inputs.treefmt-nix.flakeModule ];
      systems = [
        "x86_64-linux"
        "aarch64-darwin"
      ];
      perSystem =
        { pkgs, system, ... }:
        let
          adapter = pkgs.stdenv.mkDerivation {
            pname = "leankeel-adapter";
            version = "0.1.0";
            src = ./native;
            nativeBuildInputs = [
              pkgs.lean4
              pkgs.makeWrapper
            ];
            buildPhase = "export HOME=$TMPDIR; lake build";
            installPhase = ''
              install -Dm755 .lake/build/bin/leankeel-adapter $out/bin/leankeel-adapter
              wrapProgram $out/bin/leankeel-adapter --set LEAN_SYSROOT ${pkgs.lean4}
            '';
          };
        in
        {
          packages.adapter = adapter;
          packages.default = pkgs.rustPlatform.buildRustPackage {
            pname = "leankeel";
            version = "0.1.0";
            src = pkgs.lib.cleanSource ./.;
            cargoLock.lockFile = ./Cargo.lock;
            nativeBuildInputs = [ pkgs.makeWrapper ];
            nativeCheckInputs = [ pkgs.lean4 ];
            LEANFRAN_OFFLINE = "1";
            LEANKEEL_ADAPTER = "${adapter}/bin/leankeel-adapter";
            postInstall = ''
              wrapProgram $out/bin/leankeel \
                --set LEANKEEL_ADAPTER ${adapter}/bin/leankeel-adapter \
                --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.lean4 ]}
            '';
            meta = {
              mainProgram = "leankeel";
              license = pkgs.lib.licenses.asl20;
              platforms = pkgs.lib.platforms.unix;
            };
          };
          devShells.default = pkgs.mkShell {
            packages = [
              pkgs.cargo
              pkgs.rustc
              pkgs.rustfmt
              pkgs.clippy
              pkgs.rust-analyzer
              pkgs.gcc
              pkgs.lean4
              pkgs.gnumake
              pkgs.jq
              inputs.leanfran.packages.${system}.rust-compiler
            ];
            LEANFRAN_CLI = "${inputs.leanfran.packages.${system}.rust-compiler}/bin/leanfran";
            LEAN_SYSROOT = "${pkgs.lean4}";
          };
          treefmt = {
            projectRootFile = "flake.nix";
            programs.nixfmt.enable = true;
            programs.rustfmt.enable = true;
          };
        };
    };
}
