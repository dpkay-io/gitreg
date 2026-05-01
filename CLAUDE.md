# gitreg

## Local validation (run before every commit)

```sh
cargo fmt -- --check   # must produce no output
cargo clippy -- -D warnings
cargo test --locked
cargo build --locked
```

All four steps mirror the CI pipeline (`ci.yml`). A clean local run means CI passes.

## GitHub Actions workflow validation (run before committing any .github/workflows/*.yml change)

```sh
actionlint .github/workflows/*.yml
```

`actionlint` is installed at `~/.cargo/bin/actionlint`. A workflow file that passes `actionlint` will not fail with a parse error on GitHub. Never push a workflow change without running this first.

## Rust formatting rules

- `#[cfg(...)]` attributes go on their own line — never inline with the item they gate:
  ```rust
  // correct
  #[cfg(windows)]
  const EXT: &str = "zip";

  // wrong — rustfmt rejects this
  #[cfg(windows)] const EXT: &str = "zip";
  ```
- No alignment spaces inside attribute arguments:
  ```rust
  // correct
  #[cfg(all(target_os = "linux", target_arch = "x86_64"))]

  // wrong
  #[cfg(all(target_os = "linux",   target_arch = "x86_64"))]
  ```
- Long method chains that exceed 100 characters must be broken one method per line.

## Commit authorship

Never add AI attribution to commits, PRs, comments, or any output. No `Co-Authored-By: Claude ...` trailers, no "Generated with Claude Code" footers, no AI signatures of any kind.

## CI pipeline

Steps run on Ubuntu, macOS, and Windows: `fmt` → `clippy` → `test` → `build` → `audit` → `integration tests` (Unix only).
The matrix is cancelled if any job fails, so a fmt failure on one OS blocks everything.
