//! Study history in SQLite (`rusqlite`, bundled): sessions, their work
//! segments and tags, with versioned migrations (`PRAGMA user_version`).
//! Times are stored as UTC milliseconds since the Unix epoch (CLAUDE.md
//! rule 11); they are shown in local time.
//!
//! A running session is saved every minute and when it ends, so a crash
//! loses at most a minute; at start any session left open is closed at its
//! last saved segment.
//!
//! Migrations go both ways (`up` and `down`), and a copy of the file is made
//! before an existing history is migrated (`study.sqlite3.v1.bak`, …).
//! Deleting a session from Statistics only marks it, so "Deshacer" can bring
//! it back; marked sessions are gone for good at the next launch.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use super::machine::{Mode, Record};
use super::stats::Session;

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
    #[error("couldn't back the history up before migrating it: {0}")]
    Backup(String),
}

/// Why a session couldn't be added or changed (the Statistics window shows
/// each one). Sent as `{ "kind": "overlap", "start": … }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionError {
    /// `start` (UTC ms) of the session it runs into, so the message can name
    /// it.
    #[error("it overlaps the session started at {start}")]
    Overlap { start: i64 },
    #[error("it would end in the future")]
    Future,
    #[error("a session lasts at least a minute")]
    TooShort,
    #[error("the session in progress can't change until it ends")]
    Running,
    #[error("there's no such session")]
    NotFound,
    #[error("the study history isn't available")]
    Unavailable,
}

impl From<DbError> for SessionError {
    fn from(e: DbError) -> Self {
        log::warn!("session change failed: {e}");
        Self::Unavailable
    }
}

impl From<rusqlite::Error> for SessionError {
    fn from(e: rusqlite::Error) -> Self {
        DbError::from(e).into()
    }
}

/// The shortest session that can be added or left after an edit.
const MIN_SESSION_MS: i64 = 60_000;

struct Migration {
    up: &'static str,
    down: &'static str,
}

/// Step `i` brings the schema from version `i` to `i + 1`, and back. Never
/// edit a released step: add a new one.
const MIGRATIONS: &[Migration] = &[
    Migration {
        up: "
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
",
        down: "
    DROP TABLE segments;
    DROP TABLE sessions;
    DROP TABLE tags;
",
    },
    // Statistics (SPEC §4.3d). `noise` is the noise playing when the session
    // started (NULL without noise, and for sessions older than this column);
    // `edited` is 0 as recorded, 1 edited, 2 added by hand; `deleted_at`
    // marks a deletion that can still be undone.
    Migration {
        up: "
    ALTER TABLE sessions ADD COLUMN noise TEXT;
    ALTER TABLE sessions ADD COLUMN edited INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE sessions ADD COLUMN deleted_at INTEGER;
    CREATE INDEX sessions_by_start ON sessions(started_at);
",
        down: "
    DELETE FROM sessions WHERE deleted_at IS NOT NULL;
    DROP INDEX sessions_by_start;
    ALTER TABLE sessions DROP COLUMN deleted_at;
    ALTER TABLE sessions DROP COLUMN edited;
    ALTER TABLE sessions DROP COLUMN noise;
",
    },
];

/// The schema version this build expects.
const LATEST: i64 = MIGRATIONS.len() as i64;

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

