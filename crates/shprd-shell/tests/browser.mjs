import assert from "node:assert/strict";
import { resolve, sep } from "node:path";
import { mkdir } from "node:fs/promises";
import { installShellBridge } from "../../../web/src/shellBridge.ts";

const root = resolve(import.meta.dirname, "..");
const dist = resolve(
  process.env.SHPRD_SHELL_DIST ??
    root + "/target/dx/shprd-shell/debug/web/public",
);
const reactRoot = process.env.SHPRD_REACT_MODULE_ROOT;
const playwright = process.env.SHPRD_PLAYWRIGHT_MODULE;
const reactDist = process.env.SHPRD_REACT_DIST;
assert.ok(
  reactRoot && playwright && reactDist,
  "Set SHPRD_REACT_MODULE_ROOT, SHPRD_PLAYWRIGHT_MODULE and SHPRD_REACT_DIST to built assets",
);
const { chromium } = await import(playwright);
const evidence = root + "/evidence";
await mkdir(evidence, { recursive: true });

// Given: owning parent and another window. When: packets arrive. Then: only exact provenance replies.
const replies = [];
const parent = { postMessage: (...args) => replies.push(args) };
const owner = new EventTarget();
owner.parent = parent;
const dispose = installShellBridge("https://shell.example", owner);
const packet = {
  protocol: "shprd.shell.v1",
  type: "ping",
  request_id: "test-1",
};
function deliver(origin, source, data = packet) {
  const event = new Event("message");
  Object.assign(event, { origin, source, data });
  owner.dispatchEvent(event);
}
deliver("https://evil.example", parent);
deliver("https://shell.example", {});
deliver("null", parent);
deliver("https://shell.example", parent, { ...packet, extra: true });
assert.equal(replies.length, 0);
deliver("https://shell.example", parent);
assert.deepEqual(replies, [
  [{ ...packet, type: "ack" }, "https://shell.example"],
]);
dispose();
deliver("https://shell.example", parent);
assert.equal(replies.length, 1);

