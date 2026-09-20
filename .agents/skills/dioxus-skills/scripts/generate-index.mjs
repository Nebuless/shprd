import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { readProperties, skillDirectories } from "./validate-skills.mjs";

const args = process.argv.slice(2);
if (args.length !== 0 && (args.length !== 2 || args[0] !== "--root")) {
  console.error("usage: node scripts/generate-index.mjs [--root PATH]");
  process.exit(2);
}
const root = path.resolve(args[1] ?? ".");
const skills = [];
for (const directory of await skillDirectories(root)) {
  const properties = readProperties(directory);
  skills.push({
    name: properties.name,
    path: path.relative(root, path.join(directory, "SKILL.md")).split(path.sep).join("/"),
    description: properties.description,
    invocation: properties.metadata?.invocation,
  });
}
skills.sort((left, right) => left.name < right.name ? -1 : left.name > right.name ? 1 : 0);
if (new Set(skills.map((skill) => skill.name)).size !== skills.length) throw new Error("duplicate skill names");
await mkdir(path.join(root, "generated"), { recursive: true });
await writeFile(path.join(root, "generated/skills.json"), `${JSON.stringify({ version: 1, skills }, null, 2)}\n`);
console.log(`generated index: ${skills.length} skills`);
