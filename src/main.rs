mod cli;
mod db;
mod error;
mod event;
mod hook;
mod shell;
mod upgrade;

use chrono::{Local, TimeZone, Utc};
use clap::Parser;
use cli::{Cli, Commands, ConfigAction, ExcludeAction, IntegratorAction, RepoAction, SystemAction};
use db::{Database, RepoRecord};
use error::{GitregError, Result};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process;
use tabled::{
    settings::{location::ByColumnName, Disable},
    Table, Tabled,
};

fn db_path() -> Result<PathBuf> {
    if let Ok(val) = std::env::var("GITREG_CONFIG_DIR") {
        return Ok(PathBuf::from(val).join("gitreg.db"));
    }
    let dir = dirs::config_dir().ok_or(GitregError::NoConfigDir)?;
    Ok(dir.join("gitreg").join("gitreg.db"))
}

fn open_db() -> Result<Database> {
    Database::open(&db_path()?)
}

fn log_path() -> Option<PathBuf> {
    if let Ok(val) = std::env::var("GITREG_CONFIG_DIR") {
        return Some(PathBuf::from(val).join("gitreg.log"));
    }
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
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Path")]
    path: String,
    #[tabled(rename = "Tags")]
    tags: String,
    #[tabled(rename = "Emergency Push")]
    emergency: String,
    #[tabled(rename = "Last Git Cmd")]
    last_seen: String,
}

impl LsRow {
    fn new(r: RepoRecord) -> Self {
        let secs = r.last_seen / 1000;
        let nsecs = ((r.last_seen % 1000) * 1_000_000) as u32;
        let dt = Utc
            .timestamp_opt(secs, nsecs)
            .single()
            .unwrap_or_default()
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        Self {
            id: r.id,
            name: r.name.unwrap_or_default(),
            path: r.path,
            tags: r.tags.join(", "),
            emergency: r.emergency_branch.unwrap_or_default(),
            last_seen: dt,
        }
    }
}

fn cmd_commands() {
    println!("Usage: gitreg <COMMAND>\n");

    println!("Common Commands:");
    println!("  commands               Print all available commands categorized by type");
    println!("  init                   Initialize gitreg and inject the shell shim");
    println!("  ls                     List all tracked repositories");
    println!("  git                    Run a git command across multiple repositories");
    println!("  emergency              Force push uncommitted code to an emergency branch");
    println!();

    println!("Repository Management:");
    println!(
        "  repo scan              Scan a directory tree and register all found git repositories"
    );
    println!("  repo tag               Add a tag to a repository");
    println!("  repo untag             Remove a tag from a repository");
    println!("  repo rm                Remove a specific repository from the registry");
    println!(
        "  repo prune             Remove entries for repositories that no longer exist on disk"
    );
    println!();

    println!("Configuration:");
    println!("  config alias           Enable \"gr\" alias for \"gitreg\"");
    println!("  config autoprune       Manage daily autoprune settings");
    println!("  config exclude add     Add a path to the exclusion list");
    println!("  config exclude rm      Remove a path from the exclusion list");
    println!("  config exclude ls      List all excluded paths");
    println!();

    println!("Integrations:");
    println!("  integrator register    Register an app for an event");
    println!("  integrator unregister  Unregister an app from an event");
    println!("  integrator ls          List all registered apps and their events");
    println!("  integrator events      List all available events");
    println!("  integrator block       Block an app from receiving any events");
    println!("  integrator unblock     Unblock a previously blocked app");
    println!("  integrator rm          Remove an app and all its event registrations");
    println!();

    println!("System:");
    println!("  system upgrade         Check for a newer release and upgrade the binary in place");
    println!("  system version         Show the current version");
    println!("  system webpage         Open the github webpage");
    println!("  system uninstall       Completely uninstall gitreg");
}

