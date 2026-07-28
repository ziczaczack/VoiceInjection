use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::Serialize;

pub struct History {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: i64,
    pub created_at: i64,
    pub mode: String,
    pub model: String,
    pub language: Option<String>,
    pub duration_secs: f64,
    pub text: String,
}

pub struct NewEntry<'a> {
    pub mode: &'a str,
    pub model: &'a str,
    pub language: Option<&'a str>,
    pub duration_secs: f64,
    pub text: &'a str,
}

impl History {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path).context("open history db")?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at INTEGER NOT NULL,
                mode TEXT NOT NULL,
                model TEXT NOT NULL,
                language TEXT,
                duration_secs REAL NOT NULL,
                text TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_entries_created_at
                ON entries(created_at DESC);
            "#,
        )
        .context("init history schema")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn insert(&self, e: NewEntry<'_>) -> Result<i64> {
        let now = unix_now();
        let conn = self.conn.lock().expect("history lock poisoned");
        conn.execute(
            "INSERT INTO entries (created_at, mode, model, language, duration_secs, text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![now, e.mode, e.model, e.language, e.duration_secs, e.text],
        )
        .context("insert history entry")?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list(&self, query: Option<&str>, limit: usize, offset: usize) -> Result<Vec<Entry>> {
        let conn = self.conn.lock().expect("history lock poisoned");
        let limit = limit.clamp(1, 200) as i64;
        let offset = offset as i64;
        let rows = if let Some(q) = query.filter(|s| !s.is_empty()) {
            let pat = format!("%{q}%");
            let mut stmt = conn.prepare(
                "SELECT id, created_at, mode, model, language, duration_secs, text
                 FROM entries
                 WHERE text LIKE ?1 COLLATE NOCASE
                 ORDER BY created_at DESC
                 LIMIT ?2 OFFSET ?3",
            )?;
            let v: Vec<Entry> = stmt
                .query_map(params![pat, limit, offset], map_entry)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            v
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, created_at, mode, model, language, duration_secs, text
                 FROM entries
                 ORDER BY created_at DESC
                 LIMIT ?1 OFFSET ?2",
            )?;
            let v: Vec<Entry> = stmt
                .query_map(params![limit, offset], map_entry)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            v
        };
        Ok(rows)
    }

    pub fn count(&self, query: Option<&str>) -> Result<i64> {
        let conn = self.conn.lock().expect("history lock poisoned");
        let n: i64 = if let Some(q) = query.filter(|s| !s.is_empty()) {
            let pat = format!("%{q}%");
            conn.query_row(
                "SELECT COUNT(*) FROM entries WHERE text LIKE ?1 COLLATE NOCASE",
                params![pat],
                |r| r.get(0),
            )?
        } else {
            conn.query_row("SELECT COUNT(*) FROM entries", [], |r| r.get(0))?
        };
        Ok(n)
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().expect("history lock poisoned");
        conn.execute("DELETE FROM entries WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        let conn = self.conn.lock().expect("history lock poisoned");
        conn.execute("DELETE FROM entries", [])?;
        conn.execute("VACUUM", [])?;
        Ok(())
    }
}

fn map_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<Entry> {
    Ok(Entry {
        id: row.get(0)?,
        created_at: row.get(1)?,
        mode: row.get(2)?,
        model: row.get(3)?,
        language: row.get(4)?,
        duration_secs: row.get(5)?,
        text: row.get(6)?,
    })
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
