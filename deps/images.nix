# OCI images of the integration cluster (design: Integration tests): one
# Coordinator, one Lemma_Pool and Workers. Linux only.
{ pkgs }:
let
  inherit (pkgs) dp;

  mkImage =
    name: contents:
    pkgs.dockerTools.buildLayeredImage {
      inherit name;
      tag = "pinned";
      contents = [ dp.workspace ] ++ contents;
      config.Entrypoint = [ "${dp.workspace}/bin/${name}" ];
    };
in
{
  image-coordinator = mkImage "dp-coordinator" [ ];
  image-pool = mkImage "dp-pool" [ ];

  # A Worker starts solvers itself and uses why3find's prover configuration
  # and Calibration through the bridge (D-3, D-16).
  image-worker = mkImage "dp-worker" [
    dp.alt-ergo
    dp.cvc5
    dp.why3
    dp.why3find-dispatch
    dp.z3
  ];
}