fn cmd_init() -> Result<()> {
    let sh = shell::detect_shell();

    #[cfg(windows)]
    if let shell::ShellKind::PowerShell = &sh {
        let paths = shell::powershell_profile_paths()?;
        let mut injected: Vec<PathBuf> = Vec::new();
        for path in &paths {
            match shell::inject_powershell(path) {
                Ok(()) => injected.push(path.clone()),
                Err(GitregError::AlreadyInitialized(_)) => {}
                Err(e) => return Err(e),
            }
        }
        if injected.is_empty() {
            return Err(GitregError::AlreadyInitialized(paths[0].clone()));
        }
        println!("gitreg initialized.");
        for path in &injected {
            println!("Shell shim written to: {}", path.display());
        }
        println!("Restart your shell or run in each active terminal:");
        for path in &injected {
            println!("  . '{}'", path.display());
        }
        return Ok(());
    }

    let rc = shell::rc_file_path(&sh)?;
    let reload_hint = match sh {
        shell::ShellKind::Fish => {
            shell::inject_fish(&rc)?;
            format!("source {}", rc.display())
        }
        #[cfg(windows)]
        shell::ShellKind::PowerShell => unreachable!(),
        _ => {
            shell::inject_bash_zsh(&rc)?;
            format!("source {}", rc.display())
        }
    };

    println!("gitreg initialized.");
    println!("Shell shim written to: {}", rc.display());
    println!("Restart your shell or run:  {reload_hint}");
    Ok(())
}

fn cmd_hook(path: &Path) -> Result<()> {
    let db = open_db()?;
    let _ = check_and_run_autoprune(&db);
    if hook::run(path, &db)? {
        let path_str = path.to_str().unwrap_or_default();
        event::dispatch(&db, "registered", json!({ "path": path_str }));
    }
    Ok(())
}
fn cmd_git(targets: &str, realtime: bool, git_args: &[String]) -> Result<()> {
    let db = open_db()?;
    let repos = db.resolve_many(targets)?;

    if repos.is_empty() {
        println!("No repositories found matching '{}'", targets);
        return Ok(());
    }

    if let Some(cmd) = git_args.first() {
        let event_name = format!("git.{}", cmd);
        let repo_paths: Vec<_> = repos.iter().map(|r| &r.path).collect();
        event::dispatch(
            &db,
            &event_name,
            json!({ "args": git_args, "repos": repo_paths }),
        );
    }

    if repos.len() == 1 {
        let repo = &repos[0];
        let mut child = process::Command::new("git")
            .args(git_args)
            .current_dir(&repo.path)
            .spawn()?;
        let status = child.wait()?;
        if !status.success() {
            process::exit(status.code().unwrap_or(1));
        }
        return Ok(());
    }

    run_parallel_git(repos, git_args, realtime)
}

#[derive(Debug)]
#[allow(dead_code)]
struct ExecResult {
    repo: RepoRecord,
    success: bool,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run_parallel_git(repos: Vec<RepoRecord>, git_args: &[String], realtime: bool) -> Result<()> {
    let mut current_repos = repos;
    loop {
        let results = execute_batch(&current_repos, git_args, realtime)?;
        let failed: Vec<_> = results.iter().filter(|r| !r.success).collect();

        if failed.is_empty() {
            break;
        }

        println!(
            "\nSummary: {} succeeded, {} failed",
            current_repos.len() - failed.len(),
            failed.len()
        );
        for f in &failed {
            let name = f.repo.name.as_deref().unwrap_or(&f.repo.path);
            println!(
                "  [{}] {} -> Exit code {}",
                f.repo.id,
                name,
                f.exit_code.unwrap_or(1)
            );
        }

        println!("\nOptions: [r]etry failed, [s]hell into first failed, [q]uit");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        match input.trim().to_lowercase().as_str() {
            "r" => {
                current_repos = failed.into_iter().map(|f| f.repo.clone()).collect();
                continue;
            }
            "s" => {
                let first = &failed[0].repo;
                spawn_shell(&first.path)?;
                // After shell, we offer to retry the failed set
                current_repos = failed.into_iter().map(|f| f.repo.clone()).collect();
                continue;
            }
            _ => break,
        }
    }
    Ok(())
}

fn execute_batch(
    repos: &[RepoRecord],
    git_args: &[String],
    realtime: bool,
) -> Result<Vec<ExecResult>> {
    use std::sync::{Arc, Mutex};
    let results = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::new();

    for repo in repos {
        let repo = repo.clone();
        let args = git_args.to_vec();
        let results = Arc::clone(&results);

        let handle = std::thread::spawn(move || {
            let res = if realtime {
                execute_realtime(&repo, &args)
            } else {
                execute_buffered(&repo, &args)
            };
            let mut results = results.lock().unwrap();
            results.push(res);
        });
        handles.push(handle);
    }

    for h in handles {
        let _ = h.join();
    }

    let mut final_results = Arc::try_unwrap(results).unwrap().into_inner().unwrap();
    final_results.sort_by_key(|r| r.repo.id);
    Ok(final_results)
}

fn execute_buffered(repo: &RepoRecord, args: &[String]) -> ExecResult {
    let output = process::Command::new("git")
        .args(args)
        .current_dir(&repo.path)
        .output();

    let mut out_buf = String::new();
    let name = repo.name.as_deref().unwrap_or(&repo.path);
    out_buf.push_str(&format!("=== {} ===\n", name));

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);

