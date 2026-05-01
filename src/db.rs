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
    pub name: Option<String>,
    pub last_seen: i64,
    pub tags: Vec<String>,
}

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS repos (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    path      TEXT    NOT NULL UNIQUE,
    name      TEXT,
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
    conn.busy_timeout(Duration::from_millis(5_000))?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys = ON;")?;
    conn.execute_batch(SCHEMA)?;

    // Migration: add name column for databases created before this version.
    let has_name: bool = conn
        .prepare("PRAGMA table_info(repos)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .flatten()
        .any(|col| col == "name");
    if !has_name {
        conn.execute_batch("ALTER TABLE repos ADD COLUMN name TEXT;")?;
    }

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS tags (
            repo_id INTEGER NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
            tag     TEXT    NOT NULL,
            PRIMARY KEY (repo_id, tag)
        );",
    )?;

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
        init_conn(&conn)?;
        Ok(Self { conn })
    }

    pub fn upsert(&self, path: &str, name: Option<&str>) -> Result<()> {
        let ts = now_millis();
        self.conn.execute(
            "INSERT INTO repos (path, name, last_seen) VALUES (?1, ?2, ?3)
             ON CONFLICT(path) DO UPDATE SET last_seen = excluded.last_seen,
                                             name = COALESCE(excluded.name, name)",
            params![path, name, ts],
        )?;
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<RepoRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT r.id, r.path, r.name, r.last_seen,
                    GROUP_CONCAT(t.tag, ',') AS tags
             FROM repos r
             LEFT JOIN tags t ON t.repo_id = r.id
             GROUP BY r.id
             ORDER BY r.name ASC NULLS LAST, r.id DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let tags_str: Option<String> = row.get(4)?;
            let tags = tags_str
                .map(|s| s.split(',').map(String::from).collect())
                .unwrap_or_default();
            Ok(RepoRecord {
                id: row.get(0)?,
                path: row.get(1)?,
                name: row.get(2)?,
                last_seen: row.get(3)?,
                tags,
            })
        })?;
        let mut records = Vec::new();
        for r in rows {
            records.push(r?);
        }
        Ok(records)
    }

    pub fn resolve_target(&self, target: &str) -> Result<Option<i64>> {
        if let Ok(id) = target.parse::<i64>() {
            let result =
                self.conn
                    .query_row("SELECT id FROM repos WHERE id = ?1", params![id], |row| {
                        row.get(0)
                    });
            return match result {
                Ok(id) => Ok(Some(id)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(e.into()),
            };
        }
        let result = self.conn.query_row(
            "SELECT id FROM repos WHERE path = ?1 OR name = ?1 LIMIT 1",
            params![target],
            |row| row.get(0),
        );
        match result {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn add_tag(&self, repo_id: i64, tag: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO tags (repo_id, tag) VALUES (?1, ?2)",
            params![repo_id, tag],
        )?;
        Ok(())
    }

    pub fn remove_tag(&self, repo_id: i64, tag: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM tags WHERE repo_id = ?1 AND tag = ?2",
            params![repo_id, tag],
        )?;
        Ok(())
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

    pub fn remove_by_id(&self, id: i64) -> Result<Option<String>> {
        let result =
            self.conn
                .query_row("SELECT path FROM repos WHERE id = ?1", params![id], |row| {
                    row.get::<_, String>(0)
                });
        let path = match result {
            Ok(p) => p,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        self.conn
            .execute("DELETE FROM repos WHERE id = ?1", params![id])?;
        Ok(Some(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn upsert_and_list() {
        let db = Database::open_in_memory().unwrap();
        db.upsert("/tmp/repo_a", None).unwrap();
        db.upsert("/tmp/repo_b", None).unwrap();
        db.upsert("/tmp/repo_a", None).unwrap();
        let rows = db.list().unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|r| r.path == "/tmp/repo_a"));
        assert!(rows.iter().any(|r| r.path == "/tmp/repo_b"));
    }

    #[test]
    fn remove() {
        let db = Database::open_in_memory().unwrap();
        db.upsert("/tmp/repo_x", None).unwrap();
        let id = db.list().unwrap()[0].id;
        assert!(db.remove_by_id(id).unwrap().is_some());
        assert!(db.remove_by_id(id).unwrap().is_none());
        assert_eq!(db.list().unwrap().len(), 0);
    }

    #[test]
    fn prune_removes_missing() {
        let db = Database::open_in_memory().unwrap();
        db.upsert("/nonexistent/path/that/cannot/exist/xyz123", None)
            .unwrap();
        let removed = db.prune().unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(db.list().unwrap().len(), 0);
    }

    #[test]
    fn prune_keeps_existing() {
        let db = Database::open_in_memory().unwrap();
        let existing = std::env::temp_dir().to_str().unwrap().to_string();
        db.upsert(&existing, None).unwrap();
        let removed = db.prune().unwrap();
        assert_eq!(removed.len(), 0);
        assert_eq!(db.list().unwrap().len(), 1);
    }

    #[test]
    fn concurrent_upserts_do_not_deadlock() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = Arc::new(dir.path().join("gitreg.db"));
        // Initialize WAL mode and schema before concurrent access to avoid a
        // race where 20 threads simultaneously attempt the DELETE→WAL transition.
        drop(Database::open(&db_path).unwrap());
        let handles: Vec<_> = (0..20_usize)
            .map(|i| {
                let p = Arc::clone(&db_path);
                std::thread::spawn(move || {
                    let db = Database::open(&p).unwrap();
                    db.upsert(&format!("/tmp/repo_{i}"), None).unwrap();
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
        db.upsert("/tmp/repo_ts", None).unwrap();
        let t1 = db
            .list()
            .unwrap()
            .into_iter()
            .find(|r| r.path == "/tmp/repo_ts")
            .unwrap()
            .last_seen;
        std::thread::sleep(std::time::Duration::from_millis(2));
        db.upsert("/tmp/repo_ts", None).unwrap();
        let t2 = db
            .list()
            .unwrap()
            .into_iter()
            .find(|r| r.path == "/tmp/repo_ts")
            .unwrap()
            .last_seen;
        assert!(t2 > t1, "upsert should update last_seen timestamp");
    }

    #[test]
    fn name_stored_and_retrieved() {
        let db = Database::open_in_memory().unwrap();
        db.upsert("/tmp/repo_named", Some("octocat/hello")).unwrap();
        let rows = db.list().unwrap();
        assert_eq!(rows[0].name.as_deref(), Some("octocat/hello"));
    }

    #[test]
    fn upsert_preserves_name_on_update() {
        let db = Database::open_in_memory().unwrap();
        db.upsert("/tmp/repo_n", Some("owner/repo")).unwrap();
        db.upsert("/tmp/repo_n", None).unwrap();
        let rows = db.list().unwrap();
        assert_eq!(rows[0].name.as_deref(), Some("owner/repo"));
    }

    #[test]
    fn add_and_list_tags() {
        let db = Database::open_in_memory().unwrap();
        db.upsert("/tmp/tagged", None).unwrap();
        let id = db.list().unwrap()[0].id;
        db.add_tag(id, "work").unwrap();
        db.add_tag(id, "personal").unwrap();
        let rows = db.list().unwrap();
        assert!(rows[0].tags.contains(&"work".to_string()));
        assert!(rows[0].tags.contains(&"personal".to_string()));
    }

    #[test]
    fn remove_tag() {
        let db = Database::open_in_memory().unwrap();
        db.upsert("/tmp/tagged2", None).unwrap();
        let id = db.list().unwrap()[0].id;
        db.add_tag(id, "work").unwrap();
        db.remove_tag(id, "work").unwrap();
        let rows = db.list().unwrap();
        assert!(rows[0].tags.is_empty());
    }

    #[test]
    fn resolve_target_by_id() {
        let db = Database::open_in_memory().unwrap();
        db.upsert("/tmp/resolve_id", None).unwrap();
        let id = db.list().unwrap()[0].id;
        assert_eq!(db.resolve_target(&id.to_string()).unwrap(), Some(id));
    }

    #[test]
    fn resolve_target_by_path() {
        let db = Database::open_in_memory().unwrap();
        db.upsert("/tmp/resolve_path", None).unwrap();
        let id = db.list().unwrap()[0].id;
        assert_eq!(db.resolve_target("/tmp/resolve_path").unwrap(), Some(id));
    }

    #[test]
    fn resolve_target_by_name() {
        let db = Database::open_in_memory().unwrap();
        db.upsert("/tmp/resolve_name", Some("owner/repo")).unwrap();
        let id = db.list().unwrap()[0].id;
        assert_eq!(db.resolve_target("owner/repo").unwrap(), Some(id));
    }

    #[test]
    fn resolve_target_not_found() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.resolve_target("999").unwrap(), None);
        assert_eq!(db.resolve_target("no/such/repo").unwrap(), None);
    }

    #[test]
    fn list_sort_order() {
        let db = Database::open_in_memory().unwrap();
        // Insert unnamed repos (will be sorted by id DESC among themselves)
        db.upsert("/tmp/sort_unnamed_a", None).unwrap();
        db.upsert("/tmp/sort_unnamed_b", None).unwrap();
        // Insert named repos (sorted A-Z by name, appear before unnamed)
        db.upsert("/tmp/sort_named_z", Some("z-repo")).unwrap();
        db.upsert("/tmp/sort_named_a", Some("a-repo")).unwrap();

        let rows = db.list().unwrap();
        assert_eq!(rows.len(), 4);
        // Named repos come first, alphabetically
        assert_eq!(rows[0].name.as_deref(), Some("a-repo"));
        assert_eq!(rows[1].name.as_deref(), Some("z-repo"));
        // Unnamed repos follow, newest (highest id) first
        assert!(rows[2].name.is_none());
        assert!(rows[3].name.is_none());
        assert!(rows[2].id > rows[3].id);
    }
}
