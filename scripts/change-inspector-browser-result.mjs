#!/usr/bin/env node
import fs from "node:fs";
import { parseBrowserResultLog } from "./change-inspector-browser-contracts.mjs";

const args = process.argv.slice(2);
if (args.length !== 3) {
	console.error("usage: change-inspector-browser-result.mjs <browser-log> <output-json> <exit-code>");
	process.exit(2);
}
const [logPath, outputPath, rawExitCode] = args;
const exitCode = Number(rawExitCode);
const report = parseBrowserResultLog(fs.readFileSync(logPath, "utf8"), exitCode);
fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`);
