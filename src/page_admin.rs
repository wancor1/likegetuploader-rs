//! 管理画面（旧 admin.php）
use actix_web::{HttpRequest, HttpResponse, web};
use rusqlite::params;

use crate::page_file::redirect_page;
use crate::session::Sess;
use crate::state::WebData;
use crate::util::h;

pub async fn admin(
    st: web::Data<WebData>,
    req: HttpRequest,
    payload: web::Payload,
) -> HttpResponse {
    let sess = Sess::open(&st, &req);
    let is_admin = sess.is_admin(&st);

    // ---- ログイン ----
    if !is_admin {
        if req.method() == actix_web::http::Method::POST {
            let form = match crate::formio::read_form(&st, &req, payload).await {
                Ok(f) => f,
                Err(e) => return crate::util::fail_html(400, &format!("送信エラー: {e}")),
            };
            if let Err(e) = sess.csrf_check(&st, &form.get("csrf"), &crate::formio::req_header(&req, "X-CSRF")) {
                return e;
            }
            if st.event_count("adminfail", 900, &sess.ip, None) >= 5 {
                return crate::util::fail_html(429, "ログイン失敗が多すぎます。15分ほど待ってください。");
            }
            if let Err((code, msg)) = crate::turnstile::check(&st, &sess, "admin", true, &form.get("cf-turnstile-response")) {
                return crate::util::fail_html(code, msg);
            }
            let pw = &st.cfg.admin_password;
            if !pw.is_empty() && pw != "change-me" {
                // 定数時間比較
                if !pw.is_empty() && crate::util::consttime_eq(pw.as_bytes(), form.get("password").as_bytes()) {
                    sess.set_admin(&st, true);
                    return redirect_page(&st, &sess, &req, "/admin");
                }
            }
            st.event_add("adminfail", &sess.ip, 0);
            sess.set_flash(&st, "パスワードが違います。");
            return redirect_page(&st, &sess, &req, "/admin");
        }
        let csrf = sess.csrf(&st);
        let warn = if st.cfg.admin_password.is_empty() || st.cfg.admin_password == "change-me" {
            "<p class=\"msg error\">config.toml の admin_password を設定するまでログインできません。</p>"
        } else {
            ""
        };
        let ts = crate::turnstile::widget(&st, &sess, "admin", true);
        let body = format!(
            "<form class=\"box narrow\" method=\"post\"><h1>管理ログイン</h1>{warn}<input type=\"hidden\" name=\"csrf\" value=\"{}\"><label>パスワード <input type=\"password\" name=\"password\" required autofocus></label>{ts}<button class=\"btn btn-primary\">ログイン</button></form>",
            h(&csrf)
        );
        let opts = crate::html::PageOpts {
            title: "管理".into(),
            active: "admin",
            description: String::new(),
            url: None,
            image: None,
            typ: "website",
            noindex: true,
        };
        let html = crate::html::render(&st, &req, &sess, &opts, &body);
        let mut resp = HttpResponse::Ok().content_type("text/html; charset=utf-8").body(html);
        crate::session::finalize(&mut resp, &sess, &st, &req);
        return resp;
    }

    // ---- POST操作 ----
    if req.method() == actix_web::http::Method::POST {
        let form = match crate::formio::read_form(&st, &req, payload).await {
            Ok(f) => f,
            Err(e) => return crate::util::fail_html(400, &format!("送信エラー: {e}")),
        };
        if let Err(e) = sess.csrf_check(&st, &form.get("csrf"), &crate::formio::req_header(&req, "X-CSRF")) {
            return e;
        }
        let tab = crate::page_list::url_query_get(req.query_string(), "tab").unwrap_or_else(|| "files".into());
        let page: i64 = crate::page_list::url_query_get(req.query_string(), "page").and_then(|v| v.parse().ok()).unwrap_or(1);
        let q = crate::page_list::url_query_get(req.query_string(), "q").unwrap_or_default();
        let back = format!("/admin?tab={tab}&q={q}&page={page}");
        let back = back.replace("&q=&", "&");
        let back = if q.is_empty() { back.replace("q=&", "").replace("?tab=files&q=&page=1", "?tab=files") } else { back };

        let act = form.get("act");
        let id: i64 = form.get("id").parse().unwrap_or(0);
        match act.as_str() {
            "logout" => {
                sess.set_admin(&st, false);
                return redirect_page(&st, &sess, &req, "/");
            }
            "save" => {
                let name = crate::util::clean_name(&form.get("name"));
                let comment = form.get("comment").replace("\r\n", "\n").trim().to_string();
                let _ = st.db_call(|c| {
                    c.execute("UPDATE files SET name = ?, comment = ? WHERE id = ?", params![name, comment, id])
                });
                sess.set_flash(&st, &format!("No.{id} を更新しました。"));
            }
            "hide" => {
                let _ = st.db_call(|c| {
                    c.execute("UPDATE files SET hidden = 1 - hidden WHERE id = ?", params![id])
                });
                sess.set_flash(&st, &format!("No.{id} の公開状態を切り替えました。"));
            }
            "delete" => {
                if let Some(row) = crate::db::file_row(&st, id) {
                    st.storage_delete(&row);
                }
                let _ = st.db_call(|c| c.execute("DELETE FROM files WHERE id = ?", params![id]));
                let _ = st.db_call(|c| {
                    c.execute("UPDATE reports SET resolved = 1 WHERE file_id = ?", params![id])
                });
                sess.set_flash(&st, &format!("No.{id} を削除しました。"));
            }
            "ban" => {
                let ip = st
                    .db_call(|c| {
                        c.query_row("SELECT ip FROM files WHERE id = ?", params![id], |r| r.get::<_, String>(0))
                    })
                    .unwrap_or_default();
                if !ip.is_empty() {
                    let _ = st.db_call(|c| {
                        c.execute(
                            "INSERT INTO bans(ip, note, created_at) VALUES (?,?,?) ON CONFLICT(ip) DO UPDATE SET note = excluded.note, created_at = excluded.created_at",
                            params![ip, format!("No.{id} の投稿者"), crate::util::now()],
                        )
                    });
                    sess.set_flash(&st, &format!("No.{id} の投稿者をアップロード禁止にしました。"));
                } else {
                    sess.set_flash(&st, "この投稿は投稿者情報がありません（取り込み分など）。");
                }
            }
            "unban" => {
                let ip = form.get("ip");
                let _ = st.db_call(|c| c.execute("DELETE FROM bans WHERE ip = ?", params![ip]));
                sess.set_flash(&st, "禁止を解除しました。");
            }
            "contact_done" => {
                let _ = st.db_call(|c| {
                    c.execute("UPDATE contacts SET done = 1 - done WHERE id = ?", params![id])
                });
                sess.set_flash(&st, "お問い合わせの状態を切り替えました。");
            }
            "contact_delete" => {
                let _ = st.db_call(|c| c.execute("DELETE FROM contacts WHERE id = ?", params![id]));
                sess.set_flash(&st, "お問い合わせを削除しました。");
            }
            "resolve" => {
                let _ = st.db_call(|c| c.execute("UPDATE reports SET resolved = 1 WHERE id = ?", params![id]));
                sess.set_flash(&st, "通報を対応済みにしました。");
            }
            "mailtest" => {
                if !crate::mail::enabled(&st, None) {
                    sess.set_flash(&st, "メール通知が無効か、設定が足りません（config.toml の mail）。");
                } else {
                    let to = st.cfg.mail.to.clone();
                    match crate::mail::smtp_send(
                        &st,
                        &to,
                        &format!("[{}] テストメール", st.cfg.title),
                        &format!("likeget-uploader からのテストメールです。\nこのメールが届いていれば、通知の設定は正しくできています。\n\n{}", crate::util::base_url(&st, &req)),
                        None,
                    ) {
                        Ok(_) => sess.set_flash(&st, &format!("テストメールを {to} へ送信しました。")),
                        Err(e) => sess.set_flash(&st, &format!("テストメール送信失敗:{e}")),
                    }
                }
            }
            "ads" => {
                for (slot, _, raw) in crate::site::AD_SLOTS {
                    if *raw {
                        continue;
                    }
                    let v = form.get(&format!("ad[{slot}]"));
                    let p = st.data().join("ads").join(format!("{slot}.html"));
                    let _ = std::fs::write(p, v);
                }
                sess.set_flash(&st, "広告枠を保存しました。");
            }
            "tags" => {
                for slot in ["head", "footer"] {
                    let v = form.get(&format!("ad[{slot}]"));
                    let p = st.data().join("ads").join(format!("{slot}.html"));
                    let _ = std::fs::write(p, v);
                }
                let ct = form.get("adstxt").replace("\r\n", "\n");
                crate::site::site_text_save(&st, "ads.txt", &ct);
                sess.set_flash(&st, "解析・共通タグを保存しました。");
            }
            "site" => {
                for (file, field) in [("notice.html", "notice"), ("links.html", "links"), ("rules.html", "rules")] {
                    crate::site::site_text_save(&st, file, &form.get(field));
                }
                sess.set_flash(&st, "サイト表示を保存しました。");
            }
            "s3test" => {
                match test_s3(&st) {
                    Ok(true) => sess.set_flash(&st, "S3接続テスト成功:書き込み・確認・削除ができました。"),
                    Ok(false) => sess.set_flash(&st, "S3接続テスト:書き込みはできましたが確認に失敗しました。"),
                    Err(e) => sess.set_flash(&st, &format!("S3接続テスト失敗:{e}")),
                }
            }
            _ => {}
        }
        return redirect_page(&st, &sess, &req, &back);
    }

    let is_admin = true;
    let _ = is_admin;

    // ---- 表示 ----
    let tab = crate::page_list::url_query_get(req.query_string(), "tab").unwrap_or_else(|| "files".into());
    let open_reports: i64 = st
        .db_call(|c| c.query_row("SELECT COUNT(*) FROM reports WHERE resolved = 0", [], |r| r.get(0)))
        .unwrap_or(0);
    let open_contacts: i64 = st
        .db_call(|c| c.query_row("SELECT COUNT(*) FROM contacts WHERE done = 0", [], |r| r.get(0)))
        .unwrap_or(0);
    let csrf = sess.csrf(&st);
    let csrf_i = format!("<input type=\"hidden\" name=\"csrf\" value=\"{}\">", h(&csrf));

    let tab_link = |k: &str, v: &str, on: bool| {
        format!(
            "<a href=\"/admin?tab={k}\" class=\"{}\">{v}</a>",
            if on { "on" } else { "" }
        )
    };
    let mut tabs = String::new();
    tabs += &tab_link("files", "ファイル", tab == "files");
    tabs += &tab_link("reports", &format!("通報{open_note}", open_note = if open_reports > 0 { format!(" ({open_reports})") } else { "".to_string() }), tab == "reports");
    tabs += &tab_link(
        "contacts",
        &format!("お問い合わせ{}", if open_contacts > 0 { format!(" ({open_contacts})") } else { "".to_string() }),
        tab == "contacts",
    );
    tabs += &tab_link("bans", "投稿禁止", tab == "bans");
    tabs += &tab_link("ads", "広告枠", tab == "ads");
    tabs += &tab_link("tags", "解析・共通タグ", tab == "tags");
    tabs += &tab_link("site", "お知らせ・リンク・規約", tab == "site");
    tabs += &tab_link("system", "設定状況", tab == "system");

    let mut body = format!(
        "<div class=\"admin-head\"><h1>管理</h1><form method=\"post\">{csrf_i}<input type=\"hidden\" name=\"act\" value=\"logout\"><button class=\"btn ghost\">ログアウト</button></form></div><nav class=\"tabs\">{tabs}</nav>"
    );
    body += &tab_body(&st, &req, &sess, &tab, &csrf_i);

    let opts = crate::html::PageOpts {
        title: "管理".into(),
        active: "admin",
        description: String::new(),
        url: None,
        image: None,
        typ: "website",
        noindex: true,
    };
    let html = crate::html::render(&st, &req, &sess, &opts, &body);
    let mut resp = HttpResponse::Ok().content_type("text/html; charset=utf-8").body(html);
    crate::session::finalize(&mut resp, &sess, &st, &req);
    resp
}

