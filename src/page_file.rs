//! ファイルページ（旧 file.php）＋そのPOST操作
use actix_web::{HttpResponse, web};
use rusqlite::params;

use crate::session::Sess;
use crate::state::WebData;
use crate::util::{h, number_format};

/// /download/{id} および /file?id= の両方を受け持つ
pub async fn detail(
    st: web::Data<WebData>,
    req: actix_web::HttpRequest,
    payload: web::Payload,
) -> HttpResponse {
    let sess = Sess::open(&st, &req);
    let id: i64 = req
        .match_info()
        .get("id")
        .and_then(|s| s.parse().ok())
        .or_else(|| {
            crate::page_list::url_query_get(req.query_string(), "id")
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(0);

    let f = match crate::db::file_row(&st, id) {
        Some(f) => f,
        None => return crate::util::fail_html(404, "ファイルが見つかりません。削除された可能性があります。"),
    };
    let admin = sess.is_admin(&st);
    if f.hidden != 0 && !admin {
        return crate::util::fail_html(404, "ファイルが見つかりません。削除された可能性があります。");
    }

    if req.method() == actix_web::http::Method::POST {
        return detail_post_async(&st, &req, &sess, &f, payload).await;
    }

    let url = crate::util::file_url(&st, &req, id);
    let up = st.cfg.upload_password.clone();
    let desc = if f.comment.is_empty() {
        String::new()
    } else {
        format!("{} ／ ", f.comment)
    };
    let _ = up;

    let mut body = String::new();
    body += "<p class=\"back\"><a href=\"./\">‹ 一覧へ戻る</a></p>";
    body += &crate::site::ad(&st, "detail_top");
    body += &format!(
        "<article class=\"box detail\"><h1 class=\"fname\">{}{}</h1>",
        crate::site::file_icon(&f.name),
        h(&f.name)
    );
    if f.hidden != 0 {
        body += "<p class=\"msg error\">非公開中（管理者のみ表示）</p>";
    }
    if !f.comment.is_empty() {
        body += &format!(
            "<p class=\"fcomment\">{}</p>",
            h(&f.comment).replace("\r\n", "\n").replace('\n', "<br>")
        );
    }
    body += &format!(
        "<dl class=\"meta\"><dt>番号</dt><dd>No.{}</dd><dt>容量</dt><dd>{} <small>({} バイト)</small></dd><dt>日時</dt><dd>{}</dd><dt>ダウンロード</dt><dd>{} 回</dd><dt>MD5</dt><dd><input class=\"copy\" readonly value=\"{}\"></dd><dt>URL</dt><dd><input class=\"copy\" readonly value=\"{}\"></dd></dl>",
        f.id,
        crate::util::fmt_size(f.size),
        number_format(f.size),
        crate::util::fmt_date(f.created_at, true, st.cfg.utc_offset_hours),
        number_format(f.downloads),
        h(&f.md5),
        h(&url)
    );
    let csrf = sess.csrf(&st);
    let pass_field = if f.dlpass.is_some() {
        "<label>🔒 DLパスワード <input type=\"password\" name=\"pass\" required autocomplete=\"off\"></label>".to_string()
    } else {
        String::new()
    };
    let ts = crate::turnstile::widget(&st, &sess, "download", false);
    body += &format!(
        "<form class=\"dlform\" method=\"post\" action=\"dl\"><input type=\"hidden\" name=\"csrf\" value=\"{}\"><input type=\"hidden\" name=\"id\" value=\"{}\">{}{}<button class=\"btn btn-primary btn-big\">⇩ ダウンロード</button><p class=\"small\">ダウンロードした時点で<a href=\"rules\" target=\"_blank\">利用規約</a>に同意したものとみなします。</p></form>",
        h(&csrf), f.id, pass_field, ts
    );
    body += "</article>";
    body += &crate::site::ad(&st, "detail_bottom");
    body += &format!(
        "<div class=\"grid2 sub\">\
<details class=\"box\"><summary>投稿者用：このファイルを削除</summary><form method=\"post\" class=\"inline\"><input type=\"hidden\" name=\"csrf\" value=\"{}\"><input type=\"hidden\" name=\"act\" value=\"delete\"><input type=\"password\" name=\"delkey\" placeholder=\"削除キー\" required><button class=\"btn danger\">削除する</button></form></details>\
<details class=\"box\"><summary>不審なファイル・権利侵害を通報</summary><p class=\"small\">返信が必要な場合や削除依頼は <a href=\"contact?file={}\">お問い合わせフォーム</a> へ。</p><form method=\"post\"><input type=\"hidden\" name=\"csrf\" value=\"{}\"><input type=\"hidden\" name=\"act\" value=\"report\"><textarea name=\"reason\" rows=\"3\" maxlength=\"1000\" placeholder=\"理由（ウイルス、無断転載など）\" required></textarea><button class=\"btn\">通報する</button></form></details></div>",
        h(&csrf), id, h(&csrf)
    );

    let opts = crate::html::PageOpts {
        title: f.name.clone(),
        active: "",
        description: format!(
            "{}{}（{}）をダウンロードできます。",
            desc,
            f.name,
            crate::util::fmt_size(f.size)
        ),
        url: Some(url),
        image: None,
        typ: "article",
        noindex: f.hidden != 0,
    };
    let html = crate::html::render(&st, &req, &sess, &opts, &body);
    let mut resp = HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .insert_header(("X-Content-Type-Options", "nosniff"))
        .insert_header(("Referrer-Policy", "same-origin"))
        .body(html);
    crate::session::finalize(&mut resp, &sess, &st, &req);
    resp
}

/// /download/{id} へのPOST
async fn detail_post_async(
    st: &web::Data<WebData>,
    req: &actix_web::HttpRequest,
    sess: &Sess,
    f: &crate::db::FileRow,
    payload: web::Payload,
) -> HttpResponse {
    let id = f.id;
    let form = crate::formio::read_form(st, req, payload).await;
    let form = match form {
        Ok(fm) => fm,
        Err(e) => return crate::util::fail_html(400, &format!("送信エラー: {e}")),
    };
    if let Err(e) = sess.csrf_check(st, &form.get("csrf"), &crate::formio::req_header(req, "X-CSRF")) {
        return e;
    }
    match form.get("act").as_str() {
        "delete" => {
            if st.event_count("delfail", 3600, &sess.ip, None) >= 10 {
                return crate::util::fail_html(429, "削除キーの入力ミスが多すぎます。しばらく待ってください。");
            }
            let key = form.get("delkey");
            if f.delkey.is_none() || key.is_empty() || !f.verify_delkey(&key) {
                st.event_add("delfail", &sess.ip, id);
                return crate::util::fail_html(400, "削除キーが違います。");
            }
            let _ = st.db_call(|c| c.execute("DELETE FROM files WHERE id = ?", params![id]));
            st.storage_delete(f);
            sess.set_flash(st, &format!("No.{id} を削除しました。"));
            redirect_page(st, sess, req, &format!("{}/", crate::util::base_url(st, req)))
        }
        "report" => {
            let reason: String = form.get("reason").trim().chars().take(1000).collect();
            if reason.is_empty() {
                return crate::util::fail_html(400, "通報内容を入力してください。");
            }
            if st.event_count("report", 3600, &sess.ip, None) >= 5 {
                return crate::util::fail_html(429, "通報は少し時間をおいてください。");
            }
            let _ = st.db_call(|c| {
                c.execute(
                    "INSERT INTO reports(file_id, reason, ip, created_at) VALUES (?,?,?,?)",
                    params![id, reason, st.ip_of(&sess.ip), crate::util::now()],
                )
            });
            st.event_add("report", &sess.ip, id);
            let body = format!(
                "ファイル: No.{id} {}\n{}\n\n通報内容:\n{reason}\n\n管理画面: {}/admin?tab=reports",
                f.name,
                crate::util::file_url(st, req, id),
                crate::util::base_url(st, req)
            );
            crate::mail::notify_admin(st, "report", &format!("通報 No.{id} {}", f.name), &body, None);
            sess.set_flash(st, "通報を受け付けました。管理人が確認します。");
            redirect_page(st, sess, req, &crate::util::file_url(st, req, id))
        }
        _ => crate::util::fail_html(400, "不正な操作です。"),
    }
}

/// 共通のリダイレクト（303・新規セッションのSet-Cookieも付与）
pub fn redirect_page(
    st: &web::Data<WebData>,
    sess: &Sess,
    req: &actix_web::HttpRequest,
    url: &str,
) -> HttpResponse {
    let mut resp = HttpResponse::SeeOther()
        .insert_header(("Location", url.to_string()))
        .finish();
    crate::session::finalize(&mut resp, sess, st, req);
    resp
}
