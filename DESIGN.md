# SHPRD interface

## 1. Atmosphere & Identity
Preserve existing dense development workspace. Persistent terminal, workspace
tree, files and changes remain available beside structured agent conversations.
Panels use existing theme and accent selection rather than a separate chat theme.

## 2. Color
Canonical values live in web/src/styles.css. Use --bg for canvas, --panel and
--panel-2 for surfaces, --input-bg for editors, --border for divisions, --text
and --muted for text, --accent and --accent-soft for selection. Use --green
for connected state, --danger-text and --danger-soft for errors. Light mode and
user accent overrides apply to every added surface.

## 3. Typography
Inherit application font. Dense controls use 13px; metadata uses 11-12px;
section titles use 15px/600; empty-state headings use 18px/500. Conversation
Markdown uses 14px with 1.65 line height. Code remains monospace.

## 4. Spacing & Layout
Use existing 4px spacing steps: 8px control gaps, 12px compact padding,
16px side padding, 24px conversation padding. Workspace remains bounded by
application viewport. Session list and conversation each own their scrolling;
composer and toolbar remain outside message scroll. Flex/grid children need
min-width:0 and min-height:0. Agent rail becomes horizontal below 650px.
Terminal wrappers fill the remaining viewport even before agent chat loads.
Mobile keyboard state requires editable focus and meaningful viewport occlusion;
browser chrome, hardware-keyboard focus and pinch zoom do not reserve keyboard
space. The composer stays in flow, with floating controls above its measured
height; shortcut grids shrink within their available width.

## 5. Components
- Topbar controls: native buttons with icon, accessible name, hover, pressed,
  focus and disabled states. Chat toggle preserves mounted terminal.
- Session selector: button rows with name, path, variant and state; selected
  row has accent border and fill. Empty and disconnected states remain visible.
- Conversation: existing MarkdownPreview sanitizer, disclosure for tools and
  reasoning, explicit native-terminal handoff for unsupported content.
- Composer: labeled textarea, model selector, submit and stop; offline/error
  states retain draft. Stop acknowledgement does not imply runtime settlement.
- Inspector: existing files/history/changes primitives retain ownership.

## 6. Motion & Interaction
No decorative animation. Follow new messages only while reader is at bottom.
Keep keyboard focus visible; preserve Ctrl/Command+Enter and IME composition.
Respect existing reduced-motion styles and native disclosure behavior.

## 7. Depth & Surface
Use tonal surfaces and 1px token borders. Existing modal/tooltip shadows stay
on overlays; ordinary conversation panels add none. Control radii use existing
4px, 6px and 8px scale.

## 8. Accessibility Constraints & Accepted Debt
All controls need labels and keyboard access. Status uses text, not color alone.
Long paths truncate with titles; message content wraps without page overflow.
No new accepted accessibility debt. Verify narrow and wide layouts with real
rendered content, empty state, disconnect, long labels and tool details.
