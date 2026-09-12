//! Remote-client shell. Engines, workspaces and live terminals remain on the host.
use dioxus::prelude::*;
use shprd_shell::{BridgeAck, HostUrl};

fn main() {
    dioxus::launch(app);
}

fn app() -> Element {
    let configured = option_env!("SHPRD_HOST_URL").unwrap_or("");
    let mut input = use_signal(|| configured.to_owned());
    let mut host = use_signal(|| HostUrl::parse(configured).ok());
    let mut error = use_signal(|| {
        if !configured.is_empty() && HostUrl::parse(configured).is_err() {
            String::from("Configured SHPRD_HOST_URL is invalid.")
        } else {
            String::new()
        }
    });
    let mut status = use_signal(|| String::from("Engines and terminals run on your host."));
    let mut busy = use_signal(|| false);
    let mut ready = use_signal(|| false);
    let mut request = use_signal(|| 0_u64);
    rsx! {
        document::Title { "SHPRD" }
        style { {include_str!("../assets/shell.css")} }
        main { class: "shprd-shell",
            header { class: "shprd-toolbar",
                h1 { "SHPRD" }
                form {
                    onsubmit: move |event| async move {
                        event.prevent_default();
                        let next = match HostUrl::parse(&input()) {
                            Ok(value) => value,
                            Err(value) => { error.set(value.to_string()); return; }
                        };
                        if host().as_ref() == Some(&next) { error.set(String::new()); return; }
                        if host().is_some() {
                            let confirmed = document::eval("return window.confirm('Changing hosts reloads the workspace. Unsaved drafts will be lost. Continue?');").await;
                            if !matches!(confirmed, Ok(serde_json::Value::Bool(true))) { return; }
                        }
                        ready.set(false);
                        host.set(Some(next));
                        error.set(String::new());
                        status.set(String::from("Host opened. Sign in within the workspace if required."));
                    },
                    label { r#for: "shprd-host", "Host URL",
                        input { id: "shprd-host", r#type: "url", required: true,
                            placeholder: "https://your-host.example", value: "{input}",
                            autocomplete: "off", spellcheck: "false",
                            oninput: move |event| input.set(event.value()),
                        }
                    }
                    button { r#type: "submit", disabled: busy(), "Open host" }
                    button { r#type: "button", disabled: !ready() || busy(),
                        onclick: move |_| async move {
                            let Some(current) = host() else { return; };
                            busy.set(true);
                            error.set(String::new());
                            status.set(String::from("Checking React bridge..."));
                            request += 1;
                            let request_id = format!("shell-{}", request());
                            let eval = document::eval(&format!("return {}", include_str!("../assets/shell-bridge.js")));
                            let sent = eval.send(serde_json::json!({"origin":current.origin(), "request_id":request_id}));
                            match sent {
                                Ok(()) => match eval.await {
                                    Ok(value) => match serde_json::from_value::<BridgeAck>(value) {
                                        Ok(ack) if ack.accepts(&request_id) => status.set(String::from("React bridge connected. Workspace state preserved.")),
                                        _ => error.set(String::from("Invalid React bridge acknowledgement.")),
                                    },
                                    Err(_) => error.set(String::from("React bridge unavailable. Check host reachability and shell bridge integration.")),
                                },
                                Err(_) => error.set(String::from("Shell bridge could not send its request.")),
                            }
                            busy.set(false);
                        },
                        if busy() { "Checking..." } else { "Check bridge" }
                    }
                }
                p { id: "shprd-status", role: "status", "{status}" }
                if !error().is_empty() { p { role: "alert", "{error}" } }
            }
            if let Some(current) = host() {
                iframe { id: "shprd-react", class: "shprd-surface", title: "SHPRD workspace",
                    key: "{current.as_str()}",
                    src: current.as_str(), referrerpolicy: "no-referrer",
                    onload: move |_| ready.set(true),
                    allow: "clipboard-read; clipboard-write; fullscreen",
                }
            } else {
                p { class: "shprd-empty", "Connect to your SHPRD host to open workspaces, agents and terminals." }
            }
        }
    }
}
