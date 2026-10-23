/// S3互換ストレージ（AWS S3 / Backblaze B2 / Cloudflare R2 / iDrive e2 / Wasabi / MinIO など）
/// 外部ライブラリなしの最小実装（AWS Signature Version 4）
use sha2::{Digest, Sha256};

use crate::state::WebData;
use crate::util::sha256_hex;

pub fn enabled(st: &WebData) -> bool {
    st.cfg.storage == "s3"
}

pub fn object_key(st: &WebData, name: &str) -> String {
    format!("{}{name}", st.cfg.s3.prefix)
}

fn host_of(st: &WebData) -> (String, String) {
    // (host, scheme)
    let ep = st.cfg.s3.endpoint.trim_end_matches('/');
    match ep.split_once("://") {
        Some((scheme, rest)) => (rest.to_string(), scheme.to_string()),
        None => (ep.to_string(), "https".to_string()),
    }
}

/// (url_base, canonical_uri)
fn url_of(st: &WebData, key: &str) -> (String, String) {
    let (mut host, scheme) = host_of(st);
    let enc_key = key.split('/').map(crate::util::raw_urlencode).collect::<Vec<_>>().join("/");
    if st.cfg.s3.path_style {
        let uri = format!("/{}/{}", crate::util::raw_urlencode(&st.cfg.s3.bucket), enc_key);
        (format!("{scheme}://{host}"), uri)
    } else {
        host = format!("{}.{}", st.cfg.s3.bucket, host);
        (format!("{scheme}://{host}"), format!("/{enc_key}"))
    }
}

fn query_encode(q: &[(&str, String)]) -> String {
    let mut sorted: Vec<&(&str, String)> = q.iter().collect();
    sorted.sort();
    sorted
        .into_iter()
        .map(|(k, v)| format!("{}={}", crate::util::raw_urlencode(k), crate::util::raw_urlencode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let kb = {
        let mut kb = [0u8; 64];
        if key.len() > 64 {
            let mut h = Sha256::new();
            h.update(key);
            let r = h.finalize();
            kb[..32].copy_from_slice(&r);
        } else {
            kb[..key.len()].copy_from_slice(key);
        }
        kb
    };
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= kb[i];
        opad[i] ^= kb[i];
    }
    let inner = {
        let mut h = Sha256::new();
        h.update(&ipad);
        h.update(data);
        h.finalize()
    };
    let mut h = Sha256::new();
    h.update(&opad);
    h.update(&inner);
    h.finalize().to_vec()
}

fn signing_key(st: &WebData, date: &str) -> Vec<u8> {
    let k1 = hmac_sha256(
        format!("AWS4{}", st.cfg.s3.secret).as_bytes(),
        date.as_bytes(),
    );
    let k2 = hmac_sha256(&k1, st.cfg.s3.region.as_bytes());
    let k3 = hmac_sha256(&k2, b"s3");
    hmac_sha256(&k3, b"aws4_request")
}

enum Body {
    None,
    Bytes(Vec<u8>),
    File(std::path::PathBuf),
}

pub enum S3Error {
    Http(u16, String),
    Net(String),
}

impl std::fmt::Display for S3Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            S3Error::Net(m) => write!(f, "S3通信エラー: {m}"),
            S3Error::Http(c, m) => write!(f, "S3失敗 (HTTP {c}): {m}"),
        }
    }
}

type R = Result<(u16, Vec<(String, String)>, String), S3Error>;

