#!/usr/bin/env node
import fs from "node:fs";
import { pathToFileURL } from "node:url";
import { renderBrowserProgram } from "./change-inspector-browser-contracts.mjs";

const args = process.argv.slice(2);
if (args.length !== 4) {
	console.error("usage: change-inspector-browser-render.mjs <template> <diagnostics-module> <config-literal> <output>");
	process.exit(2);
}
const [templatePath, diagnosticsPath, configLiteral, outputPath] = args;
const diagnostics = await import(pathToFileURL(diagnosticsPath));
const rendered = renderBrowserProgram({
	source: fs.readFileSync(templatePath, "utf8"),
	diagnosticFailure: diagnostics.BrowserDiagnosticFailure.toString(),
	diagnostics: diagnostics.createBrowserDiagnostics.toString(),
	configLiteral,
});
fs.writeFileSync(outputPath, rendered);
