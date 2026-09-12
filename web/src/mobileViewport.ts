export function mobileViewportGeometry(input: {
  readonly layoutHeight: number;
  readonly viewportHeight: number;
  readonly offsetTop: number;
  readonly viewportScale: number;
  readonly baselineHeight: number;
  readonly inputFocused: boolean;
}) {
  // Pinch zoom changes the visual viewport without opening a keyboard.
  if (Math.abs(input.viewportScale - 1) > 0.01) {
    return {
      appHeight: input.layoutHeight,
      offsetTop: 0,
      keyboardInset: 0,
      keyboardOpen: false,
    };
  }
  const offsetTop = Math.max(0, input.offsetTop);
  const occlusion = Math.max(
    0,
    input.layoutHeight - input.viewportHeight - offsetTop,
  );
  const keyboardOpen =
    input.inputFocused &&
    Math.max(occlusion, input.baselineHeight - input.viewportHeight) > 120;
  const keyboardInset = keyboardOpen ? occlusion : 0;
  return {
    appHeight: input.viewportHeight + keyboardInset,
    offsetTop,
    keyboardInset,
    keyboardOpen,
  };
}
