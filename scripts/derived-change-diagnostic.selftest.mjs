import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
	DERIVED_CHANGE_DIAGNOSTIC_COLLECTION_SCHEMA_V1,
	DERIVED_CHANGE_DIAGNOSTIC_REQUEST_SCHEMA_V1,
	DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1,
	assertDerivedChangeDiagnosticOutputRootSafety,
	executeDerivedChangeDiagnosticCases,
	validateDerivedChangeDiagnosticRequest,
} from "./derived-change-diagnostic.mjs";

const digest = (digit) => digit.repeat(64);
const commit = (digit) => digit.repeat(40);
const executableSha256 = createHash("sha256")
	.update(await readFile(process.execPath))
	.digest("hex");
const fixtureAuthorityDocument = {
	schema: "pointbreak.derived-change-public-fixture-authority.v1",
	sourceCommit: commit("a"),
	sourceTree: commit("b"),
	sourceFiles: [
		{
			path: "scripts/materialize-inspector-decision-matrix.sh",
			sha256: digest("4"),
		},
		{
			path: "src/bench_support/derived_access/materializer.rs",
			sha256: digest("5"),
		},
		{
			path: "tests/support/assets/change-ready-store/5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json",
			sha256: digest("6"),
		},
		{
			path: "tests/support/assets/change-ready-store/f31956c2b820926adc74d4d03cb03820d13c9ed2739b5f7ada81611a6f8bcff1.json",
			sha256: digest("7"),
		},
	],
	witnesses: [
		"cycle-conflicted-v1",
		"duplicate-conflict-v1",
		"duplicate-equal-v1",
		"incomplete-v1",
		"missing-carrier-v1",
		"mutated-carrier-v1",
		"removal-v1",
		"topology-v1",
		"wrong-family-carrier-v1",
	].map((fixtureId, index) => ({
		fixtureId,
		authoritativeInventorySha256: digest(String((index + 1) % 10)),
		witnessSha256: digest(String((index + 2) % 10)),
	})),
};
const fixtureAuthorityBytes = `${JSON.stringify(fixtureAuthorityDocument)}\n`;
const fixtureAuthoritySha256 = createHash("sha256")
	.update(fixtureAuthorityBytes)
	.digest("hex");
const fixtureAuthorityRoot = await mkdtemp(
	join(tmpdir(), "pointbreak-diagnostic-fixture-authority-"),
);
const fixtureAuthorityPath = join(
	fixtureAuthorityRoot,
	"fixture-authority.json",
);
await writeFile(fixtureAuthorityPath, fixtureAuthorityBytes);

const campaign = () => ({
	id: "derived-change-diagnostic-001",
	requiredCaseIds: [
		"browser",
		"compile",
		"global-preflight",
		"native-child",
		"native-parent",
		"windows-ntfs",
	],
	requiredPlatformIds: ["macos_apfs", "windows_ntfs"],
	source: {
		commit: commit("a"),
		tree: commit("b"),
		rangeBaseCommit: commit("c"),
		rangeSha256: digest("c"),
	},
	rootComponent: DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1,
	product: {
		binaries: [
			{ platformId: "macos_apfs", binarySha256: executableSha256 },
			{ platformId: "windows_ntfs", binarySha256: executableSha256 },
		],
	},
	harness: {
		binaries: [
			{ platformId: "macos_apfs", binarySha256: executableSha256 },
			{ platformId: "windows_ntfs", binarySha256: executableSha256 },
		],
	},
	control: {
		binaries: [
			{
				platformId: "macos_apfs",
				role: "cli",
				binarySha256: executableSha256,
			},
			{
				platformId: "macos_apfs",
				role: "library",
				binarySha256: executableSha256,
			},
			{
				platformId: "windows_ntfs",
				role: "cli",
				binarySha256: executableSha256,
			},
			{
				platformId: "windows_ntfs",
				role: "library",
				binarySha256: executableSha256,
			},
		],
	},
	fixture: {
		authoritySha256: fixtureAuthoritySha256,
		document: structuredClone(fixtureAuthorityDocument),
	},
	platforms: [
		{
			id: "macos_apfs",
			operatingSystem: "macos",
			architecture: "aarch64",
			filesystem: "apfs",
			hostIdentitySha256: digest("2"),
		},
		{
			id: "windows_ntfs",
			operatingSystem: "windows",
			architecture: "x86_64",
			filesystem: "ntfs",
			hostIdentitySha256: digest("3"),
		},
	],
});

