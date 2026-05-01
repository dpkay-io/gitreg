# gitreg

Zero-latency background Git repository tracker.

```
shell invokes git
      │
      ▼
  bash/zsh/fish shim
      │  (find .git root, check marker)
      │
      ├─── marker fresh? ──► command git  (zero latency)
      │
      └─── marker missing/stale?
               │
               ├─► gitreg hook --path <root>   (background, disowned)
               │         │
               │         ├─► upsert into SQLite
               │         └─► write .git/gitreg_tracked  (atomic rename)
               │
               └─► command git  (zero latency)
```

The user's `git` command is **never blocked** — the hook runs in the background.

---

## Installation

### Quick install (Linux, macOS, Git Bash, WSL)

```sh
curl -sSf https://raw.githubusercontent.com/dpkay-io/gitreg/main/install.sh | bash
```

Then follow the `source` instruction printed at the end.

### From source (all platforms)

Requires [Rust](https://rustup.rs/).

```sh
cargo install --path .
gitreg init
# Restart shell or source the rc file shown by the init output
```

### Pre-built binaries

Download the archive for your platform from the [Releases](../../releases) page:

| Platform | Archive |
|---|---|
| Linux x86_64 (static) | `gitreg-x86_64-unknown-linux-musl.tar.gz` |
| Linux ARM64 (static) | `gitreg-aarch64-unknown-linux-musl.tar.gz` |
| macOS Intel | `gitreg-x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `gitreg-aarch64-apple-darwin.tar.gz` |
| Windows x86_64 | `gitreg-x86_64-pc-windows-msvc.zip` |

Extract the binary, place it on your `PATH`, then run `gitreg init`.

### Linux / macOS

```sh
tar xzf gitreg-*.tar.gz
sudo mv gitreg /usr/local/bin/
gitreg init
# Restart shell or: source ~/.bashrc  (or ~/.zshrc)
```

### Windows

Native Windows shell integration is **not supported**. Use one of the following:

**Git Bash:**
```sh
# Run inside Git Bash
unzip gitreg-x86_64-pc-windows-msvc.zip
mv gitreg.exe /usr/local/bin/
gitreg init
source ~/.bashrc
```

**WSL (Windows Subsystem for Linux):**
```sh
# Run inside WSL — download the Linux musl binary instead
tar xzf gitreg-x86_64-unknown-linux-musl.tar.gz
sudo mv gitreg /usr/local/bin/
gitreg init
source ~/.bashrc
```

---

## Commands

| Command | Description |
|---|---|
| `gitreg init` | Detect shell, inject shim into rc file |
| `gitreg ls` | List all tracked repositories with ID, path, and last-seen timestamp |
| `gitreg scan [dir] [-d <depth>]` | Scan a directory tree and register all found git repos (default depth: 3) |
| `gitreg prune` | Remove entries for repos that no longer exist on disk |
| `gitreg rm <path>` | Remove a specific repo from the registry |
| `gitreg upgrade` | Check for a newer release on GitHub and replace the binary in place |

---

## Upgrading

```sh
gitreg upgrade
```

Checks the [GitHub Releases](../../releases) page for a newer version, downloads the correct pre-built binary for your platform, and replaces the running binary in place — no external tools needed.

```
Current version: v0.1.0
Checking for updates... v0.2.0
Upgrading to v0.2.0 ...
Downloading https://github.com/.../gitreg-latest-x86_64-apple-darwin.tar.gz ...
Upgraded to v0.2.0.
```

If you are already on the latest version:

```
Current version: v0.2.0
Checking for updates... v0.2.0
Already up to date (v0.2.0).
```

On Windows the old binary is briefly renamed to `gitreg.exe.old` during the swap and is automatically deleted on the next invocation.

> **Note:** `gitreg upgrade` requires a network connection and a pre-built binary for your platform. It is not available for source builds on unsupported targets — use `cargo install --path .` to rebuild from source instead.

---

## Shell Support

| Shell | RC file modified |
|---|---|
| Bash | `~/.bashrc` |
| Zsh | `~/.zshrc` |
| Fish | `~/.config/fish/functions/git.fish` |

Windows users can use **Git Bash** or **WSL** where `$SHELL` resolves to bash.

---

## How it works

1. `gitreg init` injects a `git()` shell function into your rc file.
2. Every time you run `git`, the shim walks up to find the repo root.
3. If `.git/gitreg_tracked` is missing or contains a different path, it fires
   `gitreg hook --path <root>` **as a disowned background process**.
4. The hook canonicalizes the path, upserts it into `~/.config/gitreg/gitreg.db`,
   and atomically writes the marker file.
5. `command git "$@"` runs immediately — no waiting.

---

## Architecture Notes

The `hook` subcommand is fire-and-forget by design. It runs detached from the
shell and has no output channel, so errors are silently dropped. Contributors
should not treat the discarded `Result` in `cmd_hook` as a bug — surfacing errors
is not possible from a disowned background process.

---

## Uninstall

1. Remove the shim block from your rc file (between `# >>> gitreg-start >>>` and `# <<< gitreg-end <<<`).
2. Delete the database:
   - Linux / macOS / Git Bash / WSL: `rm ~/.config/gitreg/gitreg.db`
   - Windows (native path): `del %APPDATA%\gitreg\gitreg.db`
3. Optionally remove the binary:
   - Installed via Cargo: `cargo uninstall gitreg`
   - Installed manually: delete the `gitreg` (or `gitreg.exe`) file from wherever you placed it