/// リクエスト実行
fn request(
    st: &WebData,
    method: &str,
    key: &str,
    query: &[(&str, String)],
    body: Body,
    extra_headers: Vec<(String, String)>,
) -> R {
    let (url_base, uri) = url_of(st, key);
    let (host0, _) = host_of(st);
    let host = if st.cfg.s3.path_style {
        host0.split('/').next().unwrap_or("").to_string()
    } else {
        format!("{}.{}", st.cfg.s3.bucket, host0.split('/').next().unwrap_or(""))
    };
    let payload_hash = match &body {
        Body::None => sha256_hex(b""),
        Body::Bytes(b) => sha256_hex(b),
        Body::File(p) => {
            let data = std::fs::read(p).map_err(|e| S3Error::Net(format!("{e}")))?;
            sha256_hex(&data)
        }
    };
    let amz_date = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let date = &amz_date[..8];
    let region = if st.cfg.s3.region.is_empty() {
        "us-east-1".to_string()
    } else {
        st.cfg.s3.region.clone()
    };
    let mut headers: Vec<(String, String)> = vec![
        ("host".into(), host.clone()),
        ("x-amz-date".into(), amz_date.clone()),
        ("x-amz-content-sha256".into(), payload_hash.clone()),
    ];
    for (k, v) in extra_headers {
        headers.push((k.to_lowercase(), v));
    }
    headers.sort();
    let canon_headers: String = headers
        .iter()
        .map(|(k, v)| format!("{k}:{}\n", v.trim()))
        .collect();
    let signed = headers.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>().join(";");
    let qs = query_encode(query);
    let canonical = format!("{method}\n{uri}\n{qs}\n{canon_headers}\n{signed}\n{payload_hash}");
    let scope = format!("{date}/{region}/s3/aws4_request");
    let sts = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        sha256_hex(canonical.as_bytes())
    );
    let sig = hex::encode(hmac_sha256(&signing_key(st, date), sts.as_bytes()));
    let auth = format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed}, Signature={sig}",
        st.cfg.s3.key
    );

    let url = format!(
        "{url_base}{uri}{}",
        if qs.is_empty() { String::new() } else { format!("?{qs}") }
    );
    let mut req = ureq::request(method, &url)
        .timeout(std::time::Duration::from_secs(600));
    for (k, v) in &headers {
        if k != "host" && k != "authorization" {
            req = req.set(k, v);
        }
    }
    req = req.set("authorization", &auth);
    let resp = match &body {
        Body::None => req.call(),
        Body::Bytes(b) => req.send_bytes(b),
        Body::File(p) => {
            let data = std::fs::read(p).map_err(|e| S3Error::Net(format!("{e}")))?;
            req.send_bytes(&data)
        }
    }
    .map_err(|e| match e {
        ureq::Error::Status(code, resp) => {
            let text = resp.into_string().unwrap_or_default();
            msg_from_xml(code as u16, &text)
        }
        e => S3Error::Net(format!("{e}")),
    })?;
    let status = resp.status() as u16;
    let resp_headers: Vec<(String, String)> = resp
        .headers_names()
        .iter()
        .map(|k| {
            (
                k.to_lowercase(),
                resp.header(k).unwrap_or("").to_string(),
            )
        })
        .collect();
    let body_text = resp.into_string().unwrap_or_default();
    if status < 200 || status >= 300 {
        return Err(msg_from_xml(status, &body_text));
    }
    Ok((status, resp_headers, body_text))
}

fn msg_from_xml(status: u16, text: &str) -> S3Error {
    let m = text
        .split_once("<Message>")
        .and_then(|(_, r)| r.split_once("</Message>"))
        .map(|(a, _)| a.to_string())
        .unwrap_or_else(|| text.chars().take(200).collect());
    S3Error::Http(status, m)
}

fn disposition(name: &str) -> String {
    let ascii: String = name
        .chars()
        .map(|c| if c.is_ascii() && c >= ' ' && c != '"' && c != '\\' { c } else { '_' })
        .collect();
    format!(
        "attachment; filename=\"{ascii}\"; filename*=UTF-8''{}",
        crate::util::raw_urlencode(name)
    )
}

fn check(r: R, what: &str) -> R {
    r.map_err(|e| match e {
        S3Error::Http(c, m) => S3Error::Http(c, format!("{what}: {m}")),
        e => e,
    })
}

// ---- マルチパート ----

pub fn mp_create(st: &WebData, key: &str, filename: &str) -> Result<String, String> {
    let (_, _, body) = check(
        request(
            st,
            "POST",
            key,
            &[("uploads", String::new())],
            Body::None,
            vec![
                ("content-type".into(), "application/octet-stream".into()),
                ("content-disposition".into(), disposition(filename)),
            ],
        ),
        "CreateMultipartUpload",
    )
    .map_err(|e| e.to_string())?;
    let uid = body
        .split_once("<UploadId>")
        .and_then(|(_, r)| r.split_once("</UploadId>"))
        .map(|(a, _)| a.to_string());
    uid.ok_or_else(|| "S3: UploadIdが取得できません".to_string())
}

pub fn mp_part(
    st: &WebData,
    key: &str,
    upload_id: &str,
    part_no: usize,
    file: &std::path::Path,
) -> Result<String, String> {
    let (status, headers, _) = request(
        st,
        "PUT",
        key,
        &[
            ("partNumber", part_no.to_string()),
            ("uploadId", upload_id.to_string()),
        ],
        Body::File(file.to_path_buf()),
        vec![],
    )
    .map_err(map_err)?;
    if status < 200 || status >= 300 {
        return Err("S3 UploadPart 失敗".to_string());
    }
    Ok(headers
        .iter()
        .find(|(k, _)| k == "etag")
        .map(|(_, v)| v.trim_matches('"').to_string())
        .unwrap_or_default())
}

fn map_err(e: S3Error) -> String {
    e.to_string()
}

