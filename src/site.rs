use actix_web::web;

use crate::state::WebData;
use crate::util::h;
use std::path::PathBuf;

pub const AD_SLOTS: &[(&str, &str, bool)] = &[
    ("head", "<head>内（AdSense・Googleアナリティクス・サイト認証タグなど）", true),
    ("top", "全ページ上部（ヘッダー下）", false),
    ("list_top", "一覧の上", false),
    ("list_middle", "一覧の途中（10件目の後）", false),
    ("list_bottom", "一覧の下", false),
    ("detail_top", "ファイルページ上", false),
    ("detail_bottom", "ファイルページのダウンロードボタン下", false),
    ("side", "PC表示の右サイドバー", false),
    ("footer", "</body>直前（ポップアンダー・アクセス解析など）", true),
];

// ---- 管理画面で編集するテキスト（リンク欄・お知らせ・利用規約・ads.txt）----

pub fn site_text(st: &web::Data<WebData>, name: &str) -> String {
    let p = st.data().join("site").join(name);
    std::fs::read_to_string(p).unwrap_or_default()
}

pub fn site_text_save(st: &web::Data<WebData>, name: &str, text: &str) {
    if text.trim().is_empty() {
        let _ = std::fs::remove_file(st.data().join("site").join(name));
        return;
    }
    let p: PathBuf = st.data().join("site").join(name);
    let _ = std::fs::create_dir_all(p.parent().unwrap());
    let _ = std::fs::write(p, text);
}

// ---- 広告 ----

pub fn ad(st: &web::Data<WebData>, slot: &str) -> String {
    let f = st.data().join("ads").join(format!("{slot}.html"));
    if !AD_SLOTS.iter().any(|s| s.0 == slot) {
        return String::new();
    }
    let html = std::fs::read_to_string(&f).unwrap_or_default();
    let html = html.trim().to_string();
    if html.is_empty() {
        return String::new();
    }
    let raw = AD_SLOTS.iter().find(|s| s.0 == slot).unwrap().2;
    if raw {
        return format!("{html}\n");
    }
    format!("<div class=\"ad ad-{}\">{html}</div>", h(slot))
}

// ---- アップロード上限 ----

pub fn max_file_size(st: &web::Data<WebData>) -> i64 {
    let mut m = st.cfg.max_file_size;
    if st.cfg.demo.enabled {
        m = m.min(st.cfg.demo.max_file_size);
    }
    m
}

// ---- URL ----

pub fn file_link(st: &web::Data<WebData>, id: i64) -> String {
    if st.cfg.pretty_urls {
        format!("download/{id}")
    } else {
        format!("file?id={id}")
    }
}

// ---- アイコン ----

pub fn file_icon(name: &str) -> String {
    let ext = crate::util::ext_of(name);
    let kind = match ext.as_str() {
        "zip" | "7z" | "rar" | "lzh" | "gz" | "xz" | "tar" | "bz2" => "arc",
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" => "img",
        "mp3" | "ogg" | "wav" | "mid" | "midi" | "m4a" | "flac" => "snd",
        "xls" | "xlsx" | "xlsm" | "csv" | "txt" | "dat" | "doc" | "docx" | "pdf" | "md" => "doc",
        _ => "etc",
    };
    let shown: String = if ext.is_empty() {
        "-".to_string()
    } else {
        ext.chars().take(4).collect::<String>().to_uppercase()
    };
    format!(
        "<span class=\"ficon ficon-{kind}\" aria-hidden=\"true\">{}</span>",
        h(&shown)
    )
}

// ---- CSS/JSのURL（mtimeを版数にする）----

pub fn asset(st: &web::Data<WebData>, f: &str) -> String {
    let p = st.root.join("assets").join(f);
    let v = std::fs::metadata(&p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("assets/{f}?v={v}")
}

