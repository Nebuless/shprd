//! Dioxus shell. Android starts packaged React; engines and live terminals remain remote.
use dioxus::prelude::*;
#[cfg(not(feature = "mobile"))]
use shprd_shell::{HostUrl, initial_host_url};

fn main() {
    #[cfg(feature = "mobile")]
    dioxus::LaunchBuilder::mobile()
        .with_cfg(
            dioxus::mobile::Config::new()
                .with_custom_index(mobile_index())
                .with_root_name("shprd-dioxus-shell"),
        )
        .launch(app);

    #[cfg(not(feature = "mobile"))]
    dioxus::launch(app);
}

#[cfg(feature = "mobile")]
fn mobile_index() -> String {
    let index = include_str!("../../../server/public/index.html");
    let module_marker = "<script type=\"module\" crossorigin src=\"";
    let Some((before_module, module_tail)) = index.split_once(module_marker) else {
        return index.to_owned();
    };
    let Some((module_path, after_module)) = module_tail.split_once("\"></script>") else {
        return index.to_owned();
    };
    let bootstrap = MOBILE_BOOTSTRAP.replace("__SHPRD_VITE_MODULE__", module_path);
    format!("{before_module}<script>{bootstrap}</script>{after_module}")
        .replace("<body>", "<body><div id=\"shprd-dioxus-shell\"></div>")
}

#[cfg(feature = "mobile")]
const MOBILE_BOOTSTRAP: &str = r#"
(() => {
  const storageKey = "shprd-host-url";
  const normalize = (value) => {
    if (typeof value !== "string" || !value || /\s/.test(value)) return null;
    try {
      const url = new URL(value);
      if (!["http:", "https:"].includes(url.protocol) || !url.hostname || url.username || url.password || url.search || url.hash || url.pathname !== "/") return null;
      return url.href;
    } catch {
      return null;
    }
  };
  const start = (host) => {
    window.__SHPRD_HOST_URL__ = host;
    document.getElementById("shprd-mobile-connect")?.remove();
    const module = document.createElement("script");
    module.type = "module";
    module.src = "__SHPRD_VITE_MODULE__";
    document.head.append(module);
  };
  const savedHost = normalize(localStorage.getItem(storageKey));
  if (savedHost) return start(savedHost);
  document.addEventListener("DOMContentLoaded", () => {
    const form = document.createElement("form");
    form.id = "shprd-mobile-connect";
    form.innerHTML = '<label>Herdr host <input id="shprd-mobile-host" type="url" autocomplete="url" placeholder="https://your-host.example" required></label><button>Connect</button><p id="shprd-mobile-error" role="alert"></p>';
    document.body.prepend(form);
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      const host = normalize(document.getElementById("shprd-mobile-host")?.value);
      if (!host) {
        document.getElementById("shprd-mobile-error").textContent = "Enter an HTTP(S) origin without credentials, path, query or fragment.";
        return;
      }
      localStorage.setItem(storageKey, host);
      start(host);
    });
  });
})();
"#;

fn app() -> Element {
    #[cfg(feature = "mobile")]
    return rsx! {};

    #[cfg(not(feature = "mobile"))]
    {
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
                            },
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
}

#[cfg(all(test, feature = "mobile"))]
mod mobile_tests {
    use super::mobile_index;

    #[test]
    fn mobile_index_configures_remote_host_before_react_module() {
        let index = mobile_index();

        assert!(index.contains("window.__SHPRD_HOST_URL__"));
        assert!(index.contains("shprd-mobile-connect"));
        assert!(index.contains("id=\"shprd-dioxus-shell\""));
        assert!(index.contains("id=\"root\""));
        assert!(index.contains("module.src = \"./assets/"));
        assert!(index.contains("DOMContentLoaded"));
        assert!(!index.contains("<script type=\"module\" crossorigin src="));
    }
}