            if !stdout.is_empty() {
                out_buf.push_str(&stdout);
                if !out_buf.ends_with('\n') {
                    out_buf.push('\n');
                }
            }
            if !stderr.is_empty() {
                out_buf.push_str(&stderr);
                if !out_buf.ends_with('\n') {
                    out_buf.push('\n');
                }
            }
            print!("{}", out_buf);

            ExecResult {
                repo: repo.clone(),
                success: out.status.success(),
                exit_code: out.status.code(),
                stdout: stdout.into_owned(),
                stderr: stderr.into_owned(),
            }
        }
        Err(e) => {
            let err_msg = format!("Error: {e}\n");
            out_buf.push_str(&err_msg);
            print!("{}", out_buf);

            ExecResult {
                repo: repo.clone(),
                success: false,
                exit_code: None,
                stdout: String::new(),
                stderr: err_msg,
            }
        }
    }
}

fn execute_realtime(repo: &RepoRecord, args: &[String]) -> ExecResult {
    use std::io::{BufRead, BufReader};

    let mut child = match process::Command::new("git")
        .args(args)
        .current_dir(&repo.path)
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExecResult {
                repo: repo.clone(),
                success: false,
                exit_code: None,
                stdout: String::new(),
                stderr: e.to_string(),
            };
        }
    };

    let name = repo.name.as_deref().unwrap_or(&repo.path).to_string();
    let prefix = format!("[{}] ", name);

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let prefix_clone = prefix.clone();
    let t_out = std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(|r| r.ok()) {
            println!("{}{}", prefix_clone, line);
        }
    });

    let prefix_clone = prefix.clone();
    let t_err = std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(|r| r.ok()) {
            eprintln!("{}{}", prefix_clone, line);
        }
    });

    let _ = t_out.join();
    let _ = t_err.join();

    let status = child.wait().unwrap();
    ExecResult {
        repo: repo.clone(),
        success: status.success(),
        exit_code: status.code(),
        stdout: String::new(), // Not captured in realtime mode to save memory
        stderr: String::new(),
    }
}

fn spawn_shell(path: &str) -> Result<()> {
    println!("Spawning shell in {}...", path);
    let shell = if cfg!(windows) {
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string())
    };

    let mut child = process::Command::new(shell).current_dir(path).spawn()?;
    child.wait()?;
    Ok(())
}
fn cmd_ls(json: bool) -> Result<()> {
    let db = open_db()?;
    let records = db.list()?;

    if json {
        println!("{}", serde_json::to_string_pretty(&records).unwrap());
        return Ok(());
    }

    let exclusions = db.list_exclusions()?;

    let has_emergency = records.iter().any(|r| r.emergency_branch.is_some());
    if has_emergency {
        println!("⚠️  ATTENTION: Emergency pushes detected in some repositories!");
        println!("   Review changes and run 'gitreg emergency --clear' to dismiss.\n");
    }

    if records.is_empty() {
        if !exclusions.is_empty() {
            println!(
                "No repositories tracked yet ({} excluded paths).",
                exclusions.len()
            );
        } else {
            println!("No repositories tracked yet.");
        }
        return Ok(());
    }

    if !exclusions.is_empty() {
        println!(
            "{} git dirs ({} excluded paths)",
            records.len(),
            exclusions.len()
        );
    } else {
        println!("{} git dirs", records.len());
    }
    let rows: Vec<LsRow> = records.into_iter().map(LsRow::new).collect();
    let mut table = Table::new(rows);
    if !has_emergency {
        table.with(Disable::column(ByColumnName::new("Emergency Push")));
    }
    println!("{}", table);
    Ok(())
}

