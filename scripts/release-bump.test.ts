import { describe, expect, test } from "bun:test";
import {
  assertBumpMeetsRecommendation,
  classifyVersionBump,
  parseRecommendedReleaseBump,
} from "./release-bump";

describe("release bump validation", () => {
  test("classifies valid semantic version increases", () => {
    expect(classifyVersionBump("0.6.2", "0.6.3")).toBe("patch");
    expect(classifyVersionBump("0.6.2", "0.7.0")).toBe("minor");
    expect(classifyVersionBump("0.6.2", "1.0.0")).toBe("major");
  });

  test("rejects version decreases and non-semver versions", () => {
    expect(() => classifyVersionBump("0.6.2", "0.6.2")).toThrow(
      "must be greater",
    );
    expect(() => classifyVersionBump("0.6.2", "next")).toThrow(
      "must be in X.Y.Z form",
    );
  });

  test("allows maintainers to choose a larger recommended bump", () => {
    expect(() =>
      assertBumpMeetsRecommendation("0.6.2", "0.7.0", "patch"),
    ).not.toThrow();
    expect(() =>
      assertBumpMeetsRecommendation("0.6.2", "1.0.0", "minor"),
    ).not.toThrow();
  });

  test("rejects a release version smaller than the recommendation", () => {
    expect(() =>
      assertBumpMeetsRecommendation("0.6.2", "0.6.3", "minor"),
    ).toThrow("requires at least a minor release");
    expect(() =>
      assertBumpMeetsRecommendation("0.6.2", "0.7.0", "major"),
    ).toThrow("requires at least a major release");
  });

  test("parses Conventional Changelog recommendations", () => {
    expect(parseRecommendedReleaseBump("minor\n")).toBe("minor");
    expect(parseRecommendedReleaseBump("\n")).toBeNull();
  });

  test("rejects an invalid Conventional Changelog recommendation", () => {
    expect(() => parseRecommendedReleaseBump("release\n")).toThrow(
      "invalid recommendation",
    );
  });
});
