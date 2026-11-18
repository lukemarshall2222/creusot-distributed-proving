# Apalache, the symbolic model checker for TLA+ (TB-8).
{
  fetchurl,
  jre,
  lib,
  makeWrapper,
  stdenv,

  pin,
}:
stdenv.mkDerivation {
  pname = "apalache";
  inherit (pin) version;

  src = fetchurl { inherit (pin) url hash; };

  nativeBuildInputs = [ makeWrapper ];

  dontConfigure = true;
  dontBuild = true;

  installPhase = ''
    runHook preInstall
    mkdir -p $out/share/apalache $out/bin
    cp -r lib bin $out/share/apalache/
    patchShebangs $out/share/apalache/bin/apalache-mc
    makeWrapper $out/share/apalache/bin/apalache-mc $out/bin/apalache-mc \
      --prefix PATH : ${lib.makeBinPath [ jre ]}
    runHook postInstall
  '';

  meta = {
    description = "Symbolic model checker for TLA+";
    homepage = "https://github.com/apalache-mc/apalache";
    license = lib.licenses.asl20;
    mainProgram = "apalache-mc";
    sourceProvenance = [ lib.sourceTypes.binaryBytecode ];
  };
}
