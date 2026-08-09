import assert from "node:assert/strict";
import { mkdtemp, readFile, writeFile } from "node:fs/promises";
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
	};
	await writeFile(candidatePath, `${JSON.stringify(passingCandidate)}\n`);
	await publishPassingManifest({
		candidatePath,
		manifestPath,
		browserResult: passingResult,
	});
	assert.deepEqual(
		JSON.parse(await readFile(manifestPath, "utf8")),
		passingCandidate,
	);
});