/// 小さな操作ボタン（inline form）
fn act_btn(csrf: &str, act: &str, id: i64, label: &str, cls: &str, confirm: &str, extra: &[(&str, &str)]) -> String {
    let on = if confirm.is_empty() {
        String::new()
    } else {
        format!(" onsubmit=\"return confirm('{}')\"", crate::util::h(confirm).replace('\'', "\\'"))
    };
    let mut fields = String::new();
    for (k, v) in extra {
        fields += &format!("<input type=\"hidden\" name=\"{k}\" value=\"{}\">", crate::util::h(v));
    }
    format!(
        "<form method=\"post\" class=\"inline\"{on}>{csrf}<input type=\"hidden\" name=\"act\" value=\"{act}\"><input type=\"hidden\" name=\"id\" value=\"{id}\">{fields}<button class=\"btn small {cls}\">{label}</button></form>",
        label = h(label)
    )
}

fn tab_body(st: &web::Data<WebData>, req: &HttpRequest, sess: &Sess, tab: &str, csrf: &str) -> String {
    match tab {
        "files" => tab_files(st, req, sess, csrf),
        "reports" => tab_reports(st, csrf),
        "contacts" => tab_contacts(st, csrf),
        "bans" => tab_bans(st, csrf),
        "ads" => tab_ads(st, csrf),
        "tags" => tab_tags(st, csrf),
        "site" => tab_site(st, csrf),
        "system" => tab_system(st, csrf),
        _ => tab_files(st, req, sess, csrf),
    }
}

