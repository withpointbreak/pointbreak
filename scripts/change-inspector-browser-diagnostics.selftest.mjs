import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
	BrowserDiagnosticFailure,
	createBrowserDiagnostics,
} from "./change-inspector-browser-diagnostics.mjs";
import { publishPassingManifest } from "./change-inspector-browser-manifest.mjs";

const context = () => ({
	route: "http://127.0.0.1:4173/#/changes",
	viewport: { width: 1440, height: 1000 },
	screenshot: "screenshots/wide-changes.png",
	log: "logs/browser-gate.log",
});

test("browser program remains one expression for the Playwright runner", async () => {
	let source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	for (const [marker, replacement] of [
		[
			"__POINTBREAK_BROWSER_DIAGNOSTIC_FAILURE__",
			BrowserDiagnosticFailure.toString(),
		],
		["__POINTBREAK_BROWSER_DIAGNOSTICS__", createBrowserDiagnostics.toString()],
		["__POINTBREAK_CHANGE_BROWSER_CONFIG__", "{}"],
	]) {
		assert.ok(source.includes(marker), `missing browser marker ${marker}`);
		source = source.replace(marker, replacement);
	}

	assert.doesNotThrow(
		() => new Function(`return (${source}\n)`),
		"playwright-cli run-code parses the file as one function expression",
	);
});

test("empty ready L2 recovery is explicit, authenticated, and retained before browser readiness", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.sh", import.meta.url),
		"utf8",
	);
	const retry = source.indexOf("retry_empty_ready_l2() {");
	const emptyStart = source.indexOf(
		'start_reader_state_server "empty-ready-l2" "$reader_empty_l2_repo"',
	);
	const retryCall = source.indexOf("retry_empty_ready_l2", emptyStart);
	const l0Start = source.indexOf(
		'start_reader_state_server "l0" "$reader_l0_repo"',
	);
	assert.ok(retry >= 0, "missing empty-ready-l2 retry helper");
	assert.ok(
		emptyStart >= 0 && retryCall > emptyStart && l0Start > retryCall,
		"empty-ready-l2 must recover before another reader fixture starts",
	);
	const helper = source.slice(retry, emptyStart);
	assert.match(
		helper,
		/-X POST[\s\\]+-H "Authorization: Bearer \$token"[\s\\]+"\$base_url\/api\/derived-access\/retry"/,
	);
	assert.match(helper, /browser-empty-ready-l2-retry\.json/);
	assert.match(helper, /browser-empty-ready-l2-ready\.json/);
	assert.match(
		source,
		/logs\/browser-empty-ready-l2-retry\.json[\s\\]+logs\/browser-empty-ready-l2-ready\.json/,
		"completion must require both retained recovery records",
	);
	assert.match(
		helper,
		/servingCurrent == true[\s\S]*availability == "current"[\s\S]*rebuildInFlight == false/,
		"the ready record must wait for the recovered derived generation",
	);
});

test("reports failures from independently recoverable sections together", async () => {
	const diagnostics = createBrowserDiagnostics({ context });

	await diagnostics.section("Timeline", async () => {
		diagnostics.expect(
			false,
			"Timeline chronology",
			"newest-first label was absent",
			{
				expected: "Newest first",
				actual: "Oldest first",
			},
		);
	});
	await diagnostics.section("Changes", async () => {
		diagnostics.expect(false, "Change ordering", "card order drifted", {
			expected: ["change:a", "change:b"],
			actual: ["change:b", "change:a"],
		});
	});

	const report = diagnostics.report();
	assert.equal(report.failures.length, 2);
	assert.deepEqual(
		report.failures.map((failure) => failure.section),
		["Timeline", "Changes"],
	);
	assert.deepEqual(report.failures[0], {
		kind: "assertion",
		section: "Timeline",
		label: "Timeline chronology",
		detail: "newest-first label was absent",
		expected: "Newest first",
		actual: "Oldest first",
		route: context().route,
		viewport: context().viewport,
		screenshot: context().screenshot,
		log: context().log,
	});
	assert.match(report.text, /Timeline.*Timeline chronology/s);
	assert.match(report.text, /Changes.*Change ordering/s);
});

test("production-style checks retain semantic expected and observed values", async () => {
	const diagnostics = createBrowserDiagnostics({ context });

	await diagnostics.section("Timeline", async () => {
		diagnostics.expect(
			false,
			"bounded Timeline window",
			"expected 1-79 live events, observed 0",
		);
	});

	const [failure] = diagnostics.report().failures;
	assert.deepEqual(failure.expected, {
		condition: "bounded Timeline window",
		outcome: "satisfied",
	});
	assert.deepEqual(failure.actual, {
		condition: "bounded Timeline window",
		outcome: "failed",
		detail: "expected 1-79 live events, observed 0",
	});
});