fn cmd_integrator(action: &IntegratorAction) -> Result<()> {
    let db = open_db()?;
    match action {
        IntegratorAction::Register { app, event, socket } => {
            db.register_integration(app, event, socket)?;
            println!("Registered '{app}' for event '{event}' via '{socket}'");
        }
        IntegratorAction::Unregister { app, event } => {
            if db.unregister_integration(app, event)? {
                println!("Unregistered '{app}' from event '{event}'");
            } else {
                println!("Registration not found for '{app}' and event '{event}'");
            }
        }
        IntegratorAction::Ls => {
            let list = db.list_integrations()?;
            if list.is_empty() {
                println!("No integrator applications registered.");
            } else {
                #[derive(Tabled)]
                struct IntegrationRow {
                    #[tabled(rename = "App")]
                    app: String,
                    #[tabled(rename = "Blocked")]
                    blocked: String,
                    #[tabled(rename = "Event")]
                    event: String,
                    #[tabled(rename = "Socket")]
                    socket: String,
                }
                let rows: Vec<IntegrationRow> = list
                    .into_iter()
                    .map(|i| IntegrationRow {
                        app: i.app_name,
                        blocked: if i.is_blocked {
                            "yes".to_string()
                        } else {
                            "no".to_string()
                        },
                        event: i.event,
                        socket: i.socket_path,
                    })
                    .collect();
                println!("{}", Table::new(rows));
            }
        }
        IntegratorAction::Events => {
            println!("Available events:");
            println!("  registered    - When a new repository is registered");
            println!("  removed       - When a repository is removed from registry");
            println!("  tagged        - When a tag is added to a repository");
            println!("  untagged      - When a tag is removed from a repository");
            println!("  upgraded      - When gitreg is upgraded");
            println!("  git.<COMMAND> - When a specific git command is run (e.g. git.commit)");
        }
        IntegratorAction::Block { app } => {
            if db.block_app(app)? {
                println!("App '{app}' blocked.");
            } else {
                println!("App '{app}' not found.");
            }
        }
        IntegratorAction::Unblock { app } => {
            if db.unblock_app(app)? {
                println!("App '{app}' unblocked.");
            } else {
                println!("App '{app}' not found.");
            }
        }
        IntegratorAction::Rm { app } => {
            if db.remove_app(app)? {
                println!("App '{app}' and all its registrations removed.");
            } else {
                println!("App '{app}' not found.");
            }
        }
    }
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
            event::dispatch(&db, "removed", json!({ "path": p }));
        }
        println!(
            "Pruned {} entr{}.",
            removed.len(),
            if removed.len() == 1 { "y" } else { "ies" }
        );
    }
    Ok(())
}

fn cmd_autoprune(enable: bool, disable: bool, time: Option<String>) -> Result<()> {
    let db = open_db()?;

    if enable {
        db.set_setting("autoprune_enabled", "true")?;
    } else if disable {
        db.set_setting("autoprune_enabled", "false")?;
    }

    if let Some(t) = time {
        // Basic validation HH:MM
        let parts: Vec<&str> = t.split(':').collect();
        if parts.len() != 2
            || parts[0].parse::<u32>().is_err()
            || parts[1].parse::<u32>().is_err()
            || parts[0].len() != 2
            || parts[1].len() != 2
        {
            return Err(GitregError::InvalidFormat(
                "Use HH:MM (e.g., 00:00 or 13:45)".to_string(),
            ));
        }
        db.set_setting("autoprune_time", &t)?;
    }

    let enabled = db.is_autoprune_enabled()?;
    let sched_time = db.get_autoprune_time()?;

    println!("Daily autoprune settings:");
    println!("  Enabled: {}", if enabled { "yes" } else { "no" });
    println!("  Time:    {}", sched_time);

    Ok(())
}

fn check_and_run_autoprune(db: &Database) -> Result<()> {
    if !db.is_autoprune_enabled()? {
        return Ok(());
    }

    let now = Local::now();
    let current_date = now.format("%Y-%m-%d").to_string();
    let current_time = now.format("%H:%M").to_string();

    let last_date = db.get_last_autoprune_date()?;
    if last_date.as_deref() == Some(&current_date) {
        return Ok(());
    }

    let sched_time = db.get_autoprune_time()?;
    if current_time >= sched_time {
        let _ = db.prune();
        db.set_last_autoprune_date(&current_date)?;
    }

    Ok(())
}

