//! A fake Lens results site for the per-search browser tests (DAN-2165), served by `TestServer::http`
//! on 127.0.0.1: the home page, the upload (a redirect to the results, a robot check, or a page
//! that never leaves), the results page with the crop handles and the tabs, the explicit-results
//! notice, the Exact matches tiles, and the responses a page fetches after it loads. Nothing is
//! captured from Google; the pages are synthetic, and their JavaScript is test-only.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{Recorded, Reply};

#[derive(Clone)]
pub struct Cfg {
    /// `results`, `sorry`, `unreadable`, `hang`
    pub upload: &'static str,
    /// The picture on the page: x, y, width, height.
    pub img: [i64; 4],
    /// The crop as the handles state it: left, top, right, bottom, in percent.
    pub crop: [i64; 4],
    /// `links`, `none`
    pub tabs: &'static str,
    /// `none`, `notice`
    pub explicit: &'static str,
    pub doc_hint: bool,
    pub async_hint: bool,
    pub widen_hint: bool,
    pub d_async_ms: u64,
    pub d_widen_ms: u64,
    pub tiles: Vec<(&'static str, bool)>,
    /// The tiles are drawn this long after the Exact matches page loads.
    pub tile_delay_ms: u64,
    /// The robot check replaces the tab pages.
    pub challenge_on_tabs: bool,
}

impl Default for Cfg {
    fn default() -> Self {
        Cfg {
            upload: "results",
            img: [200, 150, 600, 400],
            crop: [0, 0, 100, 100],
            tabs: "links",
            explicit: "none",
            doc_hint: true,
            async_hint: true,
            widen_hint: true,
            d_async_ms: 220,
            d_widen_ms: 220,
            tiles: Vec::new(),
            tile_delay_ms: 0,
            challenge_on_tabs: false,
        }
    }
}

pub const MAIN_URLS: [&str; 4] = [
    "https://danbooru.donmai.us/posts/1111",
    "https://gelbooru.com/index.php?page=post&s=view&id=222",
    "https://www.pinterest.com/pin/333/",
    "https://www.google.com/search?q=1",
];
pub const ASYNC_URLS: [&str; 2] = [
    "https://safebooru.org/index.php?page=post&s=view&id=444",
    "https://i.redd.it/zzz.jpg",
];
pub const WIDEN_URLS: [&str; 2] = [
    "https://yande.re/post/show/555",
    "https://e621.net/posts/666",
];
pub const VISUAL_URLS: [&str; 2] = ["https://example.org/a/b.jpg", "https://i.redd.it/vis.jpg"];
pub const EXACT_URLS: [&str; 1] = ["https://danbooru.donmai.us/posts/9999"];

const CAPTCHA: &str = "<!doctype html><title>check</title><p>check <iframe src=\"/recaptcha/api2/anchor\" title=\"reCAPTCHA\"></iframe>";

fn thumb() -> String {
    // 256 bytes of base64 (a src of at least 80 characters), not a picture anyone decodes.
    format!("data:image/jpeg;base64,{}", "QUJD".repeat(64))
}

fn json_list(urls: &[&str]) -> String {
    format!(
        ")]}}'\n[{}]",
        urls.iter()
            .map(|u| format!("\"{u}\""))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn handles_js(c: &Cfg) -> String {
    let [ix, iy, iw, ih] = c.img;
    let [l, t, r, b] = c.crop;
    format!(
        "var IMG=[{ix},{iy},{iw},{ih}], CROP=[{l},{t},{r},{b}], DRAG=null;\n\
         function lbl(k){{ if(k==='tl')return 'top left corner of search area: left '+CROP[0]+'%, top '+CROP[1]+'%'; \
           return 'bottom right corner of search area: right '+CROP[2]+'%, bottom '+CROP[3]+'%'; }}\n\
         function render(){{ document.querySelectorAll('[data-corner]').forEach(function(n){{n.remove();}});\n\
           ['tl','br'].forEach(function(k){{ var x=IMG[0]+IMG[2]*(k==='tl'?CROP[0]:CROP[2])/100, y=IMG[1]+IMG[3]*(k==='tl'?CROP[1]:CROP[3])/100;\n\
             var h=document.createElement('div'); h.setAttribute('data-corner',k); h.id='h-'+k; h.setAttribute('aria-label',lbl(k));\n\
             h.style.cssText='position:absolute;width:16px;height:16px;background:#c33;left:'+(x-8)+'px;top:'+(y-8)+'px;z-index:5';\n\
             document.body.appendChild(h); }}); }}\n\
         render();\n\
         document.addEventListener('mousedown',function(e){{var h=e.target.closest&&e.target.closest('[data-corner]'); DRAG=h?h.getAttribute('data-corner'):null;}});\n\
         document.addEventListener('mouseup',function(e){{ if(!DRAG)return;\n\
           var px=Math.max(0,Math.min(100,Math.round((e.clientX-IMG[0])/IMG[2]*100)));\n\
           var py=Math.max(0,Math.min(100,Math.round((e.clientY-IMG[1])/IMG[3]*100)));\n\
           if(DRAG==='tl'){{CROP[0]=px;CROP[1]=py;}}else{{CROP[2]=px;CROP[3]=py;}}\n\
           var which=DRAG; DRAG=null; render();\n\
           if(which==='br'){{fetch('/search?async=1&widen=1&crop='+CROP.join(','));}} }});"
    )
}

fn results_doc(c: &Cfg, tab: &str) -> String {
    let urls: Vec<&str> = match tab {
        "main" => MAIN_URLS.to_vec(),
        "visual" => VISUAL_URLS.to_vec(),
        _ => EXACT_URLS.to_vec(),
    };
    let data = if c.doc_hint {
        format!(
            "<script>var DATA=[{}];</script>",
            urls.iter()
                .map(|u| format!("\"{u}\""))
                .collect::<Vec<_>>()
                .join(",")
        )
    } else {
        String::new()
    };
    let [ix, iy, iw, ih] = c.img;
    let handles = if tab == "main" {
        format!(
            "<div id=\"pic\" style=\"position:absolute;left:{ix}px;top:{iy}px;width:{iw}px;height:{ih}px;background:#ccc\"></div><script>{}</script>",
            handles_js(c)
        )
    } else {
        String::new()
    };
    let tabs = if c.tabs == "links" {
        "<div><a href=\"/search?ep=1&vsrid=abc123&tab=visual\" id=\"tab-visual\">Visual matches</a> \
         <a href=\"/search?ep=1&vsrid=abc123&tab=exact\" id=\"tab-exact\">Exact matches</a></div>"
            .to_owned()
    } else {
        String::new()
    };
    let explicit = if c.explicit == "notice" {
        "<div id=\"explicit\" style=\"margin:10px;padding:8px;background:#eee\" \
         onclick=\"location='/search?ep=1&vsrid=abc123&tab=exact'\">See exact matches</div>"
            .to_owned()
    } else {
        String::new()
    };
    let tiles = if tab == "exact" && !c.tiles.is_empty() {
        let body: String = c
            .tiles
            .iter()
            .map(|(text, with_thumb)| {
                let src = if *with_thumb {
                    thumb()
                } else {
                    "data:image/gif;base64,R0lGOD".to_owned()
                };
                format!("<div class=\"tile\"><div><img src=\"{src}\"></div><p>{text}</p></div>")
            })
            .collect();
        format!(
            "<div id=\"tiles\"></div><script>setTimeout(function(){{document.getElementById('tiles').innerHTML={body:?};}},{});</script>",
            c.tile_delay_ms
        )
    } else {
        String::new()
    };
    format!(
        "<!doctype html><meta charset=utf-8><title>Lens {tab}</title><body style=\"margin:0;height:1200px\">{data}{handles}{explicit}{tabs}{tiles}\
         <script>setTimeout(function(){{fetch('/search?async=1&tab={tab}');}},{});</script></body>",
        c.d_async_ms
    )
}

/// The site, as a `TestServer::http` handler over a shared configuration.
pub fn results_site(cfg: Arc<Mutex<Cfg>>) -> impl Fn(&Recorded) -> Reply + Send + Sync + 'static {
    move |req| {
        let c = cfg.lock().unwrap().clone();
        let (path, query) = req
            .target
            .split_once('?')
            .unwrap_or((req.target.as_str(), ""));
        let has = |needle: &str| {
            query
                .split('&')
                .any(|p| p == needle || p.starts_with(&format!("{needle}=")))
        };
        let value = |key: &str| -> Option<&str> {
            query
                .split('&')
                .find_map(|p| p.strip_prefix(&format!("{key}=")))
        };
        let html =
            |body: String| Reply::ok(body).header("Content-Type", "text/html; charset=utf-8");
        match path {
            "/" => html("<!doctype html><title>home</title><p>home".to_owned()),
            "/upload" => match c.upload {
                "unreadable" => {
                    html("<!doctype html><p>Something went wrong - can't read file".to_owned())
                }
                "sorry" => Reply::new(302, "").header("Location", "/sorry/index?continue=1"),
                "hang" => html("late".to_owned()).delayed(Duration::from_secs(30)),
                _ => Reply::new(302, "").header("Location", "/search?ep=1&vsrid=abc123&tab=main"),
            },
            "/sorry/index" => html("<!doctype html><p>unusual traffic".to_owned()),
            "/clear" => html("<!doctype html><p>cleared".to_owned()),
            "/recaptcha/api2/anchor" => html("<!doctype html><p>anchor".to_owned()),
            "/search" if has("async") => {
                if has("widen") {
                    let urls: &[&str] = if c.widen_hint {
                        &WIDEN_URLS
                    } else {
                        &["https://www.google.com/x"]
                    };
                    Reply::ok(json_list(urls))
                        .header("Content-Type", "application/json")
                        .delayed(Duration::from_millis(c.d_widen_ms))
                } else {
                    let urls: &[&str] = if c.async_hint {
                        &ASYNC_URLS
                    } else {
                        &["https://www.google.com/x"]
                    };
                    Reply::ok(json_list(urls)).header("Content-Type", "application/json")
                }
            }
            "/search" => {
                let tab = value("tab").unwrap_or("main");
                if c.challenge_on_tabs && tab != "main" {
                    return html(CAPTCHA.to_owned());
                }
                html(results_doc(&c, tab))
            }
            _ => html("<!doctype html><p>ok".to_owned()),
        }
    }
}
