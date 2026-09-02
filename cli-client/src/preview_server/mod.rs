mod filter;
mod framework_devices;

use crate::payload::WebhookPayload;
use crate::preview_server::framework_devices::FrameworkDevices;
use liquid::Template;
use liquid_core::ValueView;
use regex::Regex;
use serde::Deserialize;
use std::env;
use std::sync::Arc;
#[cfg(unix)]
use tokio::select;
#[cfg(unix)]
use tokio::signal::unix::{SignalKind, signal};
use warp::http::Response;
use warp::http::StatusCode;
use warp::http::header::CONTENT_TYPE;
use warp::{Filter, Reply};

static PLUGIN_LIQUID: &str = include_str!("../../../plugin/src/shared.liquid");
static RENDERER_LIQUID: &str = include_str!("../../preview-assets/template/renderer.liquid");
static PREVIEWER_LIQUID: &str = include_str!("../../preview-assets/template/previewer.liquid");
static FRAMEWORK_CSS: &str =
    include_str!("../../../trmnl-framework/public/css/3.3.1/plugins.min.css");
static FRAMEWORK_CSS_THEME_DARK: &str =
    include_str!("../../../trmnl-framework/public/css/3.3.1/themes/dark-theme.css");
static FRAMEWORK_CSS_THEME_BLACK_YELLOW: &str =
    include_str!("../../../trmnl-framework/public/css/3.3.1/themes/black-and-yellow-theme.css");
static FRAMEWORK_CSS_THEME_WHITE_RED: &str =
    include_str!("../../../trmnl-framework/public/css/3.3.1/themes/white-and-red-theme.css");
static FRAMEWORK_JS: &str = include_str!("../../../trmnl-framework/public/js/3.3.1/plugins.min.js");
static TRMNL16_BOLD_TTF: &[u8] =
    include_bytes!("../../../trmnl-framework/public/fonts/TRMNL16-Bold.ttf");
static TRMNL16_BOLD_WOFF: &[u8] =
    include_bytes!("../../../trmnl-framework/public/fonts/TRMNL16-Bold.woff");
static TRMNL16_BOLD_WOFF2: &[u8] =
    include_bytes!("../../../trmnl-framework/public/fonts/TRMNL16-Bold.woff2");

#[derive(Debug, Clone, Deserialize)]
struct RenderQuery {
    screen_classes: String,
    layout: String,
}

pub async fn launch(height: u16, payload: WebhookPayload) -> u8 {
    let renderer_tpl = Arc::new(make_renderer_tpl());
    let previewer_content = render_root(payload.data.width, height, &FrameworkDevices::load());

    let root_route = warp::path::end().map(move || warp::reply::html(previewer_content.clone()));
    let render_route = warp::path("render")
        .and(warp::query::<RenderQuery>())
        .map(move |args| handle_render(&args, &payload, &renderer_tpl.0, &renderer_tpl.1));
    let framework_css = warp::path("plugins.css").map(|| {
        warp::reply::with_header(
            {
                // I considered const concating them, but that takes ages during compile time
                format!("{FRAMEWORK_CSS}\n{FRAMEWORK_CSS_THEME_DARK}\n{FRAMEWORK_CSS_THEME_BLACK_YELLOW}\n{FRAMEWORK_CSS_THEME_WHITE_RED}")
            },
            CONTENT_TYPE,
            "text/css",
        )
    });
    let framework_js = warp::path("plugins.js")
        .map(|| warp::reply::with_header(FRAMEWORK_JS, CONTENT_TYPE, "text/javascript"));
    let trmnl16bold_ttf = warp::path!("fonts" / "TRMNL16-Bold.ttf")
        .map(|| warp::reply::with_header(TRMNL16_BOLD_TTF, CONTENT_TYPE, "font/ttf"));
    let trmnl16bold_woff = warp::path!("fonts" / "TRMNL16-Bold.woff")
        .map(|| warp::reply::with_header(TRMNL16_BOLD_WOFF, CONTENT_TYPE, "font/woff"));
    let trmnl16bold_woff2 = warp::path!("fonts" / "TRMNL16-Bold.woff2")
        .map(|| warp::reply::with_header(TRMNL16_BOLD_WOFF2, CONTENT_TYPE, "font/woff2"));

    let port = env::var("TRMNL_CONSOLE_PORT")
        .ok()
        .and_then(|port_str| port_str.parse::<_>().ok())
        // default to 0: auto-assign port
        .unwrap_or_default();

    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("trmnl-console: failed to start preview server: {err}");
            return 91;
        }
    };
    let port = match listener.local_addr() {
        Ok(addr) => addr.port(),
        Err(err) => {
            eprintln!("trmnl-console: failed to start preview server: {err}");
            return 91;
        }
    };
    let server = tokio::spawn(
        warp::serve(
            root_route
                .or(render_route)
                .or(framework_css)
                .or(framework_js)
                .or(trmnl16bold_ttf)
                .or(trmnl16bold_woff)
                .or(trmnl16bold_woff2),
        )
        .incoming(listener)
        .run(),
    );

    if env::var_os("TRMNL_CONSOLE_NO_OPEN").is_none() {
        open::that(format!("http://127.0.0.1:{port}")).ok();
    }

    println!("trmnl-console: Done rendering. Preview server started on http://127.0.0.1:{port}.");
    println!(
        "trmnl-console: We tried to open your browser, if that didn't work, open the link manually."
    );

    #[cfg(unix)]
    {
        println!("trmnl-console: Hit CTRL+C to exit.");

        let mut sig = signal(SignalKind::interrupt()).unwrap();

        select!( _ = server => {}, _ = sig.recv() => {} );

        println!();
    }
    #[cfg(not(unix))]
    {
        let _ = server.await;
    }

    0
}