fn tab_files(st: &web::Data<WebData>, req: &HttpRequest, _sess: &Sess, csrf: &str) -> String {
    let q = crate::page_list::url_query_get(req.query_string(), "q").unwrap_or_default();
    let page: i64 = crate::page_list::url_query_get(req.query_string(), "page")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
        .max(1);
    let mut rows: Vec<crate::db::FileRow> = vec![];
    let qlike = format!("%{q}%");
    let search = !q.is_empty();
    let idnum: i64 = q.parse().unwrap_or(-1);
    let _ = st.db_call(|c| {
        if search {
            let mut q = c.prepare("SELECT * FROM files WHERE name LIKE ? OR comment LIKE ? OR id = ? ORDER BY id DESC LIMIT 100 OFFSET ?")?;
            rows = q
                .query_map(
                    params![qlike, qlike, idnum, (page - 1) * 100],
                    crate::db::row_to_files,
                )?
                .filter_map(|r| r.ok())
                .collect();
        } else {
            let mut p = c.prepare("SELECT * FROM files ORDER BY id DESC LIMIT 100 OFFSET ?")?;
            rows = p
                .query_map(params![(page - 1) * 100], crate::db::row_to_files)?
                .filter_map(|r| r.ok())
                .collect();
        }
        Ok(())
    });
    let used = st.total_size();
    let quota = st.cfg.max_total_size;
    let storage_note = if crate::s3::enabled(st) {
        format!("S3 ({})", h(&st.cfg.s3.bucket))
    } else {
        "このサーバー".to_string()
    };
    let mut out = format!(
        "<p class=\"small\">使用量 {} / {} ・ 新規の保存先: {storage_note}</p>",
        crate::util::fmt_size(used),
        crate::util::fmt_size(quota)
    );
    out += &format!(
        "<form class=\"search\" method=\"get\"><input type=\"hidden\" name=\"tab\" value=\"files\"><input type=\"search\" name=\"q\" value=\"{}\" placeholder=\"検索\"><button class=\"btn\">検索</button></form>",
        h(&q)
    );
    out += "<div class=\"table-wrap\"><table class=\"list admin-list\"><thead><tr><th class=\"c-id\">番号</th><th>ファイル名・コメント</th><th class=\"c-size\">容量</th><th class=\"c-date\">日時</th><th class=\"c-dl\">DL</th><th>操作</th></tr></thead><tbody>";
    for r in &rows {
        let cls = if r.hidden != 0 { " class=\"is-hidden\"" } else { "" };
        let link = format!("/{}", crate::site::file_link(st, r.id));
        out += &format!(
            "<tr{cls}><td class=\"c-id\"><a href=\"{link}\">{}</a></td><td><form method=\"post\" class=\"edit\">{csrf}<input type=\"hidden\" name=\"act\" value=\"save\"><input type=\"hidden\" name=\"id\" value=\"{}\"><input name=\"name\" value=\"{}\"><textarea name=\"comment\" rows=\"1\">{}</textarea><button class=\"btn small\">保存</button></form></td><td class=\"c-size\">{}</td><td class=\"c-date\">{}</td><td class=\"c-dl\">{}</td><td class=\"ops\">{}{}{}</td></tr>",
            r.id,
            r.id,
            h(&r.name),
            h(&r.comment),
            crate::util::fmt_size(r.size),
            crate::util::fmt_date(r.created_at, false, st.cfg.utc_offset_hours),
            r.downloads,
            act_btn(csrf, "hide", r.id, if r.hidden != 0 { "公開" } else { "非公開" }, "", "", &[]),
            act_btn(csrf, "delete", r.id, "削除", "danger", &format!("No.{} を削除しますか？", r.id), &[]),
            if !r.ip.is_empty() {
                act_btn(csrf, "ban", r.id, "投稿者を禁止", "", "投稿者をアップロード禁止にしますか？", &[])
            } else {
                String::new()
            }
        );
    }
    out += "</tbody></table></div>";
    out += "<nav class=\"pager\">";
    if page > 1 {
        out += &format!(
            "<a href=\"/admin?tab=files&amp;q={}&amp;page={}\">‹ 前へ</a>",
            crate::util::raw_urlencode(&q),
            page - 1
        );
    }
    if rows.len() == 100 {
        out += &format!(
            "<a href=\"/admin?tab=files&amp;q={}&amp;page={}\">次へ ›</a>",
            crate::util::raw_urlencode(&q),
            page + 1
        );
    }
    out += "</nav>";
    out
}

