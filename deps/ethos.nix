# Ethos, the CPC Certificate_Checker (TB-3).
{
  cmake,
  fetchurl,
  gmp,
  lib,
  stdenv,

  pin,
}:
stdenv.mkDerivation {
  pname = "ethos";
  inherit (pin) version;

  src = fetchurl { inherit (pin) url hash; };

  nativeBuildInputs = [ cmake ];
  buildInputs = [ gmp ];

  # Upstream has no install target; cvc5's get-ethos-checker copies the binary.
  installPhase = ''
    runHook preInstall
    install -Dm755 src/ethos $out/bin/ethos
    runHook postInstall
  '';

  meta = {
    description = "Checker for proofs in the Eunoia logical framework";
    homepage = "https://github.com/cvc5/ethos";
    license = lib.licenses.bsd3;
    mainProgram = "ethos";
  };
}
