# gitreg

**High-performance, foundational infrastructure for Git repository management.**

`gitreg` is a zero-latency repository registry that serves as a lightweight backbone for your local development environment. It automatically tracks every Git repository you visit, providing a centralized, queryable index that enables developers to build sophisticated automation and custom workflows around their workspace.

---

## ⚡ Quick Install

### Linux, macOS, Git Bash, WSL

```sh
curl -sSf https://raw.githubusercontent.com/dpkay-io/gitreg/main/install.sh | bash
```

### Windows (Native PowerShell)

```powershell
irm https://raw.githubusercontent.com/dpkay-io/gitreg/main/install.ps1 | iex
```

*After installation, run `gitreg init` to set up your shell.*

---

## 🚀 Key Features

### 1. Zero-Latency Tracking
Once initialized, `gitreg` works in the background. Every time you run a `git` command in a repository, `gitreg` ensures it's registered. The registration happens in a detached background process—your workflow is never blocked.

### 2. Powerful Registry (`ls`)
See all your repositories in one place, with metadata like last-seen timestamps and custom tags.
```sh
gitreg ls
```

### 3. Smart Tagging
Organize your repositories with tags (e.g., `work`, `personal`, `oss`).
```sh
gitreg repo tag <id|name|path> work
```

### 4. Bulk Execution
Run any `git` command across multiple repositories filtered by tags.
```sh
gitreg git --tag work fetch --all
```

### 5. Emergency Safety Net
A "panic button" for your code. Force push all uncommitted changes across your tracked repositories to an emergency branch.
```sh
gitreg emergency
```

### 6. Integrators & Automation
`gitreg` is built as a platform for automation. You can register external applications to receive real-time notifications (via Unix Sockets or Named Pipes) whenever repositories are visited, tagged, or updated.

For a complete guide on building integrations, see [INTEGRATORS.md](./INTEGRATORS.md).

### 7. Never Get Lost
`gitreg` acts as a central hub for your entire workspace. No matter how many drives or deep directory structures you have, `gitreg` keeps everything indexed in a single, searchable place. You'll never have to hunt for that "one project" again—it's always just a `gitreg ls` away.

---

## 🛠️ Commands Reference

### Main Commands
| Command | Description |
|---|---|
| `gitreg init` | Initialize gitreg and inject the shell shim |
| `gitreg ls` | List all tracked repositories |
| `gitreg git <args>` | Run a git command across multiple repositories |
| `gitreg emergency` | Force push uncommitted code to an emergency branch |

### Repository Management (`repo`)
| Command | Description |
|---|---|
| `gitreg repo ls` | Alias for top-level `ls` |
| `gitreg repo scan <dir>` | Scan a directory tree for git repositories |
| `gitreg repo tag <target> <tag>` | Add a tag to a repository |
| `gitreg repo untag <target> <tag>` | Remove a tag from a repository |
| `gitreg repo rm <target>` | Remove a repository from the registry |
| `gitreg repo prune` | Remove entries for repositories that no longer exist |

### Configuration (`config`)
| Command | Description |
|---|---|
| `gitreg config alias` | Enable "gr" alias for "gitreg" |
| `gitreg config autoprune` | Manage daily autoprune settings |
| `gitreg config exclude <add\|rm\|ls>` | Manage path exclusions |

### Integrator Platform (`integrator`)
| Command | Description |
|---|---|
| `gitreg integrator register` | Register an app for an event |
| `gitreg integrator unregister` | Unregister an app from an event |
| `gitreg integrator ls` | List all registered apps |
| `gitreg integrator events` | List all available events |

### System & Maintenance (`system`)
| Command | Description |
|---|---|
| `gitreg system upgrade` | Upgrade `gitreg` in place |
| `gitreg system version` | Show current version |
| `gitreg system uninstall` | Completely uninstall gitreg |

---

## 📖 Learn More

- [INTEGRATORS.md](./INTEGRATORS.md) - Full guide for building integrations and automation.
- [DETAILS.md](./DETAILS.md) - Detailed installation options and architecture.

---

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
