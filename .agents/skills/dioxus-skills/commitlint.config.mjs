export default {
  parserPreset: {
    parserOpts: {
      headerPattern: /^(\w+)(?:\(([a-z0-9][a-z0-9-]*)\))?(!)?: (.+)$/,
      headerCorrespondence: ["type", "scope", "breaking", "subject"],
    },
  },
  rules: {
    "body-leading-blank": [2, "always"],
    "footer-leading-blank": [2, "always"],
    "header-max-length": [2, "always", 100],
    "scope-case": [2, "always", "kebab-case"],
    "subject-empty": [2, "never"],
    "subject-full-stop": [2, "never", "."],
    "type-case": [2, "always", "lower-case"],
    "type-empty": [2, "never"],
    "type-enum": [2, "always", ["build", "chore", "ci", "docs", "feat", "fix", "perf", "refactor", "revert", "style", "test"]],
  },
};
