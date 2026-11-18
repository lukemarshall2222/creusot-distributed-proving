# Kani, from the upstream release bundle (TB-7).
#
# The bundle holds kani-driver, kani-compiler, CBMC and the Kani libraries, and
# expects the Rust nightly named in its rust-toolchain-version file as the
# directory `toolchain` next to `bin`, as `cargo kani setup` arranges it.
{
  autoPatchelfHook,
  fetchurl,
  lib,
  makeWrapper,
  rust-bin,
  stdenv,
  zlib,

  pin,
}:
let
  system = stdenv.hostPlatform.system;
  bundle = pin.bundles.${system} or (throw "kani: no release bundle pinned for ${system}");

  toolchain = rust-bin.nightly.${pin.toolchain}.minimal.override {
    extensions = [
      "llvm-tools"
      "rust-src"
      "rustc-dev"
    ];
  };
in
stdenv.mkDerivation {
  pname = "kani";
  inherit (pin) version;

  src = fetchurl { inherit (bundle) url hash; };

  nativeBuildInputs = [ makeWrapper ] ++ lib.optionals stdenv.hostPlatform.isLinux [ autoPatchelfHook ];
  buildInputs = lib.optionals stdenv.hostPlatform.isLinux [
    stdenv.cc.cc.lib
    toolchain
    zlib
  ];

  dontConfigure = true;
  dontBuild = true;
  dontStrip = true;

  installPhase = ''
    runHook preInstall
    mkdir -p $out/lib/kani $out/bin
    cp -r . $out/lib/kani/
    ln -s ${toolchain} $out/lib/kani/toolchain
    # kani-driver starts goto-cc, cbmc and the other bundled tools from PATH.
    makeWrapper $out/lib/kani/bin/kani-driver $out/bin/kani \
      --prefix PATH : $out/lib/kani/bin
    makeWrapper $out/lib/kani/bin/kani-driver $out/bin/cargo-kani \
      --argv0 cargo-kani --prefix PATH : $out/lib/kani/bin
    runHook postInstall
  '';

  meta = {
    description = "Bit-precise model checker for Rust";
    homepage = "https://github.com/model-checking/kani";
    license = with lib.licenses; [
      asl20
      mit
    ];
    platforms = lib.attrNames pin.bundles;
    sourceProvenance = [ lib.sourceTypes.binaryNativeCode ];
  };
}
