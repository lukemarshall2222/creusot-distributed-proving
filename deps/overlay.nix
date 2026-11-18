# Overlay that adds the attribute set `dp`: the System and every pinned tool.
# It must be applied after Creusot's overlay, which provides `creusot.*` and
# `rust-bin`.
{
  pins,
  crane,
  kaniRustBin,
  workspaceRoot,
}:
final: prev:
let
  inherit (final) lib;

  # The Rust nightly that Creusot pins. The System is built with the toolchain
  # that Creusot verifies it with.
  rust-toolchain = final.creusot.mkRustToolchain (
    final.creusot.components
    ++ [
      "clippy"
      "rust-src"
      "rustfmt"
    ]
  );

  craneLib = (crane.mkLib final).overrideToolchain rust-toolchain;

  workspace = craneLib.buildPackage {
    pname = "dp";
    version = "0.0.0";
    src = craneLib.cleanCargoSource workspaceRoot;
    strictDeps = true;
  };

  # Patches under patches/why3find/, in file-name order (R7.9).
  why3findPatchDir = ../patches/why3find;
  why3findPatches = lib.optionals (builtins.pathExists why3findPatchDir) (
    map (name: why3findPatchDir + "/${name}") (
      lib.sort lib.lessThan (
        lib.filter (lib.hasSuffix ".patch") (lib.attrNames (builtins.readDir why3findPatchDir))
      )
    )
  );
in
{
  dp = {
    inherit rust-toolchain workspace;

    # Creusot with its Why3 framework and provers, wrapped as Creusot's flake
    # wraps its default package.
    creusot = (final.creusot.mkCreusotWrapped { isFree = false; }).override (old: {
      paths = old.paths ++ [
        final.gcc
        (final.creusot.mkRustToolchain [ ])
      ];
    });

    # Pinned by Creusot's flake.
    inherit (final.creusot)
      alt-ergo
      cvc5
      why3
      why3find
      z3
      ;

    # why3find with the dispatch-mode patch (design decision D-3). Never an
    # Unpatched_Solver kind in the Lock_File.
    why3find-dispatch = final.creusot.why3find.overrideAttrs (old: {
      name = "why3find-dispatch-${old.version}";
      patches = (old.patches or [ ]) ++ why3findPatches;
    });

    # Certificate_Checkers and the CPC signature files that Ethos loads.
    ethos = final.callPackage ./ethos.nix { pin = pins.ethos; };
    carcara = final.callPackage ./carcara.nix { pin = pins.carcara; };
    cpc-signature = final.runCommand "cpc-signature-${final.creusot.cvc5.version}" { } ''
      cp -r ${final.creusot.cvc5.src}/proofs/eo/cpc $out
    '';

    # Proof and model-checking tools.
    lean = final.lean4;
    tlc = final.tlaplus;
    apalache = final.callPackage ./apalache.nix { pin = pins.apalache; };
    kani = final.callPackage ./kani.nix {
      pin = pins.kani;
      rust-bin = kaniRustBin final;
    };
    inherit (final) cargo-mutants;

    # Interpreter and libraries of the Experiment_Runner (TB-15).
    python = final.python3.withPackages (
      ps: with ps; [
        hypothesis
        matplotlib
        numpy
        pytest
        scipy
      ]
    );

    images = import ./images.nix { pkgs = final; };
  };
}
