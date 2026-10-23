//! 一覧ページ（旧 index.php）
use actix_web::{HttpRequest, web};

use crate::state::WebData;
use crate::util::{h, number_format};

#[derive(Clone)]
pub struct ListQuery {
    pub q: String,
    pub sort: String,
    pub order: String,
    pub page: i64,
}

impl ListQuery {
    pub fn from_req(st: &web::Data<WebData>, req: &HttpRequest) -> ListQuery {
        let qs = req.query_string();
        let val = |k: &str| -> String {
            url_query_get(qs, k).unwrap_or_default()
        };
        let q = val("q").trim().to_string();
        let sorts = ["date", "id", "name", "size", "dl"];
        let sort = val("sort");
        let sort = if sorts.contains(&sort.as_str()) && !sort.is_empty() {
            sort
        } else {
            "date".into()
        };
        let order: String = if val("order") == "asc" { "asc".into() } else { "desc".into() };
        let page = val("page").parse::<i64>().unwrap_or(1).max(1);
        let _ = st;
        ListQuery { q, sort, order, page }
    }
}

pub fn url_query_get(qs: &str, key: &str) -> Option<String> {
    for pair in qs.split('&') {
        let (k, v) = pair.split_once('=')?;
        if k == key {
            return Some(percent_decode(v));
        }
    }
    None
}

pub fn percent_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(&s.replace('+', " "))
        .decode_utf8_lossy()
        .to_string()
}

fn sort_expr(sort: &str) -> &'static str {
    match sort {
        "id" => "id",
        "name" => "name COLLATE NOCASE",
        "size" => "size",
        "dl" => "downloads",
        _ => "created_at",
    }
}

/// 並べ替え・検索のためのクエリ文字列を作る
fn qs(base: &ListQuery, over: &[(&str, &str)]) -> String {
    let mut kv: Vec<(String, String)> = vec![("q".into(), base.q.clone())];
    kv.push(("sort".into(), base.sort.clone()));
    kv.push(("order".into(), base.order.clone()));
    kv.push(("page".into(), base.page.to_string()));
    for (k, v) in over {
        kv.retain(|(kk, _)| kk != k);
        kv.push((k.to_string(), v.to_string()));
    }
    kv.retain(|(k, v)| match k.as_str() {
        "q" => !v.is_empty(),
        "sort" => v != "date",
        "order" => v != "desc",
        "page" => v != "1",
        _ => true,
    });
    if kv.is_empty() {
        "./".to_string()
    } else {
        let p: Vec<String> = kv
            .into_iter()
            .map(|(k, v)| format!("{}={}", crate::util::raw_urlencode(&k), crate::util::raw_urlencode(&v)))
            .collect();
        format!("./?{}", p.join("&"))
    }
}

fn th_sort(base: &ListQuery, key: &str, label: &str, cls: &str) -> String {
    let next = if base.sort == key && base.order == "desc" { "asc" } else { "desc" };
    let mark = if base.sort == key {
        if base.order == "desc" {
            " ▼"
        } else {
            " ▲"
        }
    } else {
        ""
    };
    format!(
        "<th class=\"{cls}\"><a href=\"{}\">{}{mark}</a></th>",
        h(&qs(base, &[("sort", key), ("order", next), ("page", "1")])),
        h(label)
    )
}

pub async fn index(
    st: web::Data<WebData>,
    req: actix_web::HttpRequest,
) -> actix_web::HttpResponse {
    let sess = crate::session::Sess::open(&st, &req);
    index_impl(&st, &req, &sess)
}