const command = (source, { exitCode = 0 } = {}) => ({
	program: process.execPath,
	args: [
		"-e",
		`const fs=require("node:fs");fs.writeFileSync(process.env.POINTBREAK_DIAGNOSTIC_CASE_ROOT+"/artifact.txt",${JSON.stringify(source)});process.exit(${exitCode})`,
	],
	artifactPaths: ["artifact.txt"],
});

const diagnosticRoot = async (prefix) =>
	join(
		await mkdtemp(join(tmpdir(), prefix)),
		DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1,
	);

const request = (root) => ({
	schema: DERIVED_CHANGE_DIAGNOSTIC_REQUEST_SCHEMA_V1,
	campaign: campaign(),
	platformId: "macos_apfs",
	identityPaths: {
		product: process.execPath,
		harness: process.execPath,
		control: process.execPath,
		controlCli: process.execPath,
		fixtureAuthority: fixtureAuthorityPath,
	},
	requiredExecutables: [process.execPath],
	outputRoot: root,
	cases: [
		{
			id: "global-preflight",
			lane: "preflight",
			required: true,
			dependsOn: [],
			failureClass: "global_invalid",
			phase: "preflight",
			fixtureCheckpoint: { fixture: "public-fixture", checkpoint: "preflight" },
			mutatesRoot: false,
			...command("preflight"),
		},
		{
			id: "compile",
			lane: "compile",
			required: true,
			dependsOn: ["global-preflight"],
			failureClass: "case_failure",
			phase: "compile",
			fixtureCheckpoint: { fixture: "public-fixture", checkpoint: "compile" },
			mutatesRoot: false,
			...command("compile", { exitCode: 7 }),
		},
		{
			id: "native-parent",
			lane: "native",
			required: true,
			dependsOn: ["global-preflight"],
			failureClass: "lane_invalid",
			phase: "native",
			fixtureCheckpoint: { fixture: "public-fixture", checkpoint: "native" },
			mutatesRoot: true,
			...command("native", { exitCode: 9 }),
		},
		{
			id: "native-child",
			lane: "native",
			required: true,
			dependsOn: ["native-parent"],
			failureClass: "case_failure",
			phase: "native",
			fixtureCheckpoint: {
				fixture: "public-fixture",
				checkpoint: "native-child",
			},
			mutatesRoot: true,
			...command("must not run"),
		},
		{
			id: "browser",
			lane: "browser",
			required: true,
			dependsOn: ["global-preflight"],
			failureClass: "case_failure",
			phase: "browser",
			fixtureCheckpoint: { fixture: "public-fixture", checkpoint: "browser" },
			mutatesRoot: true,
			...command("browser"),
		},
		{
			id: "windows-ntfs",
			lane: "windows_ntfs",
			required: true,
			dependsOn: ["global-preflight"],
			unavailableReason: "host fragment was not supplied",
		},
	],
});

function retainOnlyMacosBinaryAuthority(input) {
	for (const identity of [input.campaign.product, input.campaign.harness]) {
		identity.binaries = identity.binaries.filter(
			({ platformId }) => platformId === "macos_apfs",
		);
	}
	input.campaign.control.binaries = input.campaign.control.binaries.filter(
		({ platformId }) => platformId === "macos_apfs",
	);
}