fn tab_reports(st: &web::Data<WebData>, csrf: &str) -> String {
    struct Rep {
        id: i64,
        file_id: i64,
        reason: String,
        created_at: i64,
        resolved: i64,
        name: Option<String>,
    }
    let rows: Vec<Rep> = st
        .db_call(|c| {
            let mut q = c.prepare("SELECT r.id, r.file_id, r.reason, r.created_at, r.resolved, f.name FROM reports r LEFT JOIN files f ON f.id = r.file_id ORDER BY r.resolved, r.id DESC LIMIT 200")?;
            let v: Vec<Rep> = q
                .query_map([], |r| {
                    Ok(Rep {
                        id: r.get(0)?,
                        file_id: r.get(1)?,
                        reason: r.get(2)?,
                        created_at: r.get(3)?,
                        resolved: r.get(4)?,
                        name: r.get(5)?,
                    })
                })?
                .filter_map(|x| x.ok())
                .collect();
            Ok(v)
        })
        .unwrap_or_default();
    let mut out = String::from("<div class=\"table-wrap\"><table class=\"list\"><thead><tr><th class=\"c-date\">日時</th><th>ファイル</th><th>内容</th><th>操作</th></tr></thead><tbody>");
    for r in &rows {
        let cls = if r.resolved != 0 { " class=\"is-hidden\"" } else { "" };
        let flink = match &r.name {
            Some(name) => {
                let link = format!("/{}", crate::site::file_link(st, r.file_id));
                format!("<a href=\"{link}\">No.{} {}</a>", r.file_id, h(name))
            }
            None => format!("No.{}（削除済み）", r.file_id),
        };
        let act = if r.resolved == 0 {
            let mut a = act_btn(csrf, "resolve", r.id, "対応済み", "", "", &[]);
            if r.name.is_some() {
                a += &act_btn(csrf, "delete", r.file_id, "ファイル削除", "danger", "ファイルを削除しますか？", &[]);
            }
            a
        } else {
            "済".to_string()
        };
        out += &format!(
            "<tr{cls}><td class=\"c-date\">{}</td><td>{flink}</td><td class=\"pre\">{}</td><td class=\"ops\">{act}</td></tr>",
            crate::util::fmt_date(r.created_at, false, st.cfg.utc_offset_hours),
            h(&r.reason)
        );
    }
    if rows.is_empty() {
        out += "<tr class=\"empty\"><td colspan=\"4\">通報はありません。</td></tr>";
    }
    out += "</tbody></table></div>";
    out
}

