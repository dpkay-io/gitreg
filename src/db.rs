use crate::error::Result;
use rusqlite::{params, Connection};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct Database {
    conn: Connection,
}

pub struct RepoRecord {
    pub id: i64,
    pub path: String,
    pub last_seen: i64,
}

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS repos (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    path      TEXT    NOT NULL UNIQUE,
    last_seen INTEGER NOT NULL
);";

fn now_millis() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    i64::try_from(millis).unwrap_or(i64::MAX)
}

fn init_conn(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.busy_timeout(Duration::from_millis(5_000))?;
    conn.execute_batch(SCHEMA)?;
    Ok(())
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        init_conn(&conn)?;
        Ok(Self { conn })
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    pub fn upsert(&self, path: &str) -> Result<()> {
        let ts = now_millis();
        self.conn.execute(
            "INSERT INTO repos (path, last_seen) VALUES (?1, ?2)
             ON CONFLICT(path) DO UPDATE SET last_seen = excluded.last_seen",
            params![path, ts],
        )?;
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<RepoRecord>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, path, last_seen FROM repos ORDER BY last_seen DESC")?;
        let rows = stmt.query_map([], |row| {
            Ok(RepoRecord {
                id: row.get(0)?,
                path: row.get(1)?,
                last_seen: row.get(2)?,
            })
        })?;
        let mut records = Vec::new();
        for r in rows {
            records.push(r?);
        }
        Ok(records)
    }

    pub fn prune(&self) -> Result<Vec<String>> {
        // exists() check and DELETE are not atomic — concurrent prune calls may
        // report inconsistent counts, but the final DB state remains consistent.
        let all = self.list()?;
        let mut removed = Vec::new();
        for rec in all {
            if !Path::new(&rec.path).exists() {
                self.conn
                    .execute("DELETE FROM repos WHERE path = ?1", params![rec.path])?;
                removed.push(rec.path);
            }
        }
        Ok(removed)
    }

    pub fn remove(&self, path: &str) -> Result<bool> {
        let n = self
            .conn
            .execute("DELETE FROM repos WHERE path = ?1", params![path])?;
        Ok(n > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn upsert_and_list() {
        let db = Database::open_in_memory().unwrap();
        db.upsert("/tmp/repo_a").unwrap();
        db.upsert("/tmp/repo_b").unwrap();
        db.upsert("/tmp/repo_a").unwrap();
        let rows = db.list().unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|r| r.path == "/tmp/repo_a"));
        assert!(rows.iter().any(|r| r.path == "/tmp/repo_b"));
    }

    #[test]
    fn remove() {
        let db = Database::open_in_memory().unwrap();
        db.upsert("/tmp/repo_x").unwrap();
        assert!(db.remove("/tmp/repo_x").unwrap());
        assert!(!db.remove("/tmp/repo_x").unwrap());
        assert_eq!(db.list().unwrap().len(), 0);
    }

    #[test]
    fn prune_removes_missing() {
        let db = Database::open_in_memory().unwrap();
        db.upsert("/nonexistent/path/that/cannot/exist/xyz123")
            .unwrap();
        let removed = db.prune().unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(db.list().unwrap().len(), 0);
    }

    #[test]
    fn prune_keeps_existing() {
        let db = Database::open_in_memory().unwrap();
        let existing = std::env::temp_dir().to_str().unwrap().to_string();
        db.upsert(&existing).unwrap();
        let removed = db.prune().unwrap();
        assert_eq!(removed.len(), 0);
        assert_eq!(db.list().unwrap().len(), 1);
    }

    #[test]
    fn concurrent_upserts_do_not_deadlock() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = Arc::new(dir.path().join("gitreg.db"));
        let handles: Vec<_> = (0..20_usize)
            .map(|i| {
                let p = Arc::clone(&db_path);
                std::thread::spawn(move || {
                    let db = Database::open(&p).unwrap();
                    db.upsert(&format!("/tmp/repo_{i}")).unwrap();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(Database::open(&db_path).unwrap().list().unwrap().len(), 20);
    }

    #[test]
    fn upsert_updates_timestamp() {
        let db = Database::open_in_memory().unwrap();
        db.upsert("/tmp/repo_ts").unwrap();
        let t1 = db
            .list()
            .unwrap()
            .into_iter()
            .find(|r| r.path == "/tmp/repo_ts")
            .unwrap()
            .last_seen;
        std::thread::sleep(std::time::Duration::from_millis(2));
        db.upsert("/tmp/repo_ts").unwrap();
        let t2 = db
            .list()
            .unwrap()
            .into_iter()
            .find(|r| r.path == "/tmp/repo_ts")
            .unwrap()
            .last_seen;
        assert!(t2 > t1, "upsert should update last_seen timestamp");
    }
}
