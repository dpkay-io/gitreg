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