pub(super) fn mode_name(mode: Mode) -> &'static str {
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
        let conn = Connection::open(path)?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > 0 && version < LATEST {
            backup(&conn, path, version)?;
        }
        Self::init(conn)
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
        db.migrate_to(LATEST)?;
        db.recover()?;
        db.purge()?;
        Ok(db)
    }

    fn version(&self) -> Result<i64, DbError> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    /// Up or down to `target`, one step per transaction.
    fn migrate_to(&mut self, target: i64) -> Result<(), DbError> {
        let mut version = self.version()?;
        while version != target {
            let (step, to) = if version < target {
                (version, version + 1)
            } else {
                (version - 1, version - 1)
            };
            let Some(m) = usize::try_from(step).ok().and_then(|i| MIGRATIONS.get(i)) else {
                break;
            };
            let tx = self.conn.transaction()?;
            tx.execute_batch(if to > version { m.up } else { m.down })?;
            tx.pragma_update(None, "user_version", to)?;
            tx.commit()?;
            log::info!("study database migrated from version {version} to {to}");
            version = to;
        }
        Ok(())
    }

    /// Deletions nobody undid before quitting.
    fn purge(&mut self) -> Result<(), DbError> {
        let n = self
            .conn
            .execute("DELETE FROM sessions WHERE deleted_at IS NOT NULL", [])?;
        if n > 0 {
            log::info!("removed {n} deleted study session(s)");
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

    /// `noise`: the noise playing as the session starts, if any.
    pub fn begin(
        &mut self,
        started: SystemTime,
        mode: Mode,
        tag: Option<i64>,
        noise: Option<&str>,
    ) -> Result<i64, DbError> {
        self.conn.execute(
            "INSERT INTO sessions (started_at, mode, tag_id, noise) VALUES (?1, ?2, ?3, ?4)",
            params![millis(started), mode_name(mode), tag, noise],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Writes the session as it stands; `done` also closes it.
    pub fn save(&mut self, id: i64, record: &Record, done: bool) -> Result<(), DbError> {
        let tx = self.conn.transaction()?;
        write_record(&tx, id, record)?;
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

    /// Sessions started in `[from, to)` (UTC ms), oldest first, without
    /// their segments. The one in progress is included as stored.
    pub fn sessions(&self, from: i64, to: i64) -> Result<Vec<Session>, DbError> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "{SELECT} WHERE deleted_at IS NULL AND started_at >= ?1 AND started_at < ?2
             ORDER BY started_at"
        ))?;
        let rows = stmt.query_map(params![from, to], session_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Up to `limit` sessions started before `before`, newest first.
    pub fn sessions_before(&self, before: i64, limit: usize) -> Result<Vec<Session>, DbError> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "{SELECT} WHERE deleted_at IS NULL AND started_at < ?1
             ORDER BY started_at DESC LIMIT ?2"
        ))?;
        let rows = stmt.query_map(
            params![before, i64::try_from(limit).unwrap_or(i64::MAX)],
            session_row,
        )?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Fills in the segments of the sessions started in `[from, to)`.
    pub fn load_segments(
        &self,
        sessions: &mut [Session],
        from: i64,
        to: i64,
    ) -> Result<(), DbError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT g.session_id, g.started_at, g.ended_at FROM segments g
             JOIN sessions s ON s.id = g.session_id
             WHERE s.deleted_at IS NULL AND s.started_at >= ?1 AND s.started_at < ?2
             ORDER BY g.started_at",
        )?;
        let mut rows = stmt.query(params![from, to])?;
        while let Some(r) = rows.next()? {
            let id: i64 = r.get(0)?;
            if let Some(s) = sessions.iter_mut().find(|s| s.id == id) {
                s.segments.push((r.get(1)?, r.get(2)?));
            }
        }
        Ok(())
    }

    /// Changes a session's work time and tag. Shorter cuts its last stretches
    /// of work; longer stretches the last one, so the start never moves (with
    /// no pauses, there is one stretch: only the end moves).
    pub fn edit(
        &mut self,
        id: i64,
        secs: u64,
        tag: Option<i64>,
        now: i64,
    ) -> Result<(), SessionError> {
        let net = i64::try_from(secs.saturating_mul(1000)).unwrap_or(i64::MAX);
        if net < MIN_SESSION_MS {
            return Err(SessionError::TooShort);
        }
        let tx = self.conn.transaction()?;
        let found: Option<(i64, i64)> = tx
            .query_row(
                "SELECT started_at, net_ms FROM sessions
                 WHERE id = ?1 AND deleted_at IS NULL AND ended_at IS NOT NULL",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (start, old) = found.ok_or(SessionError::NotFound)?;
        let mut segments: Vec<(i64, i64)> = {
            let mut stmt = tx.prepare(
                "SELECT started_at, ended_at FROM segments WHERE session_id = ?1 ORDER BY started_at",
            )?;
            let rows = stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        if segments.is_empty() {
            segments.push((start, start + old));
        }
        let segments = resize(segments, net);
        let end = segments.last().map_or(start + net, |s| s.1);
        check(&tx, Some(id), start, end, now)?;
        write_segments(&tx, id, &segments)?;
        tx.execute(
            "UPDATE sessions SET net_ms = ?2, ended_at = ?3, tag_id = ?4,
                 edited = CASE edited WHEN 2 THEN 2 ELSE 1 END
             WHERE id = ?1",
            params![id, net, end, tag],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// A session studied without Rumor: one stretch of `secs` from `start`.
    pub fn add(
        &mut self,
        start: i64,
        secs: u64,
        tag: Option<i64>,
        now: i64,
    ) -> Result<i64, SessionError> {
        let net = i64::try_from(secs.saturating_mul(1000)).unwrap_or(i64::MAX);
        if net < MIN_SESSION_MS {
            return Err(SessionError::TooShort);
        }
        let end = start.saturating_add(net);
        let tx = self.conn.transaction()?;
        check(&tx, None, start, end, now)?;
        tx.execute(
            "INSERT INTO sessions (started_at, ended_at, net_ms, mode, tag_id, edited)
             VALUES (?1, ?2, ?3, 'stopwatch', ?4, 2)",
            params![start, end, net, tag],
        )?;
        let id = tx.last_insert_rowid();
        write_segments(&tx, id, &[(start, end)])?;
        tx.commit()?;
        Ok(id)
    }

    /// Marks a finished session as deleted; `restore` undoes it.
    pub fn delete(&mut self, id: i64, now: i64) -> Result<(), SessionError> {
        let n = self.conn.execute(
            "UPDATE sessions SET deleted_at = ?2
             WHERE id = ?1 AND deleted_at IS NULL AND ended_at IS NOT NULL",
            params![id, now],
        )?;
        if n == 0 {
            return Err(SessionError::NotFound);
        }
        Ok(())
    }

    pub fn restore(&mut self, id: i64) -> Result<(), SessionError> {
        let n = self.conn.execute(
            "UPDATE sessions SET deleted_at = NULL WHERE id = ?1 AND deleted_at IS NOT NULL",
            [id],
        )?;
        if n == 0 {
            return Err(SessionError::NotFound);
        }
        Ok(())
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

fn write_record(tx: &Transaction<'_>, id: i64, record: &Record) -> Result<(), DbError> {
    let segments: Vec<(i64, i64)> = record
        .segments
        .iter()
        .map(|(a, b)| (millis(*a), millis(*b)))
        .collect();
    write_segments(tx, id, &segments)
}

fn write_segments(tx: &Transaction<'_>, id: i64, segments: &[(i64, i64)]) -> Result<(), DbError> {
    tx.execute("DELETE FROM segments WHERE session_id = ?1", [id])?;
    let mut insert =
        tx.prepare("INSERT INTO segments (session_id, started_at, ended_at) VALUES (?1, ?2, ?3)")?;
    for (start, end) in segments {
        insert.execute(params![id, start, end])?;
    }
    Ok(())
}

const SELECT: &str = "SELECT id, started_at, COALESCE(ended_at, started_at + net_ms), net_ms,
        tag_id, noise, mode, edited FROM sessions";

fn session_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    let net: i64 = r.get(3)?;
    Ok(Session {
        id: r.get(0)?,
        start: r.get(1)?,
        end: r.get(2)?,
        secs: u64::try_from(net / 1000).unwrap_or(0),
        tag: r.get(4)?,
        noise: r.get(5)?,
        mode: r.get(6)?,
        edited: r.get(7)?,
        running: false,
        segments: Vec::new(),
    })
}

/// Work stretches holding exactly `net` ms: cut from the end, or the last
/// one made longer.
fn resize(mut segments: Vec<(i64, i64)>, net: i64) -> Vec<(i64, i64)> {
    let mut left = net;
    let mut kept = Vec::with_capacity(segments.len());
    for (a, b) in segments.drain(..) {
        if left <= 0 {
            break;
        }
        let len = (b - a).min(left);
        kept.push((a, a + len));
        left -= len;
    }
    if left > 0 {
        if let Some(last) = kept.last_mut() {
            last.1 += left;
        }
    }
    kept
}

/// The same rules for adding and editing: no end in the future and no
/// overlap with another session (the one in progress runs until `now`).
fn check(
    tx: &Transaction<'_>,
    except: Option<i64>,
    start: i64,
    end: i64,
    now: i64,
) -> Result<(), SessionError> {
    if end > now {
        return Err(SessionError::Future);
    }
    let clash: Option<i64> = tx
        .query_row(
            "SELECT started_at FROM sessions
             WHERE deleted_at IS NULL AND id IS NOT ?1
               AND started_at < ?3 AND COALESCE(ended_at, ?4) > ?2
             ORDER BY started_at LIMIT 1",
            params![except, start, end, now],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(start) = clash {
        return Err(SessionError::Overlap { start });
    }
    Ok(())
}

/// A copy of the file before migrating it, made with `VACUUM INTO` so it is
/// consistent even with a write-ahead log.
fn backup(conn: &Connection, path: &Path, version: i64) -> Result<(), DbError> {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".v{version}.bak"));
    let copy = std::path::PathBuf::from(name);
    let _ = std::fs::remove_file(&copy);
    let target = copy.to_string_lossy().replace('\'', "''");
    conn.execute_batch(&format!("VACUUM INTO '{target}'"))
        .map_err(|e| DbError::Backup(e.to_string()))?;
    log::info!("study history backed up to {}", copy.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn t(mins: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_800_000_000 + mins * 60)
    }

    fn ms(mins: u64) -> i64 {
        millis(t(mins))
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

    /// A finished session with these work stretches (minutes).
    fn stored(db: &mut Db, segments: &[(u64, u64)]) -> i64 {
        let id = db
            .begin(t(segments[0].0), Mode::Stopwatch, None, Some("brown"))
            .unwrap();
        db.save(id, &record(segments), true).unwrap();
        id
    }

    fn all(db: &Db) -> Vec<Session> {
        db.sessions(i64::MIN, i64::MAX).unwrap()
    }

    fn with_segments(db: &Db) -> Vec<Session> {
        let mut list = all(db);
        db.load_segments(&mut list, i64::MIN, i64::MAX).unwrap();
        list
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rumor-db-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("study.sqlite3")
    }

    #[test]
    fn migrations_run_once_and_reopen_cleanly() {
        let path = temp_path("reopen");
        {
            let mut db = Db::open(&path).unwrap();
            db.upsert_tag(None, "Cálculo", "sky").unwrap();
        }
        let db = Db::open(&path).unwrap();
        assert_eq!(db.tags().unwrap().len(), 1);
        assert_eq!(db.version().unwrap(), LATEST);
        drop(db);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn an_old_history_is_backed_up_migrated_and_can_go_back() {
        let path = temp_path("migrate");
        // A version 1 history, as the beta wrote it.
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0].up).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute(
                "INSERT INTO sessions (started_at, ended_at, net_ms, mode)
                 VALUES (?1, ?2, 600000, 'stopwatch')",
                params![ms(0), ms(10)],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO segments (session_id, started_at, ended_at) VALUES (1, ?1, ?2)",
                params![ms(0), ms(10)],
            )
            .unwrap();
        }
        let mut db = Db::open(&path).unwrap();
        assert!(
            path.with_file_name("study.sqlite3.v1.bak").exists(),
            "backed up before migrating"
        );
        assert_eq!(db.version().unwrap(), 2);
        let old = all(&db);
        assert_eq!(old.len(), 1);
        assert_eq!(
            (old[0].noise.as_deref(), old[0].edited, old[0].secs),
            (None, 0, 600)
        );

        // Down to version 1 and up again: the data survives both ways.
        db.migrate_to(1).unwrap();
        let cols: Vec<String> = {
            let mut stmt = db
                .conn
                .prepare("SELECT name FROM pragma_table_info('sessions')")
                .unwrap();
            stmt.query_map([], |r| r.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert!(
            !cols
                .iter()
                .any(|c| c == "noise" || c == "edited" || c == "deleted_at")
        );
        let n: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        db.migrate_to(LATEST).unwrap();
        assert_eq!(all(&db)[0].secs, 600);
        // And all the way down.
        db.migrate_to(0).unwrap();
        let tables: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tables, 0);
        drop(db);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn sessions_keep_their_noise_and_segments() {
        let mut db = Db::in_memory().unwrap();
        // With a pause in the middle: stored whole, stretches and all.
        let id = stored(&mut db, &[(0, 20), (40, 75)]);
        let list = with_segments(&db);
        assert_eq!(list.len(), 1);
        assert_eq!(
            (list[0].id, list[0].secs, list[0].noise.as_deref()),
            (id, 55 * 60, Some("brown"))
        );
        assert_eq!(list[0].segments, vec![(ms(0), ms(20)), (ms(40), ms(75))]);
        assert_eq!(db.session(id).unwrap(), (Some(ms(75)), 55 * 60_000, None));
    }

    #[test]
    fn saving_twice_replaces_never_doubles() {
        let mut db = Db::in_memory().unwrap();
        let id = db.begin(t(200), Mode::Stopwatch, None, None).unwrap();
        db.save(id, &record(&[(200, 210)]), false).unwrap();
        db.save(id, &record(&[(200, 220)]), true).unwrap();
        let list = with_segments(&db);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].secs, 20 * 60);
        assert_eq!(list[0].segments.len(), 1);
    }

    #[test]
    fn crashed_sessions_close_at_their_last_work() {
        let mut db = Db::in_memory().unwrap();
        let id = db.begin(t(0), Mode::Stopwatch, None, None).unwrap();
        db.save(id, &record(&[(0, 12)]), false).unwrap();
        assert_eq!(db.session(id).unwrap().0, None);
        db.recover().unwrap();
        assert_eq!(db.session(id).unwrap().0, Some(ms(12)));
    }

    #[test]
    fn editing_cuts_from_the_end_or_moves_the_end() {
        let mut db = Db::in_memory().unwrap();
        let now = ms(10_000);
        // 0–30 and 40–70: an hour of work with a pause.
        let id = stored(&mut db, &[(0, 30), (40, 70)]);
        db.edit(id, 45 * 60, None, now).unwrap();
        let list = with_segments(&db);
        assert_eq!(list[0].segments, vec![(ms(0), ms(30)), (ms(40), ms(55))]);
        assert_eq!(
            (list[0].secs, list[0].end, list[0].edited),
            (45 * 60, ms(55), 1)
        );
        // Shorter than the first stretch drops the second.
        db.edit(id, 20 * 60, None, now).unwrap();
        assert_eq!(with_segments(&db)[0].segments, vec![(ms(0), ms(20))]);
        // Longer: the last stretch grows; the start stays.
        db.edit(id, 90 * 60, None, now).unwrap();
        let list = with_segments(&db);
        assert_eq!(list[0].segments, vec![(ms(0), ms(90))]);
        assert_eq!((list[0].start, list[0].end), (ms(0), ms(90)));
    }

    #[test]
    fn edits_and_additions_cant_overlap_or_reach_the_future() {
        let mut db = Db::in_memory().unwrap();
        let now = ms(1_000);
        let first = stored(&mut db, &[(0, 60)]);
        stored(&mut db, &[(120, 180)]);
        // Lengthening into the next session.
        assert_eq!(
            db.edit(first, 130 * 60, None, now),
            Err(SessionError::Overlap { start: ms(120) })
        );
        // Up to it is fine.
        db.edit(first, 120 * 60, None, now).unwrap();
        // Adding over either one, too short, or past now.
        assert_eq!(
            db.add(ms(150), 3600, None, now),
            Err(SessionError::Overlap { start: ms(120) })
        );
        assert_eq!(db.add(ms(990), 3600, None, now), Err(SessionError::Future));
        assert_eq!(db.add(ms(500), 30, None, now), Err(SessionError::TooShort));
        // A session in progress (no end yet) runs until now and can't change.
        let open = db.begin(t(600), Mode::Stopwatch, None, None).unwrap();
        db.save(open, &record(&[(600, 610)]), false).unwrap();
        assert_eq!(
            db.add(ms(700), 600, None, now),
            Err(SessionError::Overlap { start: ms(600) })
        );
        assert_eq!(db.edit(open, 600, None, now), Err(SessionError::NotFound));
        // The message names the session it runs into, as the page gets it.
        assert_eq!(
            serde_json::to_value(SessionError::Overlap { start: 7 }).unwrap(),
            serde_json::json!({ "kind": "overlap", "start": 7 })
        );
        assert_eq!(
            serde_json::to_value(SessionError::Future).unwrap(),
            serde_json::json!({ "kind": "future" })
        );
        let added = db.add(ms(300), 1800, None, now).unwrap();
        let s = all(&db).into_iter().find(|s| s.id == added).unwrap();
        assert_eq!((s.secs, s.edited, s.noise), (1800, 2, None));
        // Editing keeps "added by hand".
        db.edit(added, 1200, None, now).unwrap();
        let s = all(&db).into_iter().find(|s| s.id == added).unwrap();
        assert_eq!(s.edited, 2);
    }

    #[test]
    fn deleting_can_be_undone_until_the_next_launch() {
        let mut db = Db::in_memory().unwrap();
        let id = stored(&mut db, &[(0, 30)]);
        db.delete(id, ms(100)).unwrap();
        assert!(all(&db).is_empty());
        assert_eq!(db.delete(id, ms(100)), Err(SessionError::NotFound));
        db.restore(id).unwrap();
        assert_eq!(all(&db).len(), 1);
        db.delete(id, ms(100)).unwrap();
        db.purge().unwrap();
        assert_eq!(db.restore(id), Err(SessionError::NotFound));
    }

    #[test]
    fn pages_go_back_in_time() {
        let mut db = Db::in_memory().unwrap();
        for k in 0..5 {
            stored(&mut db, &[(k * 100, k * 100 + 10)]);
        }
        let newest = db.sessions_before(i64::MAX, 2).unwrap();
        assert_eq!(
            newest.iter().map(|s| s.start).collect::<Vec<_>>(),
            [ms(400), ms(300)]
        );
        assert_eq!(db.sessions_before(ms(300), 10).unwrap().len(), 3);
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

        let id = db
            .begin(t(0), Mode::Stopwatch, Some(calc.id), None)
            .unwrap();
        db.save(id, &record(&[(0, 10)]), true).unwrap();
        db.delete_tag(calc.id).unwrap();
        assert_eq!(db.session(id).unwrap().2, None);
        assert_eq!(all(&db)[0].secs, 600);
    }
}