test("explicit null and undefined comparison values remain diagnostic evidence", async () => {
	const diagnostics = createBrowserDiagnostics({ context });

	await diagnostics.section("Exact Revision", async () => {
		diagnostics.expect(false, "exact route", "exact identity was absent", {
			expected: null,
			actual: undefined,
		});
	});

	const [failure] = diagnostics.report().failures;
	assert.equal(failure.expected, null);
	assert.deepEqual(failure.actual, { valueType: "undefined" });
	assert.deepEqual(JSON.parse(JSON.stringify(failure)), failure);
});

test("non-finite numeric observations remain diagnostic evidence on the JSON wire", async () => {
	const diagnostics = createBrowserDiagnostics({ context });

	await diagnostics.section("Timeline", async () => {
		diagnostics.expect(false, "recorded event count", "count was unavailable", {
			expected: Number.POSITIVE_INFINITY,
			actual: {
				metrics: {
					svgWidth: Number.NaN,
					heights: [Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY],
				},
				optional: undefined,
			},
		});
	});

	const [failure] = diagnostics.report().failures;
	assert.deepEqual(failure.expected, {
		valueType: "number",
		value: "Infinity",
	});
	assert.deepEqual(failure.actual, {
		metrics: {
			svgWidth: { valueType: "number", value: "NaN" },
			heights: [
				{ valueType: "number", value: "Infinity" },
				{ valueType: "number", value: "-Infinity" },
			],
		},
		optional: { valueType: "undefined" },
	});
	assert.deepEqual(JSON.parse(JSON.stringify(failure)), failure);
});

test("stops only an invalid section and continues later independent setup", async () => {
	const diagnostics = createBrowserDiagnostics({ context });
	const visited = [];

	await diagnostics.section("Timeline", {
		setup: async () => {
			visited.push("Timeline setup");
			throw new Error("exact event transition never settled");
		},
		run: async () => {
			visited.push("Timeline body");
		},
	});
	await diagnostics.section("Attention", {
		setup: async () => {
			visited.push("Attention setup");
		},
		run: async () => {
			visited.push("Attention body");
			diagnostics.expect(false, "Attention guidance", "reason was absent");
		},
	});

	assert.deepEqual(visited, [
		"Timeline setup",
		"Attention setup",
		"Attention body",
	]);
	const report = diagnostics.report();
	assert.deepEqual(report.sections, [
		{ name: "Timeline", status: "stopped", failureCount: 1 },
		{ name: "Attention", status: "failed", failureCount: 1 },
	]);
	assert.equal(report.failures[0].kind, "section");
	assert.match(
		report.failures[0].detail,
		/exact event transition never settled/,
	);
});

test("a failed prerequisite stops its section without duplicating diagnostics", async () => {
	const diagnostics = createBrowserDiagnostics({ context });
	const visited = [];

	await diagnostics.section("Exact Revision", async () => {
		diagnostics.requireCondition(
			false,
			"exact Revision authority",
			"exact Change membership was absent",
			{ expected: "one exact member", actual: null },
		);
		visited.push("dependent transition");
	});
	await diagnostics.section("Timeline", async () => {
		visited.push("independent section");
	});

	const report = diagnostics.report();
	assert.deepEqual(visited, ["independent section"]);
	assert.equal(report.failures.length, 1);
	assert.deepEqual(
		{
			kind: report.failures[0].kind,
			label: report.failures[0].label,
			expected: report.failures[0].expected,
			actual: report.failures[0].actual,
		},
		{
			kind: "assertion",
			label: "exact Revision authority",
			expected: "one exact member",
			actual: null,
		},
	);
	assert.deepEqual(report.sections, [
		{ name: "Exact Revision", status: "stopped", failureCount: 1 },
		{ name: "Timeline", status: "passed", failureCount: 0 },
	]);
});

test("globally invalid browser state skips every later section", async () => {
	const diagnostics = createBrowserDiagnostics({ context });
	let laterSetupRan = false;

	await diagnostics.section("Timeline", async () => {
		diagnostics.abort("primary Inspector connection was lost", {
			expected: "authenticated primary Inspector remains available",
			actual: "net::ERR_CONNECTION_REFUSED",
		});
	});
	await diagnostics.section("Changes", {
		setup: async () => {
			laterSetupRan = true;
		},
		run: async () => {},
	});

	const report = diagnostics.report();
	assert.equal(laterSetupRan, false);
	assert.equal(report.globalInvalid, true);
	assert.equal(report.failures[0].kind, "fatal");
	assert.deepEqual(report.sections, [
		{ name: "Timeline", status: "stopped", failureCount: 1 },
		{ name: "Changes", status: "skipped", failureCount: 0 },
	]);
});

