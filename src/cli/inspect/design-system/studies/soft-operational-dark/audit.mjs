import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  auditTheme,
  declarations,
  themeBlocks,
} from "../../contrast-check.mjs";

const studyDirectory = path.dirname(fileURLToPath(import.meta.url));
const designSystemDirectory = path.resolve(studyDirectory, "../..");
const liveTokenPath = path.resolve(designSystemDirectory, "../assets/tokens.css");
const studyTokenPath = path.resolve(studyDirectory, "tokens.css");

const allowedTokens = new Set([
  "--bg",
  "--bg-elev",
  "--bg-row",
  "--bg-row-sel",
  "--bg-topbar",
  "--sel-bg",
  "--bg-code",
  "--border",
  "--fg",
  "--fg-dim",
]);

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function studyOverrides(css) {
  const withoutComments = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const block = withoutComments.match(
    /\[data-theme\s*=\s*["']dark["']\]\[data-tone\s*=\s*["']soft-operational["']\]\s*\{([\s\S]*?)\}/i,
  );
  assert(block, `${studyTokenPath}: missing soft operational dark block`);

  const overrides = declarations(block[1], "soft operational dark study");
  for (const token of overrides.keys()) {
    assert(
      allowedTokens.has(token),
      `study must not override held-constant token ${token}`,
    );
  }
  for (const token of allowedTokens) {
    assert(overrides.has(token), `study must declare ${token}`);
  }
  return overrides;
}

async function main() {
  const liveThemes = themeBlocks(await readFile(liveTokenPath, "utf8"));
  const overrides = studyOverrides(await readFile(studyTokenPath, "utf8"));
  const candidate = new Map([...liveThemes.get("dark"), ...overrides]);
  const results = auditTheme("dark", candidate);
  const failures = results.filter((result) => !result.passed);

  console.log("treatment             | ratio | check");
  console.log("----------------------|-------|------");
  for (const result of results) {
    console.log(
      `soft-operational-dark | ${result.ratio.toFixed(2).padStart(5)} | ${result.label}`,
    );
  }
  console.log(`\n${results.length} gating checks; ${failures.length} failures.`);
  if (failures.length > 0) process.exitCode = 1;
}

main().catch((error) => {
  console.error(`Soft operational dark audit failed: ${error.message}`);
  process.exitCode = 1;
});
