use crate::error::{GitregError, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

const GUARD_START: &str = "# >>> gitreg-start >>>";
const GUARD_END: &str = "# <<< gitreg-end <<<";

pub fn detect_shell() -> Shell {
    let shell = std::env::var("SHELL").unwrap_or_default();
    if shell.contains("zsh") {
        Shell::Zsh
    } else if shell.contains("fish") {
        Shell::Fish
    } else {
        Shell::Bash
    }
}

pub fn rc_file_path(shell: &Shell) -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or(GitregError::NoConfigDir)?;
    Ok(match shell {
        Shell::Bash => home.join(".bashrc"),
        Shell::Zsh => home.join(".zshrc"),
        Shell::Fish => dirs::config_dir()
            .ok_or(GitregError::NoConfigDir)?
            .join("fish")
            .join("functions")
            .join("git.fish"),
    })
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
