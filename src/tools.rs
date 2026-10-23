//! CLIツール: uploader.jpバックアップの取り込み（import）、S3への移行（move-to-s3）
use std::path::Path;

use rusqlite::params;
use serde::Deserialize;

use crate::config::Config;

#[derive(Deserialize)]
struct Meta {
    id: i64,
    stored: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    original: String,
    #[serde(default)]
    comment: String,
    #[serde(default)]
    size: i64,
    #[serde(default)]
    md5: String,
    #[serde(default)]
    created_at: i64,
    #[serde(default)]
    downloads: i64,
}

fn make_cli(root: &Path, cfg: Config) -> crate::state::WebData {
    crate::state::WebData {
        cfg,
        root: root.to_path_buf(),
        db: std::sync::Mutex::new(crate::db::open(root)),
        sessions: std::sync::Mutex::new(std::collections::HashMap::new()),
        senders: std::sync::Mutex::new(std::collections::HashMap::new()),
        md5s: std::sync::Mutex::new(std::collections::HashMap::new()),
    }
}

/// getuploader_backup.py で保存したフォルダを、番号・日時・DL数・コメントそのままで取り込む
///   likeget-uploader import <ディレクトリ> [--move]
/// storage=s3 が有効ならファイルはS3へ送られる
pub fn import(root: &Path, cfg: &Config, dir: &str, r#move: bool) -> bool {
    let dir = Path::new(dir);
    let meta_path = dir.join("meta.jsonl");
    if dir.as_os_str().is_empty() || !meta_path.is_file() {
        eprintln!("使い方: likeget-uploader import <バックアップフォルダ> [--move]");
        return false;
    }
    let st = make_cli(root, cfg.clone());
    let s3_on = crate::s3::enabled(&st);
    println!("保存先: {}", if s3_on { format!("S3 ({})", st.cfg.s3.bucket) } else { "このサーバー".to_string() });
    let mut ok = 0;
    let mut skip = 0;
    let mut bad = 0;

    let lines = std::fs::read_to_string(&meta_path).unwrap_or_default();
    for line in lines.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let m: Meta = match serde_json::from_str(line) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let id = m.id;
        let src = dir.join("files").join(&m.stored);
        if !src.is_file() {
            println!("見つからない: #{id} {}", m.stored);
            bad += 1;
            continue;
        }
        let exists: i64 = st
            .db_call(|c| c.query_row("SELECT 1 FROM files WHERE id = ?", params![id], |r| r.get(0)))
            .unwrap_or(0);
        if exists == 1 {
            skip += 1;
            continue;
        }
        if crate::util::md5_file(&src).unwrap_or_default() != m.md5 {
            println!("MD5不一致: #{id}");
            bad += 1;
            continue;
        }
        let base_name = m.name.clone().filter(|x| !x.is_empty()).unwrap_or_else(|| m.original.clone());
        let name = crate::util::clean_name(if base_name.is_empty() { "file" } else { &base_name });
        let mime = crate::util::mime_guess(&name).to_string();

        let mut skey: Option<String> = None;
        if s3_on {
            let k = crate::s3::object_key(&st, &crate::util::rand_hex(32));
            match crate::s3::put_file(&st, &k, &src, &name) {
                Err(e) => {
                    println!("S3送信失敗: #{id} {e}");
                    bad += 1;
                    continue;
                }
                Ok(_) => {
                    skey = Some(k);
                    if r#move {
                        let _ = std::fs::remove_file(&src);
                    }
                }
            }
        } else {
            let dest = st.file_path(id);
            let done = if r#move {
                std::fs::rename(&src, &dest)
            } else {
                std::fs::copy(&src, &dest).map(|_| ())
            };
            if done.is_err() {
                println!("コピー失敗: #{id}");
                bad += 1;
                continue;
            }
        }
        let created = if m.created_at > 0 { m.created_at } else { crate::util::now() };
        println!("#{id} {name}");
        let _ = st.db_call(|c| {
            c.execute(
                "INSERT OR IGNORE INTO files(id, name, comment, size, mime, md5, created_at, downloads, imported, skey) VALUES (?,?,?,?,?,?,?,?,1,?)",
                params![id, name, m.comment, m.size, mime, m.md5, created, m.downloads, skey],
            )
        });
        ok += 1;
    }

    // 次の新規アップロード番号が、取り込んだ最大番号の次になるようにする
    let max: i64 = st.db_call(|c| c.query_row("SELECT COALESCE(MAX(id),0) FROM files", [], |r| r.get(0))).unwrap_or(0);
    let _ = st.db_call(|c| c.execute_batch(&format!(
        "UPDATE sqlite_sequence SET seq = MAX(seq, {max}) WHERE name = 'files';
         INSERT INTO sqlite_sequence(name, seq) SELECT 'files', {max} WHERE NOT EXISTS (SELECT 1 FROM sqlite_sequence WHERE name = 'files');"
    )));

    println!("取り込み: {ok}件 / 既にあり: {skip}件 / 失敗: {bad}件");
    println!("次の新規アップロードは No.{} からになります。", max + 1);
    true
}

/// このサーバーに保存済みのファイルをS3へ移す（storage=s3 に設定してから実行）
///   likeget-uploader move-to-s3 [--keep]
pub fn move_to_s3(root: &Path, cfg: &Config, keep: bool) -> bool {
    let st = make_cli(root, cfg.clone());
    if !crate::s3::enabled(&st) {
        eprintln!("config.toml の storage が 's3' になっていません。");
        return false;
    }
    let rows: Vec<crate::db::FileRow> = crate::db::rows_from(
        &st.db.lock().unwrap(),
        "SELECT * FROM files WHERE skey IS NULL ORDER BY id",
        params![],
    );
    println!("{} 件を S3 ({}) へ移します", rows.len(), st.cfg.s3.bucket);
    let mut ok = 0;
    let mut bad = 0;
    for r in &rows {
        let src = st.file_path(r.id);
        if !src.is_file() {
            println!("見つからない: #{}", r.id);
            bad += 1;
            continue;
        }
        let key = crate::s3::object_key(&st, &crate::util::rand_hex(32));
        match crate::s3::put_file(&st, &key, &src, &r.name) {
            Err(e) => {
                println!("失敗: #{} {e}", r.id);
                bad += 1;
                continue;
            }
            Ok(_) => {
                let _ = st.db_call(|c| c.execute("UPDATE files SET skey = ? WHERE id = ?", params![key, r.id]));
                if !keep {
                    let _ = std::fs::remove_file(&src);
                }
                println!("#{} {}", r.id, r.name);
                ok += 1;
            }
        }
    }
    println!("完了: {ok}件 / 失敗: {bad}件");
    true
}
