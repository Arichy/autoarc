# Run autoarc on a target directory
# Usage:
#   just run                       # uses default ./archives
#   just run /path/to/archives     # custom dir
#   just debug /path/to/archives   # same but with debug logs

run dir="./archives":
  RUST_LOG=info cargo run --release -- autoarc {{dir}}

debug dir="./archives":
  RUST_LOG=debug cargo run --release -- autoarc {{dir}}

type FILE:
  cargo run --release -- type {{FILE}}