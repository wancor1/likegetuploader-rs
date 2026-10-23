//! 分割アップロードAPI（旧 upload.php）: init → chunk(複数回) → finish
use actix_web::{HttpResponse, web};
use md5::{Digest, Md5};
use rusqlite::params;

use crate::session::Sess;
use crate::state::WebData;
use crate::state::Sender;

fn json_ok(v: serde_json::Value) -> HttpResponse {
    let mut m = serde_json::Map::new();
    m.insert("ok".into(), serde_json::Value::Bool(true));
    if let serde_json::Value::Object(o) = v {
        for (k, v) in o {
            m.insert(k, v);
        }
    }
    HttpResponse::Ok()
        .content_type("application/json; charset=utf-8")
        .body(serde_json::Value::Object(m).to_string())
}

pub fn chunk_size(st: &WebData) -> i64 {
    let c = if st.cfg.chunk_size <= 0 { 8 * 1024 * 1024 } else { st.cfg.chunk_size };
    // S3は最後以外5MB以上が必須
    if crate::s3::enabled(st) && c < 5 * 1024 * 1024 {
        5 * 1024 * 1024
    } else {
        c
    }
}

pub fn is_banned(st: &WebData, ip: &str) -> bool {
    use rusqlite::OptionalExtension;
    st.db_call(|c| {
        c.query_row(
            "SELECT 1 FROM bans WHERE ip = ?",
            params![st.ip_of(ip)],
            |r| r.get::<_, i64>(0),
        )
        .optional()
    })
    .ok()
    .flatten()
    .is_some()
}

pub async fn upload(
    st: web::Data<WebData>,
    req: actix_web::HttpRequest,
    payload: web::Payload,
) -> HttpResponse {
    let sess = Sess::open(&st, &req);
    if req.method() != actix_web::http::Method::POST {
        return crate::util::fail_json(405, "POSTのみ受け付けます。");
    }
    let form = match crate::formio::read_form(&st, &req, payload).await {
        Ok(f) => f,
        Err(e) => return crate::util::fail_json(400, &format!("送信エラー: {e}")),
    };
    if let Err(_) = sess.csrf_check(&st, &form.get("csrf"), &crate::formio::req_header(&req, "X-CSRF")) {
        return crate::util::fail_json(400, "ページの有効期限が切れました。再読み込みしてやり直してください。");
    }
    if is_banned(&st, &sess.ip) {
        return crate::util::fail_json(403, "アップロードが制限されています。");
    }

    match form.get("action").as_str() {
        "init" => act_init(&st, &sess, &form).await,
        "chunk" => act_chunk(&st, &sess, &form).await,
        "finish" => act_finish(&st, &req, &sess, &form).await,
        _ => crate::util::fail_json(400, "不正な操作です。"),
    }
}

async fn act_init(
    st: &web::Data<WebData>,
    sess: &Sess,
    form: &crate::formio::Form,
) -> HttpResponse {
    if let Err((code, msg)) = crate::turnstile::check(st, sess, "upload", false, &form.get("cf-turnstile-response")) {
        return crate::util::fail_json(code, msg);
    }
    let up = &st.cfg.upload_password;
    if !up.is_empty() && up != &form.get("uppass") {
        return crate::util::fail_json(403, "アップロードパスワードが違います。");
    }
    if st.event_count("upload", 3600, &sess.ip, None) >= st.cfg.uploads_per_hour {
        return crate::util::fail_json(429, "アップロード数が上限に達しました。1時間ほど待ってください。");
    }

    let name = crate::util::clean_name(&form.get("name"));
    let size: i64 = form.get("size").parse().unwrap_or(-1);
    let comment = form
        .get("comment")
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string();
    if size <= 0 {
        return crate::util::fail_json(400, "空のファイルはアップロードできません。");
    }
    let max = crate::site::max_file_size(st);
    if size > max {
        return crate::util::fail_json(400, &format!("ファイルが大きすぎます（上限 {}）。", crate::util::fmt_size(max)));
    }
    if st.total_size() + size > st.cfg.max_total_size {
        return crate::util::fail_json(400, "アップローダーの容量がいっぱいです。管理人に連絡してください。");
    }
    if !crate::util::ext_allowed(&name, &st.cfg.allow_ext, &st.cfg.deny_ext) {
        return crate::util::fail_json(400, "この種類のファイルはアップロードできません。zipなどに圧縮してください。");
    }
    if comment.chars().count() as i64 > st.cfg.max_comment {
        return crate::util::fail_json(400, "コメントが長すぎます。");
    }
    let delkey = form.get("delkey");
    let dlpass = form.get("dlpass");

    cleanup_tmp(st);

    let token = crate::util::rand_hex(32);
    let mut send = Sender {
        name: name.clone(),
        size,
        comment: comment.clone(),
        delkey: if delkey.is_empty() { None } else { Some(bcrypt::hash(&delkey, bcrypt::DEFAULT_COST).unwrap_or_default()) },
        dlpass: if dlpass.is_empty() { None } else { Some(bcrypt::hash(&dlpass, bcrypt::DEFAULT_COST).unwrap_or_default()) },
        ip: st.ip_of(&sess.ip),
        next: 0,
        received: 0,
        mime: Some(crate::util::mime_guess(&name).to_string()),
        skey: None,
        s3_upload: None,
        etags: vec![],
    };

    if crate::s3::enabled(st) {
        let skey = crate::s3::object_key(st, &token);
        let uid = match crate::s3::mp_create(st, &skey, &name) {
            Ok(u) => u,
            Err(e) => {
                eprintln!("likeget-uploader: S3: {e}");
                return crate::util::fail_json(502, "保存先ストレージに接続できません。時間をおいて再度お試しください。");
            }
        };
        send.skey = Some(skey);
        send.s3_upload = Some(uid);
    } else {
        let path = st.data().join("tmp").join(format!("{token}.part"));
        if std::fs::File::create(&path).is_err() {
            return crate::util::fail_json(500, "保存に失敗しました。");
        }
    }
    st.senders.lock().unwrap().insert(token.clone(), send);
    st.md5s.lock().unwrap().insert(token.clone(), Md5::new());
    json_ok(serde_json::json!({ "token": token, "chunk": chunk_size(st) }))
}

