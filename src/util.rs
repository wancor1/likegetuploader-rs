use actix_web::{HttpRequest, HttpResponse, web};
use md5::Md5;
use rand::RngCore;
use sha2::{Digest, Sha256};

pub const APP_NAME: &str = "likeget-uploader";
pub const APP_VERSION: &str = "1.3.0";
pub const APP_REPO: &str = "https://github.com/PYU224/likegetuploader";

/// PHP htmlspecialchars 相当
pub fn h(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#039;"),
            _ => out.push(c),
        }
    }
    out
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn rand_hex(n: usize) -> String {
    let mut b = vec![0u8; n / 2 + n % 2];
    rand::thread_rng().fill_bytes(&mut b);
    hex::encode(&b)[..n].to_string()
}

pub fn fmt_size(b: i64) -> String {
    if b < 1024 {
        return format!("{b} B");
    }
    let units = ["KB", "MB", "GB", "TB"];
    let mut v = b as f64 / 1024.0;
    let mut i = 0;
    while v >= 1024.0 && i < 3 {
        v /= 1024.0;
        i += 1;
    }
    format!("{:.1} {}", v, units[i])
}

pub fn fmt_date(ts: i64, long: bool, utc_offset_hours: i64) -> String {
    use chrono::TimeZone;
    let off = chrono::FixedOffset::east_opt(utc_offset_hours as i32 * 3600).unwrap_or_else(|| chrono::FixedOffset::east_opt(0).unwrap());
    let t = chrono::Utc
        .timestamp_opt(ts, 0)
        .single()
        .unwrap_or_else(chrono::Utc::now);
    let t = t.with_timezone(&off);
    if long {
        t.format("%Y/%m/%d %H:%M:%S").to_string()
    } else {
        t.format("%y/%m/%d %H:%M").to_string()
    }
}

/// 危険文字を置換してファイル名をきれいにする
pub fn clean_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_control() || "\\/:*?\"<>|".contains(c) {
            out.push('_');
        } else {
            out.push(c);
        }
    }
    let mut out = out.trim_matches(|c| c == '.' || c == ' ').to_string();
    if out.is_empty() {
        out = "file".into();
    }
    out.chars().take(200).collect()
}

pub fn ext_of(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((_, e)) if !e.is_empty() => e.to_ascii_lowercase(),
        _ => String::new(),
    }
}

pub fn mime_guess(name: &str) -> &'static str {
    match ext_of(name).as_str() {
        "zip" => "application/zip",
        "7z" => "application/x-7z-compressed",
        "rar" => "application/vnd.rar",
        "lzh" => "application/x-lzh",
        "gz" => "application/gzip",
        "xz" => "application/x-xz",
        "tar" => "application/x-tar",
        "bz2" => "application/x-bzip2",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "webp" => "image/webp",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "mid" | "midi" => "audio/midi",
        "m4a" => "audio/mp4",
        "flac" => "audio/flac",
        "mp4" => "video/mp4",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "doc" => "application/msword",
        "xls" | "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xlsm" => "application/vnd.ms-excel.sheet.macroEnabled.12",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "iso" => "application/x-iso9660-image",
        "exe" => "application/vnd.microsoft.portable-executable",
        "dll" => "application/vnd.microsoft.portable-executable",
        _ => "application/octet-stream",
    }
}

/// 規約OKならtrue。allow があれば allow のみ許可、それ以外は deny で拒否
pub fn ext_allowed(name: &str, allow: &[String], deny: &[String]) -> bool {
    let ext = ext_of(name);
    if !allow.is_empty() {
        return allow.iter().any(|a| *a == ext);
    }
    for part in name.to_lowercase().split('.') {
        if part.is_empty() {
            continue;
        }
        if deny.iter().any(|d| d == part) {
            return false;
        }
    }
    true
}

