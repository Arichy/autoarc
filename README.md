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
# Extract every supported archive in <DIR> (top level only)
autoarc autoarc <DIR>

# Recurse into subdirectories up to N levels
autoarc autoarc <DIR> --depth 3

# Recurse without limit
autoarc autoarc <DIR> --recursive
autoarc autoarc <DIR> -r

# Inspect a single file's detected type
autoarc type ./mystery.bin

# List the entries of an archive (uses `lsar`)
autoarc lsar ./bundle.zip
```

#### Recursion modes

| Flag                    | Meaning                                                     |
|-------------------------|-------------------------------------------------------------|
| (none)                  | `--depth 1` — only the immediate contents of `<DIR>`       |
| `--depth N` / `-d N`    | Walk up to `N` directory levels (`N ≥ 1`)                  |
| `--depth 0`             | Unlimited recursion (alias for `--recursive`)               |
| `--recursive` / `-r`    | Unlimited recursion                                         |

When `depth == 1` (the default), top-level archives are first **moved into
`<DIR>/MM-DD/` and copied into `<DIR>/MM-DD_bak/`** for safety.
When `depth > 1`, archives are processed **in place** — the date-folder ritual
is skipped and our own `_out` / `MM-DD*` directories are pruned from the walk
to avoid re-processing previous runs' output.

### `just` recipes

A [`justfile`](justfile) provides shortcuts; extra args are forwarded to the
binary:

```bash
just run                                # uses default ./archives
just run /path/to/archives              # custom dir, depth=1
just run /path/to/archives -r           # custom dir, unlimited recursion
just run /path/to/archives -d 3         # custom dir, depth=3
just debug /path/to/archives -r         # same with RUST_LOG=debug
just type ./mystery.bin
```

## Behaviour

Given `autoarc autoarc /tmp/in` (default `--depth 1`):

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

With `--depth N > 1` or `--recursive`, archives are extracted **in place**
(no MM-DD movement, no backup copy):

```
/tmp/in/
├── sub/foo.zip
├── sub/foo_out/...
└── sub/nested/bar.rar
    └── bar_out/...
```

- The depth limit only applies to the **initial** scan. Archives produced *by
  extraction itself* are always queued recursively, regardless of `--depth`.
- Multi-volume archives are kept in place so their `.z02`, `.z03`, ... siblings
  remain reachable.
- Videos found during the scan are renamed in-place to enforce the canonical
  extension.
- During recursive scans, directories named `*_out`, `MM-DD`, or `MM-DD_bak`
  are skipped to avoid re-processing previous runs' artefacts.

## Logging

The default log level is `warn`; set `RUST_LOG=info` (or `debug`) to see more.
Log lines are routed above the live progress bars so the UI never tears.

## License

MIT — see [LICENSE](LICENSE) (or pick whichever license you prefer before
publishing).
