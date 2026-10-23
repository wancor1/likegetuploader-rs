/// 管理者への通知メール（SMTP・lettre使用）
/// 'tls' = STARTTLS(587) / 'ssl' = SSL(465) / 'none' = 暗号化なし、AUTH LOGIN
pub fn enabled(st: &crate::state::WebData, kind: Option<&str>) -> bool {
    let m = &st.cfg.mail;
    if !m.enabled || m.host.is_empty() || m.to.is_empty() || m.from.is_empty() {
        return false;
    }
    match kind {
        None => true,
        Some(k) => match k {
            "contact" => m.notify_contact,
            "report" => m.notify_report,
            "upload" => m.notify_upload,
            _ => false,
        },
    }
}

pub fn header_clean(s: &str) -> String {
    s.chars()
        .map(|c| if "\r\n\u{0}".contains(c) { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

fn valid_email(s: &str) -> bool {
    s.contains('@') && !s.starts_with('@') && !s.ends_with('@')
}

/// 管理者へ通知（失敗してもサイトの動作は止めない）
pub fn notify_admin(
    st: &crate::state::WebData,
    kind: &str,
    subject: &str,
    body: &str,
    reply_to: Option<&str>,
) {
    if !enabled(st, Some(kind)) {
        return;
    }
    if let Err(e) = smtp_send(
        st,
        &st.cfg.mail.to,
        &format!("[{}] {}", st.cfg.title, subject),
        body,
        reply_to,
    ) {
        eprintln!("likeget-uploader mail: {e}");
    }
}

/// SMTP送信。エラーは呼び出し側に返す
pub fn smtp_send(
    st: &crate::state::WebData,
    to: &str,
    subject: &str,
    body: &str,
    reply_to: Option<&str>,
) -> Result<(), String> {
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::transport::smtp::client::{Tls, TlsParameters};
    use lettre::{Message, SmtpTransport, Transport};

    let cfg = &st.cfg.mail;
    if !valid_email(to) || !valid_email(&cfg.from) {
        return Err(format!("メールアドレスが不正です: to={to} from={}", cfg.from));
    }
    let from_name = if cfg.from_name.is_empty() {
        st.cfg.title.clone()
    } else {
        header_clean(&cfg.from_name)
    };
    let mb = |addr: &str| -> Result<lettre::message::Mailbox, String> {
        use lettre::address::Address;
        let a = addr.trim();
        let (u, d) = a
            .rsplit_once('@')
            .ok_or_else(|| format!("メールアドレスが不正です: {addr}"))?;
        Address::new(u, d)
            .map_err(|_| format!("メールアドレスが不正です: {addr}"))
            .map(|x| lettre::message::Mailbox::new(None, x))
    };
    let mut b = Message::builder()
        .from(lettre::message::Mailbox {
            name: Some(from_name),
            email: mb(&cfg.from)?.email,
        })
        .to(mb(to)?)
        .subject(header_clean(subject));
    if let Some(rt) = reply_to.filter(|r| valid_email(r)) {
        b = b.reply_to(mb(rt)?);
    }
    let msg = b.body(body.to_string()).map_err(|e| format!("メール組み立て失敗: {e}"))?;

    let host = cfg.host.clone();
    let port = if cfg.port == 0 { 587 } else { cfg.port };
    let mut transport = SmtpTransport::builder_dangerous(&host).port(port);
    if cfg.secure != "none" {
        let tls = TlsParameters::new(host.clone()).map_err(|e| format!("TLS設定失敗: {e}"))?;
        transport = transport.tls(match cfg.secure.as_str() {
            "ssl" => Tls::Wrapper(tls),
            _ => Tls::Required(tls),
        });
    }
    if !cfg.user.is_empty() {
        transport = transport.credentials(Credentials::new(cfg.user.clone(), cfg.pass.clone()));
    }
    transport
        .build()
        .send(&msg)
        .map_err(|e| format!("SMTPエラー: {e}"))?;
    Ok(())
}
