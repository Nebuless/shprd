import { describe, expect, test } from "bun:test";
import {
  assertBumpIsIncremental,
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

  test("allows only the next version for each SemVer release class", () => {
    expect(() => assertBumpIsIncremental("0.8.0", "0.8.1")).not.toThrow();
    expect(() => assertBumpIsIncremental("0.8.0", "0.9.0")).not.toThrow();
    expect(() => assertBumpIsIncremental("0.8.0", "1.0.0")).not.toThrow();
  });

  test("rejects skipped release versions", () => {
    expect(() => assertBumpIsIncremental("0.8.0", "0.8.2")).toThrow(
      "next patch version 0.8.1",
    );
    expect(() => assertBumpIsIncremental("0.8.0", "0.9.1")).toThrow(
      "next minor version 0.9.0",
    );
    expect(() => assertBumpIsIncremental("0.8.0", "1.0.1")).toThrow(
      "next major version 1.0.0",
    );
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
