//! お問い合わせ（旧 contact.php）
use actix_web::{HttpRequest, HttpResponse, web};
use rusqlite::params;

use crate::page_file::redirect_page;
use crate::session::Sess;
use crate::state::WebData;

const CATEGORIES: [&str; 5] = ["質問・要望", "ファイルの削除依頼", "権利侵害の申し立て", "不具合の報告", "その他"];

pub async fn contact(
    st: web::Data<WebData>,
    req: HttpRequest,
    payload: web::Payload,
) -> HttpResponse {
    let sess = Sess::open(&st, &req);
    if req.method() == actix_web::http::Method::POST {
        let form = match crate::formio::read_form(&st, &req, payload).await {
            Ok(f) => f,
            Err(e) => return crate::util::fail_html(400, &format!("送信エラー: {e}")),
        };
        if let Err(e) = sess.csrf_check(&st, &form.get("csrf"), &crate::formio::req_header(&req, "X-CSRF")) {
            return e;
        }
        if crate::hdl_upload::is_banned(&st, &sess.ip) {
            return crate::util::fail_html(403, "送信が制限されています。");
        }
        if let Err((code, msg)) =
            crate::turnstile::check(&st, &sess, "contact", false, &form.get("cf-turnstile-response"))
        {
            return crate::util::fail_html(code, msg);
        }
        if st.event_count("contact", 3600, &sess.ip, None) >= 5 {
            return crate::util::fail_html(429, "送信回数が多すぎます。しばらく時間をおいてください。");
        }
        let cat_v = form.get("category");
        let cat = if CATEGORIES.contains(&cat_v.as_str()) {
            cat_v
        } else {
            "その他".to_string()
        };
        let name = crate::mail::header_clean(&form.get("name"));
        let email = crate::mail::header_clean(&form.get("email"));
        if !email.is_empty() && !email.contains('@') {
            return crate::util::fail_html(400, "メールアドレスの形式が正しくありません。");
        }
        let file_id: i64 = form.get("file_id").parse().unwrap_or(0);
        let msg_val: String = form
            .get("message")
            .replace("\r\n", "\n")
            .trim()
            .chars()
            .take(3000)
            .collect();
        if msg_val.is_empty() {
            return crate::util::fail_html(400, "お問い合わせ内容を入力してください。");
        }
        let cid = st
            .db_call(|c| {
                c.execute(
                    "INSERT INTO contacts(category, name, email, file_id, message, ip, created_at) VALUES (?,?,?,?,?,?,?)",
                    params![cat, name, email, if file_id > 0 { Some(file_id) } else { None }, msg_val, st.ip_of(&sess.ip), crate::util::now()],
                )?;
                Ok(c.last_insert_rowid())
            })
            .unwrap_or(0);
        st.event_add("contact", &sess.ip, cid);

        let b_url = crate::util::base_url(&st, &req);
        let mut body = format!(
            "種類: {cat}\nお名前: {}\nメール: {}\n",
            if name.is_empty() { "（未記入）" } else { &name },
            if email.is_empty() { "（未記入・返信不可）" } else { &email }
        );
        if file_id > 0 {
            body += &format!("対象ファイル: No.{file_id} {}\n", crate::util::file_url(&st, &req, file_id));
        }
        body += &format!("\n{msg_val}\n\n管理画面: {b_url}/admin?tab=contacts");
        if !email.is_empty() {
            body += "\nこのメールに返信すると、問い合わせた人に届きます。";
        }
        crate::mail::notify_admin(
            &st,
            "contact",
            &format!("お問い合わせ: {cat}"),
            &body,
            if email.is_empty() { None } else { Some(email.as_str()) },
        );
        sess.set_flash(
            &st,
            &if email.is_empty() {
                "お問い合わせを送信しました。".to_string()
            } else {
                "お問い合わせを送信しました。管理人から返信がある場合は、入力いただいたメールアドレスに届きます。".to_string()
            },
        );
        let mut url = format!("{b_url}/contact?sent=1");
        if file_id > 0 {
            url += &format!("&file={file_id}");
        }
        return redirect_page(&st, &sess, &req, &url);
    }

    let file_id: i64 = crate::page_list::url_query_get(req.query_string(), "file")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let sent = crate::page_list::url_query_get(req.query_string(), "sent").is_some();
    let mut body = String::new();
    if !sent {
        let csrf = sess.csrf(&st);
        let cats: String = CATEGORIES
            .iter()
            .map(|c| format!("<option>{}</option>", crate::util::h(c)))
            .collect();
        let fid_shown = if file_id > 0 { file_id.to_string() } else { String::new() };
        body += &format!(
            "<form class=\"box narrow-wide\" method=\"post\" action=\"contact\"><h1>お問い合わせ</h1><p class=\"small\">このアップローダーの管理人へのご連絡はこちらから。ファイルの削除依頼・権利侵害の申し立ては、対象のファイル番号を書いてください。</p><input type=\"hidden\" name=\"csrf\" value=\"{}\"><div class=\"grid2\"><label>種類<select name=\"category\">{cats}</select></label><label>対象ファイルの番号 <small>（任意）</small><input type=\"text\" name=\"file_id\" inputmode=\"numeric\" pattern=\"[0-9]*\" value=\"{fid_shown}\" placeholder=\"例: 4974\"></label><label>お名前 <small>（任意）</small><input type=\"text\" name=\"name\" maxlength=\"50\"></label><label>メールアドレス <small>（返信が必要な場合）</small><input type=\"email\" name=\"email\" maxlength=\"200\"></label><label class=\"full\">お問い合わせ内容<textarea name=\"message\" rows=\"8\" maxlength=\"3000\" required></textarea></label></div>{}<p><button class=\"btn btn-primary\">送信する</button></p></form>",
            crate::util::h(&csrf),
            crate::turnstile::widget(&st, &sess, "contact", true)
        );
    } else {
        body += "<p><a class=\"btn\" href=\"./\">一覧へ戻る</a></p>";
    }
    let opts = crate::html::PageOpts::basic("お問い合わせ", "contact");
    let html = crate::html::render(&st, &req, &sess, &opts, &body);
    let mut resp = HttpResponse::Ok().content_type("text/html; charset=utf-8").body(html);
    crate::session::finalize(&mut resp, &sess, &st, &req);
    resp
}
