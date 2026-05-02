# gitreg

**Zero-latency background Git repository tracker.**

`gitreg` automatically tracks every Git repository you visit, without ever slowing down your `git` commands. It provides a powerful registry to manage, tag, and execute bulk operations across your local repositories.

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

### 6. Built for Builders (Integrators)
`gitreg` isn't just a tool; it's a platform. You can build your own applications on top of the `gitreg` registry. Register your apps to receive events whenever repositories are visited, tagged, or updated.
```sh
gitreg integrator register my-app git.commit
```

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

For detailed installation options, architecture details, and more, see [DETAILS.md](./DETAILS.md).

---

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