fn index_impl(st: &web::Data<WebData>, req: &HttpRequest, sess: &crate::session::Sess) -> actix_web::HttpResponse {
    let lq = ListQuery::from_req(st, req);
    let per = st.cfg.per_page.max(1);

    // 検索条件
    let words: Vec<String> = lq
        .q
        .split(|c: char| c == ' ' || c == '　')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let sql = "SELECT COUNT(*) FROM files WHERE hidden = 0".to_string();
    let mut sql_body = String::new();
    let mut args: Vec<Box<dyn rusqlite::types::ToSql>> = vec![];
    for w in &words {
        let like = format!("%{}%", w.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        sql_body += " AND (name LIKE ? ESCAPE '\\' OR comment LIKE ? ESCAPE '\\' OR id = ?)";
        args.push(Box::new(like.clone()));
        args.push(Box::new(like));
        args.push(Box::new(
            w.parse::<i64>().unwrap_or(-1),
        ));
    }
    let _ = sql;
    let total: i64 = st.db_call(|c| {
            let mut q = c.prepare(&format!("SELECT COUNT(*) FROM files WHERE hidden = 0{sql_body}"))?;
            let refs: Vec<&dyn rusqlite::types::ToSql> = args.iter().map(|b| &**b).collect();
            q.query_row(refs.as_slice(), |r| r.get(0))
        })
        .unwrap_or(0);
    let pages: i64 = ((total as f64) / (per as f64)).ceil() as i64;
    let pages = pages.max(1);
    let page = lq.page.min(pages);
    let offset = (page - 1) * per;

    let mut sel = format!(
        "SELECT id, name, comment, size, created_at, downloads, dlpass IS NOT NULL AS locked FROM files WHERE hidden = 0{sql_body} ORDER BY {} {}, id DESC LIMIT {per} OFFSET {offset}",
        sort_expr(&lq.sort),
        lq.order
    );
    sel = sel.replace("ORDER BY  ,", "ORDER BY ,");
    let rows: Vec<(i64, String, String, i64, i64, i64, bool)> = st.db_call(|c| {
            let mut q = c.prepare(&sel)?;
            let refs: Vec<&dyn rusqlite::types::ToSql> = args.iter().map(|b| &**b).collect();
            let it = q.query_map(refs.as_slice(), |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get::<_, i64>(6)? != 0))
            })?;
            Ok(it.filter_map(|x| x.ok()).collect())
        })
        .unwrap_or_default();

    // ---- 本文組み立て ----
    let max_size = crate::site::max_file_size(st);
    let max_mb = format!("{:.1}", max_size as f64 / 1048576.0);
    let mut body = String::new();

    body += &format!(
        "<section class=\"intro\"><h1>{}</h1><p>{}</p></section>",
        h(&st.cfg.title),
        h(&st.cfg.description)
    );
    if st.cfg.demo.enabled {
        let keep = st.cfg.demo.keep_hours;
        body += &format!(
            "<div class=\"notice demo\">これはデモ用のアップローダーです。アップロードされたファイルは {keep} 時間で自動的に削除されます（1ファイル {max_mb}MB まで）。</div>"
        );
    }

    let csrf = sess.csrf(st);
    let up_pass_field = if !st.cfg.upload_password.is_empty() {
        "<label>アップロードパスワード<input type=\"password\" name=\"uppass\" maxlength=\"64\" required></label>".to_string()
    } else {
        String::new()
    };
    let ts = crate::turnstile::widget(st, sess, "upload", false);
    body += &format!(
        "<details class=\"box upload\" id=\"upload\"><summary class=\"btn btn-primary\">⇧ アップロード</summary>\
<form id=\"upform\" class=\"upform\" method=\"post\" action=\"upload\" data-chunk=\"{}\" data-max=\"{max_size}\">\
<input type=\"hidden\" name=\"csrf\" value=\"{}\">\
<label class=\"drop\" id=\"drop\"><input type=\"file\" name=\"file\" id=\"file\" multiple required><span class=\"drop-text\"><b>ファイルを選択</b> またはここへドラッグ（1ファイル {max_mb}MB まで・複数可）</span></label>\
<div class=\"grid2\">\
<label class=\"full\">コメント <small>（{}字以内）</small><textarea name=\"comment\" rows=\"2\" maxlength=\"{}\" placeholder=\"シナリオ名・バージョン・内容など\"></textarea></label>\
<label>削除キー <small>（後で自分で消すため）</small><input type=\"password\" name=\"delkey\" maxlength=\"64\" autocomplete=\"new-password\"></label>\
<label>DLパスワード <small>（任意・かけたい時だけ）</small><input type=\"password\" name=\"dlpass\" maxlength=\"64\" autocomplete=\"new-password\"></label>\
{up_pass_field}\
</div>{ts}<p class=\"agree\"><label><input type=\"checkbox\" name=\"agree\" required> <a href=\"rules\" target=\"_blank\">利用規約</a>に同意します</label></p>\
<button type=\"submit\" class=\"btn btn-primary\">アップロード開始</button><ul class=\"queue\" id=\"queue\"></ul></form>\
<noscript><p class=\"msg error\">アップロードにはJavaScriptが必要です。</p></noscript></details>",
        st.cfg.chunk_size,
        h(&csrf),
        st.cfg.max_comment,
        st.cfg.max_comment,
    );

    body += &format!(
        "<form class=\"search\" method=\"get\" action=\"./\"><input type=\"search\" name=\"q\" value=\"{}\" placeholder=\"ファイル名・コメント・番号で検索\"><button class=\"btn\">検索</button>{}</form>",
        h(&lq.q),
        if lq.q.is_empty() { String::new() } else { "<a class=\"btn ghost\" href=\"./\">解除</a>".to_string() }
    );

    body += &crate::site::ad(st, "list_top");
    body += &format!(
        "<p class=\"count\">{}{} 件{}</p>",
        if lq.q.is_empty() { "" } else { "検索結果 " },
        number_format(total),
        if pages > 1 { format!("（{page} / {pages} ページ）") } else { String::new() }
    );

    let mut table = String::from("<div class=\"table-wrap\"><table class=\"list\"><thead><tr>");
    table += &th_sort(&lq, "id", "番号", "c-id");
    table += &th_sort(&lq, "name", "ファイル", "c-name");
    table += "<th class=\"c-com\">コメント</th>";
    table += &th_sort(&lq, "size", "容量", "c-size");
    table += &th_sort(&lq, "date", "日時", "c-date");
    table += &th_sort(&lq, "dl", "DL", "c-dl");
    table += "</tr></thead><tbody>";

    let mut trs = String::new();
    for (i, (id, name, comment, size, created_at, downloads, locked)) in rows.iter().enumerate() {
        trs += &format!(
            "<tr><td class=\"c-id\">{id}</td><td class=\"c-name\">{}<a href=\"{}\">{}</a>{}</td><td class=\"c-com\">{}</td><td class=\"c-size\">{}</td><td class=\"c-date\">{}</td><td class=\"c-dl\">{}</td></tr>",
            crate::site::file_icon(name),
            h(&crate::site::file_link(st, *id)),
            h(name),
            if *locked { " <span class=\"lock\" title=\"DLパスワードあり\">🔒</span>" } else { "" },
            h(comment),
            crate::util::fmt_size(*size),
            crate::util::fmt_date(*created_at, false, st.cfg.utc_offset_hours),
            number_format(*downloads),
        );
        if i == 9 {
            let mid = crate::site::ad(st, "list_middle");
            if !mid.is_empty() {
                trs += &format!("<tr class=\"ad-row\"><td colspan=\"6\">{mid}</td></tr>");
            }
        }
    }
    if rows.is_empty() {
        trs += &format!(
            "<tr class=\"empty\"><td colspan=\"6\">{}</td></tr>",
            if lq.q.is_empty() { "まだファイルがありません。" } else { "見つかりませんでした。" }
        );
    }
    table += &trs;
    table += "</tbody></table></div>";
    body += &table;

    if pages > 1 {
        let mut pager = String::from("<nav class=\"pager\">");
        if page > 1 {
            pager += &format!("<a href=\"{}\">‹ 前へ</a>", h(&qs(&lq, &[("page", &(page - 1).to_string())])));
        }
        let mut nums: Vec<i64> = vec![1];
        for n in (page - 3).max(1)..=(pages.min(page + 3)) {
            nums.push(n);
        }
        nums.push(pages);
        nums.sort();
        nums.dedup();
        let mut prev = 0;
        for n in &nums {
            if *n - prev > 1 {
                pager += "<span class=\"gap\">…</span>";
            }
            if *n == page {
                pager += &format!("<b>{n}</b>");
            } else {
                pager += &format!("<a href=\"{}\">{n}</a>", h(&qs(&lq, &[("page", &n.to_string())])));
            }
            prev = *n;
        }
        if page < pages {
            pager += &format!("<a href=\"{}\">次へ ›</a>", h(&qs(&lq, &[("page", &(page + 1).to_string())])));
        }
        pager += "</nav>";
        body += &pager;
    }

    body += &crate::site::ad(st, "list_bottom");
    body += &crate::html::stats_box(st);
    body += &format!(
        "<div class=\"grid2 info\"><section class=\"box\"><h2>アップローダーをご利用の前に</h2><p>必ず<a href=\"rules\">利用規約</a>をご確認のうえ、同意してご利用ください。このアップローダーについてのご質問や削除依頼は、<a href=\"contact\">お問い合わせフォーム</a>から管理人へご連絡ください。</p></section><section class=\"box\"><h2>このアップローダーについて</h2><p>uploader.jp のような一覧型アップローダーを自分のサーバーに設置できるオープンソースソフト <a href=\"https://github.com/PYU224/likegetuploader\" target=\"_blank\" rel=\"noopener\">likegetuploader</a>（本版はRustで動作）で動いています。</p></section></div>"
    );

    let opts = crate::html::PageOpts {
        title: if lq.q.is_empty() {
            String::new()
        } else {
            format!("「{}」の検索結果", lq.q)
        },
        active: "list",
        description: String::new(),
        url: None,
        image: None,
        typ: "website",
        noindex: !lq.q.is_empty(),
    };
    let html = crate::html::render(st, req, sess, &opts, &body);
    let mut resp = actix_web::HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .insert_header(("X-Content-Type-Options", "nosniff"))
        .insert_header(("Referrer-Policy", "same-origin"))
        .body(html);
    crate::session::finalize(&mut resp, sess, st, req);
    resp
}




