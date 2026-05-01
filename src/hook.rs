use crate::db::Database;
use crate::error::Result;
use std::io::Write;
use std::path::Path;

pub fn run(raw_path: &Path, db: &Database) -> Result<()> {
    let canonical = dunce::canonicalize(raw_path)?;

    if !canonical.join(".git").is_dir() {
        return Ok(());
    }

    let canonical_str = match canonical.to_str() {
        Some(s) => s,
        None => return Ok(()), // non-UTF-8 path — silent skip
    };

    db.upsert(canonical_str)?;

    let marker = canonical.join(".git").join("gitreg_tracked");
    let tmp = canonical.join(".git").join("gitreg_tracked.tmp");

    let mut f = std::fs::File::create(&tmp)?;
    write!(f, "{}", canonical_str)?;
    f.flush()?;
    drop(f);

    std::fs::rename(&tmp, &marker)?;

    Ok(())
}
