{
  description = "Distributed proving backend for Creusot: pinned toolchain (R7.7, design decision D-8)";

  # Every external tool is pinned here and in deps/pins.nix.
  #
  # Creusot's own flake pins nixpkgs, the Rust nightly that Creusot needs, Why3,
  # why3find, Alt-Ergo, Z3 and cvc5. This flake pins Creusot by commit and
  # follows its nixpkgs, so the System and Creusot are built from one package
  # set. deps/pins.nix pins the tools that Creusot's flake does not provide.
  #
  # flake.lock must be generated with `nix flake lock` and committed. Because
  # the only direct input is a commit hash, the lock that Nix generates is
  # determined by that commit.
  inputs = {
    # Creusot v0.15.0
    creusot.url = "github:creusot-rs/creusot/fd4d85b2c237ef93147e99f6a0c04acfc89a6a17";
    nixpkgs.follows = "creusot/nixpkgs";

    # Only for the Rust nightly of the Kani bundle (deps/pins.nix), which is
    # newer than the nightlies that Creusot's rust-overlay lists.
    rust-overlay-kani = {
      url = "github:oxalica/rust-overlay/49f68bf7cd35f3ef31fca584a22cf3d4e13f6fec";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      creusot,
      rust-overlay-kani,
    }:
    let
      inherit (nixpkgs) lib;

      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];

      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
          # Alt-Ergo 2.6.4, which Creusot uses by default, is under the OCamlPro
          # Non-Commercial Purpose License. Accepted for this project
          # (2026-11-17); nothing else unfree is allowed.
          config.allowUnfreePredicate = pkg: builtins.elem (lib.getName pkg) [
            "alt-ergo"
            "alt-ergo-lib"
            "alt-ergo-parsers"
          ];
          overlays = [
            creusot.overlays.default
            self.overlays.default
          ];
        };

      forAllSystems = f: lib.genAttrs systems (system: f system (pkgsFor system));
    in
    {
      overlays.default = import ./deps/overlay.nix {
        pins = import ./deps/pins.nix;
        crane = creusot.inputs.crane;
        kaniRustBin = rust-overlay-kani.lib.mkRustBin { };
        workspaceRoot = ./.;
      };

      packages = forAllSystems (
        system: pkgs:
        {
          # The System.
          default = pkgs.dp.workspace;
          inherit (pkgs.dp) workspace;

          # Creusot with its Why3 framework and provers, as Creusot's flake wraps it.
          inherit (pkgs.dp) creusot;

          # Pinned tools of R7.7 criterion 1.
          inherit (pkgs.dp)
            alt-ergo
            apalache
            carcara
            cargo-mutants
            cpc-signature
            cvc5
            ethos
            kani
            lean
            python
            rust-toolchain
            tlc
            why3
            why3find
            why3find-dispatch
            z3
            ;
        }
        # OCI images of the integration cluster (Linux only).
        // lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux pkgs.dp.images
      );

      devShells = forAllSystems (
        system: pkgs: {
          default = pkgs.mkShell {
            # `pkgs.dp.creusot` is named in full: inside `with`, the name
            # `creusot` would still mean the flake input.
            packages = with pkgs.dp; [
              pkgs.dp.creusot
              alt-ergo
              apalache
              carcara
              cargo-mutants
              cvc5
              ethos
              kani
              lean
              python
              rust-toolchain
              tlc
              why3
              z3
            ];

            # Creusot's wrapper puts unpatched why3find on PATH, as Creusot_CLI
            # needs without the backend option. The patched build has the same
            # program name, so it is reached through this variable.
            DP_WHY3FIND = "${pkgs.dp.why3find-dispatch}/bin/why3find";

            # The Lock_File that dp-frontend checks local executables against.
            DP_LOCK = "${./deps/lock.toml}";

            # Creusot's Why3 prover configuration and its files, for Workers
            # and for tests that run why3find as Creusot_CLI does.
            DP_CREUSOT_HOME = "${pkgs.dp.creusot}";
            DP_CREUSOT_EXAMPLES = "${creusot}/examples";

            # Signature files that Ethos loads for CPC proofs (TB-11).
            DP_CPC_SIGNATURE = "${pkgs.dp.cpc-signature}";
          };
        }
      );

      formatter = forAllSystems (system: pkgs: pkgs.nixfmt-tree);
    };
}