test("refuses successful completion whenever any assertion failed", async () => {
	const diagnostics = createBrowserDiagnostics({ context });
	let completionPublished = false;

	await diagnostics.section("Changes", async () => {
		diagnostics.expect(false, "bounded cards", "saw 101 cards", {
			expected: "at most 100",
			actual: 101,
		});
	});

	assert.throws(
		() => {
			const completion = diagnostics.complete({ screenshotCount: 7 });
			completionPublished = completion.status === "passed";
		},
		(error) => {
			assert.ok(error instanceof BrowserDiagnosticFailure);
			assert.equal(error.report.failures.length, 1);
			assert.match(error.message, /bounded cards/);
			return true;
		},
	);
	assert.equal(completionPublished, false);
});

test("returns a serializable failed result while completion remains green-only", async () => {
	const diagnostics = createBrowserDiagnostics({ context });

	await diagnostics.section("Timeline", async () => {
		diagnostics.expect(
			false,
			"chronological order",
			"newest event was not first",
			{
				expected: "newest first",
				actual: "oldest first",
			},
		);
	});
	await diagnostics.section("Changes", async () => {
		diagnostics.expect(true, "change cards", "change cards were absent");
	});

	const result = diagnostics.result({ screenshotCount: 4 });
	assert.deepEqual(result, {
		schema: "pointbreak.change-inspector-browser-report",
		version: 1,
		status: "failed",
		assertionCount: 2,
		screenshotCount: 4,
		sectionCount: 2,
		globalInvalid: false,
		sections: [
			{ name: "Timeline", status: "failed", failureCount: 1 },
			{ name: "Changes", status: "passed", failureCount: 0 },
		],
		failures: diagnostics.report().failures,
	});
	assert.deepEqual(JSON.parse(JSON.stringify(result)), result);
	assert.throws(
		() => diagnostics.complete({ screenshotCount: 4 }),
		BrowserDiagnosticFailure,
	);
});

test("returns a compact passing completion only after every section succeeds", async () => {
	const diagnostics = createBrowserDiagnostics({ context });

	await diagnostics.section("Reader readiness", async () => {
		diagnostics.expect(true, "ready L2", "reader did not start");
	});
	await diagnostics.section("Timeline", async () => {
		diagnostics.expect(true, "event rows", "no events rendered");
	});

	assert.deepEqual(diagnostics.complete({ screenshotCount: 3 }), {
		schema: "pointbreak.change-inspector-browser-report",
		version: 1,
		status: "passed",
		assertionCount: 2,
		screenshotCount: 3,
		sectionCount: 2,
		globalInvalid: false,
		sections: [
			{ name: "Reader readiness", status: "passed", failureCount: 0 },
			{ name: "Timeline", status: "passed", failureCount: 0 },
		],
		failures: [],
	});
});

test("rendered browser program logs and returns failed diagnostics normally", async () => {
	let source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	for (const [marker, replacement] of [
		[
			"__POINTBREAK_BROWSER_DIAGNOSTIC_FAILURE__",
			BrowserDiagnosticFailure.toString(),
		],
		["__POINTBREAK_BROWSER_DIAGNOSTICS__", createBrowserDiagnostics.toString()],
		["__POINTBREAK_CHANGE_BROWSER_CONFIG__", "{}"],
	]) {
		assert.ok(source.includes(marker), `missing browser marker ${marker}`);
		source = source.replace(marker, replacement);
	}

	assert.equal(
		typeof new Function(`return (${source}\n)`)(),
		"function",
		"rendered browser program remains executable by the runner",
	);
	assert.match(
		source,
		/const completion = diagnostics\.result\(\{ screenshotCount: screenshots \}\);/,
		"the browser program obtains failed and passing reports from diagnostics",
	);
	assert.match(
		source,
		/console\.log\(`POINTBREAK_BROWSER_RESULT=\$\{JSON\.stringify\(completion\)\}`\);\s*return completion;/,
		"the logged browser report is returned to the runner",
	);
	const completionTransport = source.slice(
		source.lastIndexOf(
			"const completion = diagnostics.result({ screenshotCount: screenshots });",
		),
	);
	assert.doesNotMatch(
		completionTransport,
		/throw new BrowserDiagnosticFailure\(/,
		"a failed browser report returns normally after logging",
	);
});

test("reduced-motion evidence waits for a semantically painted retained Changes surface", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const start = source.indexOf(
		'await diagnostics.section("Polling retention and reduced motion"',
	);
	const end = source.indexOf(
		'await diagnostics.section("Browser runtime"',
		start,
	);
	assert.notEqual(start, -1, "missing reduced-motion browser section");
	assert.notEqual(
		end,
		-1,
		"reduced-motion section must precede Browser runtime",
	);
	const section = source.slice(start, end);

	const pollWait = section.indexOf("await page.waitForTimeout(3500);");
	const retainedCardAssertion = section.indexOf(
		"locator('.unit-card[data-browser-retention=\"same-generation\"]')",
	);
	const semanticPaintWait = section.indexOf(
		"const semanticChangeSurface = await page.waitForFunction",
	);
	const screenshot = section.indexOf(
		'await screenshot("wide-reduced-motion");',
	);
	assert.ok(
		pollWait >= 0,
		"reduced-motion evidence must observe a poll interval",
	);
	assert.ok(
		retainedCardAssertion > pollWait,
		"the same-generation DOM-retention assertion must remain after polling",
	);
	assert.ok(
		semanticPaintWait > retainedCardAssertion,
		"the screenshot must wait for semantic Change paint after retaining the card node",
	);
	assert.ok(
		screenshot > semanticPaintWait,
		"the reduced-motion screenshot must follow the semantic Change-paint wait",
	);
	assert.match(
		section.slice(semanticPaintWait, screenshot),
		/#master[\s\S]*data-change-list-key[\s\S]*\.unit-card\[data-change-id\][\s\S]*change-card-primary/,
		"semantic paint must require a live Changes generation, exact Change identity, and a primary review action",
	);
});

