//! Sidecar SQLite database. The only data the app owns itself: AI summaries,
//! keyed by Zotero item key. Everything else lives in Zotero.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension};

use crate::error::Result;
use crate::models::{StoredSummary, SummarySource};

pub struct Db {
    conn: Connection,
}

const MIGRATION: &str = "
CREATE TABLE IF NOT EXISTS summaries (
    item_key     TEXT PRIMARY KEY,
    summary      TEXT NOT NULL,
    provider     TEXT NOT NULL,
    model        TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    had_abstract INTEGER NOT NULL DEFAULT 1,
    source       TEXT NOT NULL DEFAULT ''
);
";

/// Columns added after 1.0.0. Each ALTER fails harmlessly with "duplicate
/// column name" when the column already exists.
const COLUMN_MIGRATIONS: &[&str] = &[
    "ALTER TABLE summaries ADD COLUMN had_abstract INTEGER NOT NULL DEFAULT 1",
    "ALTER TABLE summaries ADD COLUMN source TEXT NOT NULL DEFAULT ''",
];

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(MIGRATION)?;
    for sql in COLUMN_MIGRATIONS {
        if let Err(e) = conn.execute(sql, []) {
            let msg = e.to_string();
            if !msg.contains("duplicate column name") {
                return Err(e.into());
            }
        }
    }
    Ok(())
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        migrate(&conn)?;
        Ok(Db { conn })
    }

    pub fn open_in_memory() -> Result<Db> {
        let conn = Connection::open_in_memory()?;
        migrate(&conn)?;
        Ok(Db { conn })
    }

    pub fn get_summary(&self, item_key: &str) -> Result<Option<StoredSummary>> {
        let row = self
            .conn
            .query_row(
                "SELECT item_key, summary, provider, model, created_at, had_abstract, source
                 FROM summaries WHERE item_key = ?1",
                [item_key],
                |r| {
                    // `source` arrived in 1.2.0; older rows fall back to the
                    // 1.1.0 had_abstract flag.
                    let source_raw: String = r.get(6)?;
                    let had_abstract: bool = r.get(5)?;
                    let source = SummarySource::parse(&source_raw).unwrap_or(if had_abstract {
                        SummarySource::Abstract
                    } else {
                        SummarySource::Metadata
                    });
                    Ok(StoredSummary {
                        item_key: r.get(0)?,
                        summary: r.get(1)?,
                        provider: r.get(2)?,
                        model: r.get(3)?,
                        created_at: r.get(4)?,
                        source,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    /// Every stored summary — powers search-over-summaries and the batch
    /// "summarize missing" button. Libraries are a few thousand items at
    /// most, so loading all rows is fine.
    pub fn all_summaries(&self) -> Result<Vec<StoredSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT item_key, summary, provider, model, created_at, had_abstract, source
             FROM summaries",
        )?;
        let rows = stmt.query_map([], |r| {
            let source_raw: String = r.get(6)?;
            let had_abstract: bool = r.get(5)?;
            let source = SummarySource::parse(&source_raw).unwrap_or(if had_abstract {
                SummarySource::Abstract
            } else {
                SummarySource::Metadata
            });
            Ok(StoredSummary {
                item_key: r.get(0)?,
                summary: r.get(1)?,
                provider: r.get(2)?,
                model: r.get(3)?,
                created_at: r.get(4)?,
                source,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn upsert_summary(&self, s: &StoredSummary) -> Result<()> {
        self.conn.execute(
            "INSERT INTO summaries (item_key, summary, provider, model, created_at, had_abstract, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(item_key) DO UPDATE SET
               summary = excluded.summary,
               provider = excluded.provider,
               model = excluded.model,
               created_at = excluded.created_at,
               had_abstract = excluded.had_abstract,
               source = excluded.source",
            (
                &s.item_key,
                &s.summary,
                &s.provider,
                &s.model,
                &s.created_at,
                s.source != SummarySource::Metadata, // legacy column kept coherent
                s.source.as_str(),
            ),
        )?;
        Ok(())
    }
}
