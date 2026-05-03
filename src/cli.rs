use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "gitreg",
    about = "Zero-latency background Git repository tracker",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Print all available commands categorized by type
    #[command(name = "commands")]
    Overview,

    /// Initialize gitreg and inject the shell shim
    Init,

    /// List all tracked repositories
    Ls {
        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Run a git command across multiple repositories
    Git {
        /// Targets: 'all', tags (prefixed with '@'), or comma-separated IDs/names
        targets: String,

        /// Prefix output in real-time instead of buffering per-repository
        #[arg(long)]
        realtime: bool,

        /// The git command and its arguments
        #[arg(trailing_var_arg = true, required = true)]
        git_args: Vec<String>,
    },

    /// Manage repositories (scan, tag, untag, rm, prune)
    #[command(subcommand)]
    Repo(RepoAction),

    /// Configure gitreg (alias, exclude, autoprune)
    #[command(subcommand)]
    Config(ConfigAction),

    /// Manage integrator applications and events
    #[command(subcommand)]
    Integrator(IntegratorAction),

    /// Maintenance and information
    #[command(subcommand)]
    System(SystemAction),

    /// Record a repository (called by the shell shim)
    // Hidden from --help to keep the public interface clean. Callers can
    // still invoke it directly, but it only modifies the caller's own
    // registry — no privilege escalation is possible.
    #[command(hide = true)]
    Hook {
        #[arg(long)]
        path: PathBuf,
    },

    /// Automatically discover and register repositories in common locations
    #[command(hide = true)]
    Autoscan,

    /// Scan a directory tree (alias for repo scan)
    #[command(hide = true)]
    Scan {
        /// Directory to scan (default: current directory)
        dir: Option<PathBuf>,

        /// Maximum directory depth to recurse (default: 3)
        #[arg(long, short, default_value = "3")]
        depth: usize,
    },

    /// Add a tag (alias for repo tag)
    #[command(hide = true)]
    Tag {
        /// ID, repo name (owner/repo), or path
        target: String,
        tag: String,
    },

    /// Remove a tag (alias for repo untag)
    #[command(hide = true)]
    Untag {
        /// ID, repo name (owner/repo), or path
        target: String,
        tag: String,
    },

    /// Remove a repository (alias for repo rm)
    #[command(hide = true)]
    Rm {
        /// ID, repo name (owner/repo), or path
        target: String,
    },

    /// Prune missing repositories (alias for repo prune)
    #[command(hide = true)]
    Prune,

    /// Force push uncommitted code to an emergency branch
    #[command(alias = "fire")]
    Emergency {
        /// Dismiss the emergency push notifications
        #[arg(long, short)]
        clear: bool,
    },
}

#[derive(Subcommand)]
pub enum RepoAction {
    /// List all tracked repositories (alias for top-level 'ls')
    Ls {
        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Scan a directory tree and register all found git repositories
    Scan {
        /// Directory to scan (default: current directory)
        dir: Option<PathBuf>,

        /// Maximum directory depth to recurse (default: 3)
        #[arg(long, short, default_value = "3")]
        depth: usize,
    },

    /// Add a tag to a repository
    Tag {
        /// ID, repo name (owner/repo), or path
        target: String,
        tag: String,
    },

    /// Remove a tag from a repository
    Untag {
        /// ID, repo name (owner/repo), or path
        target: String,
        tag: String,
    },

    /// Remove a specific repository from the registry
    Rm {
        /// ID, repo name (owner/repo), or path
        target: String,
    },

    /// Remove entries for repositories that no longer exist on disk
    Prune,
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Enable "gr" alias for "gitreg"
    Alias,

    /// Manage daily autoprune settings
    Autoprune {
        /// Enable autoprune
        #[arg(long, conflicts_with = "disable")]
        enable: bool,

        /// Disable autoprune
        #[arg(long)]
        disable: bool,

        /// Set the daily run time (format: HH:MM)
        #[arg(long, value_name = "HH:MM")]
        time: Option<String>,
    },

    /// Manage path exclusions for registration and scanning
    #[command(subcommand)]
    Exclude(ExcludeAction),
}

#[derive(Subcommand)]
pub enum SystemAction {
    /// Check for a newer release and upgrade the binary in place
    Upgrade,

    /// Show the current version
    Version,

    /// Open the github webpage
    Webpage,

    /// Completely uninstall gitreg
    Uninstall,
}

#[derive(Subcommand)]
pub enum ExcludeAction {
    /// Add a path to the exclusion list
    Add {
        /// Path to exclude
        path: PathBuf,
    },
    /// Remove a path from the exclusion list
    Rm {
        /// Path to remove from exclusions
        path: PathBuf,
    },
    /// List all excluded paths
    Ls,
}

#[derive(Subcommand)]
pub enum IntegratorAction {
    /// Register an app for an event
    Register {
        /// Name of the app
        #[arg(long)]
        app: String,
        /// Event to listen for
        #[arg(long)]
        event: String,
        /// Path to the socket or named pipe
        #[arg(long)]
        socket: String,
    },
    /// Unregister an app from an event
    Unregister {
        /// Name of the app
        #[arg(long)]
        app: String,
        /// Event to unregister
        #[arg(long)]
        event: String,
    },
    /// List all registered apps and their events
    Ls,
    /// List all available events
    Events,
    /// Block an app from receiving any events
    Block {
        /// Name of the app to block
        #[arg(long)]
        app: String,
    },
    /// Unblock a previously blocked app
    Unblock {
        /// Name of the app to unblock
        #[arg(long)]
        app: String,
    },
    /// Remove an app and all its event registrations
    Rm {
        /// Name of the app to remove
        #[arg(long)]
        app: String,
    },
}
