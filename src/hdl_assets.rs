//! /ads.txt（旧 adstxt.php）と静的ファイル（assets / favicon.ico）
use actix_web::{HttpResponse, web};

use crate::state::WebData;

pub async fn adstxt(st: web::Data<WebData>) -> HttpResponse {
    let t = crate::site::site_text(&st, "ads.txt");
    if t.trim().is_empty() {
        return HttpResponse::NotFound().finish();
    }
    HttpResponse::Ok()
        .content_type("text/plain; charset=utf-8")
        .body(t)
}

pub async fn asset_file(st: web::Data<WebData>, path: web::Path<String>) -> HttpResponse {
    let rel = path.into_inner();
    if rel.contains("..") || rel.contains('\\') {
        return HttpResponse::BadRequest().finish();
    }
    serve_root(&st, format!("assets/{rel}"))
}

pub async fn favicon(st: web::Data<WebData>) -> HttpResponse {
    serve_root(&st, "favicon.ico".to_string())
}

fn serve_root(st: &web::Data<WebData>, rel: String) -> HttpResponse {
    let p = st.root.join(&rel);
    if !p.is_file() {
        return HttpResponse::NotFound().finish();
    }
    let mime = match p.extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase().as_str() {
        "css" => "text/css; charset=utf-8",
        "js" => "application/javascript; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "html" => "text/html; charset=utf-8",
        _ => "application/octet-stream",
    };
    match std::fs::read(&p) {
        Ok(bytes) => HttpResponse::Ok().content_type(mime).body(bytes),
        Err(_) => HttpResponse::NotFound().finish(),
    }
}

