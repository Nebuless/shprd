# SHPRD shell design

Retain existing Herdr Studio colors from `web/src/styles.css`. Shell is a connection toolbar, not a replacement terminal or editor.

- Background #1a1b26, panel #202331, input #11141b, border #2f3549, text #d5dcff, accent #7aa2f7, error #ffb3b3.
- System sans-serif, 14px body and 16px inputs. Spacing 4px multiples; 8px corners.
- Bounded scroll-body shell: toolbar stays in document flow; iframe fills remaining `100dvh`. React alone owns scrolling inside iframe.
- Form labels remain visible. Buttons have 44px minimum touch height, visible focus ring, disabled pending state. Status uses polite live region; errors use alert.
- Narrow view wraps controls; no horizontal overflow. No animation or decorative assets.
- Rust owns host input, validation, applied host and bridge status. Editing settings and testing bridge never remount iframe. Changing host requires explicit draft-loss confirmation.
- Existing React surface owns all terminal/editor/composer DOM and state in its own browsing context. Cross-window traffic is origin- and source-scoped.

Performance scope: no changes to retained React bundle. Native Android keyboard, IME and device performance require device verification.
