use std::sync::Arc;

use gpui::{prelude::*, px, size, App, Bounds, WindowAppearance};
use gpui_platform::application;
use zork_gui::api::GatewayClient;
use zork_gui::assets::EmbeddedAssets;
use zork_gui::components;
use zork_gui::views::RootView;
use zork_gui::window_chrome::native_titlebar_options;

const DEFAULT_GATEWAY_URL: &str = "http://127.0.0.1:3000";

fn parse_args() -> (String, Option<String>) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut gateway_url = DEFAULT_GATEWAY_URL.to_owned();
    let mut gateway_token: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--gateway-url" if i + 1 < args.len() => {
                gateway_url = args[i + 1].clone();
                i += 1;
            }
            "--gateway-token" if i + 1 < args.len() => {
                gateway_token = Some(args[i + 1].clone());
                i += 1;
            }
            "--help" | "-h" => {
                println!(
                    "zork-gui [options]\n\n\
                     Options:\n\
                     --gateway-url <url>  gateway runtime URL (default {DEFAULT_GATEWAY_URL})\n\
                     --gateway-token <t>  optional bearer token for the gateway\n\
                     Environment: ZORK_GATEWAY_TOKEN can also provide the token.\n"
                );
                std::process::exit(0);
            }
            other => {
                if other.starts_with("--") {
                    eprintln!("unknown argument: {other}");
                }
            }
        }
        i += 1;
    }
    (gateway_url, gateway_token)
}

fn main() {
    let (gateway_url, gateway_token) = parse_args();
    let client = Arc::new(GatewayClient::new(
        gateway_url,
        gateway_token.or_else(|| std::env::var("ZORK_GATEWAY_TOKEN").ok()),
    ));

    application()
        .with_assets(EmbeddedAssets)
        .run(move |cx: &mut App| {
            components::init(cx);
            cx.set_window_appearance(Some(WindowAppearance::Light));
            cx.open_window(
                gpui::WindowOptions {
                    window_bounds: Some(gpui::WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1280.0), px(800.0)),
                        cx,
                    ))),
                    titlebar: Some(native_titlebar_options()),
                    ..Default::default()
                },
                |_window, cx| cx.new(|cx| RootView::new(client.clone(), cx)),
            )
            .expect("failed to open window");
            cx.activate(true);
        });
}
