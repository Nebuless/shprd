# Patched Dioxus desktop

## Purpose
Local Dioxus 0.7.10 source patch. Android bridge URLs approved by the shell remain in the app WebView instead of opening Android's default browser.

## Ownership
Own only SHPRD's minimal Dioxus divergence. Upstream Dioxus owns all other implementation and updates.

## Local Contracts
- Keep the fork source-equivalent to Dioxus 0.7.10 except documented SHPRD patches.
- On Android, run the caller-provided navigation handler before the upstream external-link browser handoff.
- Shell caller permits only Dioxus protocol pages and HTTP(S); do not expand allowed schemes here.
- Preserve first-party bridge cookies and WebSocket auth inside the native WebView.

## Work Guidance
Rebase or remove this fork when upstream exposes equivalent Android navigation behavior. Do not edit generated Kotlin files.

## Verification
Run `cargo test -p shprd-shell`, `cargo check -p shprd-shell --no-default-features --features mobile --target aarch64-linux-android --offline`, then package and test a real APK.

## Child DOX Index
None.
