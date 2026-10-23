use serde::Deserialize;

#[derive(Clone, Deserialize, Default)]
#[serde(default)]
pub struct S3Cfg {
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub key: String,
    pub secret: String,
    pub prefix: String,
    pub path_style: bool,
    pub public_url: String,
    pub url_expires: u64,
}

impl S3Cfg {
    fn default_s3() -> Self {
        Self {
            endpoint: String::new(),
            region: String::new(),
            bucket: String::new(),
            key: String::new(),
            secret: String::new(),
            prefix: "uploads/".into(),
            path_style: true,
            public_url: String::new(),
            url_expires: 300,
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct TurnstileCfg {
    pub site_key: String,
    pub secret_key: String,
    pub upload: bool,
    pub download: bool,
    pub contact: bool,
    pub admin: bool,
    pub ttl: u64,
}

impl Default for TurnstileCfg {
    fn default() -> Self {
        Self {
            site_key: String::new(),
            secret_key: String::new(),
            upload: true,
            download: false,
            contact: true,
            admin: true,
            ttl: 600,
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct MailCfg {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub secure: String,
    pub user: String,
    pub pass: String,
    pub from: String,
    pub from_name: String,
    pub to: String,
    pub verify_peer: bool,
    pub notify_contact: bool,
    pub notify_report: bool,
    pub notify_upload: bool,
}

impl Default for MailCfg {
    fn default() -> Self {
        Self {
            enabled: false,
            host: String::new(),
            port: 587,
            secure: String::new(),
            user: String::new(),
            pass: String::new(),
            from: String::new(),
            from_name: String::new(),
            to: String::new(),
            verify_peer: true,
            notify_contact: true,
            notify_report: true,
            notify_upload: false,
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct DemoCfg {
    pub enabled: bool,
    pub keep_hours: i64,
    pub max_file_size: i64,
}

impl Default for DemoCfg {
    fn default() -> Self {
        Self {
            enabled: false,
            keep_hours: 24,
            max_file_size: 5 * 1048576,
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub title: String,
    pub description: String,
    pub notice: String,
    pub base_url: Option<String>,
    pub per_page: i64,
    pub pretty_urls: bool,
    pub utc_offset_hours: i64,
    pub ogp_image: String,
    pub twitter_site: String,
    pub theme_color: String,
    pub admin_password: String,
    pub secret: String,
    pub max_file_size: i64,
    pub max_total_size: i64,
    pub chunk_size: i64,
    pub max_comment: i64,
    pub upload_password: String,
    pub uploads_per_hour: i64,
    pub allow_ext: Vec<String>,
    pub deny_ext: Vec<String>,
    pub dl_count_window: i64,
    pub storage: String,
    pub s3: S3Cfg,
    pub turnstile: TurnstileCfg,
    pub mail: MailCfg,
    pub demo: DemoCfg,
    pub show_powered_by: bool,
    pub behind_cloudflare: bool,
    /// リッスン先（例: 0.0.0.0:8080）
    pub listen: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            title: "likeget-uploader".into(),
            description: String::new(),
            notice: String::new(),
            base_url: None,
            per_page: 50,
            pretty_urls: true,
            utc_offset_hours: 9,
            ogp_image: String::new(),
            twitter_site: String::new(),
            theme_color: "#2b3440".into(),
            admin_password: "change-me".into(),
            secret: String::new(),
            max_file_size: 200 * 1024 * 1024,
            max_total_size: 50_i64 * 1024 * 1024 * 1024,
            chunk_size: 8 * 1024 * 1024,
            max_comment: 200,
            upload_password: String::new(),
            uploads_per_hour: 20,
            allow_ext: vec![],
            deny_ext: vec![
                "php", "php3", "php4", "php5", "php7", "phtml", "phar", "cgi", "pl", "py", "sh",
                "asp", "aspx", "jsp", "htaccess", "html", "htm", "shtml", "xhtml", "svg", "js",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            dl_count_window: 3600,
            storage: "local".into(),
            s3: S3Cfg::default_s3(),
            turnstile: Default::default(),
            mail: Default::default(),
            demo: Default::default(),
            show_powered_by: true,
            behind_cloudflare: false,
            listen: None,
        }
    }
}

impl Config {
    /// config.toml があればそれ、なければ config-sample.toml を使う（PHP版と同じ挙動）
    pub fn load(root: &std::path::Path) -> Result<Config, String> {
        for name in ["config.toml", "config-sample.toml"] {
            let p = root.join(name);
            if p.is_file() {
                let s = std::fs::read_to_string(&p).map_err(|e| format!("{p:?}: {e}"))?;
                let mut cfg: Config = toml::from_str(&s).map_err(|e| format!("{name}: {e}"))?;
                if cfg.secret.is_empty() {
                    cfg.secret = crate::util::rand_hex(64);
                }
                return Ok(cfg);
            }
        }
        Err("config.toml / config-sample.toml が見つかりません。".into())
    }
}