fn render_root(width: u16, height: u16, devices: &FrameworkDevices) -> String {
    let globals = liquid::object!({
        "width": width,
        "height": height,
        "devices": devices.picker_models,
        "palettes": devices.palettes,
        "orientations": [
            {"name": "landscape", "classes": "", "label": "Landscape"},
            {"name": "portrait", "classes": "screen--portrait", "label": "Portrait"},
        ],
        "themes": [
            {"name": "default", "classes": "", "label": "Default"},
            {"name": "dark", "classes": "screen--theme-dark", "label": "Dark"},
            {"name": "black-and-yellow", "classes": "screen--theme-black-and-yellow", "label": "Black and Yellow"},
            {"name": "white-and-read", "classes": "screen--theme-white-and-red", "label": "White and Red"},
        ],
    });
    liquid::ParserBuilder::with_stdlib()
        .filter(filter::json::Json)
        .build()
        .unwrap()
        .parse(PREVIEWER_LIQUID)
        .unwrap()
        .render(&globals)
        .unwrap()
}

fn make_renderer_tpl() -> (Template, String) {
    let re = Regex::new(r"(?s)\{% template content %}\n(.*?)\n\{% endtemplate %}").unwrap();

    let plugin_content_block = re.captures(PLUGIN_LIQUID).unwrap()[1].to_string();

    (
        liquid::ParserBuilder::with_stdlib()
            .build()
            .unwrap()
            .parse(RENDERER_LIQUID)
            .unwrap(),
        plugin_content_block,
    )
}

fn handle_render(
    args: &RenderQuery,
    payload: &WebhookPayload,
    renderer_tpl: &Template,
    plugin_content: &str,
) -> warp::reply::Response {
    let mut globals = liquid::model::to_value(payload).unwrap();
    globals.as_object_mut().unwrap().insert(
        "content".into(),
        liquid::model::to_value(&plugin_content.to_string()).unwrap(),
    );
    globals.as_object_mut().unwrap().insert(
        "layout".into(),
        liquid::model::to_value(&args.layout).unwrap(),
    );
    globals.as_object_mut().unwrap().insert(
        "screen_classes".into(),
        liquid::model::to_value(&args.screen_classes).unwrap(),
    );
    globals
        .as_object_mut()
        .unwrap()
        .get_mut("data")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .insert("content_transformed".into(), liquid::model::Value::Nil);

    // if we just render once we will have all the liquid placeholders from the inner template
    // a bit ugly but we solve this by doing two render passes for now..
    let result = renderer_tpl
        .render(globals.as_object().unwrap())
        .and_then(|pass1| {
            liquid::ParserBuilder::with_stdlib()
                .filter(filter::append_random::AppendRandom)
                .filter(filter::raw::Raw)
                .filter(filter::json::Json)
                .build()?
                .parse(&pass1)?
                .render(globals.as_object().unwrap())
        });

    match result {
        Ok(content) => warp::reply::html(content).into_response(),
        Err(err) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(format!("Error rendering template: {}", err))
            .unwrap()
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::WebhookPayloadData;

    #[test]
    fn test_render_root() {
        render_root(100, 100, &FrameworkDevices::load());
    }

    #[test]
    fn test_renderer_tpl() {
        make_renderer_tpl();
    }

    #[test]
    fn test_handle_render() {
        let (tpl, plugin_content) = make_renderer_tpl();
        let resp = handle_render(
            &RenderQuery {
                screen_classes: "".to_string(),
                layout: "full".to_string(),
            },
            &WebhookPayload {
                data: WebhookPayloadData {
                    width: 10,
                    scale: 1,
                    bar: None,
                    content: "foo".to_string(),
                },
            },
            &tpl,
            &plugin_content,
        );

        assert_eq!(StatusCode::OK, resp.status());
    }
}
