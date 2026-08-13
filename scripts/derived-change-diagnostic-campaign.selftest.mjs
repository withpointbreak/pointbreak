import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
	appendFile,
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
import { join } from "node:path";
import test from "node:test";
import {
	executeDerivedChangeDiagnosticCases,
	sha256DerivedChangeDiagnosticTree,
	verifyDerivedChangeDiagnosticBindings,
} from "./derived-change-diagnostic.mjs";
import {
	createDerivedChangeDiagnosticCampaign,
	createDerivedChangeDiagnosticHostRequest,
	derivedChangeDiagnosticToolchainPreflightEnvironment,
	derivedChangeDiagnosticFilesystemProbeArguments,
	DERIVED_CHANGE_DIAGNOSTIC_AUTHORITY_SEED_SCHEMA_V1,
	DERIVED_CHANGE_DIAGNOSTIC_HOST_CONFIG_SCHEMA_V1,
	DERIVED_CHANGE_DIAGNOSTIC_MERGE_CONFIG_SCHEMA_V1,
	DERIVED_CHANGE_DIAGNOSTIC_UNAVAILABLE_HOST_CONFIG_SCHEMA_V1,
	mergeDerivedChangeDiagnosticCampaign,
	runDerivedChangeDiagnosticScriptToolPreflight,
	runDerivedChangeDiagnosticHost,
	writeUnavailableDerivedChangeDiagnosticHost,
} from "./derived-change-diagnostic-campaign.mjs";
import {
	DERIVED_CHANGE_DIAGNOSTIC_REPORT_BASENAME_V1,
	DERIVED_CHANGE_DIAGNOSTIC_REPORT_SCHEMA_V1,
	DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1,
} from "./derived-change-diagnostic-report.mjs";

const campaignSource = () =>
	readFile(
		new URL("./derived-change-diagnostic-campaign.mjs", import.meta.url),
		"utf8",
	);
const digest = (digit) => digit.repeat(64);
const commit = (digit) => digit.repeat(40);
const binarySha256 = createHash("sha256")
	.update(await readFile(process.execPath))
	.digest("hex");
const executableTreeRoot = await mkdtemp(
	join(await realpath(tmpdir()), "pointbreak-diagnostic-bound-tools-"),
);
const executableProgram = join(executableTreeRoot, "node");
await copyFile(process.execPath, executableProgram);
await chmod(executableProgram, 0o755);
const executableTreeSha256 =
	await sha256DerivedChangeDiagnosticTree(executableTreeRoot);
const controlBuildCommandSha256 = (arguments_) =>
	createHash("sha256")
		.update(JSON.stringify({ arguments: arguments_, program: "cargo" }))
		.digest("hex");
