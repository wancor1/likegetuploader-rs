//! multipart / urlencoded のフォーム読み取り
use actix_web::{FromRequest, HttpRequest, web};
use actix_multipart::Multipart;
use futures_util::StreamExt;
use std::path::PathBuf;

pub struct Form {
    pub fields: Vec<(String, String)>,
    /// "blob" 部（分割アップロードAPIで使う）
    pub blob: Option<(PathBuf, u64)>,
}

impl Form {
    pub fn get(&self, key: &str) -> String {
        self.fields
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }
}

pub fn req_header(req: &HttpRequest, name: &str) -> String {
    req.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

#[allow(dead_code)]
pub fn wants_json(req: &HttpRequest) -> bool {
    req_header(req, "Accept").contains("application/json")
}

/// urlencoded あるいは multipart を統一して読む
pub async fn read_form(
    st: &web::Data<crate::state::WebData>,
    req: &HttpRequest,
    payload: web::Payload,
) -> Result<Form, String> {
    let ct = req_header(req, "Content-Type");
    if ct.starts_with("multipart/") {
        let mut mp = Multipart::new(req.headers(), payload);
        let mut fields = Vec::new();
        let mut blob: Option<(PathBuf, u64)> = None;
        while let Some(mut field) = mp
            .next()
            .await
            .transpose()
            .map_err(|e| {
                eprintln!("formio: field err: {e:?}");
                "フォームの読み取りに失敗しました".to_string()
            })?
        {
            let disp = field.content_disposition().clone();
            let name = disp.get_name().unwrap_or("").to_string();
            let filename = disp.get_filename().unwrap_or("").to_string();
            if !filename.is_empty() {
                let (path, size) = save_blob(st, &mut field).await?;
                blob = Some((path, size));
                continue;
            }
            let mut val = String::new();
            while let Some(chunk) = field.next().await {
                let b = chunk.map_err(|e| {
                    eprintln!("formio: chunk err: {e:?}");
                    "フォームの読み取りに失敗しました".to_string()
                })?;
                val += &String::from_utf8_lossy(&b);
                if val.len() > 65536 {
                    break;
                }
            }
            if !name.is_empty() {
                fields.push((name, val));
            }
        }
        return Ok(Form { fields, blob });
    }
    let mut dev_payload = payload.into_inner();
    let form: web::Form<std::collections::BTreeMap<String, String>> =
        web::Form::from_request(req, &mut dev_payload)
            .await
            .map_err(|e| format!("フォームの読み取りに失敗しました: {e}"))?;
    let mut fields = Vec::new();
    for (k, v) in form.into_inner() {
        fields.push((k, v));
    }
    Ok(Form { fields, blob: None })
}

async fn save_blob(
    st: &web::Data<crate::state::WebData>,
    field: &mut actix_multipart::Field,
) -> Result<(PathBuf, u64), String> {
    let limit = st.cfg.chunk_size.max(16 * 1024 * 1024) as u64;
    let path = st
        .data()
        .join("tmp")
        .join(format!("_blob_{}.part", crate::util::rand_hex(16)));
    let mut file = std::fs::File::create(&path).map_err(|e| format!("{e}"))?;
    use std::io::Write;
    let mut size: u64 = 0;
    while let Some(chunk) = field.next().await {
        let b = chunk.map_err(|e| {
            eprintln!("formio: blob err: {e:?}");
            "アップロードの読み取りに失敗しました".to_string()
        })?;
        size += b.len() as u64;
        if size > limit * 2 {
            return Err("送信データが大きすぎます".to_string());
        }
        file.write_all(&b).map_err(|e| format!("{e}"))?;
    }
    Ok((path, size))
}