test("continues independent cases and skips only a failed dependency subtree", async () => {
	const root = await diagnosticRoot("pointbreak-diagnostic-cases-");
	const result = await executeDerivedChangeDiagnosticCases(request(root));
	assert.deepEqual(
		result.cases.map(({ id, status }) => ({ id, status })),
		[
			{ id: "global-preflight", status: "passed" },
			{ id: "compile", status: "failed" },
			{ id: "native-parent", status: "failed" },
			{ id: "native-child", status: "skipped" },
			{ id: "browser", status: "passed" },
			{ id: "windows-ntfs", status: "unavailable" },
		],
	);
	assert.equal(result.cases[2].failureClass, "lane_invalid");
	assert.match(result.cases[3].skipReason, /native-parent/);
	assert.match(result.cases[5].unavailableReason, /host fragment/);
	assert.equal(
		await readFile(join(root, "cases", "browser", "artifact.txt"), "utf8"),
		"browser",
	);
	assert.equal(
		result.artifacts.some((artifact) => artifact.path === "manifest.json"),
		false,
	);
	assert.equal(
		result.artifacts.some((artifact) => artifact.path === "package.json"),
		false,
	);
});

test("captures stable command logs and declared artifacts by SHA-256", async () => {
	const root = await diagnosticRoot("pointbreak-diagnostic-hashes-");
	const input = request(root);
	input.cases = input.cases.slice(0, 1);
	input.campaign.requiredCaseIds = ["global-preflight"];
	input.campaign.requiredPlatformIds = ["macos_apfs"];
	input.campaign.platforms = input.campaign.platforms.slice(0, 1);
	for (const identity of [input.campaign.product, input.campaign.harness])
		identity.binaries = identity.binaries.slice(0, 1);
	input.campaign.control.binaries = input.campaign.control.binaries.slice(0, 2);
	const result = await executeDerivedChangeDiagnosticCases(input);
	const [row] = result.cases;
	assert.match(row.log.sha256, /^[0-9a-f]{64}$/);
	assert.deepEqual(row.artifactPaths, ["cases/global-preflight/artifact.txt"]);
	assert.match(
		result.artifacts.find(({ path }) => path === row.artifactPaths[0]).sha256,
		/^[0-9a-f]{64}$/,
	);
});

test("request validation refuses duplicate mutable roots and shell-shaped commands", async () => {
	const root = await diagnosticRoot("pointbreak-diagnostic-invalid-");
	const input = request(root);
	input.cases[3].root = "shared";
	input.cases[4].root = "shared";
	assert.throws(
		() => validateDerivedChangeDiagnosticRequest(input),
		/duplicate mutable case root/,
	);

	const shell = request(root);
	shell.cases[0].program = "sh -c";
	assert.throws(
		() => validateDerivedChangeDiagnosticRequest(shell),
		/absolute executable path/,
	);
});

test("a global-invalid preflight prevents every later command", async () => {
	const root = await diagnosticRoot("pointbreak-diagnostic-global-");
	const input = request(root);
	input.cases[0] = {
		...input.cases[0],
		...command("invalid", { exitCode: 5 }),
	};
	const result = await executeDerivedChangeDiagnosticCases(input);
	assert.equal(result.cases[0].status, "failed");
	assert.ok(result.cases.slice(1).every(({ status }) => status === "skipped"));
	assert.ok(
		result.cases
			.slice(1)
			.every(({ skipReason }) => /global-preflight/.test(skipReason)),
	);
});

test("the runner refuses a non-empty output root before executing a case", async () => {
	const root = await diagnosticRoot("pointbreak-diagnostic-nonempty-");
	await mkdir(root);
	await writeFile(join(root, "owner.txt"), "preserve me");
	await assert.rejects(
		() => executeDerivedChangeDiagnosticCases(request(root)),
		/output root must be empty/,
	);
	assert.equal(await readFile(join(root, "owner.txt"), "utf8"), "preserve me");
});

