mod cli;
mod db;
mod error;
mod hook;
#[cfg(not(windows))]
mod shell;
mod upgrade;

use chrono::{TimeZone, Utc};
use clap::Parser;
use cli::{Cli, Commands};
use db::{Database, RepoRecord};
use error::{GitregError, Result};
use std::path::{Path, PathBuf};
use std::process;
use tabled::{Table, Tabled};

fn db_path() -> Result<PathBuf> {
    let dir = dirs::config_dir().ok_or(GitregError::NoConfigDir)?;
    Ok(dir.join("gitreg").join("gitreg.db"))
}

fn open_db() -> Result<Database> {
    Database::open(&db_path()?)
}

fn log_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("gitreg").join("gitreg.log"))
}

fn log_hook_error(err: &GitregError) {
    let Some(path) = log_path() else { return };
    let line = format!(
        "{} hook error: {err}\n",
        Utc::now().format("%Y-%m-%dT%H:%M:%SZ")
    );
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = f.write_all(line.as_bytes());
    }
}

#[derive(Tabled)]
struct LsRow {
    #[tabled(rename = "ID")]
    id: i64,
    #[tabled(rename = "Path")]
    path: String,
    #[tabled(rename = "Last Seen")]
    last_seen: String,
}

impl From<RepoRecord> for LsRow {
    fn from(r: RepoRecord) -> Self {
        let secs = r.last_seen / 1000;
        let nsecs = ((r.last_seen % 1000) * 1_000_000) as u32;
        let dt = Utc
            .timestamp_opt(secs, nsecs)
            .single()
            .unwrap_or_default()
            .format("%Y-%m-%d %H:%M:%S UTC")
            .to_string();
        Self {
            id: r.id,
            path: r.path,
            last_seen: dt,
        }
    }
}

#[cfg(windows)]
fn cmd_init() -> Result<()> {
    Err(GitregError::UnsupportedPlatform)
}

#[cfg(not(windows))]
fn cmd_init() -> Result<()> {
    let sh = shell::detect_shell();
    let rc = shell::rc_file_path(&sh)?;

    match sh {
        shell::Shell::Fish => shell::inject_fish(&rc)?,
        _ => shell::inject_bash_zsh(&rc)?,
    }

    println!("gitreg initialized.");
    println!("Shell shim written to: {}", rc.display());
    println!("Restart your shell or run:  source {}", rc.display());
    Ok(())
}

fn cmd_hook(path: &Path) -> Result<()> {
    let db = open_db()?;
    hook::run(path, &db)
}

fn cmd_ls() -> Result<()> {
    let db = open_db()?;
    let records = db.list()?;
    if records.is_empty() {
        println!("No repositories tracked yet.");
        return Ok(());
    }
    let rows: Vec<LsRow> = records.into_iter().map(LsRow::from).collect();
    println!("{}", Table::new(rows));
    Ok(())
}

fn cmd_prune() -> Result<()> {
    let db = open_db()?;
    let removed = db.prune()?;
    if removed.is_empty() {
        println!("Nothing to prune.");
    } else {
        for p in &removed {
            println!("Removed: {p}");
        }
        println!(
            "Pruned {} entr{}.",
            removed.len(),
            if removed.len() == 1 { "y" } else { "ies" }
        );
    }
    Ok(())
}

fn cmd_rm(raw_path: &Path) -> Result<()> {
    let canonical = dunce::canonicalize(raw_path)
        .map_err(|_| GitregError::PathNotFound(raw_path.to_path_buf()))?;
    let canonical_str = canonical
        .to_str()
        .ok_or_else(|| GitregError::PathNotFound(canonical.clone()))?;

    let db = open_db()?;
    let deleted = db.remove(canonical_str)?;

    let marker = canonical.join(".git").join("gitreg_tracked");
    if marker.exists() {
        std::fs::remove_file(&marker)?;
    }

    if deleted {
        println!("Removed: {canonical_str}");
    } else {
        eprintln!("Not tracked: {canonical_str}");
        process::exit(1);
    }
    Ok(())
}

fn cmd_scan(dir: &Path, max_depth: usize) -> Result<()> {
    use std::collections::VecDeque;

    let start =
        dunce::canonicalize(dir).map_err(|_| GitregError::PathNotFound(dir.to_path_buf()))?;
    let db = open_db()?;

    println!("Scanning {} (depth: {}) ...", start.display(), max_depth);

    let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
    queue.push_back((start, 0));

    let mut found = 0usize;
    let mut warnings = 0usize;

    while let Some((current, depth)) = queue.pop_front() {
        if current.join(".git").is_dir() {
            let Some(s) = current.to_str() else {
                eprintln!("  warning: skipping non-UTF-8 path: {}", current.display());
                warnings += 1;
                continue;
            };
            match db.upsert(s) {
                Ok(()) => {
                    println!("  {}", s);
                    found += 1;
                }
                Err(e) => {
                    eprintln!("  warning: could not register {}: {}", s, e);
                    warnings += 1;
                }
            }
            // Don't recurse into a git repo; submodules are picked up naturally
            // on first `git` use via the shell hook.
            continue;
        }

        if depth >= max_depth {
            continue;
        }

        let entries = match std::fs::read_dir(&current) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => continue,
            Err(e) => {
                eprintln!("  warning: {}: {}", current.display(), e);
                warnings += 1;
                continue;
            }
        };

        for entry in entries.flatten() {
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            // file_type() does not follow symlinks — avoids loops and duplicate
            // registrations for symlinked directories.
            if ft.is_dir() && entry.file_name() != ".git" {
                queue.push_back((entry.path(), depth + 1));
            }
        }
    }

    if warnings > 0 {
        println!("\nDone. Registered {found} repositories ({warnings} warnings).");
    } else {
        println!("\nDone. Registered {found} repositories.");
    }
    Ok(())
}

fn main() {
    #[cfg(windows)]
    {
        if let Ok(exe) = std::env::current_exe() {
            let old = exe.with_extension("exe.old");
            if let Err(e) = std::fs::remove_file(&old) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("warning: could not remove {}: {e}", old.display());
                }
            }
        }
    }

    let cli = Cli::parse();

    match &cli.command {
        Commands::Hook { path } => {
            if let Err(e) = cmd_hook(path) {
                log_hook_error(&e);
            }
        }
        Commands::Init => {
            if let Err(e) = cmd_init() {
                eprintln!("Error: {e}");
                process::exit(1);
            }
        }
        Commands::Ls => {
            if let Err(e) = cmd_ls() {
                eprintln!("Error: {e}");
                process::exit(1);
            }
        }
        Commands::Prune => {
            if let Err(e) = cmd_prune() {
                eprintln!("Error: {e}");
                process::exit(1);
            }
        }
        Commands::Rm { path } => {
            if let Err(e) = cmd_rm(path) {
                eprintln!("Error: {e}");
                process::exit(1);
            }
        }
        Commands::Scan { dir, depth } => {
            const MAX_SCAN_DEPTH: usize = 20;
            if *depth > MAX_SCAN_DEPTH {
                eprintln!("Error: --depth must be {MAX_SCAN_DEPTH} or less");
                process::exit(1);
            }
            let dir = match dir {
                Some(d) => d.clone(),
                None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            };
            if let Err(e) = cmd_scan(&dir, *depth) {
                eprintln!("Error: {e}");
                process::exit(1);
            }
        }
        Commands::Upgrade => {
            if let Err(e) = upgrade::run() {
                eprintln!("Error: {e}");
                process::exit(1);
            }
        }
    }
}
