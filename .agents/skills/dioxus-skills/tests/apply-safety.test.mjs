import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import test from "node:test";

test("guarded apply safety matrix through real Git fixtures", () => {
  const result = spawnSync("python", ["-m", "unittest", "tests.test_apply.ApplyTest"], {
    cwd: new URL("..", import.meta.url),
    encoding: "utf8",
  });
  assert.equal(result.status, 0, result.stdout + result.stderr);
});