test("preflights every executable, records global invalidity, and does not start commands", async () => {
	const root = await diagnosticRoot("pointbreak-diagnostic-executable-");
	const input = request(root);
	input.cases[4].program = "/definitely/not/a-pointbreak-executable";
	const result = await executeDerivedChangeDiagnosticCases(input);
	assert.equal(result.cases[0].status, "failed");
	assert.equal(result.cases[0].failureClass, "global_invalid");
	assert.ok(result.cases.slice(1).every(({ status }) => status === "skipped"));
});

test("sanitizes owner-store state and records missing declared artifacts without stopping peers", async () => {
	const root = await diagnosticRoot("pointbreak-diagnostic-sanitized-");
	const input = request(root);
	input.cases[0] = {
		...input.cases[0],
		...command("preflight"),
		args: [
			"-e",
			`require('node:fs').writeFileSync(process.env.POINTBREAK_DIAGNOSTIC_CASE_ROOT + '/artifact.txt', process.env.POINTBREAK_HOME ?? 'unset')`,
		],
	};
	input.cases[1].artifactPaths = ["missing.txt"];
	const result = await executeDerivedChangeDiagnosticCases(input);
	assert.equal(result.cases[1].status, "failed");
	assert.match(result.cases[1].actual.artifactErrors[0], /not retained|ENOENT/);
	assert.equal(
		result.cases.find(({ id }) => id === "browser").status,
		"passed",
	);
	assert.equal(
		await readFile(
			join(root, "cases", "global-preflight", "artifact.txt"),
			"utf8",
		),
		"unset",
	);
	assert.match(result.fragmentSha256, /^[0-9a-f]{64}$/);
});

test("a case-root creation failure is retained without aborting an independent peer", async () => {
	const root = await diagnosticRoot("pointbreak-diagnostic-case-root-failure-");
	const input = request(root);
	input.campaign.requiredCaseIds = ["broken", "maker", "peer"];
	input.campaign.requiredPlatformIds = ["macos_apfs"];
	input.campaign.platforms = input.campaign.platforms.slice(0, 1);
	retainOnlyMacosBinaryAuthority(input);
	input.cases = [
		{
			...input.cases[0],
			id: "maker",
			root: "maker",
			args: [
				"-e",
				`const fs=require("node:fs"),p=require("node:path");fs.mkdirSync(p.join(p.dirname(process.env.POINTBREAK_DIAGNOSTIC_CASE_ROOT),"broken"));fs.writeFileSync(process.env.POINTBREAK_DIAGNOSTIC_CASE_ROOT+"/artifact.txt","maker")`,
			],
		},
		{
			...input.cases[4],
			id: "broken",
			root: "broken",
			dependsOn: ["maker"],
			artifactPaths: [],
		},
		{
			...input.cases[4],
			id: "peer",
			root: "peer",
			dependsOn: ["maker"],
		},
	];

	const result = await executeDerivedChangeDiagnosticCases(input);
	assert.deepEqual(
		result.cases.map(({ id, status }) => ({ id, status })),
		[
			{ id: "maker", status: "passed" },
			{ id: "broken", status: "failed" },
			{ id: "peer", status: "passed" },
		],
	);
	assert.match(JSON.stringify(result.cases[1].actual), /EEXIST/);
});

