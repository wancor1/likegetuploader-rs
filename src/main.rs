use actix_web::{App, HttpResponse, HttpServer, web};

mod config;
mod db;
mod formio;
mod hdl_assets;
mod hdl_dl;
mod hdl_upload;
mod html;
mod mail;
mod page_admin;
mod page_contact;
mod page_file;
mod page_list;
mod page_rules;
mod s3;
mod session;
mod site;
mod state;
mod tools;
mod turnstile;
mod util;

fn main() {
    // 実行ファイルの場所をルートとする（config.toml / assets / data はそこに置く）
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let root = exe
        .parent()
        .and_then(|p| p.parent()) // debug: /target/debug → /root
        .and_then(|p| p.parent()) // debug: /target → /root
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    // config を探す順: 実行ファイル隣 → debug2段上 → カレント
    let mut root_path = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    if !root_path.join("config-sample.toml").is_file() && !root_path.join("config.toml").is_file() {
        let cand: Vec<std::path::PathBuf> = vec![
            std::env::current_dir().unwrap_or_default(),
            root,
        ];
        for c in cand {
            if c.join("config-sample.toml").is_file() || c.join("config.toml").is_file() {
                root_path = c;
                break;
            }
        }
    }

    let argv: Vec<String> = std::env::args().collect();
    let cfg = match config::Config::load(&root_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("likeget-uploader: {e}");
            std::process::exit(1);
        }
    };

    let args = &argv[1..];
    if args.first().map(|s| s.as_str()) == Some("import") {
        let dir = args.get(1).cloned().unwrap_or_default();
        let mv = args.iter().any(|a| a == "--move");
        if tools::import(&root_path, &cfg, &dir, mv) {
            return;
        }
        std::process::exit(1);
    }
    if args.first().map(|s| s.as_str()) == Some("move-to-s3") {
        let keep = args.iter().any(|a| a == "--keep");
        if tools::move_to_s3(&root_path, &cfg, keep) {
            return;
        }
        std::process::exit(1);
    }

    // ---- サーバー起動 ----
    std::fs::create_dir_all(root_path.join("data")).ok();
    let conn = db::open(&root_path);
    let data = web::Data::new(state::WebData {
        cfg: cfg.clone(),
        root: root_path.clone(),
        db: std::sync::Mutex::new(conn),
        sessions: std::sync::Mutex::new(std::collections::HashMap::new()),
        senders: std::sync::Mutex::new(std::collections::HashMap::new()),
        md5s: std::sync::Mutex::new(std::collections::HashMap::new()),
    });
    let listen = cfg.listen.clone().unwrap_or_else(|| "0.0.0.0:8080".to_string());

    println!(
        "{} {}/{} — http://{listen}",
        util::APP_NAME,
        util::APP_VERSION,
        root_path.display()
    );
    db::demo_purge(&data, true);

    let data_for_server = data.clone();
    actix_web::rt::System::new().block_on(async move {
        HttpServer::new(move || {
            App::new()
                .app_data(data_for_server.clone())
                .app_data(web::PayloadConfig::new(48 * 1024 * 1024))
                .service(web::resource("/").route(web::get().to(page_list::index)))
                .service(web::resource("rules").route(web::get().to(page_rules::rules)))
                .service(
                    web::resource("contact")
                        .route(web::get().to(page_contact::contact))
                        .route(web::post().to(page_contact::contact)),
                )
                .service(
                    web::resource("download/{id}")
                        .route(web::get().to(page_file::detail))
                        .route(web::post().to(page_file::detail)),
                )
                .service(
                    web::resource("file")
                        .route(web::get().to(page_file::detail))
                        .route(web::post().to(page_file::detail)),
                )
                .service(
                    web::resource("file.php")
                        .route(web::get().to(page_file::detail))
                        .route(web::post().to(page_file::detail)),
                )
                .service(web::resource("dl").route(web::get().to(hdl_dl::dl)).route(web::post().to(hdl_dl::dl)))
                .service(web::resource("upload").route(web::post().to(hdl_upload::upload)))
                .service(
                    web::resource("admin")
                        .route(web::get().to(page_admin::admin))
                        .route(web::post().to(page_admin::admin)),
                )
                .service(web::resource("admin.php").route(web::get().to(legacy_admin)))
                .service(web::resource("ads.txt").route(web::get().to(hdl_assets::adstxt)))
                .service(web::resource("favicon.ico").route(web::get().to(hdl_assets::favicon)))
                .service(web::resource("assets/{tail:.*}").route(web::get().to(hdl_assets::asset_file)))
        })
        .bind(&listen)
        .unwrap_or_else(|e| panic!("リッスンできませんでした({listen}): {e}"))
        .run()
        .await
        .expect("サーバーが停止しました");
    });
}

async fn legacy_admin() -> HttpResponse {
    HttpResponse::SeeOther().insert_header(("Location", "/admin")).finish()
}
