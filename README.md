# autoarc

A concurrent, multi-format archive extractor for batch unpacking encrypted archives
with a list of candidate passwords.

`autoarc` walks a target directory, recursively unpacks every supported archive it
finds, automatically tries each password in your `AUTOARC_PASSWORDS` list, and
shows live progress for each task with a final summary.

## Features

- Concurrent extraction powered by [`tokio`]
- Native handling of `zip`, `rar`, and `7z`
- Subprocess fallback to `unar` / `lsar` for split archives (`.z01`, `.001`)
- Recursive: nested archives produced by extraction are queued automatically
- Multi-password trial-and-error per archive (stops on first match)
- Live `indicatif` progress bars + a coloured run summary
- Originals are backed up under `MM-DD_bak/` before being moved into `MM-DD/`
- Detected videos (`.mp4`, `.ts`) inside archives get their extension corrected
  in-place

## Supported formats

| Extension          | Backend            |
|--------------------|--------------------|
| `.zip`             | `zip` crate        |
| `.rar`             | `unrar` crate      |
| `.7z`              | `sevenz_rust2`     |
| `.z01`, `.001`     | `unar` subprocess  |

## Prerequisites

| Tool | Required? | Purpose | Install |
|---|---|---|---|
| Rust toolchain (`cargo`, edition 2024) | yes | Build & install `autoarc` | <https://rustup.rs> |
| `unar` + `lsar` | yes for split archives (`.z01`, `.001`) and the `autoarc lsar` subcommand | Subprocess fallback backend | macOS: `brew install unar`<br>Debian/Ubuntu: `sudo apt install unar`<br>Arch: `sudo pacman -S unarchiver` |
| `file` | optional | Fallback MIME sniffer for MPEG-TS detection (`infer` doesn't cover `.ts`) | preinstalled on macOS / most Linux distros |

The ZIP, RAR, and 7z backends are pure-Rust crates — they have no external runtime
dependency, so if you only deal with single-volume archives in those formats you
can skip installing `unar` entirely.

## Install

```bash
git clone https://github.com/<you>/autoarc.git
cd autoarc
cargo install --path .
```

## Configuration

`autoarc` reads passwords from the `AUTOARC_PASSWORDS` environment variable
(comma-separated). The most convenient way is a `.env` file at the project root;
see [`.env.example`](.env.example).

```bash
cp .env.example .env
# then edit .env and add your candidate passwords
```

| Variable             | Required | Description                                                 |
|----------------------|----------|-------------------------------------------------------------|
| `AUTOARC_PASSWORDS`  | yes      | Comma-separated list of candidate passwords                 |
| `RUST_LOG`           | no       | Tracing filter (`info`, `debug`, ...). Defaults to `warn`.  |

## Usage

### CLI

```bash
# Extract every supported archive in <DIR>
autoarc autoarc <DIR>

# Inspect a single file's detected type
autoarc type ./mystery.bin

# List the entries of an archive (uses `lsar`)
autoarc lsar ./bundle.zip
```

### `just` recipes

A [`justfile`](justfile) provides shortcuts:

```bash
just run                          # uses default ./archives
just run /path/to/archives        # custom dir
just debug /path/to/archives      # same with RUST_LOG=debug
just type ./mystery.bin
```

## Behaviour

Given `autoarc autoarc /tmp/in`:

```
/tmp/in/
├── MM-DD/                # working dir for today's run
│   ├── foo.zip
│   ├── foo_out/...       # extracted contents next to the archive
│   ├── bar.rar
│   └── bar_out/...
└── MM-DD_bak/            # original copies parked for safety
    ├── foo.zip
    └── bar.rar
```

- Only the **top level** of `<DIR>` is scanned for initial archives. Nested
  subdirectories are not walked (but archives produced *by extraction* are queued
  recursively).
- Multi-volume archives are kept in place so their `.z02`, `.z03`, ... siblings
  remain reachable.
- Videos found in `<DIR>` itself are renamed to enforce the canonical extension.

## Logging

The default log level is `warn`; set `RUST_LOG=info` (or `debug`) to see more.
Log lines are routed above the live progress bars so the UI never tears.

## License

MIT — see [LICENSE](LICENSE) (or pick whichever license you prefer before
publishing).