/// IPはそのまま保存せず、秘密文字列付きハッシュで保存する（HMAC-SHA256 の先頭24文字）
pub fn ip_hash(secret: &str, ip: &str) -> String {
    let key = {
        let k = secret.as_bytes();
        let mut kb = [0u8; 64];
        if k.len() > 64 {
            let mut h = Sha256::new();
            h.update(k);
            let r = h.finalize();
            kb[..32].copy_from_slice(&r);
        } else {
            kb[..k.len()].copy_from_slice(k);
        }
        kb
    };
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= key[i];
        opad[i] ^= key[i];
    }
    let inner = {
        let mut h = Sha256::new();
        h.update(&ipad);
        h.update(ip.as_bytes());
        h.finalize()
    };
    let mut h = Sha256::new();
    h.update(&opad);
    h.update(&inner);
    hex::encode(h.finalize())[..24].to_string()
}

/// URLエンコード（rawurlencode 相当）
pub fn raw_urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        let c = *b;
        let ok = c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~');
        if ok {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{c:02X}"));
        }
    }
    out
}

/// PHP number_format 相当（3桁区切り）
pub fn number_format(n: i64) -> String {
    let neg = n < 0;
    let mut digits = n.abs().to_string();
    let mut out = String::new();
    while digits.len() > 3 {
        let (a, b) = digits.split_at(digits.len() - 3);
        if !out.is_empty() {
            out.insert(0, ',');
        }
        out.insert_str(0, b);
        digits = a.to_string();
    }
    if !out.is_empty() {
        out.insert(0, ',');
    }
    out.insert_str(0, &digits);
    if neg {
        format!("-{out}")
    } else {
        out
    }
}

pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex::encode(h.finalize())
}

pub fn md5_file(path: &std::path::Path) -> std::io::Result<String> {
    let data = std::fs::read(path)?;
    let mut h = Md5::new();
    h.update(&data);
    Ok(hex::encode(h.finalize()))
}

/// 定数時間比較（文字数が違う場合も時間差を減らす）
pub fn consttime_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut acc = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        acc |= x ^ y;
    }
    acc == 0
}

// ---- actix のレスポンスヘルパ ----

pub fn html_resp(code: u16, body: String) -> HttpResponse {
    HttpResponse::build(actix_web::http::StatusCode::from_u16(code).unwrap())
        .content_type("text/html; charset=utf-8")
        .body(body)
}

pub fn fail_html(code: u16, msg: &str) -> HttpResponse {
    html_resp(code, crate::html::error_page(code, msg))
}

pub fn fail_json(code: u16, msg: &str) -> HttpResponse {
    HttpResponse::build(actix_web::http::StatusCode::from_u16(code).unwrap())
        .content_type("application/json; charset=utf-8")
        .body(serde_json::json!({ "ok": false, "error": msg }).to_string())
}

pub fn client_ip(st: &web::Data<crate::state::WebData>, req: &HttpRequest) -> String {
    if st.cfg.behind_cloudflare {
        if let Some(v) = req.headers().get("CF-Connecting-IP") {
            if let Ok(s) = v.to_str() {
                return s.to_string();
            }
        }
    }
    req.peer_addr()
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|| "0.0.0.0".to_string())
}

pub fn is_https(st: &web::Data<crate::state::WebData>, req: &HttpRequest) -> bool {
    if let Some(v) = req.headers().get("X-Forwarded-Proto") {
        if let Ok(s) = v.to_str() {
            if s == "https" || s == "https,http" {
                return true;
            }
        }
    }
    if let Some(u) = &st.cfg.base_url {
        if u.starts_with("https://") {
            return true;
        }
    }
    req.connection_info().scheme() == "https"
}

pub fn base_url(st: &web::Data<crate::state::WebData>, req: &HttpRequest) -> String {
    if let Some(u) = &st.cfg.base_url {
        return u.trim_end_matches('/').to_string();
    }
    let host = req
        .headers()
        .get("Host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    format!(
        "{}://{host}",
        if is_https(st, req) { "https" } else { "http" }
    )
}

pub fn file_url(st: &web::Data<crate::state::WebData>, req: &HttpRequest, id: i64) -> String {
    format!("{}/{}", base_url(st, req), crate::site::file_link(st, id))
}
