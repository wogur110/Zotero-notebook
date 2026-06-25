//! Sidecar SQLite database. The only data the app owns itself: AI summaries,
//! keyed by Zotero item key. Everything else lives in Zotero.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension};

use crate::error::Result;
use crate::models::{
    ReadingState, ReadingStatus, StoredSummary, SummarySource, UsageSummary,
};

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
CREATE TABLE IF NOT EXISTS reading_state (
    item_key   TEXT PRIMARY KEY,
    status     TEXT NOT NULL,
    starred    INTEGER NOT NULL DEFAULT 0,
    note       TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS citation_cache (
    item_key   TEXT PRIMARY KEY,
    graph_json TEXT NOT NULL,
    fetched_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS usage_log (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    op            TEXT NOT NULL,
    provider      TEXT NOT NULL,
    model         TEXT NOT NULL,
    input_tokens  INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    cost_usd      REAL NOT NULL,
    created_at    TEXT NOT NULL
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

    /// Every reading-state row (status / star / note per item). Loaded once and
    /// held in the frontend (libraries are a few thousand items at most). An
    /// empty or unrecognized status string parses to `None` (item is only
    /// starred/noted).
    pub fn all_reading_states(&self) -> Result<Vec<ReadingState>> {
        let mut stmt = self.conn.prepare(
            "SELECT item_key, status, starred, note, updated_at FROM reading_state",
        )?;
        let rows = stmt.query_map([], |r| {
            let status_raw: String = r.get(1)?;
            Ok((
                r.get::<_, String>(0)?,
                status_raw,
                r.get::<_, bool>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (item_key, status_raw, starred, note, updated_at) = row?;
            out.push(ReadingState {
                item_key,
                status: ReadingStatus::parse(&status_raw),
                starred,
                note,
                updated_at,
            });
        }
        Ok(out)
    }

    pub fn upsert_reading_state(&self, s: &ReadingState) -> Result<()> {
        self.conn.execute(
            "INSERT INTO reading_state (item_key, status, starred, note, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(item_key) DO UPDATE SET
               status = excluded.status,
               starred = excluded.starred,
               note = excluded.note,
               updated_at = excluded.updated_at",
            (
                &s.item_key,
                // None -> "" (the row still exists for the star/note).
                s.status.map(|x| x.as_str()).unwrap_or(""),
                s.starred,
                &s.note,
                &s.updated_at,
            ),
        )?;
        Ok(())
    }

    pub fn delete_reading_state(&self, item_key: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM reading_state WHERE item_key = ?1", [item_key])?;
        Ok(())
    }

    /// Cached citation graph for an item as `(graph_json, fetched_at)`, or None
    /// when never fetched. The caller decides whether `fetched_at` is too old.
    pub fn get_citation_cache(&self, item_key: &str) -> Result<Option<(String, String)>> {
        let row = self
            .conn
            .query_row(
                "SELECT graph_json, fetched_at FROM citation_cache WHERE item_key = ?1",
                [item_key],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?;
        Ok(row)
    }

    pub fn upsert_citation_cache(
        &self,
        item_key: &str,
        graph_json: &str,
        fetched_at: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO citation_cache (item_key, graph_json, fetched_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(item_key) DO UPDATE SET
               graph_json = excluded.graph_json,
               fetched_at = excluded.fetched_at",
            (item_key, graph_json, fetched_at),
        )?;
        Ok(())
    }

    /// Append one row to the AI usage/cost ledger.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_usage(
        &self,
        op: &str,
        provider: &str,
        model: &str,
        input_tokens: u32,
        output_tokens: u32,
        cost_usd: f64,
        created_at: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO usage_log (op, provider, model, input_tokens, output_tokens, cost_usd, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            (op, provider, model, input_tokens, output_tokens, cost_usd, created_at),
        )?;
        Ok(())
    }

    /// Cumulative token/cost totals across the whole ledger.
    pub fn usage_summary(&self) -> Result<UsageSummary> {
        let summary = self.conn.query_row(
            "SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(cost_usd), 0.0), COUNT(*)
             FROM usage_log",
            [],
            |r| {
                Ok(UsageSummary {
                    total_input_tokens: r.get(0)?,
                    total_output_tokens: r.get(1)?,
                    total_cost_usd: r.get(2)?,
                    operation_count: r.get(3)?,
                })
            },
        )?;
        Ok(summary)
    }
}
