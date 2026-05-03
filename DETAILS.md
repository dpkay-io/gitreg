# gitreg Details

This document contains detailed information about installation, shell support, architecture, and maintenance of `gitreg`.

---

## Installation Options

### From source (all platforms)

Requires [Rust](https://rustup.rs/).

```sh
cargo install --path .
gitreg init
# Restart shell or source the rc file shown by the init output
```

### Pre-built binaries

Download the archive for your platform from the [Releases](https://github.com/dpkay-io/gitreg/releases) page:

| Platform | Archive |
|---|---|
| Linux x86_64 (static) | `gitreg-x86_64-unknown-linux-musl.tar.gz` |
| Linux ARM64 (static) | `gitreg-aarch64-unknown-linux-musl.tar.gz` |
| macOS Intel | `gitreg-x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `gitreg-aarch64-apple-darwin.tar.gz` |
| Windows x86_64 | `gitreg-x86_64-pc-windows-msvc.zip` |

Extract the binary, place it on your `PATH`, then run `gitreg init`.

### Linux / macOS Manual Install

```sh
tar xzf gitreg-*.tar.gz
sudo mv gitreg /usr/local/bin/
gitreg init
# Restart shell or: source ~/.bashrc  (or ~/.zshrc)
```

### Windows Manual Install

**Native PowerShell** (recommended, automatically runs `init`):
```powershell
irm https://raw.githubusercontent.com/dpkay-io/gitreg/main/install.ps1 | iex
```

`gitreg init` works natively in PowerShell (PS 5.1 and PS 7+), Git Bash, and WSL.

**Git Bash** and **WSL** users can also use the quick install bash command (see README), or install manually:

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

## Shell Support

| Shell | RC file modified |
|---|---|
| Bash | `~/.bashrc` |
| Zsh | `~/.zshrc` |
| Fish | `~/.config/fish/functions/git.fish` |
| PowerShell 7+ | `~\Documents\PowerShell\Microsoft.PowerShell_profile.ps1` |
| PowerShell 5.1 | `~\Documents\WindowsPowerShell\Microsoft.PowerShell_profile.ps1` |

On Windows, `gitreg init` detects the shell automatically: Git Bash and other POSIX shells (Cygwin, MSYS2) are identified via `$SHELL`; native PowerShell is the fallback when `$SHELL` is not set.

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

## Upgrading

```sh
gitreg system upgrade
```

Checks the [GitHub Releases](https://github.com/dpkay-io/gitreg/releases) page for a newer version, downloads the correct pre-built binary for your platform, and replaces the running binary in place.

On Windows, the old binary is briefly renamed to `gitreg.exe.old` during the swap and is automatically deleted on the next invocation.

---

## Uninstall

The recommended way to uninstall is:
```sh
gitreg system uninstall
```
This command will:
1. Remove the shim block and alias from your rc file.
2. Delete the database and configuration directory.
3. Remove the `gitreg` binary itself.

### Manual Uninstall (alternative)

1. Remove the shim block from your rc file (between `# >>> gitreg-start >>>` and `# <<< gitreg-end <<<`).
2. Delete the database:
   - Linux / macOS / Git Bash / WSL: `rm ~/.config/gitreg/gitreg.db`
   - Windows (native path): `del %APPDATA%\gitreg\gitreg.db`
3. Optionally remove the binary:
   - Installed via Cargo: `cargo uninstall gitreg`
   - Installed manually: delete the `gitreg` (or `gitreg.exe`) file from wherever you placed it

---

## Extensibility & Integrators

`gitreg` is designed to be a foundation for other developer tools. By tracking every repository you touch in the background, it builds a real-time index of your active projects.

### The Event System
You can register external applications to "hook" into `gitreg` events. When an event occurs, `gitreg` can notify your tool.

**Available Events:**
- `registered`: A new repository was added to the registry.
- `removed`: A repository was removed.
- `tagged` / `untagged`: Repository tags were modified.
- `git.<COMMAND>`: A specific git command was executed (e.g., `git.commit`, `git.push`).

**Example: Auto-backup on commit**
```sh
# Register a script to run every time you commit in any tracked repo
gitreg integrator register ~/scripts/backup.sh git.commit
```

---

## Architecture Notes

The `hook` subcommand (hidden) is fire-and-forget by design. It runs detached from the shell and has no output channel, so errors are silently dropped. Surfaces errors is not possible from a disowned background process.
