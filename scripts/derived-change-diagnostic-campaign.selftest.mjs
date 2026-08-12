import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
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
		browserIterationsByPlatform: { macos_apfs: 2 },
	});

const diagnosticRoot = async (prefix) =>
	join(
		await mkdtemp(join(await realpath(tmpdir()), prefix)),
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
		node: process.execPath,
		cargo: process.execPath,
		cargoNextest: process.execPath,
		rustc: process.execPath,
		git: process.execPath,
		filesystemProbe: process.execPath,
		bash: process.execPath,
		npm: process.execPath,
		playwrightCli: process.execPath,
		jq: process.execPath,
		shasum: process.execPath,
		find: process.execPath,
		sort: process.execPath,
		wc: process.execPath,
		tr: process.execPath,
		cp: process.execPath,
		head: process.execPath,
		dirname: process.execPath,
		mkdir: process.execPath,
		rm: process.execPath,
		chmod: process.execPath,
		awk: process.execPath,
		hash: process.execPath,
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
			libraryBuildCommandSha256: digest("c"),
			cliBuildCommandSha256: digest("d"),
		},
		programSha256: Object.fromEntries(
			[
				"bash",
				"git",
				"jq",
				"find",
				"sort",
				"wc",
				"tr",
				"awk",
				"cp",
				"head",
				"dirname",
				"mkdir",
				"rm",
				"hash",
			].map((name) => [name, binarySha256]),
		),
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
		"native_ntfs_stable_continuation_persists_unrelated_volume_churn",
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
	assert.ok(request.requiredExecutables.includes(process.execPath));
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
		["chmod", "POINTBREAK_CHMOD_PROGRAM"],
		["awk", "POINTBREAK_AWK_PROGRAM"],
	]) {
		assert.equal(browser.env[environmentName], config.programs[name]);
	}
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
	config.programs.cygpath = process.execPath;
	config.changeRead.programSha256.cygpath = binarySha256;
	for (const name of ["npm", "playwrightCli", "shasum", "chmod"])
		delete config.programs[name];

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
});

test("unavailable hosts retain complete cases and merge to one non-admissible Red report", async () => {
	const authority = campaign();
	const fragments = [];
	for (const platformId of authority.requiredPlatformIds) {
		const outputRoot = await diagnosticRoot(
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
	const outputRoot = await diagnosticRoot("pointbreak-diagnostic-merge-");
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