fn tab_contacts(st: &web::Data<WebData>, csrf: &str) -> String {
    struct Con {
        id: i64,
        category: String,
        name: String,
        email: String,
        file_id: Option<i64>,
        message: String,
        created_at: i64,
        done: i64,
    }
    let rows: Vec<Con> = st
        .db_call(|c| {
            let mut q = c.prepare("SELECT id, category, name, email, file_id, message, created_at, done FROM contacts ORDER BY done, id DESC LIMIT 200")?;
            let v: Vec<Con> = q
                .query_map([], |r| {
                    Ok(Con {
                        id: r.get(0)?,
                        category: r.get(1)?,
                        name: r.get(2)?,
                        email: r.get(3)?,
                        file_id: r.get(4)?,
                        message: r.get(5)?,
                        created_at: r.get(6)?,
                        done: r.get(7)?,
                    })
                })?
                .filter_map(|x| x.ok())
                .collect();
            Ok(v)
        })
        .unwrap_or_default();
    let mail_note = if crate::mail::enabled(st, Some("contact")) {
        format!("新しいお問い合わせは {} にもメールで通知されます。メールに返信すれば相手に届きます。", h(&st.cfg.mail.to))
    } else {
        "メール通知は無効です（config.toml の mail で設定できます）。".to_string()
    };
    let mut out = format!("<p class=\"small\">{mail_note}</p>");
    for r in &rows {
        let done_cls = if r.done != 0 { " is-done" } else { "" };
        let tag = if r.done != 0 { "<span class=\"tag\">対応済み</span>" } else { "" };
        let person = format!(
            "{} ・ {}",
            if r.name.is_empty() { "名前なし".to_string() } else { h(&r.name) },
            if r.email.is_empty() {
                "メールなし（返信不可）".to_string()
            } else {
                format!("<a href=\"mailto:{}?subject={}\">{}</a>", h(&r.email), crate::util::raw_urlencode(&format!("Re: {}（{}）", r.category, st.cfg.title)), h(&r.email))
            }
        );
        let target = match r.file_id {
            Some(fid) if fid > 0 => {
                let link = format!("/{}", crate::site::file_link(st, fid));
                format!(" ・ 対象 <a href=\"{link}\">No.{fid}</a>")
            }
            _ => String::new(),
        };
        let ops = format!(
            "{}{}{}",
            act_btn(csrf, "contact_done", r.id, if r.done != 0 { "未対応に戻す" } else { "対応済みにする" }, "", "", &[]),
            match r.file_id {
                Some(fid) if fid > 0 && crate::db::file_row(st, fid).is_some() => {
                    act_btn(csrf, "delete", fid, &format!("No.{fid} を削除"), "danger", "ファイルを削除しますか？", &[])
                }
                _ => String::new(),
            },
            act_btn(csrf, "contact_delete", r.id, "この問い合わせを削除", "danger", "削除しますか？", &[])
        );
        out += &format!(
            "<article class=\"box contact{done_cls}\"><header><b>{}</b><span class=\"small\">{}</span>{tag}</header><p class=\"small\">{person}{target}</p><div class=\"pre msgbody\">{}</div><div class=\"ops\">{ops}</div></article>",
            h(&r.category),
            crate::util::fmt_date(r.created_at, true, st.cfg.utc_offset_hours),
            h(&r.message)
        );
    }
    if rows.is_empty() {
        out += "<p class=\"box small\">お問い合わせはありません。</p>";
    }
    out
}