test("expands a collected child failure without losing it to launcher aggregation", async () => {
	const root = await diagnosticRoot("pointbreak-diagnostic-collection-");
	const input = request(root);
	input.campaign.requiredCaseIds = ["collection-browser"];
	input.campaign.requiredPlatformIds = ["macos_apfs"];
	input.campaign.platforms = input.campaign.platforms.slice(0, 1);
	retainOnlyMacosBinaryAuthority(input);
	input.cases = [
		{
			id: "launcher",
			lane: "browser",
			required: false,
			dependsOn: [],
			failureClass: "case_failure",
			phase: "browser-collection",
			fixtureCheckpoint: { fixture: "public-fixture", checkpoint: "browser" },
			program: process.execPath,
			args: [
				"-e",
				`process.stdout.write(${JSON.stringify(
					JSON.stringify({
						schema: DERIVED_CHANGE_DIAGNOSTIC_COLLECTION_SCHEMA_V1,
						campaignId: "derived-change-diagnostic-001",
						cases: [
							{
								id: "browser",
								lane: "browser",
								required: true,
								status: "failed",
								dependsOn: ["launcher"],
								failureClass: "case_failure",
								phase: "browser-vector",
								fixtureCheckpoint: {
									fixture: "public-fixture",
									checkpoint: "vector",
								},
								expected: "pass",
								actual: "fail",
							},
						],
					}),
				)})`,
			],
			collection: {
				schema: DERIVED_CHANGE_DIAGNOSTIC_COLLECTION_SCHEMA_V1,
				source: "stdout",
				idPrefix: "collection-",
				expectedCaseIds: ["browser"],
			},
			mutatesRoot: true,
		},
	];
	const result = await executeDerivedChangeDiagnosticCases(input);
	assert.deepEqual(
		result.cases.map(({ id, status }) => ({ id, status })),
		[
			{ id: "launcher", status: "passed" },
			{ id: "collection-browser", status: "failed" },
		],
	);
	assert.equal(result.cases[1].failureClass, "case_failure");
});

test("a collected global-invalid child stops later host observations", async () => {
	const root = await diagnosticRoot("pointbreak-diagnostic-collection-global-");
	const input = request(root);
	input.campaign.requiredCaseIds = ["collection-child", "peer"];
	input.campaign.requiredPlatformIds = ["macos_apfs"];
	input.campaign.platforms = input.campaign.platforms.slice(0, 1);
	retainOnlyMacosBinaryAuthority(input);
	const collection = {
		schema: DERIVED_CHANGE_DIAGNOSTIC_COLLECTION_SCHEMA_V1,
		campaignId: input.campaign.id,
		cases: [
			{
				id: "child",
				lane: "native",
				required: true,
				status: "failed",
				dependsOn: [],
				failureClass: "global_invalid",
				phase: "fixture-authority",
				fixtureCheckpoint: {
					fixture: "public-fixture",
					checkpoint: "authority",
				},
				expected: "exact public authority",
				actual: "mismatch",
			},
		],
	};
	input.cases = [
		{
			id: "launcher",
			lane: "native",
			required: false,
			dependsOn: [],
			failureClass: "global_invalid",
			phase: "native-collection",
			fixtureCheckpoint: {
				fixture: "public-fixture",
				checkpoint: "native",
			},
			program: process.execPath,
			args: [
				"-e",
				`process.stdout.write(${JSON.stringify(JSON.stringify(collection))})`,
			],
			collection: {
				schema: DERIVED_CHANGE_DIAGNOSTIC_COLLECTION_SCHEMA_V1,
				source: "stdout",
				idPrefix: "collection-",
				expectedCaseIds: ["child"],
			},
			mutatesRoot: true,
		},
		{
			...request(root).cases[4],
			id: "peer",
			dependsOn: [],
			root: "peer",
		},
	];

	const result = await executeDerivedChangeDiagnosticCases(input);
	assert.deepEqual(
		result.cases.map(({ id, status }) => ({ id, status })),
		[
			{ id: "launcher", status: "passed" },
			{ id: "collection-child", status: "failed" },
			{ id: "peer", status: "skipped" },
		],
	);
	assert.match(result.cases[2].skipReason, /collection-child/);
});

