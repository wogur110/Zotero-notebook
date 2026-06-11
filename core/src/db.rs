//! Sidecar SQLite database. The only data the app owns itself: AI summaries,
//! keyed by Zotero item key. Everything else lives in Zotero.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension};

use crate::error::Result;
use crate::models::StoredSummary;

pub struct Db {
    conn: Connection,
}

const MIGRATION: &str = "
CREATE TABLE IF NOT EXISTS summaries (
    item_key   TEXT PRIMARY KEY,
    summary    TEXT NOT NULL,
    provider   TEXT NOT NULL,
    model      TEXT NOT NULL,
    created_at TEXT NOT NULL
);
";

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(MIGRATION)?;
        Ok(Db { conn })
    }

    pub fn open_in_memory() -> Result<Db> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(MIGRATION)?;
        Ok(Db { conn })
    }

    pub fn get_summary(&self, item_key: &str) -> Result<Option<StoredSummary>> {
        let row = self
            .conn
            .query_row(
                "SELECT item_key, summary, provider, model, created_at
                 FROM summaries WHERE item_key = ?1",
                [item_key],
                |r| {
                    Ok(StoredSummary {
                        item_key: r.get(0)?,
                        summary: r.get(1)?,
                        provider: r.get(2)?,
                        model: r.get(3)?,
                        created_at: r.get(4)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    pub fn upsert_summary(&self, s: &StoredSummary) -> Result<()> {
        self.conn.execute(
            "INSERT INTO summaries (item_key, summary, provider, model, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(item_key) DO UPDATE SET
               summary = excluded.summary,
               provider = excluded.provider,
               model = excluded.model,
               created_at = excluded.created_at",
            (
                &s.item_key,
                &s.summary,
                &s.provider,
                &s.model,
                &s.created_at,
            ),
        )?;
        Ok(())
    }
}
