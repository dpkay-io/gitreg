use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "gitreg",
    about = "Zero-latency background Git repository tracker"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize gitreg and inject the shell shim
    Init,

    /// Record a repository (called by the shell shim)
    // Hidden from --help to keep the public interface clean. Callers can
    // still invoke it directly, but it only modifies the caller's own
    // registry — no privilege escalation is possible.
    #[command(hide = true)]
    Hook {
        #[arg(long)]
        path: PathBuf,
    },

    /// List all tracked repositories
    Ls,

    /// Remove entries for repositories that no longer exist on disk
    Prune,

    /// Remove a specific repository from the registry
    Rm { path: PathBuf },

    /// Scan a directory tree and register all found git repositories
    Scan {
        /// Directory to scan (default: current directory)
        dir: Option<PathBuf>,

        /// Maximum directory depth to recurse (default: 3)
        #[arg(long, short, default_value = "3")]
        depth: usize,
    },

    /// Check for a newer release and upgrade the binary in place
    Upgrade,
}
