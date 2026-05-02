use crate::error::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct Database {
    conn: Connection,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RepoRecord {
    pub id: i64,
    pub path: String,
    pub name: Option<String>,
    pub last_seen: i64,
    pub tags: Vec<String>,
    pub emergency_branch: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntegrationRecord {
    pub app_name: String,
    pub is_blocked: bool,
    pub event: String,
    pub socket_path: String,
}

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS repos (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    path      TEXT    NOT NULL UNIQUE,
    name      TEXT,
    last_seen INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS exclusions (
    path TEXT PRIMARY KEY
);
CREATE TABLE IF NOT EXISTS apps (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT    NOT NULL UNIQUE,
    is_blocked BOOLEAN NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS integrations (
    app_id      INTEGER NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
    event       TEXT    NOT NULL,
    socket_path TEXT    NOT NULL,
    PRIMARY KEY (app_id, event)
);
CREATE TABLE IF NOT EXISTS tags (
    repo_id INTEGER NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
    tag     TEXT    NOT NULL,
    PRIMARY KEY (repo_id, tag)
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

    let has_emergency: bool = conn
        .prepare("PRAGMA table_info(repos)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .flatten()
        .any(|col| col == "emergency_branch");
    if !has_emergency {
        conn.execute_batch("ALTER TABLE repos ADD COLUMN emergency_branch TEXT;")?;
    }

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

    pub fn upsert(&self, path: &str, name: Option<&str>) -> Result<bool> {
        let ts = now_millis();
        // Better:
        let mut stmt = self.conn.prepare("SELECT 1 FROM repos WHERE path = ?1")?;
        let exists = stmt.exists(params![path])?;

        self.conn.execute(
            "INSERT INTO repos (path, name, last_seen) VALUES (?1, ?2, ?3)
             ON CONFLICT(path) DO UPDATE SET last_seen = excluded.last_seen,
                                             name = COALESCE(excluded.name, name)",
            params![path, name, ts],
        )?;

        Ok(!exists)
    }

    pub fn list(&self) -> Result<Vec<RepoRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT r.id, r.path, r.name, r.last_seen,
                    GROUP_CONCAT(t.tag, ',') AS tags,
                    r.emergency_branch
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
                emergency_branch: row.get(5)?,
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

    pub fn set_emergency_branch(&self, id: i64, branch: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE repos SET emergency_branch = ?1 WHERE id = ?2",
            params![branch, id],
        )?;
        Ok(())
    }

    pub fn clear_all_emergency_branches(&self) -> Result<()> {
        self.conn
            .execute("UPDATE repos SET emergency_branch = NULL", [])?;
        Ok(())
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
        let exclusions = self.list_exclusions()?;
        let mut stmt = self
            .conn
            .prepare("SELECT id, path, name, last_seen FROM repos")?;
        let rows = stmt.query_map([], |row| {
            Ok(RepoRecord {
                id: row.get(0)?,
                path: row.get(1)?,
                name: row.get(2)?,
                last_seen: row.get(3)?,
                tags: Vec::new(),
                emergency_branch: None,
            })
        })?;

        let mut removed = Vec::new();
        for rec in rows {
            let rec = rec?;
            if !Path::new(&rec.path).exists() || self.is_path_excluded(&rec.path, &exclusions) {
                self.conn
                    .execute("DELETE FROM repos WHERE path = ?1", params![rec.path])?;
                removed.push(rec.path);
            }
        }
        Ok(removed)
    }

    pub fn remove_by_exclusion(&self, exclusion: &str) -> Result<usize> {
        let mut stmt = self.conn.prepare("SELECT path FROM repos")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut to_remove = Vec::new();

        let mut prefix = exclusion.to_string();
        if !prefix.ends_with(std::path::MAIN_SEPARATOR) {
            prefix.push(std::path::MAIN_SEPARATOR);
        }

        for path in rows {
            let path = path?;
            if path == exclusion || path.starts_with(&prefix) {
                to_remove.push(path);
            }
        }

        let count = to_remove.len();
        for path in to_remove {
            self.conn
                .execute("DELETE FROM repos WHERE path = ?1", params![path])?;
        }
        Ok(count)
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

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM settings WHERE key = ?1")?;
        let result = stmt.query_row(params![key], |row| row.get(0));
        match result {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn is_autoprune_enabled(&self) -> Result<bool> {
        Ok(self
            .get_setting("autoprune_enabled")?
            .map(|v| v == "true")
            .unwrap_or(true)) // Default to true
    }

    pub fn get_autoprune_time(&self) -> Result<String> {
        Ok(self
            .get_setting("autoprune_time")?
            .unwrap_or_else(|| "00:00".to_string()))
    }

    pub fn get_last_autoprune_date(&self) -> Result<Option<String>> {
        self.get_setting("last_autoprune_date")
    }

    pub fn set_last_autoprune_date(&self, date: &str) -> Result<()> {
        self.set_setting("last_autoprune_date", date)
    }

    pub fn add_exclusion(&self, path: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO exclusions (path) VALUES (?1)",
            params![path],
        )?;
        Ok(())
    }

    pub fn remove_exclusion(&self, path: &str) -> Result<bool> {
        let count = self
            .conn
            .execute("DELETE FROM exclusions WHERE path = ?1", params![path])?;
        Ok(count > 0)
    }

    pub fn list_exclusions(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM exclusions ORDER BY path")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        let mut exclusions = Vec::new();
        for r in rows {
            exclusions.push(r?);
        }
        Ok(exclusions)
    }

    pub fn resolve_many(&self, targets: &str) -> Result<Vec<RepoRecord>> {
        if targets == "all" {
            return self.list();
        }

        let mut results = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        for t in targets.split(',') {
            let t = t.trim();
            if t.is_empty() {
                continue;
            }

            if let Some(tag) = t.strip_prefix('@') {
                let mut stmt = self.conn.prepare(
                    "SELECT r.id, r.path, r.name, r.last_seen,
                            (SELECT GROUP_CONCAT(tag) FROM tags WHERE repo_id = r.id) as tags,
                            r.emergency_branch
                     FROM repos r
                     JOIN tags t2 ON t2.repo_id = r.id
                     WHERE t2.tag = ?1
                     GROUP BY r.id",
                )?;
                let rows = stmt.query_map(params![tag], |row| {
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
                        emergency_branch: row.get(5)?,
                    })
                })?;
                for r in rows {
                    let r = r?;
                    if seen_ids.insert(r.id) {
                        results.push(r);
                    }
                }
            } else if let Some(id) = self.resolve_target(t)? {
                let mut stmt = self.conn.prepare(
                    "SELECT r.id, r.path, r.name, r.last_seen,
                            (SELECT GROUP_CONCAT(tag) FROM tags WHERE repo_id = r.id) as tags,
                            r.emergency_branch
                     FROM repos r
                     WHERE r.id = ?1",
                )?;
                let mut rows = stmt.query_map(params![id], |row| {
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
                        emergency_branch: row.get(5)?,
                    })
                })?;
                if let Some(r) = rows.next() {
                    let r = r?;
                    if seen_ids.insert(r.id) {
                        results.push(r);
                    }
                }
            }
        }

        Ok(results)
    }

    pub fn is_path_excluded(&self, path: &str, exclusions: &[String]) -> bool {
        for excluded in exclusions {
            if path == excluded {
                return true;
            }
            // Check if it's a sub-path. We need to be careful with trailing slashes.
            let mut prefix = excluded.clone();
            if !prefix.ends_with(std::path::MAIN_SEPARATOR) {
                prefix.push(std::path::MAIN_SEPARATOR);
            }
            if path.starts_with(&prefix) {
                return true;
            }
        }
        false
    }

    pub fn is_excluded(&self, path: &str) -> Result<bool> {
        let exclusions = self.list_exclusions()?;
        Ok(self.is_path_excluded(path, &exclusions))
    }

    pub fn get_or_create_app(&self, name: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT OR IGNORE INTO apps (name) VALUES (?1)",
            params![name],
        )?;
        let id = self.conn.query_row(
            "SELECT id FROM apps WHERE name = ?1",
            params![name],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    pub fn block_app(&self, name: &str) -> Result<bool> {
        let count = self.conn.execute(
            "UPDATE apps SET is_blocked = 1 WHERE name = ?1",
            params![name],
        )?;
        Ok(count > 0)
    }

    pub fn unblock_app(&self, name: &str) -> Result<bool> {
        let count = self.conn.execute(
            "UPDATE apps SET is_blocked = 0 WHERE name = ?1",
            params![name],
        )?;
        Ok(count > 0)
    }

    pub fn remove_app(&self, name: &str) -> Result<bool> {
        let count = self
            .conn
            .execute("DELETE FROM apps WHERE name = ?1", params![name])?;
        Ok(count > 0)
    }

    pub fn register_integration(
        &self,
        app_name: &str,
        event: &str,
        socket_path: &str,
    ) -> Result<()> {
        let app_id = self.get_or_create_app(app_name)?;
        self.conn.execute(
            "INSERT INTO integrations (app_id, event, socket_path) VALUES (?1, ?2, ?3)
             ON CONFLICT(app_id, event) DO UPDATE SET socket_path = excluded.socket_path",
            params![app_id, event, socket_path],
        )?;
        Ok(())
    }

    pub fn unregister_integration(&self, app_name: &str, event: &str) -> Result<bool> {
        let app_id = match self
            .conn
            .query_row(
                "SELECT id FROM apps WHERE name = ?1",
                params![app_name],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
        {
            Some(id) => id,
            None => return Ok(false),
        };

        let count = self.conn.execute(
            "DELETE FROM integrations WHERE app_id = ?1 AND event = ?2",
            params![app_id, event],
        )?;
        Ok(count > 0)
    }

    pub fn list_integrations(&self) -> Result<Vec<IntegrationRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT a.name, a.is_blocked, i.event, i.socket_path
             FROM integrations i
             JOIN apps a ON a.id = i.app_id
             ORDER BY a.name ASC, i.event ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(IntegrationRecord {
                app_name: row.get(0)?,
                is_blocked: row.get(1)?,
                event: row.get(2)?,
                socket_path: row.get(3)?,
            })
        })?;
        let mut records = Vec::new();
        for r in rows {
            records.push(r?);
        }
        Ok(records)
    }

    pub fn get_active_listeners_for_event(&self, event: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT i.socket_path
             FROM integrations i
             JOIN apps a ON a.id = i.app_id
             WHERE i.event = ?1 AND a.is_blocked = 0",
        )?;
        let rows = stmt.query_map(params![event], |row| row.get::<_, String>(0))?;
        let mut paths = Vec::new();
        for r in rows {
            paths.push(r?);
        }
        Ok(paths)
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

    #[test]
    fn settings_work() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.is_autoprune_enabled().unwrap(), true);
        assert_eq!(db.get_autoprune_time().unwrap(), "00:00");

        db.set_setting("autoprune_enabled", "false").unwrap();
        assert_eq!(db.is_autoprune_enabled().unwrap(), false);

        db.set_setting("autoprune_time", "12:34").unwrap();
        assert_eq!(db.get_autoprune_time().unwrap(), "12:34");

        db.set_last_autoprune_date("2024-01-01").unwrap();
        assert_eq!(
            db.get_last_autoprune_date().unwrap(),
            Some("2024-01-01".to_string())
        );
    }

    #[test]
    fn exclusions_work() {
        let db = Database::open_in_memory().unwrap();
        let sep = std::path::MAIN_SEPARATOR.to_string();
        let p1 = format!("{}tmp{}work", sep, sep);
        let p2 = format!("{}tmp{}secret", sep, sep);

        db.add_exclusion(&p1).unwrap();
        db.add_exclusion(&p2).unwrap();

        let list = db.list_exclusions().unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.contains(&p1));
        assert!(list.contains(&p2));

        assert!(db.is_excluded(&p1).unwrap());
        assert!(db.is_excluded(&format!("{}{}repo", p1, sep)).unwrap());
        assert!(!db.is_excluded(&format!("{}tmp{}other", sep, sep)).unwrap());
        assert!(!db
            .is_excluded(&format!("{}tmp{}work-hard", sep, sep))
            .unwrap());

        db.remove_exclusion(&p1).unwrap();
        assert!(!db.is_excluded(&p1).unwrap());
        assert_eq!(db.list_exclusions().unwrap().len(), 1);
    }

    #[test]
    fn remove_by_exclusion_works() {
        let db = Database::open_in_memory().unwrap();
        let sep = std::path::MAIN_SEPARATOR.to_string();
        let p1 = format!("{}tmp{}work", sep, sep);
        db.upsert(&format!("{}{}repo1", p1, sep), None).unwrap();
        db.upsert(&format!("{}{}repo2", p1, sep), None).unwrap();
        db.upsert(&format!("{}tmp{}other", sep, sep), None).unwrap();

        let count = db.remove_by_exclusion(&p1).unwrap();
        assert_eq!(count, 2);
        assert_eq!(db.list().unwrap().len(), 1);
    }

    #[test]
    fn emergency_branches_work() {
        let db = Database::open_in_memory().unwrap();
        db.upsert("/tmp/emergency", None).unwrap();
        let id = db.list().unwrap()[0].id;

        db.set_emergency_branch(id, "my-emergency-branch").unwrap();
        let rows = db.list().unwrap();
        assert_eq!(
            rows[0].emergency_branch.as_deref(),
            Some("my-emergency-branch")
        );

        db.clear_all_emergency_branches().unwrap();
        let rows = db.list().unwrap();
        assert_eq!(rows[0].emergency_branch, None);
    }

    #[test]
    fn apps_and_integrations_work() {
        let db = Database::open_in_memory().unwrap();

        // App management
        let id = db.get_or_create_app("test-app").unwrap();
        assert!(id > 0);
        assert_eq!(db.get_or_create_app("test-app").unwrap(), id);

        assert!(db.block_app("test-app").unwrap());
        let list = db.list_integrations().unwrap();
        assert_eq!(list.len(), 0); // No integrations yet

        assert!(db.unblock_app("test-app").unwrap());

        // Integration management
        db.register_integration("test-app", "registered", "/tmp/test.sock")
            .unwrap();
        let list = db.list_integrations().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].app_name, "test-app");
        assert_eq!(list[0].event, "registered");
        assert_eq!(list[0].is_blocked, false);

        let listeners = db.get_active_listeners_for_event("registered").unwrap();
        assert_eq!(listeners.len(), 1);
        assert_eq!(listeners[0], "/tmp/test.sock");

        db.block_app("test-app").unwrap();
        let listeners = db.get_active_listeners_for_event("registered").unwrap();
        assert_eq!(listeners.len(), 0);

        assert!(db.unregister_integration("test-app", "registered").unwrap());
        assert_eq!(db.list_integrations().unwrap().len(), 0);

        assert!(db.remove_app("test-app").unwrap());
    }
}