fn tab_bans(st: &web::Data<WebData>, csrf: &str) -> String {
    struct Ban {
        ip: String,
        note: String,
        created_at: i64,
    }
    let rows: Vec<Ban> = st
        .db_call(|c| {
            let mut q = c.prepare("SELECT ip, note, created_at FROM bans ORDER BY created_at DESC")?;
            let v: Vec<Ban> = q
                .query_map([], |r| {
                    Ok(Ban { ip: r.get(0)?, note: r.get(1)?, created_at: r.get(2)? })
                })?
                .filter_map(|x| x.ok())
                .collect();
            Ok(v)
        })
        .unwrap_or_default();
    let mut out = String::from("<p class=\"small\">IPアドレスはそのまま保存せず、暗号化したハッシュで照合しています。</p><div class=\"table-wrap\"><table class=\"list\"><thead><tr><th class=\"c-date\">日時</th><th>メモ</th><th>識別子</th><th>操作</th></tr></thead><tbody>");
    for r in &rows {
        out += &format!(
            "<tr><td class=\"c-date\">{}</td><td>{}</td><td><code>{}</code></td><td><form method=\"post\" class=\"inline\">{csrf}<input type=\"hidden\" name=\"act\" value=\"unban\"><input type=\"hidden\" name=\"ip\" value=\"{}\"><button class=\"btn small\">解除</button></form></td></tr>",
            crate::util::fmt_date(r.created_at, false, st.cfg.utc_offset_hours),
            h(&r.note),
            h(&r.ip),
            h(&r.ip)
        );
    }
    if rows.is_empty() {
        out += "<tr class=\"empty\"><td colspan=\"4\">投稿禁止はありません。</td></tr>";
    }
    out += "</tbody></table></div>";
    out
}