async fn act_chunk(st: &web::Data<WebData>, sess: &Sess, form: &crate::formio::Form) -> HttpResponse {
    let token = form.get("token");
    let m = match st.senders.lock().unwrap().get(&token).cloned() {
        Some(m) => m,
        None => return crate::util::fail_json(400, "アップロードの有効期限が切れました。最初からやり直してください。"),
    };
    if m.ip != st.ip_of(&sess.ip) {
        return crate::util::fail_json(403, "不正な操作です。");
    }
    let idx: i64 = form.get("index").parse().unwrap_or(-1);
    if idx != m.next {
        return crate::util::fail_json(400, "送信順序が不正です。");
    }
    let (blob_path, len) = match &form.blob {
        Some((p, s)) => (p.clone(), *s as i64),
        None => {
            return crate::util::fail_json(400, "送信に失敗しました（サーバーのアップロード上限を確認してください）。")
        }
    };
    let is_last = m.received + len == m.size;
    if len <= 0 || len > chunk_size(st) || m.received + len > m.size {
        return crate::util::fail_json(400, "サイズが不正です。");
    }
    if crate::s3::enabled(st) && !is_last && len < 5 * 1024 * 1024 {
        return crate::util::fail_json(400, "分割サイズが不正です。");
    }

    if let Some(uid) = &m.s3_upload {
        let skey = m.skey.clone().unwrap_or_default();
        let etag = match crate::s3::mp_part(st, &skey, uid, (idx + 1) as usize, &blob_path) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("likeget-uploader: S3: {e}");
                return crate::util::fail_json(502, "保存先ストレージへの送信に失敗しました。もう一度お試しください。");
            }
        };
        if let Some(x) = st.senders.lock().unwrap().get_mut(&token) {
            x.etags.push(etag);
        }
    } else {
        let part = st.data().join("tmp").join(format!("{token}.part"));
        let data = std::fs::read(&blob_path).unwrap_or_default();
        if std::fs::OpenOptions::new()
            .append(true)
            .open(&part)
            .and_then(|mut f| std::io::Write::write_all(&mut f, &data))
            .is_err()
        {
            return crate::util::fail_json(500, "保存に失敗しました。");
        }
    }
    {
        let mut md5s = st.md5s.lock().unwrap();
        let ctx = md5s.entry(token.clone()).or_insert_with(Md5::new);
        let data = std::fs::read(&blob_path).unwrap_or_default();
        ctx.update(&data);
    }
    let _ = std::fs::remove_file(&blob_path);

    let received = m.received + len;
    if let Some(x) = st.senders.lock().unwrap().get_mut(&token) {
        x.next += 1;
        x.received = received;
    }
    json_ok(serde_json::json!({ "received": received }))
}

