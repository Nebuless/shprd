import { describe, expect, test } from "bun:test";
import { absoluteApiUrl, reloadApplicationPage } from "./ConfigMenu";

describe("application menu actions", () => {
  test("resolves browser-relative API routes against the page origin", () => {
    expect(
      absoluteApiUrl(
        "/api/connections/default/herdr-info",
        "/",
        "https://cax.tailc3b28.ts.net",
      ).href,
    ).toBe("https://cax.tailc3b28.ts.net/api/connections/default/herdr-info");
  });

  test("reloads the current page", () => {
    let reloadCount = 0;

    reloadApplicationPage({
      reload() {
        reloadCount += 1;
      },
    });

    expect(reloadCount).toBe(1);
  });
});