fn resolve_id(db: &Database, target: &str) -> Result<i64> {
    match db.resolve_target(target)? {
        Some(id) => Ok(id),
        None => {
            let canon = dunce::canonicalize(target)
                .ok()
                .and_then(|p| p.into_os_string().into_string().ok());
            let resolved = canon
                .as_deref()
                .map(|s| db.resolve_target(s))
                .transpose()?
                .flatten();
            resolved.ok_or_else(|| GitregError::NotFound(target.to_owned()))
        }
    }
}

fn cmd_rm(target: &str) -> Result<()> {
    let db = open_db()?;
    let id = resolve_id(&db, target)?;

    let path = db
        .remove_by_id(id)?
        .ok_or_else(|| GitregError::NotFound(target.to_owned()))?;

    println!("Removed: {path}");
    event::dispatch(&db, "removed", json!({ "path": path }));
    Ok(())
}

fn get_autoscan_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return paths,
    };

    #[cfg(target_os = "windows")]
    {
        paths.push(home.join("source/repos"));
        paths.push(home.join("Documents/GitHub"));
        paths.push(home.join("Projects"));
        paths.push(PathBuf::from("C:\\dev"));
        paths.push(PathBuf::from("C:\\git"));
    }

    #[cfg(target_os = "macos")]
    {
        paths.push(home.join("Developer"));
        paths.push(home.join("Documents/GitHub"));
        paths.push(home.join("src"));
        paths.push(home.join("git"));
        paths.push(home.join("Projects"));
    }

    #[cfg(target_os = "linux")]
    {
        paths.push(home.join("git"));
        paths.push(home.join("projects"));
        paths.push(home.join("src"));

        let go_github = home.join("go/src/github.com");
        if go_github.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&go_github) {
                for entry in entries.flatten() {
                    if let Ok(ft) = entry.file_type() {
                        if ft.is_dir() {
                            paths.push(entry.path());
                        }
                    }
                }
            }
        }
    }

    paths
}

fn cmd_autoscan() -> Result<()> {
    let targets = get_autoscan_paths();
    let db = open_db()?;
    let mut found = 0;

    println!("Running auto-discovery scan...");

    for target in targets {
        if !target.is_dir() {
            continue;
        }

        let canonical = match dunce::canonicalize(&target) {
            Ok(p) => p,
            Err(_) => continue,
        };

        let canonical_str = match canonical.to_str() {
            Some(s) => s,
            None => continue,
        };

        // Check if target itself is a repo
        if canonical.join(".git").is_dir() {
            if !db.is_excluded(canonical_str)? && db.upsert(canonical_str, None)? {
                println!("  {}", canonical_str);
                event::dispatch(&db, "registered", json!({ "path": canonical_str }));
                found += 1;
            }
            continue; // Don't scan inside a repo
        }

        // Scan depth 1
        let entries = match std::fs::read_dir(&canonical) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();

            if name == "node_modules" || name == ".git" {
                continue;
            }

            if let Ok(ft) = entry.file_type() {
                if ft.is_dir() && path.join(".git").is_dir() {
                    if let Some(s) = path.to_str() {
                        if !db.is_excluded(s)? && db.upsert(s, None)? {
                            println!("  {}", s);
                            event::dispatch(&db, "registered", json!({ "path": s }));
                            found += 1;
                        }
                    }
                }
            }
        }
    }

    println!("\nAuto-discovery complete. Registered {found} repositories.");
    Ok(())
}

