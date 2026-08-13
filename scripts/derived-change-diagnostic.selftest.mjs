import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
	chmod,
	copyFile,
	mkdir,
	mkdtemp,
	readFile,
	realpath,
	symlink,
	writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";

import {
	DERIVED_CHANGE_DIAGNOSTIC_COLLECTION_SCHEMA_V1,
	DERIVED_CHANGE_DIAGNOSTIC_REQUEST_SCHEMA_V1,
	DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1,
	assertDerivedChangeDiagnosticOutputRootSafety,
	executeDerivedChangeDiagnosticCases,
	sha256DerivedChangeDiagnosticTree,
	validateDerivedChangeDiagnosticRequest,
	verifyDerivedChangeDiagnosticBindings,
} from "./derived-change-diagnostic.mjs";
import {
	derivedChangeDiagnosticProgramNamesForOperatingSystem,
	derivedChangeDiagnosticTreeBoundProgramNamesForOperatingSystem,
} from "./derived-change-diagnostic-report.mjs";

const digest = (digit) => digit.repeat(64);
const commit = (digit) => digit.repeat(40);
const executableSha256 = createHash("sha256")
	.update(await readFile(process.execPath))
	.digest("hex");
const executableTreeRoot = await mkdtemp(
	join(await realpath(tmpdir()), "pointbreak-diagnostic-bound-tools-"),
);
const treeBoundExecutable = join(executableTreeRoot, "node");
await copyFile(process.execPath, treeBoundExecutable);
await chmod(treeBoundExecutable, 0o755);
const executableTreeSha256 =
	await sha256DerivedChangeDiagnosticTree(executableTreeRoot);