test("fact-graph evidence checks painted labels against server-sized node frames", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const start = source.indexOf(
		'await diagnostics.section("Fact relationship graph"',
	);
	const end = source.indexOf(
		'await diagnostics.section("Annotated diff"',
		start,
	);
	assert.notEqual(start, -1, "missing fact relationship graph section");
	assert.notEqual(end, -1, "fact graph section must precede annotated diff");
	const section = source.slice(start, end);
	assert.match(
		section,
		/nodeLabelGeometry:[\s\S]*?getBBox\(\)[\s\S]*?clippedFactGraphLabels[\s\S]*?!Number\.isFinite\(node\.frameWidth\)[\s\S]*?node\.frameWidth <= 0[\s\S]*?node\.labelWidth <= 0[\s\S]*?node\.labelLeft < node\.frameLeft[\s\S]*?node\.labelRight > node\.frameRight/,
		"browser evidence must refuse a fact label painted outside its server-sized frame",
	);
});

test("Change-graph evidence checks painted labels against server-sized node frames", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const start = source.indexOf(
		'await diagnostics.section("Change relationship graph"',
	);
	const end = source.indexOf(
		'await diagnostics.section("Fact relationship graph"',
		start,
	);
	assert.notEqual(start, -1, "missing Change relationship graph section");
	assert.notEqual(end, -1, "Change graph section must precede fact graph");
	const section = source.slice(start, end);
	assert.match(
		section,
		/nodeLabelGeometry:[\s\S]*?getBBox\(\)[\s\S]*?clippedChangeGraphLabels[\s\S]*?!Number\.isFinite\(node\.frameWidth\)[\s\S]*?node\.frameWidth <= 0[\s\S]*?node\.labelWidth <= 0[\s\S]*?node\.labelLeft < node\.frameLeft[\s\S]*?node\.labelRight > node\.frameRight/,
		"browser evidence must refuse a Change graph label painted outside its server-sized frame",
	);
});

test("annotated-diff focus assertions wait for routed hydration and active focus", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const helperStart = source.indexOf("const waitForRoutedDiffFocus =");
	const sectionStart = source.indexOf(
		'await diagnostics.section("Annotated diff"',
	);
	const sectionEnd = source.indexOf(
		'await diagnostics.section("Exact detail and reading"',
		sectionStart,
	);
	assert.notEqual(helperStart, -1, "missing routed diff focus wait helper");
	assert.notEqual(sectionStart, -1, "missing annotated diff section");
	assert.notEqual(
		sectionEnd,
		-1,
		"annotated diff section must precede exact reading",
	);
	const helper = source.slice(
		helperStart,
		source.indexOf("\n\tconst ", helperStart + 1),
	);
	assert.match(
		helper,
		/#diff-page:not\(\.hidden\)[\s\S]*getClientRects\(\)\.length > 0[\s\S]*dataset\.exactFocus === "true"[\s\S]*document\.activeElement === target/,
		"focus readiness must require a visible diff, the exact-focus marker, and active DOM focus",
	);
	const section = source.slice(sectionStart, sectionEnd);
	const fileRouteWait = section.indexOf(
		'new URLSearchParams(location.hash.split("?", 2)[1] ?? "").get(\n\t\t\t\t\t\t"file"',
	);
	const fileFocusWait = section.indexOf(
		'await waitForRoutedDiffFocus("file", diffFilePaths[0]);',
	);
	const fileAssertion = section.indexOf("const focusedDiffFile = await page");
	assert.ok(
		fileRouteWait >= 0 &&
			fileFocusWait > fileRouteWait &&
			fileAssertion > fileFocusWait,
		"file focus must wait for hydrated routed focus after the route assertion",
	);
	const factRouteWait = section.indexOf(
		'new URLSearchParams(location.hash.split("?", 2)[1] ?? "").get(\n\t\t\t\t\t\t"fact"',
		fileAssertion,
	);
	const factFocusWait = section.indexOf(
		'await waitForRoutedDiffFocus("fact", firstDiffFact);',
	);
	const factAssertion = section.indexOf("const focusedDiffFact = await page");
	assert.ok(
		factRouteWait >= 0 &&
			factFocusWait > factRouteWait &&
			factAssertion > factFocusWait,
		"fact focus must wait for hydrated routed focus after the route assertion",
	);
});

