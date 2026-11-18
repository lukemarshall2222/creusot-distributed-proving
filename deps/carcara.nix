# Carcara, the Alethe Certificate_Checker (TB-4).
{
  fetchurl,
  lib,
  m4,
  rustPlatform,

  pin,
}:
rustPlatform.buildRustPackage {
  pname = "carcara";
  inherit (pin) version;

  src = fetchurl { inherit (pin) url hash; };

  # Copy of the Cargo.lock at the pinned commit; it has no git dependencies.
  cargoLock.lockFile = ./carcara.Cargo.lock;

  # gmp-mpfr-sys builds its bundled GMP, whose configure script needs m4.
  nativeBuildInputs = [ m4 ];

  cargoBuildFlags = [
    "--package"
    "carcara-cli"
  ];
  doCheck = false;

  meta = {
    description = "Proof checker and elaborator for SMT proofs in the Alethe format";
    homepage = "https://github.com/ufmg-smite/carcara";
    license = lib.licenses.asl20;
    mainProgram = "carcara";
  };
}