pub fn mp_complete(
    st: &WebData,
    key: &str,
    upload_id: &str,
    etags: &[String],
) -> Result<(), String> {
    let mut xml = String::from("<CompleteMultipartUpload>");
    for (i, e) in etags.iter().enumerate() {
        xml += &format!("<Part><PartNumber>{}</PartNumber><ETag>\"{e}\"</ETag></Part>", i + 1);
    }
    xml += "</CompleteMultipartUpload>";
    let (_, _, body) = request(
        st,
        "POST",
        key,
        &[("uploadId", upload_id.to_string())],
        Body::Bytes(xml.into_bytes()),
        vec![("content-type".into(), "application/xml".into())],
    )
    .map_err(map_err)?;
    if body.contains("<Error>") {
        return Err("S3 CompleteMultipartUpload 失敗".into());
    }
    Ok(())
}

pub fn mp_abort(st: &WebData, key: &str, upload_id: &str) {
    let _ = request(
        st,
        "DELETE",
        key,
        &[("uploadId", upload_id.to_string())],
        Body::None,
        vec![],
    );
}

// ---- 単発操作 ----

pub fn put_file(st: &WebData, key: &str, path: &std::path::Path, filename: &str) -> Result<(), String> {
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) as i64;
    let part = 64 * 1024 * 1024i64;
    if size <= part {
        return check(
            request(
                st,
                "PUT",
                key,
                &[],
                Body::File(path.to_path_buf()),
                vec![
                    ("content-type".into(), "application/octet-stream".into()),
                    ("content-disposition".into(), disposition(filename)),
                ],
            ),
            "PutObject",
        )
        .map_err(map_err)
        .map(|_| ())
    }
    // 大きいファイルは分割して送る
    let uid = mp_create(st, key, filename)?;
    let etags = (|| -> Result<Vec<String>, String> {
        let mut out = vec![];
        let data = std::fs::read(path).map_err(|e| format!("{e}"))?;
        for (n, chunk) in data.chunks(part as usize).enumerate() {
            let tmp = st.data().join("tmp").join(format!("_up_{}.part", crate::util::rand_hex(16)));
            std::fs::write(&tmp, chunk).ok();
            let etag = mp_part(st, key, &uid, n + 1, &tmp)?;
            let _ = std::fs::remove_file(&tmp);
            out.push(etag);
        }
        Ok(out)
    })();
    match etags {
        Ok(etags) => mp_complete(st, key, &uid, &etags),
        Err(e) => {
            mp_abort(st, key, &uid);
            Err(e)
        }
    }
}

pub fn s3_delete(st: &WebData, key: &str) -> Result<(), String> {
    request(st, "DELETE", key, &[], Body::None, vec![])
        .map_err(map_err)
        .map(|_| ())
}

pub fn exists(st: &WebData, key: &str) -> Result<bool, String> {
    Ok(match request(st, "HEAD", key, &[], Body::None, vec![]) {
        Ok((s, _, _)) => s == 200,
        Err(S3Error::Http(404, _)) => false,
        Err(e) => return Err(e.to_string()),
    })
}

pub fn presign(st: &WebData, key: &str, filename: &str, expires: u64) -> String {
    let (url, uri) = url_of(st, key);
    let (host0, _) = host_of(st);
    let host = format!(
        "{}{}",
        if st.cfg.s3.path_style {
            host0.split('/').next().unwrap_or("").to_string()
        } else {
            st.cfg.s3.bucket.clone()
        },
        if st.cfg.s3.path_style {
            format!("/{}", st.cfg.s3.bucket)
        } else {
            String::new()
        }
    );
    let _ = host;
    let now = chrono::Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = &amz_date[..8];
    let region = if st.cfg.s3.region.is_empty() {
        "us-east-1".to_string()
    } else {
        st.cfg.s3.region.clone()
    };
    let scope = format!("{date}/{region}/s3/aws4_request");
    let hm: String = host_of_valid(st);
    let q = [
        ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_string()),
        (
            "X-Amz-Credential",
            format!("{}/{}", st.cfg.s3.key, scope),
        ),
        ("X-Amz-Date", amz_date.clone()),
        ("X-Amz-Expires", expires.to_string()),
        ("X-Amz-SignedHeaders", "host".to_string()),
        ("response-content-disposition", disposition(filename)),
        ("response-content-type", "application/octet-stream".to_string()),
    ];
    let qs = query_encode(&q);
    let canonical = format!("GET\n{uri}\n{qs}\nhost:{hm}\n\nhost\nUNSIGNED-PAYLOAD");
    let sts = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        sha256_hex(canonical.as_bytes())
    );
    let sig = hex::encode(hmac_sha256(&signing_key(st, date), sts.as_bytes()));
    format!("{url}?{qs}&X-Amz-Signature={sig}")
}

fn host_of_valid(st: &WebData) -> String {
    let (mut host, _) = host_of(st);
    let hp = host.split('/').next().unwrap_or("").to_string();
    host = hp;
    if st.cfg.s3.path_style {
        host
    } else {
        format!("{}.{}", st.cfg.s3.bucket, host)
    }
}
