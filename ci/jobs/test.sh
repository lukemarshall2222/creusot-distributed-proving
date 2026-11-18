#!/bin/sh
# ci/test and ci/diff: the unit, property, end-to-end and differential tests
# of the workspace, with real provers.
#
# Run from the repository root inside `nix develop .#ci`.
set -eu

# The committed Lock_File pins the platforms it lists. On another platform
# the tests use a Lock_File generated from this flake's builds, which checks
# the mechanism, not the committed pins.
platform=$(nix eval --raw --impure --expr builtins.currentSystem)
if ! grep -q "platform = \"$platform\"" deps/lock.toml; then
  generated=${DP_GENERATED_LOCK:-$(mktemp)}
  deps/gen-lock.sh > "$generated"
  echo "ci/test: deps/lock.toml has no entries for $platform; using $generated" >&2
  export DP_LOCK="$generated"
fi

# A test that cannot find its tools fails; it does not pass with a notice.
export DP_REQUIRE_TOOLS=1

# The end-to-end tests start the workspace executables.
cargo build --workspace
cargo test --workspace