test("Timeline event widen waits for the complete split-pane transition", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const narrowDetailScreenshot = source.indexOf(
		'await screenshot("narrow-timeline-event-detail");',
	);
	assert.notEqual(
		narrowDetailScreenshot,
		-1,
		"missing narrow Timeline event detail checkpoint",
	);
	const postWidenWaitStart = source.indexOf(
		"await page.waitForFunction(",
		narrowDetailScreenshot,
	);
	const postWidenAssertion = source.indexOf(
		'"Timeline event widen",',
		postWidenWaitStart,
	);
	assert.ok(
		postWidenWaitStart > narrowDetailScreenshot,
		"missing post-widen wait",
	);
	assert.ok(
		postWidenAssertion > postWidenWaitStart,
		"missing post-widen diagnostic assertion",
	);
	const wait = source.slice(postWidenWaitStart, postWidenAssertion);
	assert.match(
		wait,
		/getComputedStyle\(document\.querySelector\("#detail-back"\)\)\.display ===\s*"none"/,
		"post-widen readiness must require the wide CSS state",
	);
	assert.match(
		wait,
		/exactAction !== null[\s\S]*document\.activeElement === exactAction/,
		"post-widen readiness must retain exact-action focus",
	);
	assert.match(
		wait,
		/\["#topbar", "#toolbar", "#master-rail", "#master", "\.divider"\]\.every\([\s\S]*?document\.querySelector\(selector\)\?\.inert === false/,
		"post-widen readiness must wait for every ordinary split-pane surface to become non-inert",
	);
});

test("console 503 exemption accepts only typed primary Change transitions inside append", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const start = source.indexOf(
		"function isDeliberateChangeProjectionTransition(",
	);
	const end = source.indexOf("\n\tconst responseInspections", start);
	assert.notEqual(start, -1, "missing Change projection transition classifier");
	assert.ok(end > start, "missing classifier boundary");
	const classify = new Function(
		`${source.slice(start, end)}\nreturn isDeliberateChangeProjectionTransition;`,
	)();
	const baseUrl = "http://127.0.0.1:4173";
	const typed = {
		url: `${baseUrl}/api/v2/changes?limit=100`,
		status: 503,
		body: {
			schema: "pointbreak.inspect-change-projection-error",
			version: 1,
			code: "projection_unstable",
			retryable: true,
		},
		schema: "pointbreak.inspect-change-projection-error",
		insideAppendWindow: true,
	};
	assert.equal(classify(typed, baseUrl), true);
	assert.equal(
		classify(
			{ ...typed, url: `${baseUrl}/api/v2/attention?limit=100` },
			baseUrl,
		),
		true,
	);
	for (const rejected of [
		{ ...typed, insideAppendWindow: false },
		{ ...typed, status: 409 },
		{ ...typed, url: `${baseUrl}/api/v2/profile` },
		{ ...typed, url: `${baseUrl}/api/v2/changes-extra` },
		{ ...typed, url: `${baseUrl}/api/v2/changes/` },
		{ ...typed, url: `${baseUrl}.example/api/v2/changes` },
		{ ...typed, url: "http://127.0.0.1:4999/api/v2/changes" },
		{ ...typed, schema: "pointbreak.inspect-reader-profile" },
		{
			...typed,
			body: { ...typed.body, schema: "pointbreak.inspect-reader-profile" },
		},
		{ ...typed, body: { ...typed.body, code: "moving_journal" } },
		{ ...typed, body: { ...typed.body, version: 2 } },
		{ ...typed, body: { ...typed.body, retryable: false } },
	]) {
		assert.equal(classify(rejected, baseUrl), false, JSON.stringify(rejected));
	}
	const responseCapture = source.slice(
		source.indexOf('page.on("response"'),
		source.indexOf("\n\n\tconst layouts", source.indexOf('page.on("response"')),
	);
	for (const retained of [
		/response\.url\(\)/,
		/response\.status\(\)/,
		/response\.text\(\)/,
		/schema:/,
		/body,/,
	]) {
		assert.match(
			responseCapture,
			retained,
			"503 response evidence must retain URL, status, body, and schema",
		);
	}
	const runtimeAccounting = source.slice(
		source.indexOf("await settleResponseInspections();"),
		source.indexOf('await diagnostics.section("Browser runtime"'),
	);
	assert.match(
		runtimeAccounting,
		/error\.text !== genericServiceUnavailable[\s\S]*!error\.insideAppendWindow[\s\S]*error\.url === null[\s\S]*transitionResponsesByUrl\.get\(error\.url\)/,
		"generic console errors must consume only same-URL typed transitions observed in the append window",
	);
	assert.match(
		runtimeAccounting,
		/unexpectedServiceUnavailableResponses[\s\S]*!isDeliberateChangeProjectionTransition/,
		"every non-classified 503 response must remain a browser-runtime failure",
	);
});

