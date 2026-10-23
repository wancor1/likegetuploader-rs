use std::collections::HashMap;
use std::sync::Mutex;

use rusqlite::Connection;

pub struct WebData {
    pub cfg: crate::config::Config,
    pub root: std::path::PathBuf,
    pub db: Mutex<Connection>,
    pub sessions: Mutex<HashMap<String, Session>>,
    /// 分割アップロード途中
    pub senders: Mutex<HashMap<String, Sender>>,
    /// S3モードのファイル全体MD5の計算途中
    pub md5s: Mutex<HashMap<String, md5::Md5>>,
}

/// 分割アップロード中の状態
#[derive(Clone)]
pub struct Sender {
    pub name: String,
    pub size: i64,
    pub comment: String,
    pub delkey: Option<String>,
    pub dlpass: Option<String>,
    pub ip: String,
    pub next: i64,
    pub received: i64,
    pub mime: Option<String>,
    pub skey: Option<String>,
    pub s3_upload: Option<String>,
    pub etags: Vec<String>,
}

#[derive(Clone, Default)]
pub struct Session {
    pub csrf: String,
    pub admin: bool,
    pub flash: Option<String>,
    pub ts_until: i64,
    /// 最終活動時刻（セッション掃除用）
    pub last: i64,
}

impl WebData {
    pub fn data(&self) -> std::path::PathBuf {
        self.root.join("data")
    }
}
