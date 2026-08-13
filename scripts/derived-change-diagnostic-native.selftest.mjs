import assert from "node:assert/strict";
import {
	chmod,
	lstat,
	mkdir,
	mkdtemp,
	readFile,
	writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
	DERIVED_CHANGE_DIAGNOSTIC_CASE_COLLECTION_SCHEMA_V1,
	DERIVED_CHANGE_NATIVE_DIAGNOSTIC_CONFIG_SCHEMA_V1,
	DERIVED_CHANGE_NATIVE_LIFECYCLE_CRITERIA_V1,
	runDerivedChangeNativeDiagnostic,
} from "./derived-change-diagnostic-native.mjs";

const digest = (digit) => digit.repeat(64);
const commit = (digit) => digit.repeat(40);

test("native diagnostic continues independent tiers and skips only a failed tier lifecycle", async () => {
	const root = await mkdtemp(join(tmpdir(), "pointbreak-native-diagnostic-"));
	const sourceCheckout = join(root, "source");
	const caseRoot = join(root, "case");
	const workRoot = await mkdtemp(join(tmpdir(), "pbnw-"));
	await mkdir(sourceCheckout);
	await writeFile(join(sourceCheckout, "Cargo.lock"), "locked\n");
	const fakeHarness = join(root, "fake-harness.mjs");
	await writeFile(
		fakeHarness,
		`#!${process.execPath}
import { mkdir, readFile, writeFile } from "node:fs/promises";
const requestArgument = process.argv.find((value) => value.startsWith("--derived-access-request="));
if (process.argv.includes("--derived-access-contract")) {
  console.log(JSON.stringify({ contract: { schema: "pointbreak.qualification-derived-access-contract.v1" }, contractSha256: "${digest("a")}" }));
  process.exit(0);
}
const request = JSON.parse(await readFile(requestArgument.split("=")[1], "utf8"));
const ownerNames = new Set(["POINTBREAK_HOME", "POINTBREAK_CHANGE_READY_FIXTURE_DIR"]);
if (Object.keys(process.env).some((key) => ownerNames.has(key.toUpperCase()))) {
  console.error("owner-store environment escaped native isolation");
  process.exit(91);
}
if (process.argv.includes("--derived-change-diagnostic-native")) {
  if (request.tier === "L1") { console.error("synthetic L1 failure"); process.exit(7); }
  await mkdir(request.workspaceRoot + "/root-a", { recursive: true });
  await writeFile(request.workspaceRoot + "/root-a/source.txt", request.tier);
  console.log(JSON.stringify({
    mode: "--derived-change-diagnostic-native",
    tier: request.tier,
    admittedRootPath: request.workspaceRoot + "/root-a",
    admittedRootSha256: request.tier === "D0-128" ? "${digest("b")}" : "${digest("c")}",
    sourceUnchanged: true,
  }));
  process.exit(0);
}
if (process.argv.includes("--derived-access-lifecycle-diagnostic")) {
  console.log(JSON.stringify({ mode: "--derived-access-lifecycle-diagnostic", sourceUnchanged: true, cases: [
    { criterion: "open_bootstrap_reopen_replay_equality", status: "passed", failureDetail: null },
    { criterion: "wrong_root", status: request.tier === "L7" ? "failed" : "passed", failureDetail: request.tier === "L7" ? "synthetic wrong root" : null }
  ] }));
  process.exit(0);
}
process.exit(2);
`,
	);
	await chmod(fakeHarness, 0o755);

	const oldPointbreakHome = process.env.pointbreak_home;
	const oldReadyFixture = process.env.Pointbreak_Change_Ready_Fixture_Dir;
	process.env.pointbreak_home = "/ambient-owner-store";
	process.env.Pointbreak_Change_Ready_Fixture_Dir =
		"/ambient-change-ready-fixture";
	let result;
	try {
		result = await runDerivedChangeNativeDiagnostic({
			schema: DERIVED_CHANGE_NATIVE_DIAGNOSTIC_CONFIG_SCHEMA_V1,
			campaignId: "derived-change-diagnostic-test",
			rootAuthoritySha256: digest("d"),
			caseRoot,
			workRoot,
			sourceCheckout,
			gitProgram: process.execPath,
			source: {
				commit: commit("1"),
				tree: commit("2"),
				rangeBaseCommit: commit("3"),
				rangeSha256: digest("4"),
			},
			platform: {
				id: "macos_apfs",
				operatingSystem: "macos",
				architecture: "aarch64",
				filesystem: "apfs",
				hostIdentitySha256: digest("5"),
			},
			harness: { program: fakeHarness, argsPrefix: [] },
			lifecycleCriteria: [
				"open_bootstrap_reopen_replay_equality",
				"wrong_root",
			],
		});
	} finally {
		if (oldPointbreakHome === undefined) delete process.env.pointbreak_home;
		else process.env.pointbreak_home = oldPointbreakHome;
		if (oldReadyFixture === undefined)
			delete process.env.Pointbreak_Change_Ready_Fixture_Dir;
		else process.env.Pointbreak_Change_Ready_Fixture_Dir = oldReadyFixture;
	}
	assert.equal(
		result.schema,
		DERIVED_CHANGE_DIAGNOSTIC_CASE_COLLECTION_SCHEMA_V1,
	);

	assert.deepEqual(
		result.cases
			.filter(({ id }) => id.startsWith("native-"))
			.map(({ id, status }) => ({ id, status })),
		[
			{ id: "native-D0-128", status: "passed" },
			{ id: "native-L1", status: "failed" },
			{ id: "native-L7", status: "passed" },
		],
	);
	const l1Rows = result.cases.filter(({ id }) =>
		id.startsWith("lifecycle-L1-"),
	);
	assert.equal(l1Rows.length, 3);
	assert.ok(l1Rows.every(({ status }) => status === "skipped"));
	assert.equal(
		result.cases.find(({ id }) => id === "lifecycle-L7-wrong_root")?.status,
		"failed",
	);
	assert.equal(
		result.cases.find(
			({ id }) =>
				id === "lifecycle-D0-128-open_bootstrap_reopen_replay_equality",
		)?.status,
		"passed",
	);
	assert.equal(
		result.cases.filter(({ status }) => status === "failed").length,
		2,
	);
	assert.ok(
		result.artifactPaths.some((path) => path.endsWith("native-L1.stderr.log")),
	);
	assert.equal(
		(await lstat(join(workRoot, "native-D0-128"))).isDirectory(),
		true,
	);
	await assert.rejects(() => lstat(join(caseRoot, "native-D0-128")), {
		code: "ENOENT",
	});
	for (const artifact of result.artifactPaths)
		assert.equal((await lstat(join(caseRoot, artifact))).isFile(), true);
});

test("the frozen lifecycle inventory is complete and unique", () => {
	assert.equal(DERIVED_CHANGE_NATIVE_LIFECYCLE_CRITERIA_V1.length, 18);
	assert.equal(
		new Set(DERIVED_CHANGE_NATIVE_LIFECYCLE_CRITERIA_V1).size,
		DERIVED_CHANGE_NATIVE_LIFECYCLE_CRITERIA_V1.length,
	);
});

test("native diagnostic calls the non-terminal native collection mode", async () => {
	const source = await readFile(
		new URL("./derived-change-diagnostic-native.mjs", import.meta.url),
		"utf8",
	);
	assert.match(source, /"--derived-change-diagnostic-native"/);
	assert.match(source, /POINTBREAK_DIAGNOSTIC_WORK_ROOT/);
	assert.doesNotMatch(source, /"--derived-access-smoke"/);
	assert.doesNotMatch(source, /payload\?\.receipt/);
});
