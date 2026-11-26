# Sourced by the job scripts that run the workspace's tests with real
# tools. Run from the repository root inside `nix develop .#ci`.

# The committed Lock_File pins the platforms it lists. On another platform
# the tests use a Lock_File generated from this flake's builds, which checks
# the mechanism, not the committed pins.
platform=$(nix eval --raw --impure --expr builtins.currentSystem)
if ! grep -q "platform = \"$platform\"" deps/lock.toml; then
  generated=${DP_GENERATED_LOCK:-${TMPDIR:-/tmp}/dp-generated-lock.toml}
  if [ ! -s "$generated" ]; then
    deps/gen-lock.sh > "$generated"
  fi
  echo "ci: deps/lock.toml has no entries for $platform; using $generated" >&2
  export DP_LOCK="$generated"
fi

# A test that cannot find its tools fails; it does not pass with a notice.
export DP_REQUIRE_TOOLS=1

# Every component keeps its whole trace where it writes one (R1.5
# criterion 19).
export DP_TRACE_RETENTION=complete

# The end-to-end tests start the workspace executables.
cargo build --workspace

# `no_inputs JOB WHAT`: a `ci/` job whose inputs do not exist yet passes
# with a notice; `ci/traceability` catches a row in scope that names them.
no_inputs() {
  echo "$1: no inputs yet ($2); nothing was checked" >&2
  exit 0
}

# `registered DIR MANIFEST`: every suite directory under DIR is a test
# target of MANIFEST, so that a suite that was added is also run.
registered() {
  for suite in "$1"/*/; do
    name=$(basename "$suite")
    [ -f "$suite/main.rs" ] || continue
    grep -q "path = \"$name/main.rs\"" "$2" || { echo "ci: the suite $suite is no test target of $2" >&2; exit 1; }
  done
}
