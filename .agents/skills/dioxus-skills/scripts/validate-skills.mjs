#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { existsSync, statSync } from "node:fs";
import { readdir, readFile } from "node:fs/promises";
import path from "node:path";
import { pathToFileURL } from "node:url";

const LINK_PATTERN = /\[[^\]]*\]\(([^)]+)\)/g;

function parseArguments(argv) {
  const options = { root: process.cwd(), maxLines: 500, maxTokens: 5000 };
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!value) throw new Error(`missing value for ${flag}`);
    if (flag === "--root") options.root = path.resolve(value);
    else if (flag === "--max-lines") options.maxLines = Number(value);
    else if (flag === "--max-tokens") options.maxTokens = Number(value);
    else throw new Error(`unknown option: ${flag}`);
  }
  if (!Number.isInteger(options.maxLines) || options.maxLines < 1) throw new Error("max lines must be a positive integer");
  if (!Number.isInteger(options.maxTokens) || options.maxTokens < 1) throw new Error("max tokens must be a positive integer");
  return options;
}

function runSkillsRef(command, directory) {
  return spawnSync("mise", ["exec", "--", "skills-ref", command, directory], {
    cwd: process.cwd(),
    encoding: "utf8",
  });
}

function normativeProblems(directory) {
  const result = runSkillsRef("validate", directory);
  if (result.status === 0) return [];
  const output = `${result.stdout}${result.stderr}`;
  const problems = [...output.matchAll(/^\s*-\s+(.+)$/gm)].map((match) => match[1]);
  return problems.length ? problems : [output.trim() || `skills-ref exited ${result.status}`];
}

export function readProperties(directory) {
  const result = runSkillsRef("read-properties", directory);
  if (result.status !== 0) throw new Error(`${result.stdout}${result.stderr}`.trim());
  return JSON.parse(result.stdout);
}

function markdownBody(source) {
  const lines = source.replaceAll("\r\n", "\n").split("\n");
  const closing = lines.indexOf("---", 1);
  return { body: lines.slice(closing + 1).join("\n"), lines };
}

export async function skillDirectories(root) {
  const directories = [root];
  const skillsRoot = path.join(root, "skills");
  if (!existsSync(skillsRoot)) return directories;
  for (const entry of await readdir(skillsRoot, { withFileTypes: true })) {
    if (entry.isDirectory()) directories.push(path.join(skillsRoot, entry.name));
  }
  return directories;
}

function validateLinks(skill, directory, localErrors) {
  for (const match of skill.body.matchAll(LINK_PATTERN)) {
    const target = match[1].split(/[?#]/, 1)[0];
    if (!target || /^(?:[a-z]+:|#|\/)/i.test(target)) continue;
    const resolved = path.resolve(directory, decodeURIComponent(target));
    if (!resolved.startsWith(`${directory}${path.sep}`) || !existsSync(resolved)) {
      localErrors.push(`${path.relative(skill.root, directory) || "."}: broken relative reference: ${match[1]}`);
    }
  }
}

function validateStyle(skill, directory, options, styleErrors) {
  const relative = path.relative(skill.root, directory) || ".";
  if (skill.lines.length > options.maxLines) styleErrors.push(`${relative}: SKILL.md exceeds ${options.maxLines} lines`);
  const tokens = skill.body.match(/\S+/g)?.length ?? 0;
  if (tokens > options.maxTokens) styleErrors.push(`${relative}: SKILL.md exceeds ${options.maxTokens} estimated tokens`);
  const invocation = skill.properties.metadata?.invocation;
  if (invocation !== "model" && invocation !== "user") styleErrors.push(`${relative}: metadata.invocation must be model or user`);
  if (invocation === "model" && !/\bUse when\b/i.test(skill.properties.description)) {
    styleErrors.push(`${relative}: model-invoked description must state when to use it`);
  }
}

async function validate(options) {
  if (!existsSync(options.root) || !statSync(options.root).isDirectory()) throw new Error(`root is not a directory: ${options.root}`);
  const normativeErrors = [];
  const localErrors = [];
  const styleErrors = [];
  const names = new Map();
  const catalog = [];
  let count = 0;
  for (const directory of await skillDirectories(options.root)) {
    const relative = path.relative(options.root, directory) || ".";
    const problems = normativeProblems(directory);
    normativeErrors.push(...problems.map((problem) => `${relative}: ${problem}`));
    if (problems.length) continue;
    const skillPath = path.join(directory, "SKILL.md");
    const properties = readProperties(directory);
    const parsed = markdownBody(await readFile(skillPath, "utf8"));
    const skill = { ...parsed, properties, root: options.root };
    count += 1;
    names.set(properties.name, [...(names.get(properties.name) ?? []), relative]);
    catalog.push({
      name: properties.name,
      path: path.relative(options.root, skillPath).split(path.sep).join("/"),
      description: properties.description,
      invocation: properties.metadata?.invocation,
    });
    validateLinks(skill, directory, localErrors);
    validateStyle(skill, directory, options, styleErrors);
  }
  for (const [name, locations] of names) {
    if (locations.length > 1) localErrors.push(`duplicate skill name ${name}: ${locations.join(", ")}`);
  }
  const catalogPath = path.join(options.root, "generated", "skills.json");
  if (existsSync(catalogPath)) {
    try {
      const current = JSON.parse(await readFile(catalogPath, "utf8"));
      const expected = { version: 1, skills: catalog.sort((left, right) => left.name < right.name ? -1 : left.name > right.name ? 1 : 0) };
      if (JSON.stringify(current) !== JSON.stringify(expected)) localErrors.push("generated index is stale or malformed");
    } catch (error) {
      localErrors.push(`generated index is stale or malformed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }
  return { normativeErrors, localErrors, styleErrors, count };
}

async function main() {
  try {
    const options = parseArguments(process.argv.slice(2));
    const result = await validate(options);
    if (result.normativeErrors.length || result.localErrors.length || result.styleErrors.length) {
      for (const error of result.normativeErrors) console.error(`[normative] ${error}`);
      for (const error of result.localErrors) console.error(`[local] ${error}`);
      for (const error of result.styleErrors) console.error(`[style] ${error}`);
      return 1;
    }
    console.log(`normative ok: ${result.count} skills`);
    console.log(`local ok: ${result.count} skills`);
    console.log(`style ok: ${result.count} skills (${options.maxLines} lines, ${options.maxTokens} estimated tokens max)`);
    return 0;
  } catch (error) {
    console.error(`validator error: ${error instanceof Error ? error.message : String(error)}`);
    return 2;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) process.exitCode = await main();