fn cmd_scan(dir: &Path, max_depth: usize) -> Result<()> {
    use std::collections::VecDeque;

    let start =
        dunce::canonicalize(dir).map_err(|_| GitregError::PathNotFound(dir.to_path_buf()))?;
    let db = open_db()?;
    let exclusions = db.list_exclusions()?;

    println!("Scanning {} (depth: {}) ...", start.display(), max_depth);

    let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
    queue.push_back((start, 0));

    let mut found = 0usize;
    let mut warnings = 0usize;

    while let Some((current, depth)) = queue.pop_front() {
        let current_str = match current.to_str() {
            Some(s) => s,
            None => {
                eprintln!("  warning: skipping non-UTF-8 path: {}", current.display());
                warnings += 1;
                continue;
            }
        };

        if db.is_path_excluded(current_str, &exclusions) {
            continue;
        }

        if current.join(".git").is_dir() {
            match db.upsert(current_str, None) {
                Ok(true) => {
                    println!("  {}", current_str);
                    event::dispatch(&db, "registered", json!({ "path": current_str }));
                    found += 1;
                }
                Ok(false) => {
                    // Already registered, update timestamp only (handled by upsert)
                }
                Err(e) => {
                    eprintln!("  warning: could not register {}: {}", current_str, e);
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

fn cmd_tag(target: &str, tag: &str) -> Result<()> {
    let db = open_db()?;
    let id = resolve_id(&db, target)?;
    db.add_tag(id, tag)?;
    println!("added tag '{tag}'");
    event::dispatch(&db, "tagged", json!({ "target": target, "tag": tag }));
    Ok(())
}

fn cmd_untag(target: &str, tag: &str) -> Result<()> {
    let db = open_db()?;
    let id = resolve_id(&db, target)?;
    db.remove_tag(id, tag)?;
    println!("removed tag '{tag}'");
    event::dispatch(&db, "untagged", json!({ "target": target, "tag": tag }));
    Ok(())
}

fn sanitize_name(s: &str) -> String {
    let mut res = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            res.push(c);
        } else {
            res.push('_');
        }
    }
    res.replace("__", "_").trim_matches('_').to_string()
}

fn cmd_emergency(clear: bool) -> Result<()> {
    let db = open_db()?;
    if clear {
        db.clear_all_emergency_branches()?;
        println!("All emergency push notifications cleared.");
        return Ok(());
    }

    let repos = db.list()?;
    if repos.is_empty() {
        println!("No repositories tracked.");
        return Ok(());
    }

    let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
    let hostname = if cfg!(windows) {
        std::env::var("COMPUTERNAME").unwrap_or_else(|_| "host".to_string())
    } else {
        std::env::var("HOSTNAME").unwrap_or_else(|_| "host".to_string())
    };
    let hostname = sanitize_name(&hostname);

    let git_user = process::Command::new("git")
        .args(["config", "user.name"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "user".to_string());
    let git_user = sanitize_name(&git_user);

    println!(
        "Starting emergency push for {} repositories...",
        repos.len()
    );

    for repo in repos {
        let name = repo.name.as_deref().unwrap_or(&repo.path);
        print!("  {} ... ", name);
        use std::io::Write;
        std::io::stdout().flush()?;

        // 1. Get current branch
        let current_branch = process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(&repo.path)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|_| "HEAD".to_string());

        let safe_branch = sanitize_name(&current_branch);
        let emergency_branch = format!(
            "{}_emergency_{}_{}_{}",
            safe_branch, git_user, hostname, timestamp
        );

        // 2. Create and switch to emergency branch
        let ok = process::Command::new("git")
            .args(["checkout", "-b", &emergency_branch])
            .current_dir(&repo.path)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if !ok {
            println!("failed to create branch");
            continue;
        }

        // 3. Add all changes
        let _ = process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&repo.path)
            .status();

        // 4. Commit (skip hooks)
        let _commit_ok = process::Command::new("git")
            .args([
                "commit",
                "-m",
                &format!("EMERGENCY SAVE: {}", timestamp),
                "--no-verify",
            ])
            .current_dir(&repo.path)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        // 5. Force push (skip hooks)
        let push_ok = process::Command::new("git")
            .args([
                "push",
                "origin",
                &emergency_branch,
                "--force",
                "--no-verify",
            ])
            .current_dir(&repo.path)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if push_ok {
            db.set_emergency_branch(repo.id, &emergency_branch)?;
            println!("pushed to {}", emergency_branch);
        } else {
            println!("push failed");
        }

        // 6. Return to original branch
        let _ = process::Command::new("git")
            .args(["checkout", &current_branch])
            .current_dir(&repo.path)
            .status();
    }

    println!("\nEmergency push complete.");
    println!("Run 'gitreg emergency --clear' to dismiss notifications.");

    Ok(())
}

fn cmd_exclude(action: &ExcludeAction) -> Result<()> {
    let db = open_db()?;
    match action {
        ExcludeAction::Add { path } => {
            let canon =
                dunce::canonicalize(path).map_err(|_| GitregError::PathNotFound(path.clone()))?;
            let s = canon.to_str().ok_or_else(|| {
                GitregError::InvalidFormat("Non-UTF-8 path not supported".to_string())
            })?;
            db.add_exclusion(s)?;
            println!("Added exclusion: {s}");
            let pruned = db.remove_by_exclusion(s)?;
            if pruned > 0 {
                println!(
                    "Pruned {pruned} existing registr{} matching this exclusion.",
                    if pruned == 1 { "ation" } else { "ations" }
                );
            }
        }
        ExcludeAction::Rm { path } => {
            let s = match dunce::canonicalize(path) {
                Ok(canon) => canon.to_str().map(|s| s.to_string()),
                Err(_) => path.to_str().map(|s| s.to_string()),
            }
            .ok_or_else(|| {
                GitregError::InvalidFormat("Non-UTF-8 path not supported".to_string())
            })?;

            if db.remove_exclusion(&s)? {
                println!("Removed exclusion: {s}");
            } else {
                println!("Exclusion not found: {s}");
            }
        }
        ExcludeAction::Ls => {
            let list = db.list_exclusions()?;
            if list.is_empty() {
                println!("No exclusions configured.");
            } else {
                println!("Exclusions:");
                for p in list {
                    println!("  {p}");
                }
            }
        }
    }
    Ok(())
}

fn cmd_alias() -> Result<()> {
    let sh = shell::detect_shell();

    if shell::is_alias_enabled(&sh)? {
        println!("Alias 'gr' is already enabled.");
        return Ok(());
    }

    if shell::check_alias_conflict(&sh)? {
        println!("The 'gr' command is already being used by another tool.");
        return Ok(());
    }

    #[cfg(windows)]
    if let shell::ShellKind::PowerShell = &sh {
        let paths = shell::powershell_profile_paths()?;
        let mut injected: Vec<PathBuf> = Vec::new();
        for path in &paths {
            shell::inject_alias_powershell(path)?;
            injected.push(path.clone());
        }
        println!("Alias 'gr' enabled.");
        for path in &injected {
            println!("Alias written to: {}", path.display());
        }
        println!("Restart your shell or run in each active terminal:");
        for path in &injected {
            println!("  . '{}'", path.display());
        }
        return Ok(());
    }

    let rc = match &sh {
        shell::ShellKind::Fish => {
            let path = shell::rc_file_path(&sh)?;
            shell::inject_alias_fish(&path)?;
            path
        }
        _ => {
            let path = shell::rc_file_path(&sh)?;
            shell::inject_alias_bash_zsh(&path)?;
            path
        }
    };

    println!("Alias 'gr' enabled.");
    println!("Alias written to: {}", rc.display());
    println!("Restart your shell or run:  source {}", rc.display());

    Ok(())
}

fn cmd_version() {
    println!("gitreg {}", env!("CARGO_PKG_VERSION"));
}

fn cmd_webpage() -> Result<()> {
    let url = "https://github.com/dpkay-io/gitreg";
    let status = if cfg!(windows) {
        process::Command::new("cmd")
            .arg("/c")
            .arg("start")
            .arg(url)
            .status()
    } else if cfg!(target_os = "macos") {
        process::Command::new("open").arg(url).status()
    } else {
        process::Command::new("xdg-open").arg(url).status()
    };

    match status {
        Ok(s) if s.success() => Ok(()),
        _ => {
            println!("Could not open browser. Please visit: {url}");
            Ok(())
        }
    }
}

fn cmd_uninstall() -> Result<()> {
    println!("This will completely remove gitreg, including its database and shell shims.");
    println!("To confirm, please type 'UNINSTALL' (all caps):");

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    if input.trim() != "UNINSTALL" {
        println!("Uninstallation cancelled.");
        return Ok(());
    }

    println!("Uninstalling...");

    // 1. Shell shims
    let sh = shell::detect_shell();
    let paths = match sh {
        shell::ShellKind::Bash | shell::ShellKind::Zsh | shell::ShellKind::Fish => {
            vec![shell::rc_file_path(&sh)?]
        }
        #[cfg(windows)]
        shell::ShellKind::PowerShell => shell::powershell_profile_paths()?,
    };

    for path in paths {
        if path.exists() {
            println!("Removing shims from: {}", path.display());
            shell::remove_all_gitreg_blocks(&path)?;
        }
    }

    // 2. Config dir
    let config_dir = dirs::config_dir()
        .ok_or(GitregError::NoConfigDir)?
        .join("gitreg");
    if config_dir.exists() {
        println!("Removing configuration directory: {}", config_dir.display());
        std::fs::remove_dir_all(&config_dir)?;
    }

    // 3. Binary
    let exe = std::env::current_exe()?;
    println!("Removing binary: {}", exe.display());

    #[cfg(not(windows))]
    {
        std::fs::remove_file(&exe)?;
        println!("Uninstallation complete.");
        Ok(())
    }

    #[cfg(windows)]
    {
        let exe_str = exe
            .to_str()
            .ok_or_else(|| GitregError::InvalidFormat("Non-UTF-8 path".to_string()))?;
        process::Command::new("cmd")
            .arg("/c")
            .arg(format!(
                "start /b cmd /c (timeout /t 1 ^& del \"{}\")",
                exe_str
            ))
            .spawn()?;
        println!("Uninstallation complete.");
        process::exit(0);
    }
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
        Commands::Overview => {
            cmd_commands();
        }
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
        Commands::Autoscan => {
            if let Err(e) = cmd_autoscan() {
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
        Commands::Tag { target, tag } => {
            if let Err(e) = cmd_tag(target, tag) {
                eprintln!("Error: {e}");
                process::exit(1);
            }
        }
        Commands::Untag { target, tag } => {
            if let Err(e) = cmd_untag(target, tag) {
                eprintln!("Error: {e}");
                process::exit(1);
            }
        }
        Commands::Rm { target } => {
            if let Err(e) = cmd_rm(target) {
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
        Commands::Emergency { clear } => {
            if let Err(e) = cmd_emergency(*clear) {
                eprintln!("Error: {e}");
                process::exit(1);
            }
        }
        Commands::Ls { json } => {
            if let Err(e) = cmd_ls(*json) {
                eprintln!("Error: {e}");
                process::exit(1);
            }
        }
        Commands::Integrator(action) => {
            if let Err(e) = cmd_integrator(action) {
                eprintln!("Error: {e}");
                process::exit(1);
            }
        }
        Commands::Git {
            targets,
            realtime,
            git_args,
        } => {
            if let Err(e) = cmd_git(targets, *realtime, git_args) {
                eprintln!("Error: {e}");
                process::exit(1);
            }
        }
        Commands::Repo(action) => match action {
            RepoAction::Ls { json } => {
                if let Err(e) = cmd_ls(*json) {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }
            }
            RepoAction::Scan { dir, depth } => {
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
            RepoAction::Tag { target, tag } => {
                if let Err(e) = cmd_tag(target, tag) {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }
            }
            RepoAction::Untag { target, tag } => {
                if let Err(e) = cmd_untag(target, tag) {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }
            }
            RepoAction::Rm { target } => {
                if let Err(e) = cmd_rm(target) {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }
            }
            RepoAction::Prune => {
                if let Err(e) = cmd_prune() {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }
            }
        },
        Commands::Config(action) => match action {
            ConfigAction::Alias => {
                if let Err(e) = cmd_alias() {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }
            }
            ConfigAction::Autoprune {
                enable,
                disable,
                time,
            } => {
                if let Err(e) = cmd_autoprune(*enable, *disable, time.clone()) {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }
            }
            ConfigAction::Exclude(action) => {
                if let Err(e) = cmd_exclude(action) {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }
            }
        },
        Commands::System(action) => match action {
            SystemAction::Upgrade => {
                if let Err(e) = upgrade::run() {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }
                if let Ok(db) = open_db() {
                    event::dispatch(
                        &db,
                        "upgraded",
                        json!({ "version": env!("CARGO_PKG_VERSION") }),
                    );
                }
            }
            SystemAction::Version => {
                cmd_version();
            }
            SystemAction::Webpage => {
                if let Err(e) = cmd_webpage() {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }
            }
            SystemAction::Uninstall => {
                if let Err(e) = cmd_uninstall() {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }
            }
        },
    }
}
