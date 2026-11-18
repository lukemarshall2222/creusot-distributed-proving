# Versions and content hashes of the tools that Creusot's flake does not pin
# (R7.7 criterion 1). Every `hash` is the SHA-256 of the file at `url`.
#
# Carcara is pinned to the commit that cvc5 1.4.1 names in its
# contrib/get-carcara-checker script, so the checker matches the proofs that
# the pinned cvc5 prints. Ethos is newer than the commit cvc5 names; see below.
{
  # Ethos is ten commits after the commit that cvc5 1.4.1 names
  # (08e4aa40, release 0.2.5). At 08e4aa40 a reference file given with
  # `--reference=X` is parsed, but the check that every `assume` is a reference
  # assertion is switched off again when the proof file is loaded, so R5.3
  # criterion 9 and AS-30 fail. This commit, "Fix open issues (#272)", fixes
  # that; no release contains it yet.
  ethos = {
    version = "c7d3db902bee86b8ef61c3cd7695b0b0ba8d838b";
    url = "https://github.com/cvc5/ethos/archive/c7d3db902bee86b8ef61c3cd7695b0b0ba8d838b.tar.gz";
    hash = "sha256-iUEcxyKN+DZ+khK3TJS4a6XSl5Gu++9EHD8ahWAIVGE=";
  };

  carcara = {
    # Cargo package version 1.1.0 at this commit. cvc5 names the fork
    # hanielb/carcara; the commit is served from the upstream repository.
    version = "394edbb15ba95c47893f1d821fddde7e016af178";
    url = "https://github.com/ufmg-smite/carcara/archive/394edbb15ba95c47893f1d821fddde7e016af178.tar.gz";
    hash = "sha256-KFYkMsp0E6Zi0l8D4yjKtLe5v2SbLKaSaKJVpEpd3uY=";
  };

  apalache = {
    version = "0.63.0";
    url = "https://github.com/apalache-mc/apalache/releases/download/v0.63.0/apalache.tgz";
    hash = "sha256-DRNceldWoLtHF0pnZ8oF2+u8atPiJU8PGfpXTqhtTag=";
  };

  # Kani release bundles. Each bundle needs the Rust nightly named in its
  # rust-toolchain-version file.
  kani = {
    version = "0.68.0";
    toolchain = "2026-08-21";
    bundles = {
      aarch64-darwin = {
        url = "https://github.com/model-checking/kani/releases/download/kani-0.68.0/kani-0.68.0-aarch64-apple-darwin.tar.gz";
        hash = "sha256-pdOaXV50glOlU6pi8pXGw5coeSeijl4yaR1/8u2gw5g=";
      };
      aarch64-linux = {
        url = "https://github.com/model-checking/kani/releases/download/kani-0.68.0/kani-0.68.0-aarch64-unknown-linux-gnu.tar.gz";
        hash = "sha256-AELSaIvxVQJw3vSnnJaq7qE4YxgYy0q1SAEg8tUalCw=";
      };
      x86_64-linux = {
        url = "https://github.com/model-checking/kani/releases/download/kani-0.68.0/kani-0.68.0-x86_64-unknown-linux-gnu.tar.gz";
        hash = "sha256-MuK0hNc+3gu/ZKLPCHnEJZQiSX2K7E7mfUSN6a54Q9M=";
      };
    };
  };
}