fn tab_ads(st: &web::Data<WebData>, csrf: &str) -> String {
    let mut out = String::from("<p>バナー広告などのタグをそのまま貼り付けて保存してください。空欄の枠は表示されません。<br><small>AdSenseの自動広告・アナリティクスは「解析・共通タグ」タブへ。data/ads/枠名.html を直接編集しても同じです。</small></p><form method=\"post\" class=\"ads-form\">");
    out += csrf;
    out += "<input type=\"hidden\" name=\"act\" value=\"ads\">";
    for (slot, label, raw) in crate::site::AD_SLOTS {
        if *raw {
            continue;
        }
        let f = st.data().join("ads").join(format!("{slot}.html"));
        let cur = std::fs::read_to_string(&f).unwrap_or_default();
        out += &format!(
            "<label><b>{}</b> <code>{slot}</code><textarea name=\"ad[{slot}]\" rows=\"4\" spellcheck=\"false\">{}</textarea></label>",
            h(label),
            h(&cur)
        );
    }
    out += "<button class=\"btn btn-primary\">保存</button></form>";
    out
}

fn tab_tags(st: &web::Data<WebData>, csrf: &str) -> String {
    let rf = |slot: &str| -> String {
        std::fs::read_to_string(st.data().join("ads").join(format!("{slot}.html"))).unwrap_or_default()
    };
    format!(
        "<form method=\"post\" class=\"ads-form\">{csrf}<input type=\"hidden\" name=\"act\" value=\"tags\">\
<label><b>&lt;head&gt; 内に入れるタグ</b> <small>— Googleアナリティクス、AdSense（自動広告・審査用コード）、Search Console等のサイト認証 meta タグ。全ページに入ります。</small><textarea name=\"ad[head]\" rows=\"8\" spellcheck=\"false\">{}</textarea></label>\
<label><b>&lt;/body&gt; 直前に入れるタグ</b> <small>— ポップアンダー、末尾に置くよう指定された解析タグなど。</small><textarea name=\"ad[footer]\" rows=\"5\" spellcheck=\"false\">{}</textarea></label>\
<label><b>ads.txt</b> <small>— AdSense等に求められる /ads.txt の中身（例: google.com, pub-0000000000000000, DIRECT, f08c47fec0942fa0）。</small><textarea name=\"adstxt\" rows=\"4\" spellcheck=\"false\">{}</textarea></label>\
<button class=\"btn btn-primary\">保存</button></form>",
        h(&rf("head")),
        h(&rf("footer")),
        h(&crate::site::site_text(st, "ads.txt"))
    )
}

fn tab_site(st: &web::Data<WebData>, csrf: &str) -> String {
    format!(
        "<form method=\"post\" class=\"ads-form\">{csrf}<input type=\"hidden\" name=\"act\" value=\"site\">\
<label><b>お知らせ</b> <small>— 全ページ上部に表示（HTML可・空なら非表示）</small><textarea name=\"notice\" rows=\"3\" spellcheck=\"false\">{}</textarea></label>\
<label><b>リンク欄</b> <small>— サイドバー（スマホでは一覧の下）に表示。HTML可</small><textarea name=\"links\" rows=\"8\" spellcheck=\"false\">{}</textarea></label>\
<label><b>利用規約</b> <small>— 空にすると標準の文面。HTML可</small><textarea name=\"rules\" rows=\"14\" spellcheck=\"false\">{}</textarea></label>\
<button class=\"btn btn-primary\">保存</button></form>",
        h(&crate::site::site_text(st, "notice.html")),
        h(&crate::site::site_text(st, "links.html")),
        h(&crate::site::site_text(st, "rules.html"))
    )
}

fn onoff(b: bool) -> String {
    if b {
        "<b class=\"on\">有効</b>".to_string()
    } else {
        "<span class=\"off\">無効</span>".to_string()
    }
}