test("an aggregate failure cannot publish a passing completion manifest", async () => {
	const root = await mkdtemp(join(tmpdir(), "pointbreak-browser-diagnostics-"));
	const candidatePath = join(root, ".manifest.json.tmp");
	const manifestPath = join(root, "manifest.json");
	const candidate = {
		gate: "change-inspector-browser-verify",
		status: "passed",
	};
	await writeFile(candidatePath, `${JSON.stringify(candidate)}\n`);

	await assert.rejects(
		publishPassingManifest({
			candidatePath,
			manifestPath,
			browserResult: {
				schema: "pointbreak.change-inspector-browser-report",
				version: 1,
				status: "passed",
				assertionCount: 2,
				screenshotCount: 1,
				sectionCount: 2,
				globalInvalid: false,
				sections: [
					{ name: "Timeline", status: "failed", failureCount: 1 },
					{ name: "Changes", status: "passed", failureCount: 0 },
				],
				failures: [{ section: "Timeline", label: "chronology" }],
			},
		}),
		/recorded 1 browser diagnostic failure/,
	);
	await assert.rejects(readFile(manifestPath), /ENOENT/);

	const passingResult = {
		schema: "pointbreak.change-inspector-browser-report",
		version: 1,
		status: "passed",
		assertionCount: 2,
		screenshotCount: 1,
		sectionCount: 2,
		globalInvalid: false,
		sections: [
			{ name: "Timeline", status: "passed", failureCount: 0 },
			{ name: "Changes", status: "passed", failureCount: 0 },
		],
		failures: [],
	};
	await assert.rejects(
		publishPassingManifest({
			candidatePath,
			manifestPath,
			browserResult: passingResult,
		}),
		/manifest candidate omitted assertionCount or screenshotCount/,
	);
	await writeFile(
		candidatePath,
		`${JSON.stringify({ ...candidate, assertionCount: 3, screenshotCount: 1 })}\n`,
	);
	await assert.rejects(
		publishPassingManifest({
			candidatePath,
			manifestPath,
			browserResult: passingResult,
		}),
		/assertionCount 3 did not match browser result 2/,
	);
	const passingCandidate = {
		...candidate,
		assertionCount: passingResult.assertionCount,
		screenshotCount: passingResult.screenshotCount,
		evidenceInventory: [],
	};
	await mkdir(join(root, "browser-artifacts"), { recursive: true });
	await mkdir(join(root, "logs"), { recursive: true });
	const retainedFiles = new Map([
		["browser-artifacts/wide-timeline.png", "wide PNG bytes"],
		["logs/browser-gate.log", "browser gate log bytes"],
		["logs/browser-program.mjs", "browser program bytes"],
		["logs/browser-result.json", '{"status":"passed"}\n'],
	]);
	for (const [path, bytes] of retainedFiles) {
		await writeFile(join(root, path), bytes);
	}
	passingCandidate.evidenceInventory = [...retainedFiles.entries()]
		.map(([path, bytes]) => ({
			path,
			sha256: createHash("sha256").update(bytes).digest("hex"),
		}))
		.sort((left, right) => left.path.localeCompare(right.path));
	await writeFile(candidatePath, `${JSON.stringify(passingCandidate)}\n`);
	await publishPassingManifest({
		candidatePath,
		manifestPath,
		browserResult: passingResult,
		evidenceRoot: root,
	});
	assert.deepEqual(
		JSON.parse(await readFile(manifestPath, "utf8")),
		passingCandidate,
	);
});

