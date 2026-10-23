use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::util::{ip_hash, now};

pub fn open(root: &std::path::Path) -> Connection {
    let data = root.join("data");
    for d in ["", "files", "tmp", "ads", "site"] {
        std::fs::create_dir_all(data.join(d)).ok();
    }
    let conn = Connection::open(data.join("uploader.sqlite")).expect("DBを開けませんでした");
    conn.pragma_update(None, "journal_mode", "WAL").ok();
    conn.pragma_update(None, "busy_timeout", 5000).ok();
    conn.pragma_update(None, "foreign_keys", "ON").ok();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            comment TEXT NOT NULL DEFAULT '',
            size INTEGER NOT NULL,
            mime TEXT NOT NULL DEFAULT '',
            md5 TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL,
            downloads INTEGER NOT NULL DEFAULT 0,
            delkey TEXT,
            dlpass TEXT,
            ip TEXT NOT NULL DEFAULT '',
            hidden INTEGER NOT NULL DEFAULT 0,
            imported INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS files_created ON files(created_at);
        CREATE TABLE IF NOT EXISTS bans (ip TEXT PRIMARY KEY, note TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS reports (
            id INTEGER PRIMARY KEY, file_id INTEGER NOT NULL, reason TEXT NOT NULL,
            ip TEXT NOT NULL, created_at INTEGER NOT NULL, resolved INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE IF NOT EXISTS contacts (
            id INTEGER PRIMARY KEY, category TEXT NOT NULL, name TEXT NOT NULL DEFAULT '', email TEXT NOT NULL DEFAULT '',
            file_id INTEGER, message TEXT NOT NULL, ip TEXT NOT NULL, created_at INTEGER NOT NULL, done INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE IF NOT EXISTS events (kind TEXT NOT NULL, ip TEXT NOT NULL, ref INTEGER NOT NULL DEFAULT 0, at INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS events_idx ON events(kind, ip, at);
        ",
    )
    .expect("DB初期化に失敗しました");
    let has_skey: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('files') WHERE name = 'skey'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if has_skey == 0 {
        conn.execute("ALTER TABLE files ADD COLUMN skey TEXT", []).ok();
    }
    conn
}

#[derive(Clone)]
pub struct FileRow {
    pub id: i64,
    pub name: String,
    pub comment: String,
    pub size: i64,
    #[allow(dead_code)]
    pub mime: String,
    pub md5: String,
    pub created_at: i64,
    pub downloads: i64,
    pub delkey: Option<String>,
    pub dlpass: Option<String>,
    pub ip: String,
    pub hidden: i64,
    /// 取り込み済みファイルはデモモード自動削除の対象外
    #[allow(dead_code)]
    pub imported: i64,
    pub skey: Option<String>,
}

impl FileRow {
    pub fn verify_delkey(&self, attempt: &str) -> bool {
        match &self.delkey {
            None => false,
            Some(hash) => format!("{attempt}") != "" && bcrypt::verify(attempt, hash).unwrap_or(false),
        }
    }

    pub fn verify_dlpass(&self, attempt: &str) -> bool {
        match &self.dlpass {
            None => true,
            Some(hash) => bcrypt::verify(attempt, hash).unwrap_or(false),
        }
    }
}

pub fn row_to_files(r: &Row) -> rusqlite::Result<FileRow> {
    Ok(FileRow {
        id: r.get("id")?,
        name: r.get("name")?,
        comment: r.get("comment")?,
        size: r.get("size")?,
        mime: r.get("mime")?,
        md5: r.get("md5")?,
        created_at: r.get("created_at")?,
        downloads: r.get("downloads")?,
        delkey: r.get("delkey")?,
        dlpass: r.get("dlpass")?,
        ip: r.get("ip")?,
        hidden: r.get("hidden")?,
        imported: r.get("imported")?,
        skey: r.get("skey")?,
    })
}

pub fn file_row(st: &crate::state::WebData, id: i64) -> Option<FileRow> {
    st.db_call(|c| {
            c.query_row("SELECT * FROM files WHERE id = ?", params![id], row_to_files)
                .optional()
        })
        .ok()
        .flatten()
}

impl crate::state::WebData {
    pub fn db_call<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> rusqlite::Result<T> {
        let c = self.db.lock().unwrap();
        f(&c)
    }

    pub fn ip_of(&self, ip: &str) -> String {
        ip_hash(&self.cfg.secret, ip)
    }

    pub fn event_count(&self, kind: &str, seconds: i64, ip: &str, r#ref: Option<i64>) -> i64 {
        if let Some(r) = r#ref {
            self.db_call(|c| {
                    c.query_row(
                        "SELECT COUNT(*) FROM events WHERE kind=? AND ip=? AND at>? AND ref=?",
                        params![kind, self.ip_of(ip), now() - seconds, r],
                        |x| x.get(0),
                    )
                })
                .unwrap_or(0)
        } else {
            self.db_call(|c| {
                    c.query_row(
                        "SELECT COUNT(*) FROM events WHERE kind=? AND ip=? AND at>?",
                        params![kind, self.ip_of(ip), now() - seconds],
                        |x| x.get(0),
                    )
                })
                .unwrap_or(0)
        }
    }

    pub fn event_add(&self, kind: &str, ip: &str, r#ref: i64) {
        let c = self.db.lock().unwrap();
        let _ = c.execute(
            "INSERT INTO events(kind, ip, ref, at) VALUES (?,?,?,?)",
            params![kind, self.ip_of(ip), r#ref, now()],
        );
        if rand::Rng::gen_range(&mut rand::thread_rng(), 0..50) == 0 {
            let _ = c.execute("DELETE FROM events WHERE at < ?", params![now() - 86400 * 2]);
        }
    }

    pub fn total_size(&self) -> i64 {
        self.db_call(|c| c.query_row("SELECT COALESCE(SUM(size),0) FROM files", [], |r| r.get(0)))
            .unwrap_or(0)
    }

    /// (ファイル数, ダウンロード合計)
    pub fn stats(&self) -> (i64, i64) {
        self.db_call(|c| {
                c.query_row(
                    "SELECT COUNT(*), COALESCE(SUM(downloads),0) FROM files",
                    [],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
                )
            })
            .unwrap_or((0, 0))
    }

    pub fn file_path(&self, id: i64) -> std::path::PathBuf {
        self.data().join("files").join(id.to_string())
    }

    /// ファイル本体を消す（ローカル / S3）
    pub fn storage_delete(&self, row: &FileRow) {
        if let Some(skey) = &row.skey {
            if let Err(e) = crate::s3::s3_delete(self, skey) {
                eprintln!("likeget-uploader: S3削除失敗: {e}");
            }
        } else {
            let _ = std::fs::remove_file(self.file_path(row.id));
        }
    }

    /// ローカル保存が有効（storage=local）
    #[allow(dead_code)]
    pub fn local_storage(&self) -> bool {
        self.cfg.storage != "s3"
    }
}

/// デモモード: 新しくアップロードされたファイルを一定時間で自動削除する
pub fn demo_purge(st: &crate::state::WebData, force: bool) {
    use rand::Rng;
    if !st.cfg.demo.enabled || (!force && rand::thread_rng().gen_range(0..10) != 0) {
        return;
    }
    let rows;
    {
        let c = st.db.lock().unwrap();
        rows = rows_from(
            &c,
            "SELECT * FROM files WHERE imported = 0 AND created_at < ?",
            params![now() - st.cfg.demo.keep_hours * 3600],
        );
    }
    for f in &rows {
        st.storage_delete(f);
        let _ = st.db_call(|c| c.execute("DELETE FROM files WHERE id = ?", params![f.id]));
    }
}

/// SELECT * FROM files 相当のヘルパ
pub fn rows_from(c: &Connection, sql: &str, args: impl rusqlite::Params) -> Vec<FileRow> {
    let mut q = match c.prepare(sql) {
        Ok(q) => q,
        Err(_) => return vec![],
    };
    let out = match q.query_map(args, row_to_files) {
        Ok(it) => it.filter_map(|r| r.ok()).collect(),
        Err(_) => vec![],
    };
    out
}

