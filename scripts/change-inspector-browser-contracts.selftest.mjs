import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";
import vm from "node:vm";
import {
	installBrowserTransportObserver,
	parseBrowserResultLog,
	renderBrowserProgram,
	timelineReadyInPage,
} from "./change-inspector-browser-contracts.mjs";

const passingReport = { schema: "pointbreak.change-inspector-browser-report", version: 1, status: "passed",
	assertionCount: 1, screenshotCount: 0, sectionCount: 1, globalInvalid: false,
	sections: [{ name: "contract", status: "passed", failureCount: 0 }], failures: [] };
const resultLog = (report = passingReport) => `### Result\n${JSON.stringify(report)}\n`;

test("renderer replaces each marker once and preserves the config literal", () => {
	const configLiteral = JSON.stringify({ text: "$& $$ $` $' ` $()\n", nested: { value: true } });
	const source = "(__POINTBREAK_BROWSER_DIAGNOSTIC_FAILURE__);(__POINTBREAK_BROWSER_DIAGNOSTICS__);(__POINTBREAK_CHANGE_BROWSER_CONFIG__)";
	const rendered = renderBrowserProgram({
		source,
		diagnosticFailure: "class Failure {}", diagnostics: "function diagnostics() {}", configLiteral,
	});
	assert.ok(rendered.endsWith(`;(${configLiteral})`));
	assert.throws(() => renderBrowserProgram({ source: "", diagnosticFailure: "x", diagnostics: "y", configLiteral: "{}" }), /occurred 0 times/);
	assert.throws(() => renderBrowserProgram({ source: source.replace("__POINTBREAK_BROWSER_DIAGNOSTICS__", "__POINTBREAK_BROWSER_DIAGNOSTICS____POINTBREAK_BROWSER_DIAGNOSTICS__"), diagnosticFailure: "x", diagnostics: "y", configLiteral: "{}" }), /DIAGNOSTICS__ occurred 2 times/);
});

test("renderer CLI rejects bad arguments and preserves literal bytes", async (t) => {
	const root = await mkdtemp(path.join(os.tmpdir(), "pointbreak-browser-render-"));
	t.after(() => rm(root, { recursive: true, force: true }));
	const template = path.join(root, "template.mjs"); const output = path.join(root, "output.mjs");
	await writeFile(template, "(__POINTBREAK_BROWSER_DIAGNOSTIC_FAILURE__);(__POINTBREAK_BROWSER_DIAGNOSTICS__);(__POINTBREAK_CHANGE_BROWSER_CONFIG__)");
	const cli = fileURLToPath(new URL("./change-inspector-browser-render.mjs", import.meta.url));
	assert.equal(spawnSync(process.execPath, [cli]).status, 2);
	const literal = JSON.stringify({ literal: "$& $$ $` $' $() ` untouched" });
	const diagnostics = fileURLToPath(new URL("./change-inspector-browser-diagnostics.mjs", import.meta.url));
	assert.equal(spawnSync(process.execPath, [cli, template, diagnostics, literal, output]).status, 0);
	assert.ok((await readFile(output, "utf8")).endsWith(`;(${literal})`));
});

test("strict result parser accepts one report and rejects every ambiguous terminal", () => {
	assert.deepEqual(parseBrowserResultLog(resultLog()), passingReport);
	for (const [label, log, exitCode = 0] of [
		["Error", `### Error\nboom\n${resultLog()}`], ["duplicate", resultLog() + resultLog()],
		["malformed", "### Result\n{bad}\n"], ["wrong schema", resultLog({ ...passingReport, schema: "wrong" })],
		["missing", "plain output\n"], ["nonzero", resultLog(), 9],
	]) assert.throws(() => parseBrowserResultLog(log, exitCode), undefined, label);
});

test("result parser CLI preserves failed reports and never writes rejected output", async (t) => {
	const root = await mkdtemp(path.join(os.tmpdir(), "pointbreak-browser-result-"));
	t.after(() => rm(root, { recursive: true, force: true }));
	const log = path.join(root, "browser.log");
	const output = path.join(root, "result.json");
	const cli = fileURLToPath(new URL("./change-inspector-browser-result.mjs", import.meta.url));
	const failedReport = { ...passingReport, status: "failed", failures: [{ message: "expected failure" }] };
	await writeFile(log, resultLog(failedReport));
	assert.equal(spawnSync(process.execPath, [cli, log, output, "0"]).status, 0);
	assert.deepEqual(JSON.parse(await readFile(output, "utf8")), failedReport);
	for (const [label, contents, status] of [
		["malformed", "### Result\n{bad}\n", "0"],
		["Error", `### Error\nboom\n${resultLog(failedReport)}`, "0"],
		["nonzero", resultLog(failedReport), "9"],
	]) {
		await rm(output, { force: true });
		await writeFile(log, contents);
		assert.notEqual(spawnSync(process.execPath, [cli, log, output, status]).status, 0, label);
		await assert.rejects(readFile(output), /ENOENT/, label);
	}
});

