//! 利用規約（旧 rules.php）
use actix_web::{HttpResponse, web};

use crate::session::Sess;
use crate::state::WebData;

pub async fn rules(st: web::Data<WebData>, req: actix_web::HttpRequest) -> HttpResponse {
    let sess = Sess::open(&st, &req);
    let custom = crate::site::site_text(&st, "rules.html").trim().to_string();
    let mut body = String::new();
    body += "<article class=\"box prose\"><h1>利用規約</h1>";
    if !custom.is_empty() {
        body += &custom;
    } else {
        body += &format!(
            "<p>このアップローダーは、{}の目的に沿ったファイル（シナリオ、MOD、素材、ツールなど）を共有するための場所です。ご利用いただいた時点で、以下に同意したものとみなします。</p>\
<h2>禁止事項</h2><ul><li>著作権・肖像権その他の権利を侵害するファイル（無断転載、作者が配布を認めていない素材の再配布など）</li><li>ウイルス・マルウェアその他、他人に害を与えるファイル</li><li>法令や公序良俗に反するファイル</li><li>アップローダーの目的と関係のないファイル</li></ul>\
<h2>削除について</h2><ul><li>投稿者は、アップロード時に設定した削除キーで、いつでも自分のファイルを削除できます。</li><li>管理人は、規約に反する、または不適切と判断したファイルを予告なく削除できます。</li><li>権利侵害などはファイルページの「通報」からお知らせください。</li></ul>\
<h2>免責</h2><ul><li>ファイルの内容について管理人は責任を負いません。ダウンロードしたファイルは各自の責任でご利用ください。</li><li>障害などにより、ファイルが予告なく失われる可能性があります。大切なファイルは必ず手元にも保存してください。</li></ul>",
            crate::util::h(&st.cfg.title)
        );
    }
    body += "</article>";
    let opts = crate::html::PageOpts::basic("利用規約", "rules");
    let html = crate::html::render(&st, &req, &sess, &opts, &body);
    let mut resp = HttpResponse::Ok().content_type("text/html; charset=utf-8").body(html);
    crate::session::finalize(&mut resp, &sess, &st, &req);
    resp
}