test("deduplicates shared collection artifacts retained by the launcher and children", async () => {
	const root = await diagnosticRoot(
		"pointbreak-diagnostic-collection-artifacts-",
	);
	const input = request(root);
	input.campaign.requiredCaseIds = ["collection-first", "collection-second"];
	input.campaign.requiredPlatformIds = ["macos_apfs"];
	input.campaign.platforms = input.campaign.platforms.slice(0, 1);
	retainOnlyMacosBinaryAuthority(input);
	const collection = {
		schema: DERIVED_CHANGE_DIAGNOSTIC_COLLECTION_SCHEMA_V1,
		campaignId: input.campaign.id,
		artifactPaths: ["shared.log"],
		cases: ["first", "second"].map((id) => ({
			id,
			lane: "native",
			required: true,
			status: "passed",
			dependsOn: [],
			artifactPaths: ["shared.log"],
		})),
	};
	input.cases = [
		{
			id: "launcher",
			lane: "native",
			required: false,
			dependsOn: [],
			failureClass: "lane_invalid",
			phase: "native-collection",
			fixtureCheckpoint: { fixture: "public-fixture", checkpoint: "native" },
			program: process.execPath,
			args: [
				"-e",
				`const fs=require("node:fs");fs.writeFileSync(process.env.POINTBREAK_DIAGNOSTIC_CASE_ROOT+"/shared.log","shared");process.stdout.write(${JSON.stringify(JSON.stringify(collection))})`,
			],
			collection: {
				schema: DERIVED_CHANGE_DIAGNOSTIC_COLLECTION_SCHEMA_V1,
				source: "stdout",
				idPrefix: "collection-",
				expectedCaseIds: ["first", "second"],
			},
			mutatesRoot: true,
		},
	];
	const result = await executeDerivedChangeDiagnosticCases(input);
	assert.equal(
		result.artifacts.filter(({ path }) => path.endsWith("/shared.log")).length,
		1,
	);
	assert.ok(
		result.cases
			.slice(1)
			.every(({ artifactPaths }) => artifactPaths.length === 1),
	);
});

test("a recoverable collection exit cannot hide a missing declared artifact", async () => {
	const root = await diagnosticRoot(
		"pointbreak-diagnostic-collection-missing-",
	);
	const input = request(root);
	input.campaign.requiredCaseIds = ["collection-child"];
	input.campaign.requiredPlatformIds = ["macos_apfs"];
	input.campaign.platforms = input.campaign.platforms.slice(0, 1);
	retainOnlyMacosBinaryAuthority(input);
	input.cases = [
		{
			id: "launcher",
			lane: "browser",
			required: false,
			dependsOn: [],
			failureClass: "lane_invalid",
			phase: "browser-collection",
			fixtureCheckpoint: { fixture: "public-fixture", checkpoint: "browser" },
			program: process.execPath,
			args: [
				"-e",
				`process.stdout.write(${JSON.stringify(
					JSON.stringify({
						schema: DERIVED_CHANGE_DIAGNOSTIC_COLLECTION_SCHEMA_V1,
						campaignId: input.campaign.id,
						cases: [
							{
								id: "child",
								lane: "browser",
								required: true,
								status: "passed",
								dependsOn: [],
							},
						],
					}),
				)});process.exit(1)`,
			],
			artifactPaths: ["missing-result.json"],
			collection: {
				schema: DERIVED_CHANGE_DIAGNOSTIC_COLLECTION_SCHEMA_V1,
				source: "stdout",
				idPrefix: "collection-",
				expectedCaseIds: ["child"],
				completeExitCodes: [1],
			},
			mutatesRoot: true,
		},
	];
	const result = await executeDerivedChangeDiagnosticCases(input);
	assert.equal(result.cases[0].status, "failed");
	assert.equal(result.cases[1].status, "skipped");
	assert.match(result.cases[0].actual.artifactErrors[0], /missing-result/);
});

test("retains an explicit public allowed-signers authority before signature preflight", async () => {
	const root = await diagnosticRoot("pointbreak-diagnostic-signers-");
	const authorityDirectory = await mkdtemp(
		join(tmpdir(), "pointbreak-diagnostic-authority-"),
	);
	const authority = join(authorityDirectory, "allowed-signers");
	await writeFile(authority, "public-authority-only\n");
	const input = request(root);
	input.cases = input.cases.slice(0, 1);
	input.campaign.requiredCaseIds = ["global-preflight"];
	input.campaign.requiredPlatformIds = ["macos_apfs"];
	input.campaign.platforms = input.campaign.platforms.slice(0, 1);
	retainOnlyMacosBinaryAuthority(input);
	input.sourcePreflight = {
		sourceRoot: process.cwd(),
		allowedSignersPath: authority,
	};
	const result = await executeDerivedChangeDiagnosticCases(input);
	assert.equal(result.cases[0].failureClass, "global_invalid");
	assert.match(
		result.artifacts.find(({ path }) => path === "authority/allowed-signers")
			.sha256,
		/^[0-9a-f]{64}$/,
	);
});

