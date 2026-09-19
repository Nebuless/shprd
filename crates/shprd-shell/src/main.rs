//! Remote-client shell. Engines, workspaces and live terminals remain on the host.
use dioxus::prelude::*;
use shprd_shell::{HostUrl, initial_host_url};

fn main() {
    dioxus::launch(app);
}

fn app() -> Element {
    let configured = option_env!("SHPRD_HOST_URL");
    let initial = initial_host_url(configured, cfg!(feature = "web"));
    let initial_host = initial.as_ref().ok().and_then(Clone::clone);
    let initial_error = initial.is_err();
    let mut host = use_signal(|| initial_host);
    let mut host_draft = use_signal(String::new);
    let mut host_error = use_signal(|| None::<String>);
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
            } else {
                section { class: "shprd-connect", "aria-labelledby": "shprd-connect-title",
                    h1 { id: "shprd-connect-title", "Connect to SHPRD" }
                    if initial_error {
                        p { class: "shprd-host-error", role: "alert", "Configured SHPRD_HOST_URL is invalid." }
                    }
                    p { "Enter your remote SHPRD host URL." }
                    label { class: "shprd-host-label", "Host URL"
                        input {
                            id: "shprd-host-url",
                            r#type: "url",
                            value: "{host_draft}",
                            placeholder: "https://your-host.example",
                            autocomplete: "url",
                            oninput: move |event| {
                                host_draft.set(event.value());
                                host_error.set(None);
                            },
                        }
                    }
                    button {
                        r#type: "button",
                        onclick: move |_| match HostUrl::parse(&host_draft()) {
                            Ok(next) => {
                                host.set(Some(next));
                                host_error.set(None);
                            }
                            Err(error) => host_error.set(Some(error.to_string())),
                        },
                        "Connect"
                    }
                    if let Some(error) = host_error() {
                        p { class: "shprd-host-error", role: "alert", "{error}" }
                    }
                }
            }
        }
    }
}
