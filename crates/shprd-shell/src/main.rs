//! Remote-client shell. Engines, workspaces and live terminals remain on the host.
use dioxus::prelude::*;
use shprd_shell::initial_host_url;

fn main() {
    dioxus::launch(app);
}

fn app() -> Element {
    let configured = option_env!("SHPRD_HOST_URL");
    let initial = initial_host_url(configured, cfg!(feature = "web"));
    let initial_host = initial.as_ref().ok().and_then(Clone::clone);
    let initial_error = initial.is_err();
    let host = use_signal(|| initial_host);
    rsx! {
        document::Title { "SHPRD" }
        style { {include_str!("../assets/shell.css")} }
        script { {include_str!("../assets/shell-controls.js")} }
        main { class: "shprd-shell",
            if let Some(current) = host() {
                iframe { id: "shprd-react", class: "shprd-surface", title: "SHPRD workspace",
                    key: "{current.as_str()}",
                    src: current.as_str(), referrerpolicy: "origin",
                    allow: "clipboard-read; clipboard-write; fullscreen",
                }
            } else if initial_error {
                p { class: "shprd-empty", "Configured SHPRD_HOST_URL is invalid." }
            } else {
                p { class: "shprd-empty", "Connect to your SHPRD host through the workspace settings." }
            }
        }
    }
}