test("derived Change diagnostics are inadmissible to browser completion publication", async () => {
	const root = await mkdtemp(join(tmpdir(), "pointbreak-browser-boundary-"));
	const candidatePath = join(root, ".renamed-candidate.json.tmp");
	const manifestPath = join(root, "manifest.json");
	const passingResult = {
		schema: "pointbreak.change-inspector-browser-report",
		version: 1,
		status: "passed",
		assertionCount: 0,
		screenshotCount: 0,
		sectionCount: 1,
		globalInvalid: false,
		sections: [{ name: "Timeline", status: "passed", failureCount: 0 }],
		failures: [],
	};

	for (const schema of [
		"pointbreak.derived-change-diagnostic-report.v1",
		"pointbreak.derived-change-diagnostic-fragment.v1",
		"pointbreak.derived-change-diagnostic-collection.v1",
	]) {
		await assert.rejects(
			publishPassingManifest({
				candidatePath,
				manifestPath,
				browserResult: { ...passingResult, schema },
				evidenceRoot: root,
			}),
			/derived Change diagnostic.*never browser completion evidence/,
		);
		await writeFile(
			candidatePath,
			`${JSON.stringify({
				schema,
				gate: "change-inspector-browser-verify",
				status: "passed",
				assertionCount: 0,
				screenshotCount: 0,
			})}\n`,
		);
		await assert.rejects(
			publishPassingManifest({
				candidatePath,
				manifestPath,
				browserResult: passingResult,
				evidenceRoot: root,
			}),
			/derived Change diagnostic.*never browser completion evidence/,
		);
	}

	await assert.rejects(
		publishPassingManifest({
			candidatePath: join(root, "derived-change-diagnostic-report.json"),
			manifestPath,
			browserResult: passingResult,
			evidenceRoot: root,
		}),
		/derived Change diagnostic.*never browser completion evidence/,
	);
	await assert.rejects(
		publishPassingManifest({
			candidatePath,
			manifestPath,
			browserResult: passingResult,
			evidenceRoot: join(root, "derived-change-diagnostic", "browser"),
		}),
		/derived Change diagnostic.*never browser completion evidence/,
	);

	await writeFile(
		candidatePath,
		`${JSON.stringify({
			gate: "change-inspector-browser-verify",
			status: "passed",
			assertionCount: 0,
			screenshotCount: 0,
			evidenceInventory: [],
		})}\n`,
	);
	await assert.rejects(
		publishPassingManifest({
			candidatePath,
			manifestPath,
			browserResult: passingResult,
			browserResultPath: join(
				root,
				"derived-change-diagnostic",
				"browser-result.json",
			),
			evidenceRoot: root,
		}),
		/derived Change diagnostic.*never browser completion evidence/,
	);

	await writeFile(
		candidatePath,
		`${JSON.stringify({
			gate: "change-inspector-browser-verify",
			status: "passed",
			assertionCount: 0,
			screenshotCount: 0,
			evidenceInventory: [
				{
					path: "logs/derived-change-diagnostic/payload.json",
					sha256: "0".repeat(64),
				},
			],
		})}\n`,
	);
	await assert.rejects(
		publishPassingManifest({
			candidatePath,
			manifestPath,
			browserResult: passingResult,
			evidenceRoot: root,
		}),
		/derived Change diagnostic.*never browser completion evidence/,
	);

	const inventoryRoot = await mkdtemp(
		join(tmpdir(), "pointbreak-browser-inventory-boundary-"),
	);
	await mkdir(join(inventoryRoot, "browser-artifacts"));
	await mkdir(join(inventoryRoot, "logs"));
	const diagnosticBytes = `${JSON.stringify({
		schema: "pointbreak.derived-change-diagnostic-collection.v1",
		cases: [],
	})}\n`;
	const retainedFiles = new Map([
		["logs/browser-gate.log", "browser gate log bytes"],
		["logs/browser-program.mjs", "browser program bytes"],
		["logs/browser-result.json", diagnosticBytes],
	]);
	for (const [path, bytes] of retainedFiles) {
		await writeFile(join(inventoryRoot, path), bytes);
	}
	const inventoryCandidate = {
		gate: "change-inspector-browser-verify",
		status: "passed",
		assertionCount: 0,
		screenshotCount: 0,
		evidenceInventory: [...retainedFiles.entries()]
			.map(([path, bytes]) => ({
				path,
				sha256: createHash("sha256").update(bytes).digest("hex"),
			}))
			.sort((left, right) => left.path.localeCompare(right.path)),
	};
	const inventoryCandidatePath = join(inventoryRoot, ".candidate.json.tmp");
	const inventoryManifestPath = join(inventoryRoot, "manifest.json");
	await writeFile(
		inventoryCandidatePath,
		`${JSON.stringify(inventoryCandidate)}\n`,
	);
	await assert.rejects(
		publishPassingManifest({
			candidatePath: inventoryCandidatePath,
			manifestPath: inventoryManifestPath,
			browserResult: passingResult,
			evidenceRoot: inventoryRoot,
		}),
		/derived Change diagnostic.*never browser completion evidence/,
	);
});

