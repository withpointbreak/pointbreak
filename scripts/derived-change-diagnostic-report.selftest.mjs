import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import test from "node:test";

import {
	DERIVED_CHANGE_DIAGNOSTIC_FRAGMENT_SCHEMA_V1,
	DERIVED_CHANGE_DIAGNOSTIC_REPORT_SCHEMA_V1,
	DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1,
	finalizeDerivedChangeDiagnosticFragment,
	mergeDerivedChangeDiagnosticReport,
	validateDerivedChangeDiagnosticCampaign,
	validateDerivedChangeDiagnosticReport,
} from "./derived-change-diagnostic-report.mjs";

const digest = (digit) => digit.repeat(64);
const commit = (digit) => digit.repeat(40);
const canonicalJson = (value) => {
	if (value === null || typeof value !== "object") return JSON.stringify(value);
	if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
	return `{${Object.keys(value)
		.sort()
		.map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`)
		.join(",")}}`;
};
const fixtureIds = [
	"cycle-conflicted-v1",
	"duplicate-conflict-v1",
	"duplicate-equal-v1",
	"incomplete-v1",
	"missing-carrier-v1",
	"mutated-carrier-v1",
	"removal-v1",
	"topology-v1",
	"wrong-family-carrier-v1",
];
const fixtureAuthority = () => ({
	authoritySha256: digest("1"),
	document: {
		schema: "pointbreak.derived-change-public-fixture-authority.v1",
		sourceCommit: commit("a"),
		sourceTree: commit("b"),
		sourceFiles: [
			{
				path: "scripts/materialize-inspector-decision-matrix.sh",
				sha256: digest("2"),
			},
			{
				path: "src/bench_support/derived_access/materializer.rs",
				sha256: digest("3"),
			},
			{
				path: "tests/support/assets/change-ready-store/5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json",
				sha256: digest("4"),
			},
			{
				path: "tests/support/assets/change-ready-store/f31956c2b820926adc74d4d03cb03820d13c9ed2739b5f7ada81611a6f8bcff1.json",
				sha256: digest("5"),
			},
		],
		witnesses: fixtureIds.map((fixtureId, index) => ({
			fixtureId,
			authoritativeInventorySha256: digest(String((index + 4) % 10)),
			witnessSha256: digest(String((index + 5) % 10)),
		})),
	},
});

const campaign = () => ({
	id: "derived-change-diagnostic-001",
	source: {
		commit: commit("a"),
		tree: commit("b"),
		rangeBaseCommit: commit("c"),
		rangeSha256: digest("c"),
	},
	rootComponent: DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1,
	product: {
		binaries: [{ platformId: "macos_apfs", binarySha256: digest("d") }],
	},
	harness: {
		binaries: [{ platformId: "macos_apfs", binarySha256: digest("e") }],
	},
	control: {
		binaries: [
			{
				platformId: "macos_apfs",
				role: "cli",
				binarySha256: digest("0"),
			},
			{
				platformId: "macos_apfs",
				role: "library",
				binarySha256: digest("f"),
			},
		],
	},
	fixture: fixtureAuthority(),
	requiredCaseIds: ["compile-product"],
	platforms: [
		{
			id: "macos_apfs",
			operatingSystem: "macos",
			architecture: "aarch64",
			filesystem: "apfs",
			hostIdentitySha256: digest("2"),
		},
	],
	requiredPlatformIds: ["macos_apfs"],
});

const fragment = (overrides = {}) => {
	const identity = campaign();
	return finalizeDerivedChangeDiagnosticFragment({
		schema: DERIVED_CHANGE_DIAGNOSTIC_FRAGMENT_SCHEMA_V1,
		campaign: identity,
		platform: identity.platforms[0],
		artifacts: [
			{ path: "logs/compile.log", sha256: digest("3") },
			{ path: "artifacts/fixture.json", sha256: digest("4") },
		],
		cases: [
			{
				id: "compile-product",
				lane: "compile",
				required: true,
				attempted: true,
				status: "passed",
				dependsOn: [],
				log: { path: "logs/compile.log", sha256: digest("3") },
				artifactPaths: ["artifacts/fixture.json"],
			},
		],
		...overrides,
	});
};

const merge = (fragments, identity = campaign()) =>
	mergeDerivedChangeDiagnosticReport({ campaign: identity, fragments });

test("rejects duplicate case identifiers across host fragments", () => {
	const duplicate = fragment();
	duplicate.artifacts = [
		{ path: "logs/compile-second.log", sha256: digest("5") },
		{ path: "artifacts/fixture-second.json", sha256: digest("6") },
	];
	duplicate.cases[0].log = {
		path: "logs/compile-second.log",
		sha256: digest("5"),
	};
	duplicate.cases[0].artifactPaths = ["artifacts/fixture-second.json"];
	assert.throws(
		() =>
			merge([fragment(), finalizeDerivedChangeDiagnosticFragment(duplicate)]),
		/duplicate diagnostic case id: compile-product/,
	);
});

test("rejects a reordered or tampered finalized host fragment", () => {
	const source = fragment({
		cases: [
			{
				id: "compile-product",
				lane: "compile",
				required: true,
				attempted: true,
				status: "passed",
				dependsOn: [],
			},
			{
				id: "optional-policy",
				lane: "policy",
				required: false,
				attempted: true,
				status: "passed",
				dependsOn: ["compile-product"],
			},
		],
	});
	const reordered = structuredClone(source);
	reordered.cases.reverse();
	assert.throws(() => merge([reordered]), /fragment SHA-256 differs/);

	const tampered = structuredClone(source);
	tampered.cases[0].lane = "native";
	assert.throws(() => merge([tampered]), /fragment SHA-256 differs/);
});

test("rejects an unknown dependency", () => {
	const input = fragment();
	input.cases[0].dependsOn = ["missing-case"];
	assert.throws(
		() => merge([finalizeDerivedChangeDiagnosticFragment(input)]),
		/unknown dependency missing-case/,
	);
});

test("rejects cyclic dependencies", () => {
	const identity = campaign();
	identity.requiredCaseIds = ["first", "second"];
	const input = fragment({
		campaign: identity,
		cases: [
			{
				id: "first",
				lane: "native",
				required: true,
				attempted: true,
				status: "passed",
				dependsOn: ["second"],
			},
			{
				id: "second",
				lane: "native",
				required: true,
				attempted: true,
				status: "passed",
				dependsOn: ["first"],
			},
		],
	});
	assert.throws(() => merge([input], identity), /dependency cycle/);
});

test("requires sorted complete case and platform authority inventories", () => {
	const missing = fragment({
		cases: [
			{
				id: "optional-policy",
				lane: "policy",
				required: false,
				attempted: true,
				status: "passed",
				dependsOn: [],
			},
		],
	});
	assert.throws(() => merge([missing]), /required case inventory differs/);

	const extra = campaign();
	extra.requiredCaseIds = ["compile-product", "missing-browser-case"];
	assert.throws(
		() => merge([fragment({ campaign: extra })], extra),
		/required case inventory differs/,
	);

	const unsorted = campaign();
	unsorted.requiredCaseIds = ["z", "a"];
	assert.throws(
		() => merge([fragment()], unsorted),
		/required case inventory must be sorted/,
	);

	const platform = campaign();
	platform.requiredPlatformIds = ["other-platform"];
	assert.throws(
		() => merge([fragment()], platform),
		/required platform inventory differs/,
	);
});

test("requires platform architecture in the bound host identity", () => {
	const identity = campaign();
	delete identity.platforms[0].architecture;
	assert.throws(
		() => validateDerivedChangeDiagnosticCampaign(identity),
		/platform architecture must be non-empty text/,
	);
});

test("requires both exact control binary roles on every platform", () => {
	const missingCli = campaign();
	missingCli.control.binaries = missingCli.control.binaries.filter(
		({ role }) => role !== "cli",
	);
	assert.throws(
		() => merge([fragment({ campaign: missingCli })], missingCli),
		/control binary inventory.*cli.*library/i,
	);

	const duplicateLibrary = campaign();
	duplicateLibrary.control.binaries[0].role = "library";
	assert.throws(
		() => merge([fragment({ campaign: duplicateLibrary })], duplicateLibrary),
		/control binary inventory.*cli.*library/i,
	);
});

test("binds the complete public fixture witness and source authority", () => {
	const missingWitness = campaign();
	missingWitness.fixture.document.witnesses.pop();
	assert.throws(
		() => merge([fragment({ campaign: missingWitness })], missingWitness),
		/public fixture witness inventory/i,
	);

	const missingMaterializer = campaign();
	missingMaterializer.fixture.document.sourceFiles =
		missingMaterializer.fixture.document.sourceFiles.slice(1);
	assert.throws(
		() => merge([fragment({ campaign: missingMaterializer })], missingMaterializer),
		/public fixture source authority/i,
	);
});

test("rejects malformed skipped and unavailable records", () => {
	const skipped = fragment();
	skipped.cases[0] = {
		...skipped.cases[0],
		attempted: true,
		status: "skipped",
		dependsOn: [],
	};
	assert.throws(
		() => finalizeDerivedChangeDiagnosticFragment(skipped),
		/skipped.*unattempted.*reason.*dependency/i,
	);

	const unavailable = fragment();
	unavailable.cases[0] = {
		...unavailable.cases[0],
		attempted: false,
		status: "unavailable",
		unavailableReason: "",
	};
	assert.throws(
		() => finalizeDerivedChangeDiagnosticFragment(unavailable),
		/unavailable.*reason/i,
	);
});

test("rejects mismatched campaign identities", () => {
	for (const [label, mutate] of [
		["campaign", (value) => (value.campaign.id = "another-campaign")],
		[
			"source",
			(value) => {
				value.campaign.source.commit = commit("9");
				value.campaign.fixture.document.sourceCommit = commit("9");
			},
		],
		[
			"product",
			(value) =>
				(value.campaign.product.binaries[0].binarySha256 = digest("8")),
		],
		[
			"harness",
			(value) =>
				(value.campaign.harness.binaries[0].binarySha256 = digest("7")),
		],
		[
			"control",
			(value) =>
				(value.campaign.control.binaries[0].binarySha256 = digest("6")),
		],
		[
			"fixture",
			(value) => (value.campaign.fixture.authoritySha256 = digest("5")),
		],
	]) {
		const input = fragment();
		mutate(input);
		assert.throws(
			() => merge([finalizeDerivedChangeDiagnosticFragment(input)]),
			new RegExp(
				label === "campaign"
					? "campaign identity differs"
					: "campaign identity differs",
			),
		);
	}

	const platform = fragment();
	platform.platform = {
		...platform.platform,
		hostIdentitySha256: digest("9"),
	};
	assert.throws(
		() => merge([finalizeDerivedChangeDiagnosticFragment(platform)]),
		/platform identity differs/,
	);
});

test("preserves every status and is green only when each required case passed", () => {
	const identity = campaign();
	identity.requiredCaseIds = ["compile"];
	const input = fragment({
		campaign: identity,
		cases: [
			{
				id: "compile",
				lane: "compile",
				required: true,
				attempted: true,
				status: "passed",
				dependsOn: [],
			},
			{
				id: "optional-failure",
				lane: "policy",
				required: false,
				attempted: true,
				status: "failed",
				dependsOn: ["compile"],
				failureClass: "case_failure",
				phase: "policy",
				fixtureCheckpoint: { fixture: "public-fixture", checkpoint: "policy" },
				expected: "passes",
				actual: "failed",
			},
			{
				id: "skipped-child",
				lane: "native",
				required: false,
				attempted: false,
				status: "skipped",
				dependsOn: ["optional-failure"],
				skipReason: "parent assertion failed",
			},
			{
				id: "unknown-optional",
				lane: "platform",
				required: false,
				attempted: false,
				status: "unknown",
				dependsOn: [],
				unknownReason: "host did not return a result",
			},
		],
	});
	const report = merge([input], identity);
	assert.equal(report.verdict, "green");
	assert.deepEqual(report.counts, {
		required: 1,
		attempted: 2,
		passed: 1,
		failed: 1,
		skipped: 1,
		unavailable: 0,
		unknown: 1,
	});
	assert.deepEqual(
		report.cases.map(({ id, status }) => ({ id, status })),
		[
			{ id: "compile", status: "passed" },
			{ id: "optional-failure", status: "failed" },
			{ id: "skipped-child", status: "skipped" },
			{ id: "unknown-optional", status: "unknown" },
		],
	);
});

test("marks a required unavailable case red while retaining it", () => {
	const input = fragment();
	input.cases[0] = {
		...input.cases[0],
		attempted: false,
		status: "unavailable",
		unavailableReason: "Windows host is unavailable",
	};
	const report = merge([finalizeDerivedChangeDiagnosticFragment(input)]);
	assert.equal(report.verdict, "red");
	assert.equal(report.counts.unavailable, 1);
	assert.equal(report.cases[0].status, "unavailable");
});

test("rejects invalid artifact and log SHA-256 identities", () => {
	const artifact = fragment();
	artifact.artifacts[0].sha256 = "not-a-digest";
	assert.throws(
		() => finalizeDerivedChangeDiagnosticFragment(artifact),
		/artifact SHA-256/i,
	);

	const log = fragment();
	log.cases[0].log.sha256 = digest("z");
	assert.throws(
		() => finalizeDerivedChangeDiagnosticFragment(log),
		/log SHA-256/i,
	);
});

test("emits only the non-admissible diagnostic report shape with no conversion", () => {
	const report = merge([fragment()]);
	assert.deepEqual(Object.keys(report).sort(), [
		"admissible",
		"artifactInventory",
		"campaign",
		"cases",
		"counts",
		"lanes",
		"reportSha256",
		"schema",
		"verdict",
		"version",
	]);
	assert.equal(report.schema, DERIVED_CHANGE_DIAGNOSTIC_REPORT_SCHEMA_V1);
	assert.equal(report.admissible, false);
	assert.equal("receipt" in report, false);
	assert.equal("package" in report, false);
	assert.equal("manifest" in report, false);
});

test("binds and verifies the final report self-hash", () => {
	const report = merge([fragment()]);
	assert.match(report.reportSha256, /^[0-9a-f]{64}$/);
	assert.doesNotThrow(() => validateDerivedChangeDiagnosticReport(report));

	const tampered = structuredClone(report);
	tampered.reportSha256 = digest("0");
	assert.throws(
		() => validateDerivedChangeDiagnosticReport(tampered),
		/report SHA-256 differs/,
	);
});

test("binds the reserved root and exact sorted per-platform binary inventories", () => {
	const root = campaign();
	root.rootComponent = "another-root";
	assert.throws(() => merge([fragment()], root), /root component/i);

	const binaries = campaign();
	binaries.product.binaries = [
		{ platformId: "other", binarySha256: digest("8") },
		...binaries.product.binaries,
	];
	assert.throws(() => merge([fragment()], binaries), /binary inventory/i);
});

test("requires contextual classified failed rows and forbids failure fields elsewhere", () => {
	const missing = fragment();
	missing.cases[0] = {
		...missing.cases[0],
		status: "failed",
		failureClass: "case_failure",
		expected: "pass",
		actual: "fail",
	};
	assert.throws(
		() => finalizeDerivedChangeDiagnosticFragment(missing),
		/phase/i,
	);

	const passed = fragment();
	passed.cases[0].failureClass = "case_failure";
	assert.throws(
		() => finalizeDerivedChangeDiagnosticFragment(passed),
		/failure class.*nonfailed/i,
	);
});

test("revalidates derived counts, lanes, inventories, dependencies, and verdict", () => {
	const report = merge([fragment()]);
	const resign = (value) => {
		const preimage = structuredClone(value);
		delete preimage.reportSha256;
		value.reportSha256 = createHash("sha256")
			.update(canonicalJson(preimage))
			.digest("hex");
		return value;
	};
	for (const [mutate, expected] of [
		[(value) => (value.counts.passed = 0), /counts differ/],
		[(value) => (value.lanes[0].counts.passed = 0), /lanes differ/],
		[(value) => (value.artifactInventory = []), /not retained/],
		[(value) => (value.cases[0].dependsOn = ["missing"]), /unknown dependency/],
		[(value) => (value.verdict = "red"), /verdict differs/],
	]) {
		const tampered = structuredClone(report);
		mutate(tampered);
		assert.throws(
			() => validateDerivedChangeDiagnosticReport(resign(tampered)),
			expected,
		);
	}
});

test("uses canonical self-hashes independent of object member insertion order", () => {
	const report = merge([fragment()]);
	const reordered = Object.fromEntries(Object.entries(report).reverse());
	assert.doesNotThrow(() => validateDerivedChangeDiagnosticReport(reordered));
});
