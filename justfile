# Run autoarc on a target directory
# Usage:
#   just run                          # uses default ./archives, depth=1
#   just run /path/to/archives        # custom dir
#   just run /path/to/archives -r     # recurse into all subdirectories
#   just run /path/to/archives -d 3   # recurse up to 3 levels
#   just run /path/to/archives -n     # dry-run: print plan and exit
#   just run /path/to/archives -y     # skip the [y/N] confirmation prompt
#   just debug /path/to/archives      # same but with RUST_LOG=debug

run dir="./archives" *args="":
  RUST_LOG=info cargo run --release -- {{dir}} {{args}}

debug dir="./archives" *args="":
  RUST_LOG=debug cargo run --release -- {{dir}} {{args}}

type FILE:
  cargo run --release -- type {{FILE}}

# Format every Rust source file in place using rustfmt defaults.
fmt:
  cargo fmt --all

# Verify the codebase is rustfmt-clean (non-zero exit if any diff).
# Suitable for CI / pre-commit hooks.
fmt-check:
  cargo fmt --all -- --check

# Strict clippy run — promotes every warning to an error.
lint:
  cargo clippy --all-targets --release -- -D warnings

# Aggregate "is this PR ready?" check: format + lint + build.
check: fmt-check lint
  cargo build --release