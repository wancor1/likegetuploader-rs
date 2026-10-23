use actix_web::{HttpRequest, HttpResponse, web};

use crate::state::{Session, WebData};

pub const COOKIE_NAME: &str = "lgu";

/// リクエストにくっつくセッション状態
pub struct Sess {
    pub sid: String,
    /// 新しく発行した場合 true（レスポンスへ Set-Cookie する）
    pub created: bool,
    pub ip: String,
}

impl Sess {
    pub fn open(st: &web::Data<WebData>, req: &HttpRequest) -> Sess {
        let ip = crate::util::client_ip(st, req);
        let cookie_sid = req
            .cookie(COOKIE_NAME)
            .map(|c| c.value().to_string())
            .filter(|s| s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit()));
        let mut created = false;
        let sid;
        {
            let mut m = st.sessions.lock().unwrap();
            match cookie_sid {
                Some(s) if m.contains_key(&s) => sid = s,
                _ => {
                    let now = crate::util::now();
                    m.retain(|_, v| v.ts_until > now - 86400 * 14);
                    loop {
                        let s = crate::util::rand_hex(32);
                        if !m.contains_key(&s) {
                            m.insert(
                                s.clone(),
                                Session {
                                    csrf: crate::util::rand_hex(32),
                                    ..Default::default()
                                },
                            );
                            sid = s;
                            break;
                        }
                    }
                    created = true;
                }
            }
        }
        Sess { sid, created, ip }
    }

    pub fn map_mut<T>(&self, st: &web::Data<WebData>, f: impl FnOnce(&mut Session) -> T) -> T {
        let mut m = st.sessions.lock().unwrap();
        let s = m.get_mut(&self.sid).expect("session should exist");
        f(s)
    }

    pub fn map_ref<T>(&self, st: &web::Data<WebData>, f: impl FnOnce(&Session) -> T) -> T {
        let m = st.sessions.lock().unwrap();
        match m.get(&self.sid) {
            Some(s) => f(s),
            None => f(&Session::default()),
        }
    }

    pub fn csrf(&self, st: &web::Data<WebData>) -> String {
        self.map_ref(st, |s| s.csrf.clone())
    }

    /// CSRF検証。悪ければ Err(レスポンス)
    pub fn csrf_check(
        &self,
        st: &web::Data<WebData>,
        form_csrf: &str,
        header_csrf: &str,
    ) -> Result<(), HttpResponse> {
        let want = self.csrf(st);
        let got = if form_csrf.is_empty() { header_csrf } else { form_csrf };
        if want.is_empty() || got.is_empty() || want != got {
            return Err(
                crate::util::fail_html(400, "ページの有効期限が切れました。再読み込みしてやり直してください。")
            );
        }
        Ok(())
    }

    pub fn set_flash(&self, st: &web::Data<WebData>, msg: &str) {
        self.map_mut(st, |s| s.flash = Some(msg.to_string()));
    }

    pub fn take_flash(&self, st: &web::Data<WebData>) -> Option<String> {
        self.map_mut(st, |s| s.flash.take())
    }

    pub fn is_admin(&self, st: &web::Data<WebData>) -> bool {
        self.map_ref(st, |s| s.admin)
    }

    pub fn set_admin(&self, st: &web::Data<WebData>, on: bool) {
        self.map_mut(st, |s| s.admin = on);
    }

    pub fn ts_passed(&self, st: &web::Data<WebData>) -> bool {
        self.map_ref(st, |s| s.ts_until > crate::util::now())
    }

    pub fn ts_mark(&self, st: &web::Data<WebData>, always: bool) {
        if always {
            return;
        }
        let ttl = st.cfg.turnstile.ttl as i64;
        self.map_mut(st, |s| s.ts_until = crate::util::now() + ttl);
    }
}

/// 新しいsidをセッションストアに入れて返す
pub fn solidify(_st: &web::Data<WebData>, sess: &Sess) {
    let _ = sess; // open() 時点でストアに入れているため特別処理は不要
}

pub fn cookie_header(sid: &str, secure: bool) -> String {
    format!(
        "{COOKIE_NAME}={sid}; HttpOnly; SameSite=Lax; Path=/{}",
        if secure { "; Secure" } else { "" }
    )
}

/// レスポンスに Set-Cookie を付ける
pub fn finalize(resp: &mut HttpResponse, sess: &Sess, st: &web::Data<WebData>, req: &HttpRequest) {
    if sess.created {
        let secure = crate::util::is_https(st, req);
        if let (Ok(a), Ok(b), v) = (
            "Set-Cookie".parse(),
            cookie_header(&sess.sid, secure).parse(),
            cookie_header(&sess.sid, secure),
        ) {
            resp.headers_mut().insert(a, b);
            let _ = v;
        }
    }
}
