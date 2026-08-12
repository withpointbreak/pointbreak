import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { chmod, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

const source = async (name) =>
	readFile(new URL(`./${name}`, import.meta.url), "utf8");

const browserRunner = (browser, config) => {
	const source = browser.replace(
		"__POINTBREAK_DERIVED_CHANGE_DIAGNOSTIC_BROWSER_CONFIG__",
		JSON.stringify(config),
	);
	return new Function(
		`return (${source}\n)`,
	)();
};

const runtimeFailureResult = async (browser) => {
	const listeners = new Map();
	const runner = browserRunner(browser, {
		campaignId: "selftest-runtime-errors",
		iterations: 0,
		fixture: {
			id: "fixture-selftest",
			rawWitnessSha256: "c".repeat(64),
			actualAuthoritativeInventorySha256: "a".repeat(64),
			topologyCheckpointSha256: "d".repeat(64),
			fixtureModuleSha256: "e".repeat(64),
			topologyMaterializerSha256: "b".repeat(64),
		},
		artifactDir: "/tmp/unused",
		server: { baseUrl: "http://127.0.0.1:1", token: "selftest" },
	});
	const originalLog = console.log;
	console.log = () => {};
	try {
		return await runner({
			on(event, listener) {
				listeners.set(event, listener);
			},
			async goto() {
				listeners.get("pageerror")(new Error("first page error"));
				listeners.get("pageerror")(new Error("second page error"));
				listeners.get("console")({
					type: () => "error",
					text: () => "first console error",
				});
			},
			url: () => "http://127.0.0.1:1/#/",
			viewportSize: () => ({ width: 390, height: 844 }),
		});
	} finally {
		console.log = originalLog;
	}
};

test("derived Change diagnostic browser runner is isolated and non-terminal", async () => {
	const shell = await source("derived-change-diagnostic-browser.sh");
	const browser = await source("derived-change-diagnostic-browser.mjs");
	const materializer = await source("materialize-inspector-decision-matrix.sh");

	assert.match(shell, /--root <empty external case root>/);
	assert.match(shell, /--campaign-id <nonempty>/);
	assert.match(shell, /--iterations <positive bounded count>/);
	assert.match(shell, /verify-commit/);
	assert.match(shell, /source worktree must be clean/);
	assert.match(shell, /materialize-inspector-decision-matrix\.sh/);
	assert.match(shell, /store derived build/);
	assert.match(shell, /inspect --repo/);
	assert.match(shell, /derived-change-diagnostic-browser\.mjs/);
	assert.match(shell, /POINTBREAK_EXPECTED_FIXTURE_ID/);
	assert.match(shell, /POINTBREAK_EXPECTED_TOPOLOGY_CHECKPOINT_SHA256/);
	assert.match(shell, /POINTBREAK_EXPECTED_TOPOLOGY_MATERIALIZER_SHA256/);
	assert.match(shell, /fixture-checkpoint\.json/);
	assert.match(shell, /raw witness hash differs from fixture bytes/);
	assert.match(
		shell,
		/public fixture topology checkpoint differs from the expected authority/,
	);
	assert.match(
		shell,
		/"\$BASH" "\$snapshot_scripts\/materialize-inspector-decision-matrix\.sh"/,
	);
	for (const program of [
		"GIT",
		"JQ",
		"FIND",
		"SORT",
		"WC",
		"TR",
		"AWK",
		"HASH",
		"CP",
		"HEAD",
		"DIRNAME",
		"MKDIR",
		"RM",
	]) {
		assert.match(shell, new RegExp(`POINTBREAK_${program}_PROGRAM`));
		assert.match(materializer, new RegExp(`POINTBREAK_${program}_PROGRAM`));
	}
	assert.match(shell, /materializer tools changed during the diagnostic/);
	assert.match(shell, /materializer snapshot changed during the diagnostic/);
	assert.match(shell, /POINTBREAK_HASH_PROGRAM_MODE=shasum/);
	assert.match(materializer, /POINTBREAK_HASH_PROGRAM_MODE/);
	assert.match(materializer, /shasum\|sha256sum/);
	assert.doesNotMatch(shell, /change-inspector-browser-manifest/);
	assert.doesNotMatch(shell, /manifest\.json/);
	assert.doesNotMatch(shell, /seq 1 351/);

	assert.match(
		browser,
		/getComputedStyle\(document\.querySelector\("#detail-back"\)\)\.display ===\s*"none"/,
	);
	assert.match(
		browser,
		/exactAction !== null[\s\S]*document\.activeElement === exactAction/,
	);
	assert.match(
		browser,
		/ordinarySplitPaneSelectors = \[[\s\S]*?"#topbar"[\s\S]*?"#toolbar"[\s\S]*?"#master-rail"[\s\S]*?"#master"[\s\S]*?"\.divider"/,
	);
	assert.match(
		browser,
		/waitForWideExactEvent[\s\S]*?selectors\.every\([\s\S]*?document\.querySelector\(selector\)\?\.inert === false/,
	);
	assert.match(
		browser,
		/for \(let iteration = 1; iteration <= config\.iterations; iteration \+= 1\)/,
	);
	assert.match(
		browser,
		/lane: "browser",[\s\S]*?required: true,[\s\S]*?attempted: true,[\s\S]*?dependsOn: \[\],[\s\S]*?phase:[\s\S]*?fixtureCheckpoint:[\s\S]*?artifactPaths:/,
	);
	assert.match(browser, /failureClass = "case_failure"/);
	assert.match(
		browser,
		/schema: "pointbreak\.derived-change-diagnostic-collection\.v1"/,
	);
	assert.match(
		shell,
		/\.schema == "pointbreak\.derived-change-diagnostic-collection\.v1"/,
	);
	assert.match(browser, /id: "browser-runtime-pageerror"/);
	assert.match(browser, /id: "browser-runtime-console"/);
	assert.match(browser, /const pageErrorCase = \{[\s\S]*?observations: \[\]/);
	assert.match(browser, /const consoleCase = \{[\s\S]*?observations: \[\]/);
	assert.match(browser, /for \(const error of pageErrors\)/);
	assert.match(browser, /for \(const error of consoleErrors\)/);
	assert.match(
		browser,
		/cases\.push\(pageErrorCase\)[\s\S]*?cases\.push\(consoleCase\)/,
	);
	assert.doesNotMatch(browser, /browser-pageerror-\$\{index \+ 1\}/);
	assert.doesNotMatch(browser, /browser-console-\$\{index \+ 1\}/);
	assert.doesNotMatch(browser, /const failures = \[\]/);
	assert.doesNotMatch(browser, /\n\t\tfailures,/);
	assert.match(shell, /fixtureId/);
	assert.match(browser, /actualAuthoritativeInventorySha256/);
	assert.match(browser, /rawWitnessSha256/);
	assert.match(browser, /topologyCheckpointSha256/);
	assert.match(browser, /topologyMaterializerSha256/);
	assert.match(shell, /all\(\.status == "passed"\)/);
	assert.doesNotMatch(browser, /pointbreak\.change-inspector-browser-report/);
	assert.doesNotMatch(
		browser,
		/pointbreak\.derived-change-diagnostic-report\.v1/,
	);
});

test("browser snapshots the authority source and binds optional cygpath explicitly", async () => {
	const shell = await source("derived-change-diagnostic-browser.sh");
	const materializer = await source("materialize-inspector-decision-matrix.sh");

	for (const name of [
		"POINTBREAK_EXPECTED_SOURCE_COMMIT",
		"POINTBREAK_EXPECTED_SOURCE_TREE",
	]) {
		assert.match(shell, new RegExp(name));
	}
	assert.match(shell, /commit that differs from the expected authority/);
	assert.match(shell, /tree that differs from the expected authority/);
	assert.match(shell, /after browser diagnostic/);
	assert.match(shell, /sourceCommit/);
	assert.match(shell, /sourceTree/);

	assert.match(shell, /POINTBREAK_CYGPATH_PROGRAM/);
	assert.match(shell, /must be an absolute program path or absent/);
	assert.match(shell, /materializerCygpathBinding/);
	assert.match(
		shell,
		/cygpath: \{path: \$cygpathPath, sha256: \$cygpathSha256\}/,
	);
	assert.match(materializer, /POINTBREAK_CYGPATH_PROGRAM/);
	assert.match(materializer, /POINTBREAK_CYGPATH_PROGRAM\+x/);
	assert.match(materializer, /"\$cygpath_program" -u/);
	assert.doesNotMatch(materializer, /cygpath -u/);
});

test("browser binds a normalized topology checkpoint while retaining raw fixture observations", async () => {
	const shell = await source("derived-change-diagnostic-browser.sh");
	const browser = await source("derived-change-diagnostic-browser.mjs");

	assert.match(shell, /POINTBREAK_EXPECTED_TOPOLOGY_CHECKPOINT_SHA256/);
	assert.match(shell, /POINTBREAK_DIAGNOSTIC_WORK_ROOT/);
	assert.match(shell, /scripts\/derived-change-diagnostic-fixture\.mjs/);
	assert.match(shell, /fixtureModuleSha256/);
	assert.match(shell, /fixture-checkpoint\.json/);
	assert.match(shell, /topologyCheckpointSha256/);
	assert.match(shell, /rawWitnessSha256/);
	assert.match(shell, /actualAuthoritativeInventorySha256/);
	assert.match(shell, /fixture_root="\$diagnostic_work_root\//);
	assert.match(shell, /fixture root must be outside --root/);
	assert.doesNotMatch(
		shell,
		/POINTBREAK_EXPECTED_AUTHORITATIVE_INVENTORY_SHA256/,
	);
	assert.doesNotMatch(shell, /POINTBREAK_EXPECTED_FIXTURE_WITNESS_SHA256/);
	assert.doesNotMatch(
		shell,
		/fixture witness bytes differ from the expected SHA-256/,
	);
	assert.match(browser, /rawWitnessSha256/);
	assert.match(browser, /actualAuthoritativeInventorySha256/);
	assert.match(browser, /topologyCheckpointSha256/);
});

test("browser rejects a signed-source snapshot before it can drift from authority", async () => {
	const root = await mkdtemp(join(tmpdir(), "pointbreak-browser-source-"));
	const fakeGit = join(root, "git");
	const caseRoot = join(root, "case-root");
	await writeFile(
		fakeGit,
		`#!/bin/sh
case "$*" in
  *"status --porcelain --untracked-files=all"*) exit 0 ;;
  *"rev-parse HEAD"*) printf '%s\\n' '${"a".repeat(40)}' ;;
esac
exit 0
`,
	);
	await chmod(fakeGit, 0o755);
	try {
		let error;
		try {
			await execFileAsync(
				process.env.BASH ?? "/bin/bash",
				[
					new URL("./derived-change-diagnostic-browser.sh", import.meta.url)
						.pathname,
					"--root",
					caseRoot,
					"--campaign-id",
					"selftest-source-authority",
					"--iterations",
					"1",
				],
				{
					env: {
						...process.env,
						POINTBREAK_BINARY: process.execPath,
						POINTBREAK_GIT_PROGRAM: fakeGit,
						POINTBREAK_EXPECTED_FIXTURE_ID: "fixture-selftest",
						POINTBREAK_EXPECTED_TOPOLOGY_CHECKPOINT_SHA256: "a".repeat(64),
						POINTBREAK_EXPECTED_TOPOLOGY_MATERIALIZER_SHA256: "c".repeat(64),
						POINTBREAK_EXPECTED_SOURCE_COMMIT: "d".repeat(40),
						POINTBREAK_EXPECTED_SOURCE_TREE: "e".repeat(40),
						POINTBREAK_CYGPATH_PROGRAM: "absent",
						POINTBREAK_DIAGNOSTIC_WORK_ROOT: join(root, "fixture-work"),
					},
				},
			);
		} catch (caught) {
			error = caught;
		}
		assert.ok(error, "browser wrapper accepted a source commit mismatch");
		assert.match(
			error.stderr,
			/commit that differs from the expected authority/,
		);
		await assert.rejects(readFile(caseRoot));
	} finally {
		await rm(root, { recursive: true, force: true });
	}
});

test("runtime error cases retain their own empty-error expectations", async () => {
	const browser = await source("derived-change-diagnostic-browser.mjs");
	const result = await runtimeFailureResult(browser);
	assert.equal(
		result.schema,
		"pointbreak.derived-change-diagnostic-collection.v1",
	);
	const pageErrors = result.cases.find(
		(caseResult) => caseResult.id === "browser-runtime-pageerror",
	);
	const consoleErrors = result.cases.find(
		(caseResult) => caseResult.id === "browser-runtime-console",
	);

	assert.deepEqual(pageErrors.expected, { pageErrors: [] });
	assert.deepEqual(consoleErrors.expected, { consoleErrors: [] });
	assert.deepEqual(pageErrors.fixtureCheckpoint, {
		fixture: "fixture-selftest",
		fixtureId: "fixture-selftest",
		rawWitnessSha256: "c".repeat(64),
		actualAuthoritativeInventorySha256: "a".repeat(64),
		topologyCheckpointSha256: "d".repeat(64),
		fixtureModuleSha256: "e".repeat(64),
		topologyMaterializerSha256: "b".repeat(64),
		checkpoint: "browser-runtime",
	});
	assert.deepEqual(
		pageErrors.actual.observations.map((observation) => observation.detail),
		["first page error", "second page error"],
	);
	assert.deepEqual(
		consoleErrors.actual.observations.map((observation) => observation.detail),
		["first console error"],
	);
});

test("a rejected authenticated bootstrap is aggregated and skips only transitions", async () => {
	const browser = await source("derived-change-diagnostic-browser.mjs");
	const runner = browserRunner(browser, {
		campaignId: "selftest-bootstrap-failure",
		iterations: 2,
		fixture: {
			id: "fixture-selftest",
			rawWitnessSha256: "c".repeat(64),
			actualAuthoritativeInventorySha256: "a".repeat(64),
			topologyCheckpointSha256: "d".repeat(64),
			fixtureModuleSha256: "e".repeat(64),
			topologyMaterializerSha256: "b".repeat(64),
		},
		artifactDir: "/tmp/unused",
		server: { baseUrl: "http://127.0.0.1:1", token: "selftest" },
	});
	const originalLog = console.log;
	console.log = () => {};
	let result;
	try {
		result = await runner({
			on() {},
			async goto() {
				throw new Error("bootstrap navigation failed");
			},
			url: () => "about:blank",
			viewportSize: () => ({ width: 390, height: 844 }),
		});
	} finally {
		console.log = originalLog;
	}
	assert.deepEqual(
		result.cases.map(({ id, status }) => ({ id, status })),
		[
			{ id: "browser-bootstrap", status: "failed" },
			{ id: "browser-widen-1", status: "skipped" },
			{ id: "browser-widen-2", status: "skipped" },
			{ id: "browser-runtime-pageerror", status: "passed" },
			{ id: "browser-runtime-console", status: "passed" },
		],
	);
	assert.equal(result.cases[0].failureClass, "lane_invalid");
	assert.deepEqual(result.cases[1].dependsOn, ["browser-bootstrap"]);
	assert.match(result.cases[1].skipReason, /browser-bootstrap/);
	assert.ok(
		result.cases.every(
			({ fixtureCheckpoint }) => fixtureCheckpoint.fixture === "fixture-selftest",
		),
	);
});

test("materializer requires an explicit hash mode for an explicit hash program", async () => {
	const materializer = new URL(
		"./materialize-inspector-decision-matrix.sh",
		import.meta.url,
	);
	let error;
	try {
		await execFileAsync(
			process.env.BASH ?? "/bin/bash",
			[materializer.pathname],
			{
				env: {
					...process.env,
					POINTBREAK_HASH_PROGRAM: process.execPath,
					POINTBREAK_HASH_PROGRAM_MODE: "unsupported",
				},
			},
		);
	} catch (caught) {
		error = caught;
	}
	assert.ok(error, "materializer accepted an unsupported explicit hash mode");
	assert.match(
		error.stderr,
		/POINTBREAK_HASH_PROGRAM_MODE must be shasum or sha256sum/,
	);
});
