import { expect, test } from "bun:test";

const styles = await Bun.file(`${import.meta.dir}/styles.css`).text();

test("compact composer keeps utilities and primary actions in full-width rows", () => {
  expect(styles).toContain("@media (max-width: 375px)");
  expect(styles).toMatch(
    /@media \(max-width: 375px\) \{[\s\S]*?\.terminal-composer-actions \{[\s\S]*?display: grid;[\s\S]*?grid-template-columns: repeat\(6, minmax\(0, 1fr\)\);/,
  );
  expect(styles).toMatch(
    /@media \(max-width: 375px\) \{[\s\S]*?\.terminal-composer-submit \{[\s\S]*?grid-column: span 3;[\s\S]*?min-width: 0;/,
  );
  expect(styles).toMatch(
    /@media \(max-width: 375px\) \{[\s\S]*?\.terminal-composer-paste-image \{[\s\S]*?grid-column: span 2;[\s\S]*?min-width: 0;/,
  );
});
