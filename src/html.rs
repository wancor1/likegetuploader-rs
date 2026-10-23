use actix_web::{HttpRequest, web};
use std::fmt::Write as _;

use crate::state::WebData;
use crate::util::{h, number_format};

pub struct PageOpts {
    pub title: String,
    pub active: &'static str,
    pub description: String,
    pub url: Option<String>,
    pub image: Option<String>,
    pub typ: &'static str,
    pub noindex: bool,
}

impl PageOpts {
    pub fn basic(title: &str, active: &'static str) -> PageOpts {
        PageOpts {
            title: title.to_string(),
            active,
            description: String::new(),
            url: None,
            image: None,
            typ: "website",
            noindex: false,
        }
    }
}

fn mb_strimwidth(s: &str, max_words: usize) -> String {
    // UTF-8の文字数で200程度に切り詰める（PHP mb_strimwidth 相当の近似）
    if s.chars().count() <= max_words {
        return s.to_string();
    }
    s.chars().take(max_words - 1).collect::<String>() + "…"
}

/// ページ全体のHTMLを組み立てる
pub fn render(
    st: &web::Data<WebData>,
    req: &HttpRequest,
    sess: &crate::session::Sess,
    opts: &PageOpts,
    body: &str,
) -> String {
    let site = &st.cfg.title;
    let full = if opts.title.is_empty() {
        h(site)
    } else {
        format!("{} - {}", h(&opts.title), h(site))
    };
    let desc_src = if opts.description.is_empty() {
        st.cfg.description.clone()
    } else {
        opts.description.clone()
    };
    let desc = mb_strimwidth(&desc_src.replace("\n", " ").replace("\r", " "), 200);
    let is_admin = render_check_admin(st, req);
    let b_url = crate::util::base_url(st, req);
    let u = opts
        .url
        .clone()
        .unwrap_or_else(|| format!("{b_url}/"));
    let image = match &opts.image {
        Some(i) => abs_url(&b_url, i),
        None => {
            if st.cfg.ogp_image.is_empty() {
                abs_url(&b_url, "assets/ogp.png")
            } else {
                abs_url(&b_url, &st.cfg.ogp_image)
            }
        }
    };
    let twitter = st.cfg.twitter_site.trim_start_matches('@').to_string();
    let links = crate::site::site_text(st, "links.html").trim().to_string();
    let side_html = if links.is_empty() {
        crate::site::ad(st, "side")
    } else {
        format!(
            "<section class=\"box links\"><h2>リンク</h2>{}</section>{}",
            links,
            crate::site::ad(st, "side")
        )
    };
    let note_cfg = st.cfg.notice.clone();
    let notice_raw = crate::site::site_text(st, "notice.html");
    let notice = if !notice_raw.trim().is_empty() {
        notice_raw
    } else {
        note_cfg
    };

    let mut out = String::with_capacity(body.len() + 2500);

    // ---- head ----
    let _ = write!(out, "<!doctype html>\n<html lang=\"ja\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<base href=\"/\"><title>{full}</title>\n<meta name=\"description\" content=\"{}\">\n", h(&desc));
    if opts.noindex {
        let _ = write!(out, "<meta name=\"robots\" content=\"noindex, nofollow\">\n");
    } else {
        let _ = write!(out, "<link rel=\"canonical\" href=\"{}\">\n", h(&u));
    }
    let _ = write!(out, "<meta name=\"theme-color\" content=\"{}\">\n", h(&st.cfg.theme_color));
    let _ = write!(out, "<link rel=\"icon\" href=\"favicon.ico\" sizes=\"any\">\n<link rel=\"icon\" href=\"{}\" type=\"image/svg+xml\">\n<link rel=\"apple-touch-icon\" href=\"{}\">\n", h(&crate::site::asset(st, "favicon.svg")), h(&crate::site::asset(st, "apple-touch-icon.png")));
    let _ = write!(out, "<meta property=\"og:site_name\" content=\"{}\">\n<meta property=\"og:title\" content=\"{}\">\n<meta property=\"og:description\" content=\"{}\">\n<meta property=\"og:type\" content=\"{}\">\n<meta property=\"og:url\" content=\"{}\">\n<meta property=\"og:image\" content=\"{}\">\n<meta property=\"og:locale\" content=\"ja_JP\">\n<meta name=\"twitter:card\" content=\"summary_large_image\">\n", h(site), h(if opts.title.is_empty() { site } else { &opts.title }), h(&desc), h(opts.typ), h(&u), h(&image));
    if !twitter.is_empty() {
        let _ = write!(out, "<meta name=\"twitter:site\" content=\"@{}\">\n", h(&twitter));
    }
    let _ = write!(out, "<link rel=\"stylesheet\" href=\"{}\">\n", h(&crate::site::asset(st, "style.css")));
    if crate::turnstile::any_enabled(st) {
        out.push_str("<script src=\"https://challenges.cloudflare.com/turnstile/v0/api.js\" async defer></script>");
    }
    out.push_str(&crate::site::ad(st, "head"));
    out.push_str("</head>\n<body>\n");

    // ---- header ----
    let _ = write!(
        out,
        "<header class=\"site-head\"><div class=\"wrap head-inner\"><a class=\"brand\" href=\"./\">{}</a><nav class=\"nav\">\
<a href=\"./#upload\" class=\"{}\">アップロード</a>\
<a href=\"./\" class=\"{}\">一覧</a>\
<a href=\"rules\" class=\"{}\">利用規約</a>\
<a href=\"contact\" class=\"{}\">お問い合わせ</a>\
",
        h(site),
        cls("upload", opts.active),
        cls("list", opts.active),
        cls("rules", opts.active),
        cls("contact", opts.active)
    );
    if is_admin {
        let _ = write!(out, "<a href=\"admin\" class=\"{}\">管理</a>", cls("admin", opts.active));
    }
    out.push_str("</nav></div></header>\n");
    if !side_html.is_empty() {
        out.push_str("<div class=\"wrap has-side\">");
    } else {
        out.push_str("<div class=\"wrap\">");
    }
    out.push_str("<main class=\"main\">");
    if !notice.trim().is_empty() {
        let _ = write!(out, "<div class=\"notice\">{}</div>", notice);
    }
    // flash（セッションから取り出して表示）
    if let Some(m) = sess.take_flash(st) {
        let _ = write!(out, "<div class=\"box msg ok\">{}</div>", h(&m));
    }
    out.push_str(&crate::site::ad(st, "top"));
    out.push_str(body);
    out.push_str("</main>");
    if !side_html.is_empty() {
        let _ = write!(out, "<aside class=\"side\">{side_html}</aside>");
    }
    out.push_str("</div>\n<footer class=\"site-foot\"><div class=\"wrap\"><a href=\"rules\">利用規約</a> ・ <a href=\"contact\">お問い合わせ</a> ・ <a href=\"admin\">管理</a>");
    if st.cfg.show_powered_by {
        let _ = write!(
            out,
            "<span class=\"ver\">Powered by <a href=\"{}\" target=\"_blank\" rel=\"noopener\">{}</a> {}</span>",
            h(crate::util::APP_REPO),
            h(crate::util::APP_NAME),
            h(crate::util::APP_VERSION)
        );
    }
    out.push_str("</div></footer>\n");
    let _ = write!(out, "<script src=\"{}\" defer></script>", h(&crate::site::asset(st, "app.js")));
    out.push_str(&crate::site::ad(st, "footer"));
    out.push_str("</body>\n</html>");
    out
}

