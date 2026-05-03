use crate::db::Database;
use crate::error::Result;
use std::io::Write;
use std::path::Path;

pub fn run(raw_path: &Path, db: &Database) -> Result<bool> {
    let canonical = dunce::canonicalize(raw_path)?;

    if !canonical.join(".git").is_dir() {
        return Ok(false);
    }

    let canonical_str = match canonical.to_str() {
        Some(s) => s,
        None => return Ok(false), // non-UTF-8 path — silent skip
    };

    if db.is_excluded(canonical_str)? {
        return Ok(false);
    }

    let git_dir = canonical.join(".git");
    let name = extract_repo_name(&git_dir);
    let registered = db.upsert(canonical_str, name.as_deref())?;

    let marker = git_dir.join("gitreg_tracked");
    let tmp = git_dir.join("gitreg_tracked.tmp");

    let mut f = std::fs::File::create(&tmp)?;
    write!(f, "{}", canonical_str)?;
    f.flush()?;
    drop(f);

    std::fs::rename(&tmp, &marker)?;

    Ok(registered)
}

pub fn extract_repo_name(git_dir: &Path) -> Option<String> {
    let content = std::fs::read_to_string(git_dir.join("config")).ok()?;
    let mut in_origin = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == r#"[remote "origin"]"# {
            in_origin = true;
            continue;
        }
        if in_origin {
            if trimmed.starts_with('[') {
                break;
            }
            if let Some(rest) = trimmed.strip_prefix("url") {
                let rest = rest.trim_start();
                if let Some(url) = rest.strip_prefix('=') {
                    return parse_origin_owner_repo(url.trim());
                }
            }
        }
    }
    None
}

fn parse_origin_owner_repo(url: &str) -> Option<String> {
    let path = if let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    {
        let slash = rest.find('/')?;
        &rest[slash + 1..]
    } else if let Some((_, p)) = url.split_once(':') {
        p
    } else {
        return None;
    };
    let path = path.trim_end_matches('/').trim_end_matches(".git");
    let mut parts = path.splitn(3, '/');
    let owner = parts.next().filter(|s| !s.is_empty())?;
    let repo = parts.next().filter(|s| !s.is_empty())?;
    Some(format!("{owner}/{repo}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_https_url() {
        assert_eq!(
            parse_origin_owner_repo("https://github.com/octocat/hello-world.git"),
            Some("octocat/hello-world".into())
        );
    }

    #[test]
    fn parse_https_url_no_git_suffix() {
        assert_eq!(
            parse_origin_owner_repo("https://github.com/octocat/hello"),
            Some("octocat/hello".into())
        );
    }

    #[test]
    fn parse_ssh_url() {
        assert_eq!(
            parse_origin_owner_repo("git@github.com:octocat/hello-world.git"),
            Some("octocat/hello-world".into())
        );
    }

    #[test]
    fn parse_unknown_url_returns_none() {
        assert_eq!(parse_origin_owner_repo("file:///local/path"), None);
    }
}
