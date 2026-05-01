# Security Policy

## Reporting a Vulnerability

Please report security vulnerabilities via [GitHub Security Advisories](https://github.com/dpkay-io/gitreg/security/advisories/new) rather than opening a public issue.

Include a description of the vulnerability, steps to reproduce, and the potential impact. You can expect an acknowledgement within 72 hours and a fix timeline within 14 days for confirmed issues.

## Threat Model

### Self-updater (`gitreg upgrade`)

`gitreg upgrade` fetches the latest release archive from GitHub Releases and replaces the running binary.

**SHA256 verification:** The updater downloads the `.sha256` sidecar file published alongside each release archive and aborts if the digest does not match. This protects against accidental download corruption and CDN-level content substitution.

**Trust anchor:** The trust anchor is the GitHub repository and the Actions secrets used to publish releases — not a separate signing key. A compromise of the repository or its CI pipeline would allow an attacker to push a malicious release that passes hash verification. Users who require a stronger guarantee (e.g. hardware-backed code signing) should build from source via `cargo install`.

**No elevated privileges:** `gitreg upgrade` replaces only the running binary. It does not require root or administrator access.

### General scope

gitreg reads the local filesystem and writes to a local SQLite database in the config directory. It does not transmit telemetry. All network access is limited to `gitreg upgrade`.