const libraryControlBuildCommandSha256 = controlBuildCommandSha256([
	"+stable",
	"test",
	"--locked",
	"--features",
	"longitudinal-counting",
	"--lib",
	"--no-run",
]);
const cliControlBuildCommandSha256 = controlBuildCommandSha256([
	"+stable",
	"test",
	"--locked",
	"--features",
	"longitudinal-counting",
	"--bin",
	"pointbreak",
	"--no-run",
]);
assert.equal(
	libraryControlBuildCommandSha256,
	"966d15957e6ecda1a78444e921bb84b3bec41f2c62217162404c861695f02ae8",
);
assert.equal(
	cliControlBuildCommandSha256,
	"8b9e6043823a7c106699b6737bfba986a21fcdb53c1259a9ad648a1ad2188add",
);
const binaryIdentities = {
	macos_apfs: {
		product: digest("0"),
		harness: digest("1"),
		library: digest("2"),
		cli: digest("3"),
	},
	windows_ntfs: {
		product: digest("4"),
		harness: digest("5"),
		library: digest("6"),
		cli: digest("7"),
	},
};
const fixtureAuthorityDocument = {
	schema: "pointbreak.derived-change-public-fixture-authority.v2",
	sourceCommit: commit("1"),
	sourceTree: commit("2"),
	sourceFiles: [
		{
			path: "scripts/derived-change-diagnostic-fixture.mjs",
			sha256: digest("4"),
		},
		{
			path: "scripts/materialize-inspector-decision-matrix.sh",
			sha256: digest("5"),
		},
		{
			path: "src/bench_support/derived_access/materializer.rs",
			sha256: digest("6"),
		},
		{
			path: "tests/support/assets/change-ready-store/5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json",
			sha256: digest("7"),
		},
		{
			path: "tests/support/assets/change-ready-store/f31956c2b820926adc74d4d03cb03820d13c9ed2739b5f7ada81611a6f8bcff1.json",
			sha256: digest("8"),
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
		"wrong-family-carrier-v1",
	].map((fixtureId, index) => ({
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

const platforms = () => [
	{
		id: "macos_apfs",
		operatingSystem: "macos",
		architecture: "aarch64",
		filesystem: "apfs",
		hostIdentitySha256: digest("a"),
	},
	{
		id: "windows_ntfs",
		operatingSystem: "windows",
		architecture: "x86_64",
		filesystem: "ntfs",
		hostIdentitySha256: digest("b"),
	},
];

const commonProgramNames = [
	"awk",
	"bash",
	"cargo",
	"cargoNextest",
	"cp",
	"dirname",
	"filesystemProbe",
	"find",
	"git",
	"hash",
	"head",
	"jq",
	"mkdir",
	"node",
	"rm",
	"rustc",
	"sort",
	"tr",
	"wc",
];
const programNames = {
	macos_apfs: [
		...commonProgramNames,
		"browserExecutable",
		"chmod",
		"playwrightCli",
		"shasum",
		"sleep",
		"vitestCli",
	].sort(),
	windows_ntfs: [...commonProgramNames, "cygpath"].sort(),
};
const treeBoundProgramNames = new Set([
	"browserExecutable",
	"playwrightCli",
	"vitestCli",
]);
const programIdentities = () =>
	platforms()
		.flatMap((platform) =>
			programNames[platform.id].map((name) => ({
				platformId: platform.id,
				name,
				program:
					platform.operatingSystem === "windows"
						? `C:\\tools\\${name}.exe`
						: executableProgram,
				binarySha256,
				...(treeBoundProgramNames.has(name)
					? {
							treeRoot: executableTreeRoot,
							treeSha256: executableTreeSha256,
						}
					: {}),
			})),
		)
		.sort((left, right) =>
			`${left.platformId}\0${left.name}`.localeCompare(
				`${right.platformId}\0${right.name}`,
			),
		);

const campaign = () =>
	createDerivedChangeDiagnosticCampaign({
		schema: DERIVED_CHANGE_DIAGNOSTIC_AUTHORITY_SEED_SCHEMA_V1,
		id: "derived-change-diagnostic-selftest",
		source: {
			commit: commit("1"),
			tree: commit("2"),
			rangeBaseCommit: commit("3"),
			rangeSha256: digest("4"),
		},
		fixture: {
			authoritySha256: fixtureAuthoritySha256,
			document: structuredClone(fixtureAuthorityDocument),
		},
		platforms: platforms(),
		product: {
			binaries: platforms().map(({ id }) => ({
				platformId: id,
				binarySha256: binaryIdentities[id].product,
			})),
		},
		harness: {
			binaries: platforms().map(({ id }) => ({
				platformId: id,
				binarySha256: binaryIdentities[id].harness,
			})),
		},
		control: {
			binaries: platforms().flatMap(({ id }) =>
				["cli", "library"].map((role) => ({
					platformId: id,
					role,
					binarySha256: binaryIdentities[id][role],
				})),
			),
		},
		programs: programIdentities(),
		browserIterationsByPlatform: { macos_apfs: 2 },
	});

const diagnosticRoot = async (prefix) =>
	join(
		await mkdtemp(join(await realpath(tmpdir()), prefix)),
		DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1,
	);

const nestedDiagnosticRoot = async (prefix) =>
	join(
		await mkdtemp(join(await realpath(tmpdir()), prefix)),
		"evidence",
		"host",
		DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1,
	);

test("Windows filesystem probes use the fsutil volume spelling", () => {
	assert.deepEqual(
		derivedChangeDiagnosticFilesystemProbeArguments(
			"windows",
			"C:\\Users\\kevin\\diagnostic",
		),
		["fsinfo", "volumeinfo", "C:"],
	);
});

test("toolchain preflight blocks rustup installation without changing compile policy env", () => {
	assert.deepEqual(
		derivedChangeDiagnosticToolchainPreflightEnvironment({
			CARGO_HOME: "/scratch/cargo-home",
			RUSTUP_HOME: "/scratch/rustup-home",
		}),
		{
			CARGO_HOME: "/scratch/cargo-home",
			RUSTUP_AUTO_INSTALL: "0",
			RUSTUP_DIST_SERVER: "http://127.0.0.1:9",
			RUSTUP_HOME: "/scratch/rustup-home",
			RUSTUP_UPDATE_ROOT: "http://127.0.0.1:9/rustup",
		},
	);
});

const hostConfig = async () => ({
	schema: DERIVED_CHANGE_DIAGNOSTIC_HOST_CONFIG_SCHEMA_V1,
	campaign: campaign(),
	platformId: "macos_apfs",
	sourceCheckout: process.cwd(),
	outputRoot: await diagnosticRoot("pointbreak-diagnostic-host-"),
	temporaryRoot: await mkdtemp(
		join(await realpath(tmpdir()), "pointbreak-diagnostic-work-"),
	),
	identityPaths: {
		product: join(fixtureAuthorityRoot, "product"),
		harness: join(fixtureAuthorityRoot, "harness"),
		control: join(fixtureAuthorityRoot, "library-control"),
		controlCli: join(fixtureAuthorityRoot, "cli-control"),
		fixtureAuthority: fixtureAuthorityPath,
	},
	programs: {
		node: executableProgram,
		cargo: executableProgram,
		cargoNextest: executableProgram,
		rustc: executableProgram,
		git: executableProgram,
		filesystemProbe: executableProgram,
		bash: executableProgram,
		browserExecutable: executableProgram,
		playwrightCli: executableProgram,
		vitestCli: executableProgram,
		jq: executableProgram,
		shasum: executableProgram,
		find: executableProgram,
		sort: executableProgram,
		wc: executableProgram,
		tr: executableProgram,
		cp: executableProgram,
		head: executableProgram,
		dirname: executableProgram,
		mkdir: executableProgram,
		rm: executableProgram,
		chmod: executableProgram,
		sleep: executableProgram,
		awk: executableProgram,
		hash: executableProgram,
	},
	changeRead: {
		execution: { cargoLockSha256: digest("9") },
		product: {
			versionSha256: digest("a"),
			buildProfile: "release",
			enabledFeatures: ["longitudinal-counting"],
			buildCommandSha256: digest("b"),
		},
		harnessArgsPrefix: [],
		controls: {
			libraryBuildCommandSha256: libraryControlBuildCommandSha256,
			cliBuildCommandSha256: cliControlBuildCommandSha256,
		},
		hashMode: "shasum",
		summaryQuery: "matrix",
	},
	browserIterations: 2,
});

test("the supported derived Change campaign retains every diagnostic lane", async () => {
	const source = await campaignSource();

	for (const required of [
		"--all-targets",
		"--all-features",
		"--keep-going",
		"--no-fail-fast",
		"existing_path_identity_ignores_equivalent_lexical_spellings",
		"stable_authority_successor_does_not_wait_for_a_busy_writer",
		"native_ntfs_stable_continuation_defers_unrelated_volume_churn_to_maintenance",
		"--derived-change-diagnostic-identity",
		"derived-change-diagnostic-browser.sh",
		"derived-change-diagnostic-native.mjs",
		"derived-change-diagnostic-fixture.selftest.mjs",
		"cargoNextest",
		"rustc",
		"actual preinstalled executable",
	]) {
		assert.match(source, new RegExp(required.replaceAll("-", "\\-")));
	}
	assert.match(source, /run-host/);
	assert.match(source, /unavailable-host/);
	assert.match(source, /merge/);
	assert.match(source, /dependsOn/);
	assert.match(source, /mutatesRoot/);
	assert.match(source, /\["--exact", config\.testName/);
	assert.doesNotMatch(source, /harness\?\.contract\?\.derivation/);
	assert.doesNotMatch(
		source,
		/DERIVED_CHANGE_DIAGNOSTIC_HOST_FRAGMENT_BASENAME_V1,\s*DERIVED_CHANGE_DIAGNOSTIC_HOST_FRAGMENT_BASENAME_V1/s,
	);
});

test("campaign authority freezes complete platform, native, and browser inventories", () => {
	const authority = campaign();
	assert.deepEqual(authority.requiredPlatformIds, [
		"macos_apfs",
		"windows_ntfs",
	]);
	for (const id of [
		"macos_apfs.compile-all-targets",
		"macos_apfs.topology-v1.read.stale_page_token",
		"macos_apfs.change-read.identity-postflight",
		"macos_apfs.lifecycle-L7-wrong_root",
		"macos_apfs.browser-widen-2",
		"macos_apfs.browser-runtime-pageerror",
		"windows_ntfs.platform-path-identity",
		"windows_ntfs.platform-carrier-mutation",
		"windows_ntfs.platform-carrier-missing",
		"windows_ntfs.platform-busy-writer",
		"windows_ntfs.platform-volume-churn",
		"windows_ntfs.lifecycle-D0-128-open_bootstrap_reopen_replay_equality",
	]) {
		assert.ok(authority.requiredCaseIds.includes(id), id);
	}
	assert.deepEqual(
		authority.requiredCaseIds,
		[...authority.requiredCaseIds].sort(),
	);
	assert.equal(
		new Set(authority.requiredCaseIds).size,
		authority.requiredCaseIds.length,
	);
	assert.deepEqual(
		authority.programs,
		programIdentities(),
		"campaign authority must retain the complete sorted platform program inventory",
	);
});

test("campaign authority rejects incomplete or substituted program identities", async () => {
	const missing = campaign();
	missing.programs = missing.programs.filter(
		({ platformId, name }) => platformId !== "macos_apfs" || name !== "cargo",
	);
	const incomplete = await hostConfig();
	incomplete.campaign = missing;
	assert.throws(
		() => createDerivedChangeDiagnosticHostRequest(incomplete),
		/program inventory/i,
	);

	const substituted = await hostConfig();
	substituted.programs.cargo = "/definitely/not/the-bound-cargo";
	assert.throws(
		() => createDerivedChangeDiagnosticHostRequest(substituted),
		/program identity differs from campaign authority/i,
	);
	const missingHostProgram = await hostConfig();
	delete missingHostProgram.programs.sleep;
	assert.throws(
		() => createDerivedChangeDiagnosticHostRequest(missingHostProgram),
		/host program inventory differs from campaign authority/i,
	);
	const extraHostProgram = await hostConfig();
	extraHostProgram.programs.unbound = executableProgram;
	assert.throws(
		() => createDerivedChangeDiagnosticHostRequest(extraHostProgram),
		/host program inventory differs from campaign authority/i,
	);
});

test("program bytes are rechecked by serialized preflight and postflight bindings", async () => {
	const config = await hostConfig();
	const root = await mkdtemp(
		join(await realpath(tmpdir()), "pointbreak-diagnostic-program-"),
	);
	const cargo = join(root, "cargo");
	await copyFile(process.execPath, cargo);
	await chmod(cargo, 0o755);
	const cargoSha256 = createHash("sha256")
		.update(await readFile(cargo))
		.digest("hex");
	const identity = config.campaign.programs.find(
		({ platformId, name }) =>
			platformId === config.platformId && name === "cargo",
	);
	identity.program = cargo;
	identity.binarySha256 = cargoSha256;
	config.programs.cargo = cargo;
	const request = createDerivedChangeDiagnosticHostRequest(config);
	const before = await verifyDerivedChangeDiagnosticBindings(request);
	assert.equal(before?.programIdentityFailures, undefined);

	await appendFile(cargo, "drift");
	const after = await verifyDerivedChangeDiagnosticBindings(request);
	assert.deepEqual(
		after.programIdentityFailures.map(({ name }) => name),
		["cargo"],
	);
});

test("program dependency trees are rechecked by serialized bindings", async () => {
	const config = await hostConfig();
	const root = await mkdtemp(
		join(await realpath(tmpdir()), "pointbreak-diagnostic-program-tree-"),
	);
	const playwrightCli = join(root, "playwright-cli");
	const dependency = join(root, "dependency.js");
	await copyFile(process.execPath, playwrightCli);
	await chmod(playwrightCli, 0o755);
	await writeFile(dependency, "export const identity = 1;\n");
	const identity = config.campaign.programs.find(
		({ platformId, name }) =>
			platformId === config.platformId && name === "playwrightCli",
	);
	identity.program = playwrightCli;
	identity.binarySha256 = createHash("sha256")
		.update(await readFile(playwrightCli))
		.digest("hex");
	identity.treeRoot = root;
	identity.treeSha256 = await sha256DerivedChangeDiagnosticTree(root);
	config.programs.playwrightCli = playwrightCli;
	const request = createDerivedChangeDiagnosticHostRequest(config);
	const before = await verifyDerivedChangeDiagnosticBindings(request);
	assert.equal(before?.programIdentityFailures, undefined);

	await appendFile(dependency, "export const drift = 2;\n");
	const after = await verifyDerivedChangeDiagnosticBindings(request);
	assert.deepEqual(
		after.programIdentityFailures.map(({ name }) => name),
		["playwrightCli"],
	);
});

test("program dependency tree authority rejects escaping symlinks", async () => {
	const parent = await mkdtemp(
		join(await realpath(tmpdir()), "pointbreak-diagnostic-tree-symlink-"),
	);
	const root = join(parent, "tree");
	const outside = join(parent, "outside.js");
	await mkdir(root);
	await writeFile(outside, "export const outside = true;\n");
	await symlink(outside, join(root, "escape.js"));
	await assert.rejects(
		sha256DerivedChangeDiagnosticTree(root),
		/program tree symlink escapes its root/,
	);
});

test("script tool preflight rejects a hash-valid non-Node Playwright wrapper", async () => {
	const root = await mkdtemp(
		join(await realpath(tmpdir()), "pointbreak-diagnostic-script-tools-"),
	);
	const validCli = join(root, "valid-cli.mjs");
	const shellWrapper = join(root, "playwright-wrapper.sh");
	await writeFile(validCli, 'process.stdout.write("1.0.0\\n");\n');
	await writeFile(shellWrapper, "#!/bin/sh\nexit 0\n");
	await chmod(shellWrapper, 0o755);
	await assert.rejects(
		runDerivedChangeDiagnosticScriptToolPreflight({
			node: process.execPath,
			vitestCli: validCli,
			playwrightCli: shellWrapper,
			browserExecutable: process.execPath,
			sourceCheckout: process.cwd(),
		}),
		/Playwright must be loadable through its exact bound launcher/,
	);
	await runDerivedChangeDiagnosticScriptToolPreflight({
		node: process.execPath,
		vitestCli: validCli,
		playwrightCli: validCli,
		browserExecutable: process.execPath,
		sourceCheckout: process.cwd(),
	});
});

test("host requests use explicit dependencies and disjoint destructive roots", async () => {
	const config = await hostConfig();
	const request = createDerivedChangeDiagnosticHostRequest(config);
	const mutableRoots = request.cases
		.filter(({ mutatesRoot }) => mutatesRoot)
		.map(({ root }) => root);
	assert.equal(new Set(mutableRoots).size, mutableRoots.length);
	assert.equal(request.cases.at(0).id, "macos_apfs.preflight");
	assert.equal(request.cases.at(-1).id, "macos_apfs.postflight");
	assert.deepEqual(request.cases.at(-1).dependsOn, [
		"macos_apfs.preflight",
		"macos_apfs.product-version",
	]);

	const compile = request.cases.find(({ id }) =>
		id.endsWith("compile-all-targets"),
	);
	assert.equal(compile.program, config.programs.cargo);
	assert.equal(compile.args[0], "build");
	assert.equal(compile.args.includes("+stable"), false);
	assert.equal(compile.args.includes("--no-run"), false);
	assert.ok(compile.args.includes("--all-targets"));
	assert.ok(compile.args.includes("--all-features"));
	assert.ok(compile.args.includes("--keep-going"));
	assert.deepEqual(compile.args.slice(-2), ["--jobs", "2"]);
	const policy = request.cases.find(({ id }) =>
		id.endsWith("policy-derived-access"),
	);
	assert.equal(policy.program, config.programs.cargoNextest);
	assert.deepEqual(policy.args.slice(0, 3), ["nextest", "run", "--locked"]);
	assert.ok(policy.args.includes("--no-fail-fast"));
	const webPolicy = request.cases.find(({ id }) => id.endsWith("policy-web"));
	assert.equal(webPolicy.program, config.programs.node);
	assert.deepEqual(webPolicy.args.slice(0, 3), [
		config.programs.vitestCli,
		"run",
		"--no-cache",
	]);
	for (const entry of [compile, policy]) {
		assert.equal(entry.env.CARGO_TARGET_DIR, undefined);
		assert.equal(entry.env.CARGO, config.programs.cargo);
		assert.equal(entry.env.RUSTC, config.programs.rustc);
		assert.equal(entry.env.RUSTUP_AUTO_INSTALL, undefined);
		assert.equal(entry.env.RUSTUP_DIST_SERVER, undefined);
		assert.equal(entry.env.RUSTUP_UPDATE_ROOT, undefined);
	}
	assert.ok(request.requiredExecutables.includes(config.programs.cargo));
	assert.ok(request.requiredExecutables.includes(config.programs.cargoNextest));
	assert.ok(request.requiredExecutables.includes(config.programs.rustc));
	const preflightConfig = JSON.parse(
		request.cases.find(({ id }) => id.endsWith("product-version")).env[
			"POINTBREAK_DERIVED_CHANGE_BINARY_PREFLIGHT"
		],
	);
	assert.equal(preflightConfig.cargo, config.programs.cargo);
	assert.equal(preflightConfig.cargoNextest, config.programs.cargoNextest);
	assert.equal(preflightConfig.rustc, config.programs.rustc);
	assert.equal(preflightConfig.node, config.programs.node);
	assert.equal(preflightConfig.vitestCli, config.programs.vitestCli);
	assert.equal(preflightConfig.playwrightCli, config.programs.playwrightCli);
	assert.equal(
		preflightConfig.browserExecutable,
		config.programs.browserExecutable,
	);
	assert.deepEqual(
		request.cases
			.filter(
				({ id }) => id.includes(".platform-") && !id.endsWith("volume-churn"),
			)
			.map(
				({ env }) =>
					JSON.parse(env.POINTBREAK_DERIVED_CHANGE_CONTROL_CASE).testName,
			),
		[
			"bench_support::derived_access::change_read::instrumented::tests::existing_path_identity_ignores_equivalent_lexical_spellings",
			"bench_support::derived_access::runner_tests::candidate_open_preserves_admitted_truth_and_accounts_for_governed_namespaces",
			"bench_support::derived_access::materializer::change_fixture_tests::change_fixtures_exercise_their_declared_derived_outcomes",
			"session::derived_access::lifecycle::tests::stable_authority_successor_does_not_wait_for_a_busy_writer",
		],
	);

	const native = request.cases.find(({ id }) => id.endsWith("native-stateful"));
	assert.equal(native.collection.expectedCaseIds.length, 60);
	const changeRead = request.cases.find(({ id }) =>
		id.endsWith("change-read-stateful"),
	);
	assert.equal(changeRead.failureClass, "lane_invalid");
	assert.equal(changeRead.collection.expectedCaseIds.length, 148);
	assert.ok(
		changeRead.collection.expectedCaseIds.includes(
			"change-read.global-preflight",
		),
	);
	assert.ok(
		changeRead.collection.expectedCaseIds.includes(
			"topology-v1.read.stale_page_token",
		),
	);
	assert.ok(
		changeRead.collection.expectedCaseIds.includes(
			"change-read.identity-postflight",
		),
	);
	const browser = request.cases.find(({ id }) =>
		id.endsWith("browser-transition"),
	);
	assert.deepEqual(browser.collection.completeExitCodes, [1]);
	assert.deepEqual(browser.collection.expectedCaseIds, [
		"browser-bootstrap",
		"browser-runtime-console",
		"browser-runtime-pageerror",
		"browser-widen-1",
		"browser-widen-2",
	]);
	assert.ok(browser.artifactPaths.includes("logs/fixture-checkpoint.json"));
	assert.ok(
		browser.artifactPaths.includes(
			"harness/scripts/derived-change-diagnostic-fixture.mjs",
		),
	);
	assert.ok(request.requiredExecutables.includes(executableProgram));
	assert.equal(request.temporaryRoot, config.temporaryRoot);
	assert.ok(
		request.cases.every(
			({ program }) => program === undefined || program.startsWith("/"),
		),
	);
});

test("host requests execute only the configured candidate checkout and retained authority", async () => {
	const config = await hostConfig();
	config.sourceCheckout = join(tmpdir(), "pointbreak-diagnostic-candidate");
	config.allowedSignersPath = join(
		tmpdir(),
		"pointbreak-public-allowed-signers",
	);
	const request = createDerivedChangeDiagnosticHostRequest(config);
	const sourceScript = (name) => join(config.sourceCheckout, "scripts", name);

	for (const suffix of [
		"preflight",
		"product-version",
		"platform-path-identity",
		"postflight",
	]) {
		const record = request.cases.find(({ id }) => id.endsWith(suffix));
		assert.equal(
			record.args[0],
			sourceScript("derived-change-diagnostic-campaign.mjs"),
		);
	}
	const native = request.cases.find(({ id }) => id.endsWith("native-stateful"));
	assert.equal(
		native.args[0],
		sourceScript("derived-change-diagnostic-native.mjs"),
	);
	const changeRead = request.cases.find(({ id }) =>
		id.endsWith("change-read-stateful"),
	);
	assert.equal(
		changeRead.args[0],
		sourceScript("derived-change-diagnostic-change-read.mjs"),
	);
	const changeReadConfig = JSON.parse(
		changeRead.env.POINTBREAK_DERIVED_CHANGE_CHANGE_READ_CONFIG,
	);
	assert.equal(changeReadConfig.campaignId, config.campaign.id);
	assert.equal(changeReadConfig.product.program, config.identityPaths.product);
	assert.equal(changeReadConfig.harness.program, config.identityPaths.harness);
	assert.equal(
		changeReadConfig.controls.library.program,
		config.identityPaths.control,
	);
	assert.equal(
		changeReadConfig.controls.cli.program,
		config.identityPaths.controlCli,
	);
	assert.equal(
		changeReadConfig.execution.binarySha256,
		config.campaign.harness.binaries.find(
			({ platformId }) => platformId === config.platformId,
		).binarySha256,
	);
	assert.equal(changeReadConfig.workRoot, config.temporaryRoot);
	assert.equal(
		changeReadConfig.controls.library.buildCommandSha256,
		libraryControlBuildCommandSha256,
	);
	assert.equal(
		changeReadConfig.controls.cli.buildCommandSha256,
		cliControlBuildCommandSha256,
	);
	const nativeConfig = JSON.parse(
		native.env.POINTBREAK_DERIVED_CHANGE_NATIVE_CONFIG,
	);
	assert.equal(nativeConfig.workRoot, config.temporaryRoot);
	const browser = request.cases.find(({ id }) =>
		id.endsWith("browser-transition"),
	);
	assert.equal(
		browser.args[0],
		sourceScript("derived-change-diagnostic-browser.sh"),
	);
	assert.equal(
		browser.env.POINTBREAK_EXPECTED_SOURCE_COMMIT,
		config.campaign.source.commit,
	);
	assert.equal(
		browser.env.POINTBREAK_EXPECTED_SOURCE_TREE,
		config.campaign.source.tree,
	);
	assert.equal(browser.env.POINTBREAK_CYGPATH_PROGRAM, "absent");
	assert.equal(
		browser.env.POINTBREAK_ALLOWED_SIGNERS_PATH,
		join(config.outputRoot, "authority", "allowed-signers"),
	);
	for (const [name, environmentName] of [
		["bash", "POINTBREAK_BASH_PROGRAM"],
		["browserExecutable", "POINTBREAK_BROWSER_EXECUTABLE"],
		["git", "POINTBREAK_GIT_PROGRAM"],
		["jq", "POINTBREAK_JQ_PROGRAM"],
		["node", "POINTBREAK_NODE_PROGRAM"],
		["shasum", "POINTBREAK_SHASUM_PROGRAM"],
		["find", "POINTBREAK_FIND_PROGRAM"],
		["sort", "POINTBREAK_SORT_PROGRAM"],
		["wc", "POINTBREAK_WC_PROGRAM"],
		["tr", "POINTBREAK_TR_PROGRAM"],
		["cp", "POINTBREAK_CP_PROGRAM"],
		["head", "POINTBREAK_HEAD_PROGRAM"],
		["dirname", "POINTBREAK_DIRNAME_PROGRAM"],
		["mkdir", "POINTBREAK_MKDIR_PROGRAM"],
		["rm", "POINTBREAK_RM_PROGRAM"],
		["sleep", "POINTBREAK_SLEEP_PROGRAM"],
		["chmod", "POINTBREAK_CHMOD_PROGRAM"],
		["awk", "POINTBREAK_AWK_PROGRAM"],
	]) {
		assert.equal(browser.env[environmentName], config.programs[name]);
	}
	assert.equal(browser.env.PLAYWRIGHT_CLI, config.programs.playwrightCli);
	assert.equal(browser.env.CI, "1");
	assert.equal(browser.env.NO_UPDATE_NOTIFIER, "1");
	const topologyAuthority = config.campaign.fixture.document.topologyCheckpoint;
	const materializerAuthority =
		config.campaign.fixture.document.sourceFiles.find(
			({ path }) => path === "scripts/materialize-inspector-decision-matrix.sh",
		);
	assert.equal(browser.env.POINTBREAK_EXPECTED_FIXTURE_ID, "topology-v1");
	assert.equal(
		browser.env.POINTBREAK_EXPECTED_TOPOLOGY_CHECKPOINT_SHA256,
		topologyAuthority.checkpointSha256,
	);
	assert.equal(
		browser.env.POINTBREAK_EXPECTED_AUTHORITATIVE_INVENTORY_SHA256,
		undefined,
	);
	assert.equal(
		browser.env.POINTBREAK_EXPECTED_FIXTURE_WITNESS_SHA256,
		undefined,
	);
	assert.equal(
		browser.env.POINTBREAK_EXPECTED_TOPOLOGY_MATERIALIZER_SHA256,
		materializerAuthority.sha256,
	);
	const postflight = request.cases.find(({ id }) => id.endsWith("postflight"));
	const boundRequest = JSON.parse(Object.values(postflight.env)[0]);
	assert.equal(boundRequest.cases.length, request.cases.length);
	assert.equal(boundRequest.outputRoot, config.outputRoot);
	assert.equal(boundRequest.temporaryRoot, config.temporaryRoot);
});

test("rejects absolute Cargo control build metadata without the frozen stable contract", async () => {
	const config = await hostConfig();
	config.changeRead.controls.libraryBuildCommandSha256 =
		createHash("sha256")
			.update(
				JSON.stringify({
					arguments: [
						"+stable",
						"test",
						"--locked",
						"--features",
						"longitudinal-counting",
						"--lib",
						"--no-run",
					],
					program: config.programs.cargo,
				}),
			)
			.digest("hex");
	assert.throws(
		() => createDerivedChangeDiagnosticHostRequest(config),
		/control build command metadata differs from frozen contract/,
	);
});

test("serialized postflight bindings accept a populated preserved scratch root", async () => {
	const config = await hostConfig();
	const request = createDerivedChangeDiagnosticHostRequest(config);
	const postflight = request.cases.find(({ id }) => id.endsWith("postflight"));
	const boundRequest = JSON.parse(Object.values(postflight.env)[0]);
	await mkdir(join(config.temporaryRoot, "w", "000"), { recursive: true });
	const bindingFailure =
		await verifyDerivedChangeDiagnosticBindings(boundRequest);
	assert.equal(bindingFailure?.temporaryRootFailure, undefined);
});

test("Windows host requests retain the complete native Change-read collection", async () => {
	const config = await hostConfig();
	config.platformId = "windows_ntfs";
	config.browserIterations = 0;
	config.changeRead.hashMode = "sha256sum";
	config.programs = Object.fromEntries(
		config.campaign.programs
			.filter(({ platformId }) => platformId === "windows_ntfs")
			.map(({ name, program }) => [name, program]),
	);

	const request = createDerivedChangeDiagnosticHostRequest(config);
	const changeRead = request.cases.find(({ id }) =>
		id.endsWith("change-read-stateful"),
	);
	assert.equal(changeRead.collection.expectedCaseIds.length, 148);
	assert.equal(
		JSON.parse(changeRead.env.POINTBREAK_DERIVED_CHANGE_CHANGE_READ_CONFIG)
			.programs.hash.mode,
		"sha256sum",
	);
	assert.equal(
		request.cases.some(({ id }) => id.includes("browser-")),
		false,
	);
	assert.equal(
		request.cases.some(({ id }) => id.endsWith("policy-web")),
		false,
	);
	assert.ok(
		request.cases.some(({ id }) => id.endsWith("platform-volume-churn")),
	);
	const volumeChurn = request.cases.find(({ id }) =>
		id.endsWith("platform-volume-churn"),
	);
	assert.equal(
		JSON.parse(volumeChurn.env.POINTBREAK_DERIVED_CHANGE_CONTROL_CASE).testName,
		"session::derived_access::lifecycle::tests::native_ntfs_stable_continuation_defers_unrelated_volume_churn_to_maintenance",
	);
});

test("run-host creates a nested absent output root after safety validation", async () => {
	const config = await hostConfig();
	config.outputRoot = await nestedDiagnosticRoot(
		"pointbreak-diagnostic-run-host-nested-",
	);

	const { fragment, fragmentPath } =
		await runDerivedChangeDiagnosticHost(config);

	assert.equal(fragment.platform.id, config.platformId);
	assert.equal(
		JSON.parse(await readFile(fragmentPath, "utf8")).fragmentSha256,
		fragment.fragmentSha256,
	);
});

test("unavailable hosts retain complete cases and merge to one non-admissible Red report", async () => {
	const authority = campaign();
	const fragments = [];
	for (const platformId of authority.requiredPlatformIds) {
		const outputRoot = await nestedDiagnosticRoot(
			`pointbreak-diagnostic-${platformId}-`,
		);
		const { fragment, fragmentPath } =
			await writeUnavailableDerivedChangeDiagnosticHost({
				schema: DERIVED_CHANGE_DIAGNOSTIC_UNAVAILABLE_HOST_CONFIG_SCHEMA_V1,
				campaign: authority,
				platformId,
				sourceCheckout: process.cwd(),
				outputRoot,
				reason: "synthetic unavailable host",
			});
		assert.ok(fragment.cases.every(({ status }) => status === "unavailable"));
		fragments.push(fragmentPath);
	}
	const outputRoot = await nestedDiagnosticRoot(
		"pointbreak-diagnostic-merge-",
	);
	const { report, reportPath } = await mergeDerivedChangeDiagnosticCampaign({
		schema: DERIVED_CHANGE_DIAGNOSTIC_MERGE_CONFIG_SCHEMA_V1,
		campaign: authority,
		sourceCheckout: process.cwd(),
		outputRoot,
		fragmentPaths: fragments,
	});
	assert.equal(report.schema, DERIVED_CHANGE_DIAGNOSTIC_REPORT_SCHEMA_V1);
	assert.equal(report.admissible, false);
	assert.equal(report.verdict, "red");
	assert.equal(report.counts.unavailable, authority.requiredCaseIds.length);
	assert.equal(
		reportPath,
		join(outputRoot, DERIVED_CHANGE_DIAGNOSTIC_REPORT_BASENAME_V1),
	);
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
});

test("diagnostic output roots cannot enter the source checkout", async () => {
	const config = await hostConfig();
	config.outputRoot = join(
		process.cwd(),
		DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1,
	);
	assert.throws(
		() => createDerivedChangeDiagnosticHostRequest(config),
		/outside the source checkout/,
	);
});

test("diagnostic output roots resolve symbolic-link ancestors before creation", async () => {
	const parent = await mkdtemp(
		join(tmpdir(), "pointbreak-diagnostic-symlink-"),
	);
	const link = join(parent, "link");
	await symlink(
		process.cwd(),
		link,
		process.platform === "win32" ? "junction" : "dir",
	);
	const config = await hostConfig();
	config.outputRoot = join(link, DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1);
	await assert.rejects(
		() =>
			executeDerivedChangeDiagnosticCases(
				createDerivedChangeDiagnosticHostRequest(config),
			),
		/resolve outside the source checkout/,
	);
});

test("the diagnostic campaign has no terminal evidence path", async () => {
	const source = await campaignSource();

	for (const forbidden of [
		"--derived-access-fragment",
		"--derived-access-package",
		"--derived-access-verify-package",
		"change-inspector-browser-verify.sh",
		"change-inspector-browser-manifest.mjs",
	]) {
		assert.doesNotMatch(source, new RegExp(forbidden.replaceAll("-", "\\-")));
	}
	assert.doesNotMatch(source, /shell\s*:\s*true/);
});