async fn act_finish(
    st: &web::Data<WebData>,
    req: &actix_web::HttpRequest,
    sess: &Sess,
    form: &crate::formio::Form,
) -> HttpResponse {
    let token = form.get("token");
    let m = match st.senders.lock().unwrap().get(&token).cloned() {
        Some(m) => m,
        None => return crate::util::fail_json(400, "アップロードの有効期限が切れました。最初からやり直してください。"),
    };
    if m.ip != st.ip_of(&sess.ip) {
        return crate::util::fail_json(403, "不正な操作です。");
    }
    if m.received != m.size {
        return crate::util::fail_json(400, "ファイルが最後まで届いていません。");
    }

    if m.s3_upload.is_some() {
        let skey_v = m.skey.clone().unwrap_or_default();
        if let Err(e) = crate::s3::mp_complete(st, &skey_v, m.s3_upload.as_deref().unwrap_or(""), &m.etags) {
            eprintln!("likeget-uploader: S3: {e}");
            crate::s3::mp_abort(st, &skey_v, m.s3_upload.as_deref().unwrap_or(""));
            st.senders.lock().unwrap().remove(&token);
            st.md5s.lock().unwrap().remove(&token);
            return crate::util::fail_json(502, "保存先ストレージでの結合に失敗しました。最初からやり直してください。");
        }
        let md5 = {
            let md5s = st.md5s.lock().unwrap();
            hex::encode(md5s.get(&token).cloned().unwrap_or_else(Md5::new).finalize())
        };
        st.md5s.lock().unwrap().remove(&token);
        let mime = m.mime.clone().unwrap_or_else(|| "application/octet-stream".into());
        let id = match insert_file(
            st,
            &m.name, &m.comment, m.size, &mime, &md5, m.delkey.as_deref(), m.dlpass.as_deref(), &m.ip, Some(&skey_v),
        ) {
            Ok(v) => v,
            Err(_) => return crate::util::fail_json(500, "保存に失敗しました。"),
        };
        st.senders.lock().unwrap().remove(&token);
        return complete_response(st, req, sess, id, &m);
    }

    // ローカル保存
    let part = st.data().join("tmp").join(format!("{token}.part"));
    let pcs = std::fs::metadata(&part).map(|x| x.len() as i64).unwrap_or(-1);
    if pcs != m.size {
        return crate::util::fail_json(400, "ファイルが最後まで届いていません。");
    }
    let md5 = crate::util::md5_file(&part).unwrap_or_default();
    let mime = crate::util::mime_guess(&m.name).to_string();
    let id = match insert_file(
        st,
        &m.name, &m.comment, m.size, &mime, &md5, m.delkey.as_deref(), m.dlpass.as_deref(), &m.ip, None,
    ) {
        Ok(v) => v,
        Err(_) => return crate::util::fail_json(500, "保存に失敗しました。"),
    };
    let dest = st.file_path(id);
    if std::fs::rename(&part, &dest).is_err() {
        let _ = st.db_call(|c| c.execute("DELETE FROM files WHERE id = ?", params![id]));
        return crate::util::fail_json(500, "保存に失敗しました。");
    }
    st.senders.lock().unwrap().remove(&token);
    st.md5s.lock().unwrap().remove(&token);
    complete_response(st, req, sess, id, &m)
}

fn insert_file(
    st: &web::Data<WebData>,
    name: &str,
    comment: &str,
    size: i64,
    mime: &str,
    md5: &str,
    delkey: Option<&str>,
    dlpass: Option<&str>,
    ip: &str,
    skey: Option<&str>,
) -> Result<i64, String> {
    st.db_call(|c| {
            c.execute(
                "INSERT INTO files(name, comment, size, mime, md5, created_at, delkey, dlpass, ip, skey) VALUES (?,?,?,?,?,?,?,?,?,?)",
                params![name, comment, size, mime, md5, crate::util::now(), delkey, dlpass, ip, skey],
            )?;
            Ok(c.last_insert_rowid())
        })
        .map_err(|e| e.to_string())
}

fn complete_response(
    st: &web::Data<WebData>,
    req: &actix_web::HttpRequest,
    sess: &Sess,
    id: i64,
    m: &Sender,
) -> HttpResponse {
    st.event_add("upload", &sess.ip, id);
    let b_url = crate::util::base_url(st, req);
    crate::mail::notify_admin(
        st,
        "upload",
        &format!("新しいアップロード No.{id}"),
        &format!(
            "ファイル: {}\nサイズ: {}\nコメント: {}\n\n{}\n管理画面: {b_url}/admin",
            m.name,
            crate::util::fmt_size(m.size),
            m.comment,
            crate::util::file_url(st, req, id)
        ),
        None,
    );
    json_ok(serde_json::json!({
        "id": id,
        "url": format!("/{}", crate::site::file_link(st, id)),
        "link": crate::site::file_link(st, id),
        "name": m.name,
    }))
}

/// 古い作業ファイルの掃除（1日以上前のもの。S3の途中データも中止させる）
fn cleanup_tmp(st: &web::Data<WebData>) {
    use rand::Rng;
    if rand::thread_rng().gen_range(0..20) != 0 {
        return;
    }
    let tokens: Vec<String> = {
        let s = st.senders.lock().unwrap();
        s.keys().cloned().collect()
    };
    for tok in tokens {
        let part = st.data().join("tmp").join(format!("{tok}.part"));
        let ok = std::fs::metadata(&part)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if ok < 86400 {
            continue;
        }
        if let Some(m) = st.senders.lock().unwrap().get(&tok).cloned() {
            if let (Some(skey), Some(uid)) = (m.skey.clone(), m.s3_upload.clone()) {
                crate::s3::mp_abort(st, &skey, &uid);
            }
        }
        st.senders.lock().unwrap().remove(&tok);
        st.md5s.lock().unwrap().remove(&tok);
        let _ = std::fs::remove_file(&part);
    }
}

impl WebData {
    pub fn senders_lock_update(&self, token: &str, f: impl FnOnce(&mut Sender)) {
        if let Some(x) = self.senders.lock().unwrap().get_mut(token) {
            f(x);
        }
    }
    pub fn senders_clone(&self, token: &str) -> Option<Sender> {
        self.senders.lock().unwrap().get(token).cloned()
    }
    pub fn senders_remove(&self, token: &str) {
        self.senders.lock().unwrap().remove(token);
    }
}