fn tab_system(st: &web::Data<WebData>, csrf: &str) -> String {
    let ts = &st.cfg.turnstile;
    let local: i64 = st
        .db_call(|c| c.query_row("SELECT COUNT(*) FROM files WHERE skey IS NULL", [], |r| r.get(0)))
        .unwrap_or(0);
    let remote: i64 = st
        .db_call(|c| c.query_row("SELECT COUNT(*) FROM files WHERE skey IS NOT NULL", [], |r| r.get(0)))
        .unwrap_or(0);
    let s3_note = if crate::s3::enabled(st) {
        let dl = if st.cfg.s3.public_url.is_empty() {
            format!("署名付きURL（{}秒有効）", st.cfg.s3.url_expires)
        } else {
            format!("公開URL経由:{}", h(&st.cfg.s3.public_url))
        };
        format!(
            "<tr><th>新規ファイルの保存先</th><td>S3:{} / {}{}</td></tr><tr><th>保存済みファイル</th><td>このサーバー {} 件 ・ S3 {} 件</td></tr><tr><th>S3からのダウンロード</th><td>{}{dl}</td></tr>",
            h(&st.cfg.s3.endpoint),
            h(&st.cfg.s3.bucket),
            if st.cfg.s3.prefix.is_empty() { String::new() } else { format!(" / {}", h(&st.cfg.s3.prefix)) },
            crate::util::number_format(local),
            crate::util::number_format(remote),
            ""
        )
    } else {
        format!(
            "<tr><th>新規ファイルの保存先</th><td>このサーバー（data/files）</td></tr><tr><th>保存済みファイル</th><td>このサーバー {} 件 ・ S3 {} 件</td></tr>",
            crate::util::number_format(local),
            crate::util::number_format(remote)
        )
    };
    let ts_key_missing = ts.site_key.is_empty() || ts.secret_key.is_empty();
    let mail_note = if crate::mail::enabled(st, None) {
        format!(
            "{}：{}<br><small>お問い合わせ {} ・ 通報 {} ・ 新規アップロード {}</small>",
            onoff(true),
            h(&st.cfg.mail.to),
            if crate::mail::enabled(st, Some("contact")) { "○" } else { "×" },
            if crate::mail::enabled(st, Some("report")) { "○" } else { "×" },
            if crate::mail::enabled(st, Some("upload")) { "○" } else { "×" }
        )
    } else {
        onoff(false)
    };
    let demo_note = if st.cfg.demo.enabled {
        format!(
            "{}（新規アップロードは {} 時間で自動削除・1ファイル {} まで）",
            onoff(true),
            st.cfg.demo.keep_hours,
            crate::util::fmt_size(crate::site::max_file_size(st))
        )
    } else {
        onoff(false)
    };
    let mut out = String::from("<p class=\"small\">これらは config.toml で変更します。</p><div class=\"table-wrap\"><table class=\"list kv\">");
    out += &s3_note;
    out += &format!(
        "<tr><th>Turnstile</th><td>アップロード {} ・ ダウンロード {} ・ お問い合わせ {} ・ 管理ログイン {}{}</td></tr>",
        onoff(ts.upload),
        onoff(ts.download),
        onoff(ts.contact),
        onoff(ts.admin),
        if ts_key_missing { "<br><small>サイトキー・シークレットキーが未設定のため全て無効です。</small>" } else { "" }
    );
    out += &format!("<tr><th>メール通知</th><td>{mail_note}</td></tr>");
    out += &format!("<tr><th>デモモード</th><td>{demo_note}</td></tr>");
    out += &format!("<tr><th>アップロードパスワード</th><td>{}</td></tr>", onoff(!st.cfg.upload_password.is_empty()));
    out += &format!("<tr><th>Cloudflare経由</th><td>{}</td></tr>", onoff(st.cfg.behind_cloudflare));
    out += "</table></div><div class=\"btnrow\">";
    if crate::s3::enabled(st) {
        out += &format!("<form method=\"post\">{csrf}<input type=\"hidden\" name=\"act\" value=\"s3test\"><button class=\"btn\">S3接続テスト</button></form>");
    }
    if crate::mail::enabled(st, None) {
        out += &format!("<form method=\"post\">{csrf}<input type=\"hidden\" name=\"act\" value=\"mailtest\"><button class=\"btn\">テストメール送信</button></form>");
    }
    out += "</div>";
    out
}



fn test_s3(st: &web::Data<WebData>) -> Result<bool, String> {
    let key = crate::s3::object_key(st, &format!("_lgu_test_{}", crate::util::rand_hex(8)));
    let tmp = st.data().join("tmp").join(format!("_test_{}.txt", crate::util::rand_hex(8)));
    std::fs::write(&tmp, "likeget-uploader connection test").map_err(|e| format!("{e}"))?;
    let r = crate::s3::put_file(st, &key, &tmp, "test.txt")
        .and_then(|_| crate::s3::exists(st, &key));
    let _ = std::fs::remove_file(&tmp);
    let _ = crate::s3::s3_delete(st, &key);
    r
}

