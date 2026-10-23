//! ダウンロード本体（旧 dl.php）
use actix_web::{HttpResponse, web};
use rusqlite::params;

use crate::session::Sess;
use crate::state::WebData;

pub async fn dl(
    st: web::Data<WebData>,
    req: actix_web::HttpRequest,
    payload: web::Payload,
) -> HttpResponse {
    let sess = Sess::open(&st, &req);
    if req.method() != actix_web::http::Method::POST {
        // 直リンクは詳細ページへ回す
        let id: i64 = crate::page_list::url_query_get(req.query_string(), "id")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let url = if id > 0 {
            crate::util::file_url(&st, &req, id)
        } else {
            format!("{}/", crate::util::base_url(&st, &req))
        };
        return crate::page_file::redirect_page(&st, &sess, &req, &url);
    }

    let form = match crate::formio::read_form(&st, &req, payload).await {
        Ok(f) => f,
        Err(e) => return crate::util::fail_html(400, &format!("送信エラー: {e}")),
    };
    if let Err(e) = sess.csrf_check(&st, &form.get("csrf"), &crate::formio::req_header(&req, "X-CSRF")) {
        return e;
    }
    let id: i64 = match form.get("id").parse() {
        Ok(v) => v,
        Err(_) => return crate::util::fail_html(400, "不正な操作です。"),
    };
    let f = match crate::db::file_row(&st, id) {
        Some(f) => f,
        None => return crate::util::fail_html(404, "ファイルが見つかりません。"),
    };
    if f.hidden != 0 && !sess.is_admin(&st) {
        return crate::util::fail_html(404, "ファイルが見つかりません。");
    }
    if let Err((code, msg)) = crate::turnstile::check(&st, &sess, "download", false, &form.get("cf-turnstile-response")) {
        return crate::util::fail_html(code, msg);
    }
    if f.dlpass.is_some() {
        if st.event_count("dlfail", 3600, &sess.ip, None) >= 20 {
            return crate::util::fail_html(429, "パスワードの入力ミスが多すぎます。しばらく待ってください。");
        }
        if !f.verify_dlpass(&form.get("pass")) {
            st.event_add("dlfail", &sess.ip, id);
            return crate::util::fail_html(400, "DLパスワードが違います。");
        }
    }

    let path = st.file_path(id);
    if f.skey.is_none() && !path.is_file() {
        return crate::util::fail_html(410, "ファイル本体が見つかりません。管理人に連絡してください。");
    }

    // 同じ人の連続DLは数えない
    if st.event_count("dl", st.cfg.dl_count_window, &sess.ip, Some(id)) == 0 {
        let _ = st.db_call(|c| c.execute("UPDATE files SET downloads = downloads + 1 WHERE id = ?", params![id]));
        st.event_add("dl", &sess.ip, id);
    }

    // S3保存のファイルはストレージから直接
    if let Some(skey) = &f.skey {
        if !st.cfg.s3.public_url.is_empty() {
            let enc = skey
                .split('/')
                .map(crate::util::raw_urlencode)
                .collect::<Vec<_>>()
                .join("/");
            let url = format!("{}/{}", st.cfg.s3.public_url.trim_end_matches('/'), enc);
            return crate::page_file::redirect_page(&st, &sess, &req, &url);
        }
        let url = crate::s3::presign(&st, skey, &f.name, st.cfg.s3.url_expires.max(1));
        return crate::page_file::redirect_page(&st, &sess, &req, &url);
    }

    let name = f.name.clone();
    let ascii: String = name
        .chars()
        .map(|c| if c.is_ascii_graphic() && c != '"' && c != '\\' && c != ',' && c != ';' { c } else { '_' })
        .collect();
    let disp = format!(
        "attachment; filename=\"{}\"; filename*=UTF-8''{}",
        ascii,
        crate::util::raw_urlencode(&name)
    );
    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(_) => return crate::util::fail_html(410, "ファイル本体が見つかりません。管理人に連絡してください。"),
    };
    let len = match file.metadata().await {
        Ok(m) => m.len(),
        Err(_) => 0,
    };
    let stream = actix_web::body::SizedStream::new(len, tokio_util::io::ReaderStream::new(file));
    HttpResponse::Ok()
        .content_type("application/octet-stream")
        .insert_header(("Content-Disposition", disp))
        .insert_header(("X-Content-Type-Options", "nosniff"))
        .insert_header(("Cache-Control", "private, no-transform"))
        .body(stream)
}