test("rejects destructive-root symlinks and canonical owner-store targets", async () => {
	const parent = await mkdtemp(
		join(tmpdir(), "pointbreak-diagnostic-owner-link-"),
	);
	const ownerStore = join(parent, "owner-data");
	const indirection = join(parent, "indirection");
	await mkdir(ownerStore);
	await symlink(
		ownerStore,
		indirection,
		process.platform === "win32" ? "junction" : "dir",
	);
	const oldPointbreakHome = process.env.POINTBREAK_HOME;
	process.env.POINTBREAK_HOME = ownerStore;
	try {
		await assert.rejects(
			() =>
				assertDerivedChangeDiagnosticOutputRootSafety(
					join(indirection, DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1),
					process.cwd(),
				),
			/cannot enter POINTBREAK_HOME/,
		);
	} finally {
		if (oldPointbreakHome === undefined) delete process.env.POINTBREAK_HOME;
		else process.env.POINTBREAK_HOME = oldPointbreakHome;
	}

	const target = await mkdtemp(
		join(tmpdir(), "pointbreak-diagnostic-root-target-"),
	);
	const linkParent = await mkdtemp(
		join(tmpdir(), "pointbreak-diagnostic-root-link-"),
	);
	const linkedRoot = join(
		linkParent,
		DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1,
	);
	await symlink(
		target,
		linkedRoot,
		process.platform === "win32" ? "junction" : "dir",
	);
	await assert.rejects(
		() =>
			assertDerivedChangeDiagnosticOutputRootSafety(linkedRoot, process.cwd()),
		/must not be a symbolic link/,
	);
});

test("rejects source-local destructive roots and globalizes an exact identity mismatch", async () => {
	const sourceLocal = request(
		join(process.cwd(), DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1),
	);
	sourceLocal.sourcePreflight = { sourceRoot: process.cwd() };
	assert.throws(
		() => validateDerivedChangeDiagnosticRequest(sourceLocal),
		/outside the source checkout/,
	);

	const root = await diagnosticRoot("pointbreak-diagnostic-identity-");
	const input = request(root);
	input.identityPaths.fixtureAuthority = "/definitely/not/a-fixture-authority";
	const result = await executeDerivedChangeDiagnosticCases(input);
	assert.equal(result.cases[0].failureClass, "global_invalid");
	assert.match(JSON.stringify(result.cases[0].actual), /fixture authority/);
});

test("rejects a fixture authority file whose document differs from the campaign", async () => {
	const authorityDirectory = await mkdtemp(
		join(tmpdir(), "pointbreak-diagnostic-fixture-document-"),
	);
	const authority = join(authorityDirectory, "fixture-authority.json");
	const differentDocument = structuredClone(fixtureAuthorityDocument);
	differentDocument.witnesses[0].witnessSha256 = digest("f");
	const bytes = `${JSON.stringify(differentDocument)}\n`;
	await writeFile(authority, bytes);

	const root = await diagnosticRoot("pointbreak-diagnostic-fixture-document-");
	const input = request(root);
	input.identityPaths.fixtureAuthority = authority;
	input.campaign.fixture.authoritySha256 = createHash("sha256")
		.update(bytes)
		.digest("hex");
	const result = await executeDerivedChangeDiagnosticCases(input);
	assert.equal(result.cases[0].failureClass, "global_invalid");
	assert.match(
		JSON.stringify(result.cases[0].actual),
		/fixture authority document differs from the campaign/,
	);
});
