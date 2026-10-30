//! Study history in SQLite (`rusqlite`, bundled): sessions, their work
//! segments and tags, with versioned migrations (`PRAGMA user_version`).
//! Times are stored as UTC milliseconds since the Unix epoch (CLAUDE.md
//! rule 11); they are shown in local time.
//!
//! A running session is saved every minute and when it ends, so a crash
//! loses at most a minute; at start any session left open is closed at its
//! last saved segment.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use super::machine::{Mode, Record};

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("the study history isn't available")]
    Unavailable,
    #[error("a tag needs a name")]
    EmptyName,
    #[error("there is already a tag called \"{0}\"")]
    DuplicateName(String),
}

/// Each step brings the schema from version `i` to `i + 1`. Never edit a
/// released step: add a new one.
const MIGRATIONS: &[&str] = &["
    CREATE TABLE tags (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL UNIQUE COLLATE NOCASE,
        color TEXT NOT NULL,
        created_at INTEGER NOT NULL
    );
    CREATE TABLE sessions (
        id INTEGER PRIMARY KEY,
        started_at INTEGER NOT NULL,
        ended_at INTEGER,
        net_ms INTEGER NOT NULL DEFAULT 0,
        mode TEXT NOT NULL,
        tag_id INTEGER REFERENCES tags(id) ON DELETE SET NULL
    );
    CREATE TABLE segments (
        id INTEGER PRIMARY KEY,
        session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        started_at INTEGER NOT NULL,
        ended_at INTEGER NOT NULL
    );
    CREATE INDEX segments_by_time ON segments(started_at, ended_at);
    CREATE INDEX segments_by_session ON segments(session_id);
"];

/// The colours a tag can have (tokens `--tag-*` in tokens.css).
pub const TAG_COLORS: [&str; 6] = ["rose", "amber", "sage", "sky", "lilac", "stone"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub color: String,
}

pub fn millis(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Stopwatch => "stopwatch",
        Mode::Pomodoro => "pomodoro",
    }
}

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self, DbError> {
        Self::init(Connection::open(path)?)
    }

    /// A throwaway database, for tests.
    #[cfg(test)]
    pub fn in_memory() -> Result<Self, DbError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, DbError> {
        conn.pragma_update(None, "foreign_keys", true)?;
        // Safe and fast for a single writer.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let mut db = Self { conn };
        db.migrate()?;
        db.recover()?;
        Ok(db)
    }

    fn migrate(&mut self) -> Result<(), DbError> {
        let version: i64 = self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        for (i, step) in (0_i64..)
            .zip(MIGRATIONS)
            .skip(usize::try_from(version).unwrap_or(0))
        {
            let tx = self.conn.transaction()?;
            tx.execute_batch(step)?;
            tx.pragma_update(None, "user_version", i + 1)?;
            tx.commit()?;
            log::info!("study database migrated to version {}", i + 1);
        }
        Ok(())
    }

    /// Closes sessions left open by a crash at their last saved work.
    fn recover(&mut self) -> Result<(), DbError> {
        let n = self.conn.execute(
            "UPDATE sessions SET ended_at = COALESCE(
                 (SELECT MAX(ended_at) FROM segments WHERE session_id = sessions.id),
                 started_at)
             WHERE ended_at IS NULL",
            [],
        )?;
        if n > 0 {
            log::info!("closed {n} study session(s) left open");
        }
        Ok(())
    }

    pub fn begin(
        &mut self,
        started: SystemTime,
        mode: Mode,
        tag: Option<i64>,
    ) -> Result<i64, DbError> {
        self.conn.execute(
            "INSERT INTO sessions (started_at, mode, tag_id) VALUES (?1, ?2, ?3)",
            params![millis(started), mode_name(mode), tag],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Writes the session as it stands; `done` also closes it.
    pub fn save(&mut self, id: i64, record: &Record, done: bool) -> Result<(), DbError> {
        let tx = self.conn.transaction()?;
        write_segments(&tx, id, record)?;
        tx.execute(
            "UPDATE sessions SET net_ms = ?2, ended_at = ?3 WHERE id = ?1",
            params![
                id,
                i64::try_from(record.net.as_millis()).unwrap_or(i64::MAX),
                done.then(|| millis(record.ended)),
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// A session with no work is not kept.
    pub fn discard(&mut self, id: i64) -> Result<(), DbError> {
        self.conn
            .execute("DELETE FROM sessions WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn set_tag(&mut self, id: i64, tag: Option<i64>) -> Result<(), DbError> {
        self.conn.execute(
            "UPDATE sessions SET tag_id = ?2 WHERE id = ?1",
            params![id, tag],
        )?;
        Ok(())
    }

    /// Work stored between two instants, split exactly at the edges (a
    /// session across midnight counts on each day for its own part).
    pub fn worked_between(
        &self,
        from: SystemTime,
        to: SystemTime,
        except: Option<i64>,
    ) -> Result<Duration, DbError> {
        let ms: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(MAX(0, MIN(ended_at, ?2) - MAX(started_at, ?1))), 0)
             FROM segments
             WHERE ended_at > ?1 AND started_at < ?2 AND session_id IS NOT ?3",
            params![millis(from), millis(to), except],
            |r| r.get(0),
        )?;
        Ok(Duration::from_millis(u64::try_from(ms).unwrap_or(0)))
    }

    pub fn tags(&self) -> Result<Vec<Tag>, DbError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, color FROM tags ORDER BY name COLLATE NOCASE")?;
        let rows = stmt.query_map([], |r| {
            Ok(Tag {
                id: r.get(0)?,
                name: r.get(1)?,
                color: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Creates a tag (`id` = None) or renames / recolours one.
    pub fn upsert_tag(&mut self, id: Option<i64>, name: &str, color: &str) -> Result<Tag, DbError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(DbError::EmptyName);
        }
        let color = if TAG_COLORS.contains(&color) {
            color
        } else {
            TAG_COLORS[0]
        };
        let clash: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM tags WHERE name = ?1 COLLATE NOCASE",
                [name],
                |r| r.get(0),
            )
            .optional()?;
        if clash.is_some_and(|c| Some(c) != id) {
            return Err(DbError::DuplicateName(name.to_owned()));
        }
        let id = match id {
            Some(id) => {
                self.conn.execute(
                    "UPDATE tags SET name = ?2, color = ?3 WHERE id = ?1",
                    params![id, name, color],
                )?;
                id
            }
            None => {
                self.conn.execute(
                    "INSERT INTO tags (name, color, created_at) VALUES (?1, ?2, ?3)",
                    params![name, color, millis(SystemTime::now())],
                )?;
                self.conn.last_insert_rowid()
            }
        };
        Ok(Tag {
            id,
            name: name.to_owned(),
            color: color.to_owned(),
        })
    }

    /// Deletes a tag; its sessions stay, untagged.
    pub fn delete_tag(&mut self, id: i64) -> Result<(), DbError> {
        self.conn.execute("DELETE FROM tags WHERE id = ?1", [id])?;
        Ok(())
    }

    #[cfg(test)]
    fn session(&self, id: i64) -> Result<(Option<i64>, i64, Option<i64>), DbError> {
        Ok(self.conn.query_row(
            "SELECT ended_at, net_ms, tag_id FROM sessions WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?)
    }
}

fn write_segments(tx: &Transaction<'_>, id: i64, record: &Record) -> Result<(), DbError> {
    tx.execute("DELETE FROM segments WHERE session_id = ?1", [id])?;
    let mut insert =
        tx.prepare("INSERT INTO segments (session_id, started_at, ended_at) VALUES (?1, ?2, ?3)")?;
    for (start, end) in &record.segments {
        insert.execute(params![id, millis(*start), millis(*end)])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(mins: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_800_000_000 + mins * 60)
    }

    fn record(segments: &[(u64, u64)]) -> Record {
        let segments: Vec<_> = segments.iter().map(|&(a, b)| (t(a), t(b))).collect();
        Record {
            started: segments[0].0,
            ended: segments[segments.len() - 1].1,
            net: segments
                .iter()
                .map(|(a, b)| b.duration_since(*a).unwrap())
                .sum(),
            mode: Mode::Stopwatch,
            segments,
        }
    }

    #[test]
    fn migrations_run_once_and_reopen_cleanly() {
        let dir = std::env::temp_dir().join(format!("rumor-db-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("study.sqlite3");
        let _ = std::fs::remove_file(&path);
        {
            let mut db = Db::open(&path).unwrap();
            db.upsert_tag(None, "Cálculo", "sky").unwrap();
        }
        let db = Db::open(&path).unwrap();
        assert_eq!(db.tags().unwrap().len(), 1);
        let version: i64 = db
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn totals_split_a_session_across_midnight() {
        let mut db = Db::in_memory().unwrap();
        let id = db.begin(t(0), Mode::Stopwatch, None).unwrap();
        // 23:30–00:45 with a pause 23:50–00:10; midnight is minute 30.
        db.save(id, &record(&[(0, 20), (40, 75)]), true).unwrap();
        let day1 = db
            .worked_between(t(0) - Duration::from_secs(3600), t(30), None)
            .unwrap();
        let day2 = db.worked_between(t(30), t(30 + 24 * 60), None).unwrap();
        assert_eq!(day1, Duration::from_secs(20 * 60));
        assert_eq!(day2, Duration::from_secs(35 * 60));
        assert_eq!(
            db.session(id).unwrap(),
            (Some(millis(t(75))), 55 * 60_000, None)
        );
    }

    #[test]
    fn daily_sum_adds_every_session_once() {
        let mut db = Db::in_memory().unwrap();
        for (a, b) in [(0, 30), (60, 90), (120, 125)] {
            let id = db.begin(t(a), Mode::Pomodoro, None).unwrap();
            db.save(id, &record(&[(a, b)]), true).unwrap();
        }
        // Saving the same session twice replaces, never doubles.
        let id = db.begin(t(200), Mode::Stopwatch, None).unwrap();
        db.save(id, &record(&[(200, 210)]), false).unwrap();
        db.save(id, &record(&[(200, 220)]), true).unwrap();
        let all = db.worked_between(t(0), t(24 * 60), None).unwrap();
        assert_eq!(all, Duration::from_secs(85 * 60));
        let others = db.worked_between(t(0), t(24 * 60), Some(id)).unwrap();
        assert_eq!(others, Duration::from_secs(65 * 60));
    }

    #[test]
    fn crashed_sessions_close_at_their_last_work() {
        let mut db = Db::in_memory().unwrap();
        let id = db.begin(t(0), Mode::Stopwatch, None).unwrap();
        db.save(id, &record(&[(0, 12)]), false).unwrap();
        assert_eq!(db.session(id).unwrap().0, None);
        db.recover().unwrap();
        assert_eq!(db.session(id).unwrap().0, Some(millis(t(12))));
    }

    #[test]
    fn tags_are_unique_and_deleting_one_keeps_its_sessions() {
        let mut db = Db::in_memory().unwrap();
        let calc = db.upsert_tag(None, "Cálculo", "sky").unwrap();
        assert!(matches!(
            db.upsert_tag(None, " cálculo ", "rose"),
            Err(DbError::DuplicateName(_))
        ));
        assert!(matches!(
            db.upsert_tag(None, "  ", "rose"),
            Err(DbError::EmptyName)
        ));
        let eda = db.upsert_tag(None, "EDA", "not-a-colour").unwrap();
        assert_eq!(eda.color, TAG_COLORS[0]);
        let renamed = db.upsert_tag(Some(calc.id), "Cálculo II", "sage").unwrap();
        assert_eq!(renamed.id, calc.id);
        assert_eq!(
            db.tags()
                .unwrap()
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>(),
            ["Cálculo II", "EDA"]
        );

        let id = db.begin(t(0), Mode::Stopwatch, Some(calc.id)).unwrap();
        db.save(id, &record(&[(0, 10)]), true).unwrap();
        db.delete_tag(calc.id).unwrap();
        assert_eq!(db.session(id).unwrap().2, None);
        assert_eq!(
            db.worked_between(t(0), t(60), None).unwrap(),
            Duration::from_secs(600)
        );
    }
}
