export default {
  extends: ["@commitlint/config-conventional"],
  ignores: [
    (message) =>
      /^Release \d+\.\d+\.\d+(?: \(#\d+\))?(?:\n\nCo-authored-by: [^\n]+)?\n?$/.test(
        message,
      ),
  ],
};