async function staticResponse(base, pathname) {
  const target = resolve(
    base,
    "." + (pathname === "/" ? "/index.html" : pathname),
  );
  if (!target.startsWith(base + sep))
    return new Response("Invalid path", { status: 400 });
  const file = Bun.file(target);
  return (await file.exists())
    ? new Response(file)
    : new Response("Missing", { status: 404 });
}
const bundled = await Bun.build({
  entrypoints: [root + "/tests/react-fixture.jsx"],
  target: "browser",
  plugins: [
    {
      name: "existing-react",
      setup(build) {
        build.onResolve({ filter: /^react(?:\/.*)?$/ }, ({ path }) => ({
          path: resolve(
            reactRoot,
            path === "react" ? "react/index.js" : path + ".js",
          ),
        }));
        build.onResolve({ filter: /^react-dom\/client$/ }, () => ({
          path: resolve(reactRoot, "react-dom/client.js"),
        }));
      },
    },
  ],
});
assert.equal(bundled.success, true, String(bundled.logs));
let shellOrigin;
function serveFixture(port) {
  return Bun.serve({
    hostname: "127.0.0.1",
    port,
    async fetch(request) {
      const path = new URL(request.url).pathname;
      if (path === "/fixture.js") return new Response(bundled.outputs[0]);
      if (path === "/")
        return new Response(
          '<!doctype html><html lang="en"><head><meta name="viewport" content="width=device-width,initial-scale=1"><title>React bridge fixture</title></head><body><div id="root"></div><script>window.shellOrigin=' +
            JSON.stringify(shellOrigin) +
            '</script><script type="module" src="/fixture.js"></script></body></html>',
          { headers: { "content-type": "text/html" } },
        );
      return new Response("Missing", { status: 404 });
    },
  });
}
const defaultFixture = serveFixture(8787);
const fixture = serveFixture(0);
const shell = Bun.serve({
  hostname: "127.0.0.1",
  port: 0,
  fetch(request) {
    return staticResponse(dist, new URL(request.url).pathname);
  },
});
shellOrigin = shell.url.origin;
const fixtureOrigin = fixture.url.origin;
let retained;
const browser = await chromium.launch({
  executablePath: process.env.SHPRD_CHROME,
  headless: true,
});
try {
  const page = await browser.newPage({
    viewport: { width: 1280, height: 900 },
  });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto(shellOrigin);
  assert.equal(await page.locator(".shprd-toolbar").count(), 0);
  assert.equal(await page.locator(".shprd-settings").count(), 0);
  await page.locator("#shprd-react").waitFor();
  const frame = page.frameLocator("#shprd-react");
  await frame.locator("#draft").waitFor();
  const [fixtureFrame] = page
    .frames()
    .filter((candidate) => candidate !== page.mainFrame());
  assert.ok(fixtureFrame, "fixture iframe was not attached");
  assert.deepEqual(
    await fixtureFrame.evaluate(() => window.requestShellCheck()),
    { protocol: "shprd.shell.v1", type: "bridge.checked" },
  );

  let requested;
  let release;
  const navigation = new Promise((resolve) => {
    requested = resolve;
  });
  const ready = new Promise((resolve) => {
    release = resolve;
  });
  await page.route(fixtureOrigin + "/", async (route) => {
    requested();
    await ready;
    await route.continue();
  });
  await frame.locator("#draft").fill("Draft survives shell updates");
  const request = fixtureFrame.evaluate(
    (host) => window.requestShellHost(host),
    fixtureOrigin,
  );
  await navigation;
  await request;
  const navigated = page.waitForEvent(
    "framenavigated",
    (candidate) => candidate === fixtureFrame,
  );
  release();
  await navigated;
  await frame.locator("#draft").waitFor();
  assert.notEqual(
    await frame.locator("#draft").inputValue(),
    "Draft survives shell updates",
  );
  await page.screenshot({ path: evidence + "/shell-desktop.png" });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: evidence + "/shell-mobile-width.png" });
  assert.equal(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
    true,
  );

  // Run retained product bundle, not a rewritten editor or terminal.
  {
    const retainedDist = resolve(reactDist);
    const html = await Bun.file(retainedDist + "/index.html").text();
    retained = Bun.serve({
      hostname: "127.0.0.1",
      port: 0,
      fetch(request) {
        const path = new URL(request.url).pathname;
        if (path === "/")
          return new Response(html, {
            headers: { "content-type": "text/html" },
          });
        return staticResponse(retainedDist, path);
      },
    });
    await page.setViewportSize({ width: 1280, height: 900 });
    await fixtureFrame.evaluate(
      (host) => window.requestShellHost(host),
      retained.url.origin,
    );
    await page
      .frameLocator("#shprd-react")
      .locator("#root > *")
      .first()
      .waitFor();
    const retainedFrame = page.frameLocator("#shprd-react");
    await retainedFrame
      .locator("button.topbar-button.menu-button")
      .first()
      .click();
    await retainedFrame.getByText("Shell", { exact: true }).waitFor();
    await retainedFrame
      .getByRole("button", { name: "Check bridge", exact: true })
      .click();
    await retainedFrame
      .getByText("React bridge connected", { exact: false })
      .waitFor();
    await retainedFrame.getByLabel("Host URL").fill(defaultFixture.url.origin);
    page.once("dialog", (dialog) => dialog.accept());
    await retainedFrame
      .getByRole("button", { name: "Open host", exact: true })
      .click();
    await frame.locator("#draft").waitFor();
    await page.screenshot({ path: evidence + "/retained-react-desktop.png" });
    await page.setViewportSize({ width: 390, height: 844 });
    await page.screenshot({
      path: evidence + "/retained-react-mobile-width.png",
    });
    assert.equal(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
      true,
    );
  }
  assert.deepEqual(errors, []);
  console.log(
    "SHELL_BROWSER_PASS: scoped round trip, cleanup, draft retention, host cancellation, validation, desktop/mobile, retained React=" +
      Boolean(retained),
  );
} finally {
  await browser.close();
  shell.stop(true);
  fixture.stop(true);
  defaultFixture.stop(true);
  retained?.stop(true);
}
