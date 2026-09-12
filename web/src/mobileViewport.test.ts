import { expect, test } from "bun:test";
import { mobileViewportGeometry } from "./mobileViewport";

const closed = {
  layoutHeight: 820,
  viewportHeight: 820,
  offsetTop: 0,
  viewportScale: 1,
  baselineHeight: 820,
  inputFocused: false,
};

test("keyboard closed uses full height and ignores browser chrome", () => {
  expect(mobileViewportGeometry(closed)).toEqual({
    appHeight: 820,
    offsetTop: 0,
    keyboardInset: 0,
    keyboardOpen: false,
  });
  expect(
    mobileViewportGeometry({
      ...closed,
      viewportHeight: 770,
      inputFocused: true,
    }),
  ).toEqual({
    appHeight: 770,
    offsetTop: 0,
    keyboardInset: 0,
    keyboardOpen: false,
  });
});

test("visual viewport keyboard lifts content exactly once and clears after dismissal", () => {
  expect(
    mobileViewportGeometry({
      ...closed,
      viewportHeight: 500,
      inputFocused: true,
    }),
  ).toEqual({
    appHeight: 820,
    offsetTop: 0,
    keyboardInset: 320,
    keyboardOpen: true,
  });
  expect(
    mobileViewportGeometry({ ...closed, inputFocused: true }).keyboardOpen,
  ).toBe(false);
  expect(
    mobileViewportGeometry({ ...closed, viewportHeight: 500 }).keyboardInset,
  ).toBe(0);
});

test("resized layout keyboard is detected without double subtracting its height", () => {
  expect(
    mobileViewportGeometry({
      ...closed,
      layoutHeight: 500,
      viewportHeight: 500,
      inputFocused: true,
    }),
  ).toEqual({
    appHeight: 500,
    offsetTop: 0,
    keyboardInset: 0,
    keyboardOpen: true,
  });
});

test("pinch zoom never mimics keyboard and viewport panning is counted once", () => {
  expect(
    mobileViewportGeometry({
      ...closed,
      viewportHeight: 410,
      viewportScale: 2,
      inputFocused: true,
    }),
  ).toEqual({
    appHeight: 820,
    offsetTop: 0,
    keyboardInset: 0,
    keyboardOpen: false,
  });
  expect(
    mobileViewportGeometry({
      ...closed,
      viewportHeight: 500,
      offsetTop: 40,
      inputFocused: true,
    }),
  ).toEqual({
    appHeight: 780,
    offsetTop: 40,
    keyboardInset: 280,
    keyboardOpen: true,
  });
});