function readinessContext({ hash = "#/", mounted = "#/timeline", rows = 1, stamp = "sha256:x", active = 0 } = {}) {
	const list = mounted === null ? null : { dataset: { timelineRoute: mounted } };
	const context = vm.createContext({ location: { hash }, document: {
		querySelector: (selector) => selector === "#timeline" ? list : { textContent: stamp },
		querySelectorAll: () => ({ length: rows }),
	}, __pointbreakBrowserTransportObserver: { snapshot: () => ({ activeCount: active }) } });
	context.globalThis = context; return context;
}

test("serialized Timeline readiness is exact boolean over route, DOM, stamp, and transport", () => {
	const source = timelineReadyInPage.toString();
	for (const [label, options, expected] of [
		["alias", {}, true], ["explicit", { hash: "#/timeline?limit=100", mounted: "#/timeline?limit=100" }, true],
		["missing mount", { mounted: null }, false], ["wrong mount", { mounted: "#/changes" }, false],
		["zero rows", { rows: 0 }, false], ["missing stamp", { stamp: "—" }, false], ["pending body", { active: 1 }, false],
	]) {
		const actual = vm.runInContext(`(${source})()`, readinessContext(options));
		assert.equal(typeof actual, "boolean", label); assert.equal(actual, expected, label); assert.equal(Boolean(actual), expected, label);
	}
});

const deferred = () => { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; };
function installSerializedObserver(scope) {
	const context = vm.createContext(scope);
	context.globalThis = context;
	vm.runInContext(`(${installBrowserTransportObserver.toString()})(globalThis)`, context);
	return context;
}

async function observerCase({ abortAfterBody = false, rejectBody = false }) {
	const body = deferred(); const controller = new AbortController();
	const response = { status: 200, text: () => body.promise };
	const fetchPromise = Promise.resolve(response);
	const scope = installSerializedObserver({ fetch: () => fetchPromise, Request, URL, structuredClone,
		location: { href: "http://example.test/#/", hash: "#/" }, document: {
			querySelector: (selector) => selector === "#timeline" ? { dataset: { timelineRoute: "#/timeline" } } : { textContent: "sha256:x" },
			querySelectorAll: () => ({ length: 1 }),
		} });
	const observedFetch = scope.fetch("http://example.test/api/v2/profile", { signal: controller.signal });
	assert.equal(observedFetch, fetchPromise);
	const observed = await observedFetch;
	assert.equal(observed, response);
	assert.equal(vm.runInContext(`(${timelineReadyInPage.toString()})()`, scope), false);
	const text = observed.text();
	assert.equal(text, body.promise);
	if (rejectBody) { controller.abort("superseded"); body.reject(new Error("body failed")); }
	else { body.resolve("{}"); await text; if (abortAfterBody) controller.abort("late"); }
	await text.catch(() => {}); await Promise.resolve();
	return { snapshot: scope.__pointbreakBrowserTransportObserver.snapshot(), ready: vm.runInContext(`(${timelineReadyInPage.toString()})()`, scope) };
}

test("observer separates pending body, body failure, and post-completion signal", async () => {
	const { snapshot: failed } = await observerCase({ rejectBody: true });
	assert.equal(failed.activeCount, 0); assert.equal(failed.records[0].terminal, "body-reject");
	assert.equal(failed.records[0].signalNotifications.length, 1); assert.equal(failed.records[0].signalObservationOnly, true);
	const { snapshot: completed, ready: completedReady } = await observerCase({ abortAfterBody: true });
	assert.equal(completed.activeCount, 0); assert.equal(completed.records[0].terminal, "response-body");
	assert.equal(completed.records[0].signalNotifications[0].terminalAtNotification, "response-body");
	assert.equal(completed.records[0].requestFailureBinding, "unavailable");
	assert.equal(completedReady, true);
});

test("explicit null init signal does not inherit a Request signal", async () => {
	const body = deferred(); const controller = new AbortController();
	const response = { status: 200, text: () => body.promise };
	const fetchPromise = Promise.resolve(response);
	const scope = installSerializedObserver({ fetch: () => fetchPromise, Request, URL, structuredClone,
		location: { href: "http://example.test/#/" } });
	const request = new Request("http://example.test/api/v2/profile", { signal: controller.signal });
	const observedFetch = scope.fetch(request, { signal: null });
	assert.equal(observedFetch, fetchPromise);
	const observed = await observedFetch;
	const text = observed.text();
	assert.equal(text, body.promise);
	controller.abort("inherited-only");
	assert.equal(scope.__pointbreakBrowserTransportObserver.snapshot().records[0].signalNotifications.length, 0);
	body.resolve("{}"); await text; await Promise.resolve();
	const snapshot = scope.__pointbreakBrowserTransportObserver.snapshot();
	assert.equal(snapshot.activeCount, 0);
	assert.equal(snapshot.records[0].terminal, "response-body");
});
