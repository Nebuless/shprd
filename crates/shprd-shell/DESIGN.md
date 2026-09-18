# SHPRD shell design

Retain existing SHPRD colors from `web/src/styles.css`. Shell hosts one retained React iframe; React owns all visible frontend controls.

- Background #1a1b26, panel #202331, input #11141b, border #2f3549, text #d5dcff, accent #7aa2f7, error #ffb3b3.
- System sans-serif, 14px body and 16px inputs. Spacing 4px multiples; 8px corners.
- Iframe fills `100dvh`. React alone owns scrolling and all visible settings inside iframe.
- React Menu Shell controls have visible labels, 44px minimum touch buttons, focus ring, pending state, live status and error state.
- Narrow view has no horizontal overflow. No animation or decorative assets.
- Parent shell validates exact source/origin protocol packets. React owns host input, confirmation, applied-host status and bridge status. Bridge check never remounts iframe; host changes require explicit draft-loss confirmation.
- Existing React surface owns all terminal/editor/composer DOM and state in its own browsing context. Cross-window traffic is origin- and source-scoped.

Performance scope: no changes to retained React bundle. Native Android keyboard, IME and device performance require device verification.