const fixtureAuthorityDocument = {
	schema: "pointbreak.derived-change-public-fixture-authority.v2",
	sourceCommit: commit("a"),
	sourceTree: commit("b"),
	sourceFiles: [
		{
			path: "scripts/derived-change-diagnostic-fixture.mjs",
			sha256: digest("3"),
		},
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
	]
		.filter((fixtureId) => fixtureId !== "topology-v1")
		.map((fixtureId, index) => ({
			fixtureId,
			authoritativeInventorySha256: digest(String((index + 1) % 10)),
			witnessSha256: digest(String((index + 2) % 10)),
		})),
	topologyCheckpoint: {
		schema: "pointbreak.derived-change-topology-fixture-checkpoint.v1",
		fixtureId: "topology-v1",
		checkpointSha256: digest("9"),
	},
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

const macosProgramNames =
	derivedChangeDiagnosticProgramNamesForOperatingSystem("macos");
const windowsProgramNames =
	derivedChangeDiagnosticProgramNamesForOperatingSystem("windows");
const macosTreeBoundProgramNames = new Set(
	derivedChangeDiagnosticTreeBoundProgramNamesForOperatingSystem("macos"),
);
const windowsTreeBoundProgramNames = new Set(
	derivedChangeDiagnosticTreeBoundProgramNamesForOperatingSystem("windows"),
);

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
	signatureAuthoritySha256: fixtureAuthoritySha256,
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
	programs: [
		...macosProgramNames.map((name) => ({
				platformId: "macos_apfs",
				name,
				program: macosTreeBoundProgramNames.has(name)
					? treeBoundExecutable
					: process.execPath,
				binarySha256: executableSha256,
				...(macosTreeBoundProgramNames.has(name)
					? {
							treeRoot: executableTreeRoot,
							treeSha256: executableTreeSha256,
						}
					: {}),
			})),
		...windowsProgramNames.map((name) => ({
			platformId: "windows_ntfs",
			name,
			program: `C:\\tools\\${name}.exe`,
			binarySha256: executableSha256,
			...(windowsTreeBoundProgramNames.has(name)
				? { treeRoot: "C:\\tools", treeSha256: executableTreeSha256 }
				: {}),
		})),
	],
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
			systemRoot: "c:\\windows",
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

const diagnosticRoot = async (prefix) => {
	const parent = await mkdtemp(join(await realpath(tmpdir()), prefix));
	await mkdir(join(parent, "scratch"));
	return join(parent, DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1);
};

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
	temporaryRoot: join(dirname(root), "scratch"),
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

function sourcePreflight(input, sourceRoot) {
	const programs = new Map(
		input.campaign.programs
			.filter(({ platformId }) => platformId === input.platformId)
			.map((identity) => [identity.name, identity]),
	);
	const git = programs.get("git");
	return {
		sourceRoot,
		gitProgram: git.program,
		gitExecPath: join(git.treeRoot, "libexec", "git-core"),
		sshKeygenProgram: programs.get("sshKeygen").program,
		allowedSignersSha256: input.campaign.signatureAuthoritySha256,
	};
}

function retainOnlyMacosBinaryAuthority(input) {
	for (const identity of [input.campaign.product, input.campaign.harness]) {
		identity.binaries = identity.binaries.filter(
			({ platformId }) => platformId === "macos_apfs",
		);
	}
	input.campaign.control.binaries = input.campaign.control.binaries.filter(
		({ platformId }) => platformId === "macos_apfs",
	);
	input.campaign.programs = input.campaign.programs.filter(
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

test("creates a nested absent output root after validating its existing ancestor", async () => {
	const parent = await mkdtemp(
		join(await realpath(tmpdir()), "pointbreak-diagnostic-nested-output-"),
	);
	const scratch = join(parent, "scratch");
	const root = join(
		parent,
		"evidence",
		"host",
		DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1,
	);
	await mkdir(scratch);
	const input = request(root);
	input.temporaryRoot = scratch;

	const result = await executeDerivedChangeDiagnosticCases(input);

	assert.equal(result.cases.length, input.cases.length);
	assert.equal(
		await readFile(join(root, "cases", "browser", "artifact.txt"), "utf8"),
		"browser",
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
	input.campaign.programs = input.campaign.programs.filter(
		({ platformId }) => platformId === "macos_apfs",
	);
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
	const missingProgram = "/definitely/not/a-pointbreak-executable";
	input.cases[4].program = missingProgram;
	const identity = input.campaign.programs.find(
		({ platformId, name }) =>
			platformId === input.platformId && name === "cargoNextest",
	);
	identity.program = missingProgram;
	identity.binarySha256 = digest("f");
	const result = await executeDerivedChangeDiagnosticCases(input);
	assert.equal(result.cases[0].status, "failed");
	assert.equal(result.cases[0].failureClass, "global_invalid");
	assert.ok(result.cases.slice(1).every(({ status }) => status === "skipped"));
});

test("rejects a tree-bound program through an escaping symlink ancestor", async () => {
	const root = await diagnosticRoot("pointbreak-diagnostic-tree-escape-");
	const tree = await mkdtemp(
		join(await realpath(tmpdir()), "pointbreak-diagnostic-tree-root-"),
	);
	const outside = await mkdtemp(
		join(await realpath(tmpdir()), "pointbreak-diagnostic-tree-outside-"),
	);
	await copyFile(process.execPath, join(outside, "node"));
	await chmod(join(outside, "node"), 0o755);
	await symlink(
		outside,
		join(tree, "bin"),
		process.platform === "win32" ? "junction" : "dir",
	);
	const input = request(root);
	for (const identity of input.campaign.programs.filter(
		({ platformId, name }) =>
			platformId === "macos_apfs" && macosTreeBoundProgramNames.has(name),
	)) {
		identity.program = join(tree, "bin", "node");
		identity.treeRoot = tree;
		identity.treeSha256 = digest("f");
	}
	const failure = await verifyDerivedChangeDiagnosticBindings(input);
	assert.ok(failure?.programIdentityFailures?.length);
	assert.match(
		JSON.stringify(failure.programIdentityFailures),
		/must not traverse symbolic links/,
	);
});

test("rejects conflicting authority hashes for a shared program dependency tree", async () => {
	const root = await diagnosticRoot("pointbreak-diagnostic-tree-conflict-");
	const input = request(root);
	input.campaign.programs.find(
		({ platformId, name }) => platformId === "macos_apfs" && name === "cargo",
	).treeSha256 = digest("f");
	const failure = await verifyDerivedChangeDiagnosticBindings(input);
	assert.ok(failure?.programIdentityFailures?.length);
	assert.match(
		JSON.stringify(failure.programIdentityFailures),
		/shared program dependency tree authority differs/,
	);
});

test("source signature preflight uses the bound Git helper with no ambient PATH", async () => {
	const root = await diagnosticRoot("pointbreak-diagnostic-bound-signature-");
	const tools = await mkdtemp(
		join(await realpath(tmpdir()), "pointbreak-diagnostic-bound-git-"),
	);
	const bin = join(tools, "bin");
	const execPath = join(tools, "libexec", "git-core");
	const marker = `${tools}-invocations.jsonl`;
	await Promise.all([
		mkdir(bin, { recursive: true }),
		mkdir(execPath, { recursive: true }),
	]);
	const git = join(bin, "git");
	const input = request(root);
	const source = input.campaign.source;
	source.rangeSha256 = createHash("sha256").update("").digest("hex");
	await writeFile(
		git,
		`#!${process.execPath}
import { appendFileSync } from "node:fs";
appendFileSync(${JSON.stringify(marker)}, JSON.stringify({ args: process.argv.slice(2), path: process.env.PATH, execPath: process.env.GIT_EXEC_PATH }) + "\\n");
const args = process.argv.slice(2).join(" ");
if (args.endsWith("rev-parse HEAD") || args.includes("^{commit}")) process.stdout.write(${JSON.stringify(`${source.commit}\n`)});
else if (args.includes("^{tree}")) process.stdout.write(${JSON.stringify(`${source.tree}\n`)});
`,
	);
	await chmod(git, 0o755);
	const gitIdentity = input.campaign.programs.find(
		({ platformId, name }) =>
			platformId === input.platformId && name === "git",
	);
	gitIdentity.program = git;
	gitIdentity.binarySha256 = createHash("sha256")
		.update(await readFile(git))
		.digest("hex");
	gitIdentity.treeRoot = tools;
	gitIdentity.treeSha256 = await sha256DerivedChangeDiagnosticTree(tools);
	input.sourcePreflight = {
		sourceRoot: process.cwd(),
		gitProgram: git,
		gitExecPath: execPath,
		sshKeygenProgram: process.execPath,
		allowedSignersPath: fixtureAuthorityPath,
		allowedSignersSha256: fixtureAuthoritySha256,
	};
	const failure = await verifyDerivedChangeDiagnosticBindings(input);
	assert.ok(failure?.sourcePreflightFailure);
	const invocations = (await readFile(marker, "utf8"))
		.trim()
		.split("\n")
		.map((line) => JSON.parse(line));
	const signature = invocations.find(({ args }) =>
		args.includes("verify-commit"),
	);
	assert.ok(signature);
	assert.ok(
		signature.args.includes(`gpg.ssh.program=${process.execPath}`),
	);
	assert.equal(signature.path, "");
	assert.equal(signature.execPath, execPath);
});

test("sanitizes owner-store state and records missing declared artifacts without stopping peers", async () => {
	const root = await diagnosticRoot("pointbreak-diagnostic-sanitized-");
	const invalid = request(root);
	invalid.cases[0].env = { pointbreak_home: "/must-not-be-admitted" };
	assert.throws(
		() => validateDerivedChangeDiagnosticRequest(invalid),
		/owner-store state/,
	);
	const input = request(root);
	input.cases[0] = {
		...input.cases[0],
		...command("preflight"),
		args: [
			"-e",
			`const ownerNames=new Set(['POINTBREAK_HOME','POINTBREAK_STORE','POINTBREAK_QUALIFICATION_CORPUS','POINTBREAK_DERIVED_ACCESS','POINTBREAK_CHANGE_READY_FIXTURE_DIR']);const observed=Object.keys(process.env).filter((key)=>ownerNames.has(key.toUpperCase()));require('node:fs').writeFileSync(process.env.POINTBREAK_DIAGNOSTIC_CASE_ROOT + '/artifact.txt', observed.join(',') || 'unset')`,
		],
	};
	input.cases[1].artifactPaths = ["missing.txt"];
	const ownerEnvironment = {
		pointbreak_home: process.env.pointbreak_home,
		Pointbreak_Change_Ready_Fixture_Dir:
			process.env.Pointbreak_Change_Ready_Fixture_Dir,
	};
	process.env.pointbreak_home = "/ambient-owner-store";
	process.env.Pointbreak_Change_Ready_Fixture_Dir =
		"/ambient-change-ready-fixture";
	let result;
	try {
		result = await executeDerivedChangeDiagnosticCases(input);
	} finally {
		for (const [key, value] of Object.entries(ownerEnvironment)) {
			if (value === undefined) delete process.env[key];
			else process.env[key] = value;
		}
	}
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

test("routes each case's temporary environment through an isolated scratch directory", async () => {
	const root = await diagnosticRoot("pointbreak-diagnostic-scratch-env-");
	const input = request(root);
	input.campaign.requiredCaseIds = ["global-preflight"];
	input.campaign.requiredPlatformIds = ["macos_apfs"];
	input.campaign.platforms = input.campaign.platforms.slice(0, 1);
	retainOnlyMacosBinaryAuthority(input);
	input.cases = [
		{
			...input.cases[0],
			env: {
				CARGO_TARGET_DIR: "/must-be-overridden",
				CC: "/bound/cc",
				CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER: "/bound/linker",
				GIT_EXEC_PATH: "/bound/git-core",
				PATH: "/bound/bin",
			},
			args: [
				"-e",
				`require('node:fs').writeFileSync(process.env.POINTBREAK_DIAGNOSTIC_CASE_ROOT+'/artifact.txt',JSON.stringify({caseRoot:process.env.POINTBREAK_DIAGNOSTIC_CASE_ROOT,workRoot:process.env.POINTBREAK_DIAGNOSTIC_WORK_ROOT,home:process.env.HOME,userprofile:process.env.USERPROFILE,npmCache:process.env.npm_config_cache,xdgCache:process.env.XDG_CACHE_HOME,xdgConfig:process.env.XDG_CONFIG_HOME,xdgData:process.env.XDG_DATA_HOME,xdgState:process.env.XDG_STATE_HOME,appData:process.env.APPDATA,localAppData:process.env.LOCALAPPDATA,tmpdir:process.env.TMPDIR,tmp:process.env.TMP,temp:process.env.TEMP,cargo:process.env.CARGO,cargoBuildRustc:process.env.CARGO_BUILD_RUSTC,cargoNetOffline:process.env.CARGO_NET_OFFLINE,cargoTargetRustc:process.env.CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTC,nextestProfile:process.env.NEXTEST_PROFILE,nextestUserConfig:process.env.NEXTEST_USER_CONFIG_FILE,rustc:process.env.RUSTC,rustcWrapper:process.env.RUSTC_WRAPPER,rustupToolchain:process.env.RUSTUP_TOOLCHAIN,cargoHome:process.env.CARGO_HOME,rustupHome:process.env.RUSTUP_HOME,target:process.env.CARGO_TARGET_DIR,path:process.env.PATH,cc:process.env.CC,hostCc:process.env.HOST_CC,targetCxx:process.env.TARGET_CXX,hostAr:process.env.HOST_AR,targetRanlib:process.env.TARGET_RANLIB,comSpec:process.env.ComSpec,linker:process.env.CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER,gitExecPath:process.env.GIT_EXEC_PATH}))`,
			],
		},
	];

	const ambient = Object.fromEntries(
		[
			"CARGO",
			"CARGO_BUILD_RUSTC",
			"CARGO_HOME",
			"CARGO_NET_OFFLINE",
			"CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTC",
			"CC",
			"ComSpec",
			"GIT_EXEC_PATH",
			"HOST_AR",
			"HOST_CC",
			"NEXTEST_PROFILE",
			"NEXTEST_USER_CONFIG_FILE",
			"RUSTC",
			"RUSTC_WRAPPER",
			"RUSTUP_HOME",
			"RUSTUP_TOOLCHAIN",
			"TARGET_CXX",
			"TARGET_RANLIB",
			"PATH",
		].map((key) => [key, process.env[key]]),
	);
	Object.assign(process.env, {
		CARGO: "/ambient/cargo",
		CARGO_BUILD_RUSTC: "/ambient/cargo-build-rustc",
		CARGO_HOME: "/ambient/cargo-home",
		CARGO_NET_OFFLINE: "true",
		CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTC: "/ambient/target-rustc",
		CC: "/ambient/cc",
		ComSpec: "/ambient/cmd",
		GIT_EXEC_PATH: "/ambient/git-core",
		HOST_AR: "/ambient/host-ar",
		HOST_CC: "/ambient/host-cc",
		NEXTEST_PROFILE: "ambient-profile",
		NEXTEST_USER_CONFIG_FILE: "/ambient/nextest-config.toml",
		RUSTC: "/ambient/rustc",
		RUSTC_WRAPPER: "/ambient/rustc-wrapper",
		RUSTUP_HOME: "/ambient/rustup-home",
		RUSTUP_TOOLCHAIN: "ambient-toolchain",
		TARGET_CXX: "/ambient/target-cxx",
		TARGET_RANLIB: "/ambient/target-ranlib",
		PATH: "/ambient/bin",
	});
	try {
		await executeDerivedChangeDiagnosticCases(input);
	} finally {
		for (const [key, value] of Object.entries(ambient)) {
			if (value === undefined) delete process.env[key];
			else process.env[key] = value;
		}
	}
	const environment = JSON.parse(
		await readFile(
			join(root, "cases", "global-preflight", "artifact.txt"),
			"utf8",
		),
	);
	assert.equal(environment.caseRoot, join(root, "cases", "global-preflight"));
	assert.equal(environment.workRoot, join(input.temporaryRoot, "w", "000"));
	assert.equal(environment.home, environment.workRoot);
	assert.equal(environment.userprofile, environment.workRoot);
	assert.equal(environment.npmCache, join(environment.workRoot, "npm-cache"));
	assert.equal(environment.xdgCache, join(environment.workRoot, "xdg-cache"));
	assert.equal(environment.xdgConfig, join(environment.workRoot, "xdg-config"));
	assert.equal(environment.xdgData, join(environment.workRoot, "xdg-data"));
	assert.equal(environment.xdgState, join(environment.workRoot, "xdg-state"));
	assert.equal(environment.appData, join(environment.workRoot, "app-data"));
	assert.equal(
		environment.localAppData,
		join(environment.workRoot, "local-app-data"),
	);
	assert.equal(environment.tmpdir, environment.workRoot);
	assert.equal(environment.tmp, environment.workRoot);
	assert.equal(environment.temp, environment.workRoot);
	assert.equal(environment.cargo, undefined);
	assert.equal(environment.cargoBuildRustc, undefined);
	assert.equal(environment.cargoNetOffline, undefined);
	assert.equal(environment.cargoTargetRustc, undefined);
	assert.equal(environment.nextestProfile, undefined);
	assert.equal(environment.nextestUserConfig, undefined);
	assert.equal(environment.rustc, undefined);
	assert.equal(environment.rustcWrapper, undefined);
	assert.equal(environment.rustupToolchain, undefined);
	assert.equal(environment.cargoHome, join(environment.workRoot, "cargo-home"));
	assert.equal(
		environment.rustupHome,
		join(environment.workRoot, "rustup-home"),
	);
	assert.equal(environment.target, join(environment.workRoot, "target"));
	assert.equal(environment.path, "/bound/bin");
	assert.equal(environment.cc, "/bound/cc");
	assert.equal(environment.hostCc, undefined);
	assert.equal(environment.targetCxx, undefined);
	assert.equal(environment.hostAr, undefined);
	assert.equal(environment.targetRanlib, undefined);
	assert.equal(environment.comSpec, undefined);
	assert.equal(environment.linker, "/bound/linker");
	assert.equal(environment.gitExecPath, "/bound/git-core");
	assert.equal(
		environment.workRoot.includes(DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1),
		false,
	);
	assert.equal(await verifyDerivedChangeDiagnosticBindings(input), null);
});

test("requires an empty, non-symlink scratch root disjoint from output and source", async () => {
	const root = await diagnosticRoot("pointbreak-diagnostic-scratch-safety-");
	const missing = request(root);
	delete missing.temporaryRoot;
	assert.throws(
		() => validateDerivedChangeDiagnosticRequest(missing),
		/temporary root must be absolute/,
	);

	const nested = request(root);
	nested.temporaryRoot = join(root, "scratch");
	assert.throws(
		() => validateDerivedChangeDiagnosticRequest(nested),
		/temporary root must be disjoint from the diagnostic output root/,
	);

	const sourceNested = request(root);
	sourceNested.temporaryRoot = join(process.cwd(), "scratch");
	assert.throws(
		() => validateDerivedChangeDiagnosticRequest(sourceNested),
		/temporary root must be disjoint from the source checkout/,
	);

	const reserved = request(root);
	reserved.temporaryRoot = join(
		dirname(root),
		"separate",
		DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1,
		"scratch",
	);
	assert.throws(
		() => validateDerivedChangeDiagnosticRequest(reserved),
		/temporary root cannot enter a diagnostic or owner-store component/,
	);

	const ownerComponent = request(root);
	ownerComponent.temporaryRoot = join(
		dirname(root),
		"separate",
		".git",
		"scratch",
	);
	assert.throws(
		() => validateDerivedChangeDiagnosticRequest(ownerComponent),
		/temporary root cannot enter a diagnostic or owner-store component/,
	);

	const nonempty = request(root);
	await writeFile(join(nonempty.temporaryRoot, "preserve.txt"), "preserve");
	await assert.rejects(
		() => executeDerivedChangeDiagnosticCases(nonempty),
		/temporary root must be empty/,
	);
	assert.equal(
		await readFile(join(nonempty.temporaryRoot, "preserve.txt"), "utf8"),
		"preserve",
	);

	const parent = await mkdtemp(
		join(tmpdir(), "pointbreak-diagnostic-scratch-link-"),
	);
	const target = join(parent, "target");
	const linked = join(parent, "scratch");
	await mkdir(target);
	await symlink(
		target,
		linked,
		process.platform === "win32" ? "junction" : "dir",
	);
	const linkedInput = request(root);
	linkedInput.temporaryRoot = linked;
	await assert.rejects(
		() => executeDerivedChangeDiagnosticCases(linkedInput),
		/temporary root must not traverse symbolic links/,
	);
	const ancestorTarget = join(parent, "ancestor-target");
	const ancestorLink = join(parent, "ancestor-link");
	await mkdir(join(ancestorTarget, "scratch"), { recursive: true });
	await symlink(
		ancestorTarget,
		ancestorLink,
		process.platform === "win32" ? "junction" : "dir",
	);
	const ancestorLinkedInput = request(root);
	ancestorLinkedInput.temporaryRoot = join(ancestorLink, "scratch");
	await assert.rejects(
		() => executeDerivedChangeDiagnosticCases(ancestorLinkedInput),
		/temporary root must not traverse symbolic links/,
	);

	const ownerRoot = await diagnosticRoot(
		"pointbreak-diagnostic-scratch-owner-",
	);
	const ownerInput = request(ownerRoot);
	const oldPointbreakHome = process.env.pointbreak_home;
	process.env.pointbreak_home = ownerInput.temporaryRoot;
	try {
		await assert.rejects(
			() => executeDerivedChangeDiagnosticCases(ownerInput),
			/cannot enter pointbreak_home/i,
		);
	} finally {
		if (oldPointbreakHome === undefined) delete process.env.pointbreak_home;
		else process.env.pointbreak_home = oldPointbreakHome;
	}
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

test("a collected global-invalid child still runs the final always-attempt postflight", async () => {
	const root = await diagnosticRoot("pointbreak-diagnostic-collection-global-");
	const input = request(root);
	input.campaign.requiredCaseIds = ["collection-child", "peer", "postflight"];
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
		{
			id: "postflight",
			lane: "preflight",
			required: true,
			dependsOn: [],
			failureClass: "global_invalid",
			alwaysAttempt: true,
			phase: "postflight",
			fixtureCheckpoint: {
				fixture: "public-fixture",
				checkpoint: "postflight",
			},
			mutatesRoot: false,
			...command("postflight"),
		},
	];

	const result = await executeDerivedChangeDiagnosticCases(input);
	assert.deepEqual(
		result.cases.map(({ id, status }) => ({ id, status })),
		[
			{ id: "launcher", status: "passed" },
			{ id: "collection-child", status: "failed" },
			{ id: "peer", status: "skipped" },
			{ id: "postflight", status: "passed" },
		],
	);
	assert.match(result.cases[2].skipReason, /collection-child/);
	assert.equal(result.cases[3].attempted, true);
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

test("rejects collection artifacts that traverse a symlinked parent", async () => {
	const root = await diagnosticRoot(
		"pointbreak-diagnostic-collection-symlink-",
	);
	const input = request(root);
	input.campaign.requiredCaseIds = ["collection-child"];
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
				status: "passed",
				dependsOn: [],
				artifactPaths: ["escape/secret.txt"],
			},
		],
	};
	input.cases = [
		{
			id: "launcher",
			lane: "native",
			required: false,
			dependsOn: [],
			failureClass: "lane_invalid",
			phase: "native-collection",
			fixtureCheckpoint: {
				fixture: "public-fixture",
				checkpoint: "native",
			},
			program: process.execPath,
			args: [
				"-e",
				`const fs=require("node:fs"),p=require("node:path");const outside=p.join(process.env.POINTBREAK_DIAGNOSTIC_WORK_ROOT,"outside");fs.mkdirSync(outside);fs.writeFileSync(p.join(outside,"secret.txt"),"private");fs.symlinkSync(outside,p.join(process.env.POINTBREAK_DIAGNOSTIC_CASE_ROOT,"escape"),process.platform==="win32"?"junction":"dir");process.stdout.write(${JSON.stringify(JSON.stringify(collection))})`,
			],
			collection: {
				schema: DERIVED_CHANGE_DIAGNOSTIC_COLLECTION_SCHEMA_V1,
				source: "stdout",
				idPrefix: "collection-",
				expectedCaseIds: ["child"],
			},
			mutatesRoot: true,
		},
	];

	const result = await executeDerivedChangeDiagnosticCases(input);
	assert.deepEqual(
		result.cases.map(({ id, status }) => ({ id, status })),
		[
			{ id: "launcher", status: "failed" },
			{ id: "collection-child", status: "skipped" },
		],
	);
	assert.equal(
		result.artifacts.some(({ path }) => path.endsWith("/escape/secret.txt")),
		false,
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
	input.campaign.signatureAuthoritySha256 = createHash("sha256")
		.update(await readFile(authority))
		.digest("hex");
	input.cases = input.cases.slice(0, 1);
	input.campaign.requiredCaseIds = ["global-preflight"];
	input.campaign.requiredPlatformIds = ["macos_apfs"];
	input.campaign.platforms = input.campaign.platforms.slice(0, 1);
	retainOnlyMacosBinaryAuthority(input);
	input.sourcePreflight = {
		...sourcePreflight(input, process.cwd()),
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
	const oldPointbreakHome = process.env.Pointbreak_Home;
	process.env.Pointbreak_Home = ownerStore;
	try {
		await assert.rejects(
			() =>
				assertDerivedChangeDiagnosticOutputRootSafety(
					join(indirection, DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1),
					process.cwd(),
				),
			/cannot enter Pointbreak_Home/i,
		);
	} finally {
		if (oldPointbreakHome === undefined) delete process.env.Pointbreak_Home;
		else process.env.Pointbreak_Home = oldPointbreakHome;
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
	sourceLocal.sourcePreflight = sourcePreflight(sourceLocal, process.cwd());
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
