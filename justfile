# Run autoarc on a target directory
# Usage:
#   just run                          # uses default ./archives, depth=1
#   just run /path/to/archives        # custom dir
#   just run /path/to/archives -r     # recurse into all subdirectories
#   just run /path/to/archives -d 3   # recurse up to 3 levels
#   just debug /path/to/archives      # same but with RUST_LOG=debug

run dir="./archives" *args="":
  RUST_LOG=info cargo run --release -- autoarc {{dir}} {{args}}

debug dir="./archives" *args="":
  RUST_LOG=debug cargo run --release -- autoarc {{dir}} {{args}}

type FILE:
  cargo run --release -- type {{FILE}}