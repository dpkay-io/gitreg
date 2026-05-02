use crate::error::{GitregError, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub enum ShellKind {
    Bash,
    Zsh,
    Fish,
    #[cfg(windows)]
    PowerShell,
}

const GUARD_START: &str = "# >>> gitreg-start >>>";
const GUARD_END: &str = "# <<< gitreg-end <<<";

const ALIAS_GUARD_START: &str = "# >>> gitreg-alias-start >>>";
const ALIAS_GUARD_END: &str = "# <<< gitreg-alias-end <<<";

pub fn detect_shell() -> ShellKind {
    let shell = std::env::var("SHELL").unwrap_or_default();
    if shell.contains("zsh") {
        return ShellKind::Zsh;
    }
    if shell.contains("fish") {
        return ShellKind::Fish;
    }
    if !shell.is_empty() {
        // $SHELL is set: treat as bash-compatible (Git Bash, Cygwin, MSYS2, …)
        return ShellKind::Bash;
    }
    // $SHELL not set — on Windows this means native PowerShell
    #[cfg(windows)]
    return ShellKind::PowerShell;
    #[cfg(not(windows))]
    ShellKind::Bash
}

pub fn rc_file_path(shell: &ShellKind) -> Result<PathBuf> {
    match shell {
        ShellKind::Bash => Ok(dirs::home_dir()
            .ok_or(GitregError::NoConfigDir)?
            .join(".bashrc")),
        ShellKind::Zsh => Ok(dirs::home_dir()
            .ok_or(GitregError::NoConfigDir)?
            .join(".zshrc")),
        ShellKind::Fish => Ok(dirs::config_dir()
            .ok_or(GitregError::NoConfigDir)?
            .join("fish")
            .join("functions")
            .join("git.fish")),
        #[cfg(windows)]
        ShellKind::PowerShell => {
            let docs = dirs::document_dir().ok_or(GitregError::NoConfigDir)?;
            // PSHOME points to the PowerShell installation directory.
            // Windows PowerShell 5.1 uses a path containing "WindowsPowerShell";
            // PowerShell 7+ does not.
            let ps_dir = if std::env::var("PSHOME")
                .unwrap_or_default()
                .contains("WindowsPowerShell")
            {
                "WindowsPowerShell"
            } else {
                "PowerShell"
            };
            Ok(docs.join(ps_dir).join("Microsoft.PowerShell_profile.ps1"))
        }
    }
}

/// Returns profile paths for both Windows PowerShell 5.1 and PowerShell 7+.
/// `gitreg init` injects into all of them so the shim works regardless of
/// which PS version the user runs, and regardless of which version was active
/// when `gitreg init` was called.
#[cfg(windows)]
pub fn powershell_profile_paths() -> Result<Vec<PathBuf>> {
    let docs = dirs::document_dir().ok_or(GitregError::NoConfigDir)?;
    Ok(vec![
        docs.join("WindowsPowerShell")
            .join("Microsoft.PowerShell_profile.ps1"),
        docs.join("PowerShell")
            .join("Microsoft.PowerShell_profile.ps1"),
    ])
}

fn bash_zsh_shim() -> String {
    format!(
        r#"{start}
git() {{
    local current_dir="$PWD"
    local git_root=""
    while [[ "$current_dir" != "/" ]]; do
        if [[ -d "$current_dir/.git" ]]; then git_root="$current_dir"; break; fi
        current_dir=$(dirname "$current_dir")
    done
    if [[ -z "$git_root" ]]; then command git "$@"; return; fi
    local marker="$git_root/.git/gitreg_tracked"
    local needs_reg=true
    if [[ -f "$marker" ]]; then
        local recorded_path; read -r recorded_path < "$marker"
        [[ "$recorded_path" == "$git_root" ]] && needs_reg=false
    fi
    if [[ "$needs_reg" == "true" ]]; then (gitreg hook --path "$git_root" > /dev/null 2>&1 & disown); fi
    command git "$@"
}}
{end}"#,
        start = GUARD_START,
        end = GUARD_END,
    )
}

fn fish_shim() -> String {
    format!(
        r#"{start}
function git
    set current_dir $PWD
    set git_root ""
    while test "$current_dir" != "/"
        if test -d "$current_dir/.git"
            set git_root $current_dir
            break
        end
        set current_dir (dirname $current_dir)
    end
    if test -z "$git_root"
        command git $argv
        return
    end
    set marker "$git_root/.git/gitreg_tracked"
    set needs_reg true
    if test -f "$marker"
        read -l recorded_path < "$marker"
        if test "$recorded_path" = "$git_root"
            set needs_reg false
        end
    end
    if test "$needs_reg" = "true"
        gitreg hook --path "$git_root" > /dev/null 2>&1 &
    end
    command git $argv
end
{end}"#,
        start = GUARD_START,
        end = GUARD_END,
    )
}

#[cfg(windows)]
fn powershell_shim() -> String {
    format!(
        r#"{start}
$script:_gitreg_git = (Get-Command git -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1).Source
if (-not $script:_gitreg_git) {{ $script:_gitreg_git = "git.exe" }}
function git {{
    $current_dir = $PWD.Path
    $git_root = $null
    while ($true) {{
        if (Test-Path (Join-Path $current_dir ".git")) {{
            $git_root = $current_dir
            break
        }}
        $parent = Split-Path $current_dir -Parent
        if (-not $parent -or $parent -eq $current_dir) {{ break }}
        $current_dir = $parent
    }}
    if ($null -eq $git_root) {{
        & $script:_gitreg_git @args
        return
    }}
    $marker = Join-Path $git_root ".git\gitreg_tracked"
    $needs_reg = $true
    if (Test-Path $marker) {{
        $recorded = (Get-Content $marker -TotalCount 1 -ErrorAction SilentlyContinue)
        if ($recorded -and $recorded.Trim() -eq $git_root) {{ $needs_reg = $false }}
    }}
    if ($needs_reg) {{
        $null = Start-Process "gitreg" -ArgumentList @("hook", "--path", $git_root) -WindowStyle Hidden -PassThru -ErrorAction SilentlyContinue
    }}
    & $script:_gitreg_git @args
}}
{end}"#,
        start = GUARD_START,
        end = GUARD_END,
    )
}

const MAX_RC_BYTES: u64 = 10 * 1024 * 1024;

fn read_rc_file(path: &Path) -> Result<String> {
    if path.exists() {
        if fs::metadata(path)?.len() > MAX_RC_BYTES {
            return Err(std::io::Error::other("rc file exceeds 10 MiB limit").into());
        }
        Ok(fs::read_to_string(path)?)
    } else {
        Ok(String::new())
    }
}

pub fn inject_bash_zsh(rc_path: &Path) -> Result<()> {
    let existing = read_rc_file(rc_path)?;

    if existing.contains(GUARD_START) {
        return Err(GitregError::AlreadyInitialized(rc_path.to_path_buf()));
    }

    let mut content = existing;
    if !content.ends_with('\n') && !content.is_empty() {
        content.push('\n');
    }
    content.push('\n');
    content.push_str(&bash_zsh_shim());
    content.push('\n');

    fs::write(rc_path, content)?;
    Ok(())
}

pub fn inject_fish(fish_path: &Path) -> Result<()> {
    let existing = {
        let content = read_rc_file(fish_path)?;
        if content.contains(GUARD_START) {
            return Err(GitregError::AlreadyInitialized(fish_path.to_path_buf()));
        }
        content
    };

    if let Some(parent) = fish_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut content = existing;
    if !content.ends_with('\n') && !content.is_empty() {
        content.push('\n');
    }
    content.push('\n');
    content.push_str(&fish_shim());
    content.push('\n');

    fs::write(fish_path, content)?;
    Ok(())
}

#[cfg(windows)]
pub fn inject_powershell(profile_path: &Path) -> Result<()> {
    let existing = read_rc_file(profile_path)?;

    if existing.contains(GUARD_START) {
        return Err(GitregError::AlreadyInitialized(profile_path.to_path_buf()));
    }

    if let Some(parent) = profile_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut content = existing;
    if !content.ends_with('\n') && !content.is_empty() {
        content.push('\n');
    }
    content.push('\n');
    content.push_str(&powershell_shim());
    content.push('\n');

    fs::write(profile_path, content)?;
    Ok(())
}

fn bash_zsh_alias_shim() -> String {
    format!(
        "{start}\nalias gr='gitreg'\n{end}",
        start = ALIAS_GUARD_START,
        end = ALIAS_GUARD_END
    )
}

fn fish_alias_shim() -> String {
    format!(
        "{start}\nfunction gr\n    gitreg $argv\nend\n{end}",
        start = ALIAS_GUARD_START,
        end = ALIAS_GUARD_END
    )
}

#[cfg(windows)]
fn powershell_alias_shim() -> String {
    format!(
        "{start}\nSet-Alias -Name gr -Value gitreg\n{end}",
        start = ALIAS_GUARD_START,
        end = ALIAS_GUARD_END
    )
}

pub fn check_alias_conflict(shell: &ShellKind) -> Result<bool> {
    // 1. Check PATH
    let cmd = if cfg!(windows) { "where" } else { "which" };
    let status = std::process::Command::new(cmd)
        .arg("gr")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    if let Ok(s) = status {
        if s.success() {
            return Ok(true);
        }
    }

    // 2. Check RC files (ignoring our own blocks)
    let paths = match shell {
        ShellKind::Bash | ShellKind::Zsh | ShellKind::Fish => vec![rc_file_path(shell)?],
        #[cfg(windows)]
        ShellKind::PowerShell => powershell_profile_paths()?,
    };

    for path in paths {
        if !path.exists() {
            continue;
        }
        let content = read_rc_file(&path)?;
        // Strip our own alias block before checking
        let mut stripped = content.clone();
        if let Some(start_idx) = stripped.find(ALIAS_GUARD_START) {
            if let Some(end_idx) = stripped.find(ALIAS_GUARD_END) {
                if end_idx > start_idx {
                    stripped.replace_range(start_idx..end_idx + ALIAS_GUARD_END.len(), "");
                }
            }
        }

        let conflict = match shell {
            ShellKind::Bash | ShellKind::Zsh => {
                stripped.contains("alias gr=") || stripped.contains("gr()")
            }
            ShellKind::Fish => stripped.contains("alias gr") || stripped.contains("function gr"),
            #[cfg(windows)]
            ShellKind::PowerShell => {
                stripped.contains("Set-Alias") && stripped.contains("gr")
                    || stripped.contains("function gr")
            }
        };

        if conflict {
            return Ok(true);
        }
    }

    Ok(false)
}

pub fn is_alias_enabled(shell: &ShellKind) -> Result<bool> {
    let paths = match shell {
        ShellKind::Bash | ShellKind::Zsh | ShellKind::Fish => vec![rc_file_path(shell)?],
        #[cfg(windows)]
        ShellKind::PowerShell => powershell_profile_paths()?,
    };

    for path in paths {
        if path.exists() {
            let content = read_rc_file(&path)?;
            if content.contains(ALIAS_GUARD_START) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub fn inject_alias_bash_zsh(rc_path: &Path) -> Result<()> {
    let mut content = read_rc_file(rc_path)?;
    if content.contains(ALIAS_GUARD_START) {
        return Ok(());
    }

    if !content.ends_with('\n') && !content.is_empty() {
        content.push('\n');
    }
    content.push('\n');
    content.push_str(&bash_zsh_alias_shim());
    content.push('\n');

    fs::write(rc_path, content)?;
    Ok(())
}

pub fn inject_alias_fish(fish_path: &Path) -> Result<()> {
    if let Some(parent) = fish_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // For Fish, we create/overwrite ~/.config/fish/functions/gr.fish
    // But we should check if it's already there and not ours.
    if fish_path.exists() {
        let content = read_rc_file(fish_path)?;
        if !content.contains(ALIAS_GUARD_START) && !content.is_empty() {
            // It's a conflict if it's not our block and not empty
            return Ok(()); // Should be handled by check_alias_conflict
        }
    }

    fs::write(fish_path, fish_alias_shim())?;
    Ok(())
}

#[cfg(windows)]
pub fn inject_alias_powershell(profile_path: &Path) -> Result<()> {
    let mut content = read_rc_file(profile_path)?;
    if content.contains(ALIAS_GUARD_START) {
        return Ok(());
    }

    if let Some(parent) = profile_path.parent() {
        fs::create_dir_all(parent)?;
    }

    if !content.ends_with('\n') && !content.is_empty() {
        content.push('\n');
    }
    content.push('\n');
    content.push_str(&powershell_alias_shim());
    content.push('\n');

    fs::write(profile_path, content)?;
    Ok(())
}

fn remove_block(content: &str, start_guard: &str, end_guard: &str) -> String {
    let mut new_content = String::new();
    let mut in_block = false;
    let lines: Vec<&str> = content.lines().collect();

    for line in lines {
        if line.contains(start_guard) {
            in_block = true;
            continue;
        }
        if line.contains(end_guard) {
            in_block = false;
            continue;
        }
        if !in_block {
            new_content.push_str(line);
            // Don't add a newline to the very last line if the original didn't have one,
            // but we usually want to preserve the structure.
            // Simplified: always add newline, we'll trim extra ones at the end if needed.
            new_content.push('\n');
        }
    }

    // Clean up multiple trailing newlines that might result from block removal
    while new_content.ends_with("\n\n") {
        new_content.pop();
    }

    new_content
}

pub fn remove_all_gitreg_blocks(rc_path: &Path) -> Result<()> {
    if !rc_path.exists() {
        return Ok(());
    }
    let content = read_rc_file(rc_path)?;
    let mut new_content = remove_block(&content, GUARD_START, GUARD_END);
    new_content = remove_block(&new_content, ALIAS_GUARD_START, ALIAS_GUARD_END);

    let trimmed = new_content.trim();
    if trimmed.is_empty() {
        // If the file is now empty or only contains whitespace, and it's a file we likely created
        // (like git.fish), we could delete it. But for .bashrc etc. we should just truncate.
        // For simplicity and safety, we just write the (possibly empty) string.
        fs::write(rc_path, "")?;
    } else {
        fs::write(rc_path, new_content)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_bash_zsh_conflict_detection() {
        let dir = tempdir().unwrap();
        let rc_path = dir.path().join(".bashrc");

        // No file = no conflict
        let content = "";
        fs::write(&rc_path, content).unwrap();
        // We can't easily test check_alias_conflict because it checks PATH
        // But we can test the logic inside it if we refactor it slightly or test the injection

        let mut content = "alias gr='some-other-tool'\n".to_string();
        fs::write(&rc_path, &content).unwrap();

        // Manual check of the logic we put in check_alias_conflict
        let stripped = content.clone();
        let conflict = stripped.contains("alias gr=") || stripped.contains("gr()");
        assert!(conflict);

        content = "gr() { echo hi; }\n".to_string();
        fs::write(&rc_path, &content).unwrap();
        let conflict = content.contains("alias gr=") || content.contains("gr()");
        assert!(conflict);
    }

    #[test]
    fn test_alias_injection_idempotency() {
        let dir = tempdir().unwrap();
        let rc_path = dir.path().join(".bashrc");

        inject_alias_bash_zsh(&rc_path).unwrap();
        let content1 = fs::read_to_string(&rc_path).unwrap();
        assert!(content1.contains(ALIAS_GUARD_START));

        inject_alias_bash_zsh(&rc_path).unwrap();
        let content2 = fs::read_to_string(&rc_path).unwrap();
        assert_eq!(content1, content2);
    }

    #[test]
    fn test_remove_all_blocks() {
        let dir = tempdir().unwrap();
        let rc_path = dir.path().join(".bashrc");

        let initial_content = "some-user-setting\n";
        fs::write(&rc_path, initial_content).unwrap();

        inject_bash_zsh(&rc_path).unwrap();
        inject_alias_bash_zsh(&rc_path).unwrap();

        let mid_content = fs::read_to_string(&rc_path).unwrap();
        assert!(mid_content.contains(GUARD_START));
        assert!(mid_content.contains(ALIAS_GUARD_START));

        remove_all_gitreg_blocks(&rc_path).unwrap();

        let final_content = fs::read_to_string(&rc_path).unwrap();
        assert!(!final_content.contains(GUARD_START));
        assert!(!final_content.contains(ALIAS_GUARD_START));
        assert!(final_content.contains("some-user-setting"));
        assert_eq!(final_content.trim(), "some-user-setting");
    }
}
