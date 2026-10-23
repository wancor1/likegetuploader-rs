/// Cloudflare Turnstile（人間確認）
use actix_web::web;

fn tcfg(st: &crate::state::WebData) -> &crate::config::TurnstileCfg {
    &st.cfg.turnstile
}

pub fn enabled(st: &web::Data<crate::state::WebData>, r#where: &str) -> bool {
    let t = tcfg(st);
    !t.site_key.is_empty() && !t.secret_key.is_empty() && match r#where {
        "upload" => t.upload,
        "download" => t.download,
        "contact" => t.contact,
        "admin" => t.admin,
        _ => false,
    }
}

pub fn any_enabled(st: &web::Data<crate::state::WebData>) -> bool {
    ["upload", "download", "admin", "contact"].iter().any(|w| enabled(st, w))
}

/// 一度確認が通れば、ttl秒の間は同じ人に再確認しない（管理ログインは毎回確認）
pub fn widget(
    st: &web::Data<crate::state::WebData>,
    sess: &crate::session::Sess,
    r#where: &str,
    always: bool,
) -> String {
    if !enabled(st, r#where) || (!always && sess.ts_passed(st)) {
        return String::new();
    }
    format!(
        "<div class=\"cf-turnstile ts-box\" data-sitekey=\"{}\" data-theme=\"auto\" data-language=\"ja\"></div>",
        crate::util::h(&tcfg(st).site_key)
    )
}

/// 検証。失敗時は Err((ステータス, メッセージ))
pub fn check(
    st: &web::Data<crate::state::WebData>,
    sess: &crate::session::Sess,
    r#where: &str,
    always: bool,
    token: &str,
) -> Result<(), (u16, &'static str)> {
    if !enabled(st, r#where) || (!always && sess.ts_passed(st)) {
        return Ok(());
    }
    if token.is_empty() {
        return Err((403, "「人間であることを確認」のチェックを完了してください。"));
    }
    let form = format!(
        "secret={}&response={}&remoteip={}",
        crate::util::raw_urlencode(&tcfg(st).secret_key),
        crate::util::raw_urlencode(token),
        crate::util::raw_urlencode(&sess.ip)
    );
    let resp = match ureq::post("https://challenges.cloudflare.com/turnstile/v0/siteverify")
        .timeout(std::time::Duration::from_secs(10))
        .send_string(&form)
    {
        Ok(r) => r,
        Err(_) => return Err((502, "人間確認の検証に失敗しました。時間をおいてお試しください。")),
    };
    let text = resp.into_string().unwrap_or_default();
    let ok: bool = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("success").and_then(|s| s.as_bool()))
        .unwrap_or(false);
    if !ok {
        return Err((403, "人間確認に失敗しました。ページを再読み込みしてもう一度お試しください。"));
    }
    if !always {
        sess.ts_mark(st, false);
    }
    Ok(())
}