test("completion manifests bind a sorted SHA-256 inventory of retained browser evidence", async () => {
	const root = await mkdtemp(join(tmpdir(), "pointbreak-browser-evidence-"));
	const evidenceRoot = join(root, "evidence");
	const artifactDir = join(evidenceRoot, "browser-artifacts");
	const logDir = join(evidenceRoot, "logs");
	await mkdir(artifactDir, { recursive: true });
	await mkdir(logDir, { recursive: true });

	const retainedFiles = new Map([
		["browser-artifacts/narrow-timeline.png", "narrow PNG bytes"],
		["browser-artifacts/wide-timeline.png", "wide PNG bytes"],
		["logs/browser-gate.log", "browser gate log bytes"],
		["logs/browser-program.mjs", "browser program bytes"],
		["logs/browser-result.json", '{"status":"passed"}\n'],
	]);
	for (const [path, bytes] of retainedFiles) {
		await writeFile(join(evidenceRoot, path), bytes);
	}
	const evidenceInventory = [...retainedFiles.entries()]
		.map(([path, bytes]) => ({
			path,
			sha256: createHash("sha256").update(bytes).digest("hex"),
		}))
		.sort((left, right) => left.path.localeCompare(right.path));
	const browserResult = {
		schema: "pointbreak.change-inspector-browser-report",
		version: 1,
		status: "passed",
		assertionCount: 2,
		screenshotCount: 2,
		sectionCount: 1,
		globalInvalid: false,
		sections: [{ name: "Timeline", status: "passed", failureCount: 0 }],
		failures: [],
	};
	const candidate = {
		gate: "change-inspector-browser-verify",
		status: "passed",
		assertionCount: browserResult.assertionCount,
		screenshotCount: browserResult.screenshotCount,
		evidenceInventory,
	};

	const missingPath = join(root, ".missing-manifest.json.tmp");
	await writeFile(
		missingPath,
		`${JSON.stringify({ ...candidate, evidenceInventory: evidenceInventory.slice(1) })}\n`,
	);
	await assert.rejects(
		publishPassingManifest({
			candidatePath: missingPath,
			manifestPath: join(root, "missing-manifest.json"),
			browserResult,
			evidenceRoot,
		}),
		/retained browser evidence|evidence inventory/i,
	);

	const unsortedPath = join(root, ".unsorted-manifest.json.tmp");
	await writeFile(
		unsortedPath,
		`${JSON.stringify({
			...candidate,
			evidenceInventory: [...evidenceInventory].reverse(),
		})}\n`,
	);
	await assert.rejects(
		publishPassingManifest({
			candidatePath: unsortedPath,
			manifestPath: join(root, "unsorted-manifest.json"),
			browserResult,
			evidenceRoot,
		}),
		/sorted.*evidence inventory|evidence inventory.*sorted/i,
	);

	const tamperedPath = join(root, ".tampered-manifest.json.tmp");
	await writeFile(tamperedPath, `${JSON.stringify(candidate)}\n`);
	await writeFile(
		join(logDir, "browser-gate.log"),
		"tampered browser gate log",
	);
	await assert.rejects(
		publishPassingManifest({
			candidatePath: tamperedPath,
			manifestPath: join(root, "tampered-manifest.json"),
			browserResult,
			evidenceRoot,
		}),
		/SHA-256|digest|evidence inventory/i,
	);

	await writeFile(
		join(logDir, "browser-gate.log"),
		retainedFiles.get("logs/browser-gate.log"),
	);
	const validCandidatePath = join(root, ".valid-manifest.json.tmp");
	const validManifestPath = join(root, "valid-manifest.json");
	const validCandidateBytes = `${JSON.stringify(candidate)}\n`;
	await writeFile(validCandidatePath, validCandidateBytes);
	await publishPassingManifest({
		candidatePath: validCandidatePath,
		manifestPath: validManifestPath,
		browserResult,
		evidenceRoot,
	});
	assert.deepEqual(
		JSON.parse(await readFile(validManifestPath, "utf8")),
		candidate,
	);
	assert.equal(await readFile(validManifestPath, "utf8"), validCandidateBytes);

	const duplicateCandidatePath = join(root, ".duplicate-manifest.json.tmp");
	await writeFile(duplicateCandidatePath, `${JSON.stringify(candidate)}\n`);
	const publishedBytes = await readFile(validManifestPath);
	await assert.rejects(
		publishPassingManifest({
			candidatePath: duplicateCandidatePath,
			manifestPath: validManifestPath,
			browserResult,
			evidenceRoot,
		}),
		/already exists/i,
	);
	assert.deepEqual(await readFile(validManifestPath), publishedBytes);
});