fn cls(active: &str, own: &str) -> &'static str {
    if active == own {
        "on"
    } else {
        ""
    }
}

fn abs_url(base: &str, path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        path.to_string()
    } else {
        format!("{base}/{}", path.trim_start_matches('/'))
    }
}

/// 管理者かどうか（セッションクッキーから）。ページ描画のためだけの読み取り
fn render_check_admin(st: &web::Data<WebData>, req: &HttpRequest) -> bool {
    let sid = req
        .cookie(crate::session::COOKIE_NAME)
        .map(|c| c.value().to_string())
        .filter(|s| s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit()));
    match sid {
        None => false,
        Some(sid) => {
            let m = st.sessions.lock().unwrap();
            m.get(&sid).map(|s| s.admin).unwrap_or(false)
        }
    }
}

pub fn error_page(code: u16, msg: &str) -> String {
    format!(
        "<!doctype html><html lang=\"ja\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>エラー ({code})</title><link rel=\"stylesheet\" href=\"assets/style.css\"></head><body><div class=\"wrap\"><div class=\"box msg error\"><p>{}</p><p><a class=\"btn\" href=\"./\">一覧へ戻る</a></p></div></div></body></html>",
        h(msg)
    )
}

/// 統計ブロック（一覧ページ共通）
pub fn stats_box(st: &web::Data<WebData>) -> String {
    let used = st.total_size();
    let quota = st.cfg.max_total_size;
    let (n, d) = st.stats();
    let pct = if quota > 0 {
        (used as f64 / quota as f64 * 100.0) as i64
    } else {
        0
    };
    let meter = if quota > 0 {
        ((used as f64 / quota as f64 * 100.0).min(100.0) as i64).to_string()
    } else {
        "0".into()
    };
    format!(
        "<section class=\"box stats-box\"><h2>統計</h2><dl><dt>ディスク</dt><dd>{} / {}（{pct}%）<span class=\"meter\"><i style=\"width:{meter}%\"></i></span></dd><dt>ファイル</dt><dd>{} ファイル</dd><dt>ダウンロード</dt><dd>{} ダウンロード</dd></dl></section>",
        crate::util::fmt_size(used),
        crate::util::fmt_size(quota),
        number_format(n),
        number_format(d)
    )
}
