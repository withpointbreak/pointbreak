import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { lstat, mkdir, open, readdir, readFile } from "node:fs/promises";
import {
	hostname,
	arch as nodeArchitecture,
	platform as nodePlatform,
} from "node:os";
import {
	basename,
	isAbsolute,
	join,
	relative,
	resolve,
	sep,
	win32,
} from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
	assertDerivedChangeDiagnosticOutputRootSafety,
	DERIVED_CHANGE_DIAGNOSTIC_COLLECTION_SCHEMA_V1,
	DERIVED_CHANGE_DIAGNOSTIC_REQUEST_SCHEMA_V1,
	executeDerivedChangeDiagnosticCases,
	verifyDerivedChangeDiagnosticBindings,
} from "./derived-change-diagnostic.mjs";
import {
	DERIVED_CHANGE_NATIVE_DIAGNOSTIC_CONFIG_SCHEMA_V1,
	DERIVED_CHANGE_NATIVE_LIFECYCLE_CRITERIA_V1,
} from "./derived-change-diagnostic-native.mjs";
import {
	DERIVED_CHANGE_CHANGE_READ_DIAGNOSTIC_CONFIG_SCHEMA_V1,
	derivedChangeChangeReadChildDescriptors,
	validateDerivedChangeChangeReadDiagnosticConfig,
} from "./derived-change-diagnostic-change-read.mjs";
import {
	DERIVED_CHANGE_DIAGNOSTIC_FRAGMENT_SCHEMA_V1,
	DERIVED_CHANGE_DIAGNOSTIC_REPORT_BASENAME_V1,
	DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1,
	finalizeDerivedChangeDiagnosticFragment,
	mergeDerivedChangeDiagnosticReport,
	validateDerivedChangeDiagnosticCampaign,
	validateDerivedChangeDiagnosticReport,
} from "./derived-change-diagnostic-report.mjs";

export const DERIVED_CHANGE_DIAGNOSTIC_AUTHORITY_SEED_SCHEMA_V1 =
	"pointbreak.derived-change-diagnostic-authority-seed.v1";
export const DERIVED_CHANGE_DIAGNOSTIC_HOST_CONFIG_SCHEMA_V1 =
	"pointbreak.derived-change-diagnostic-host-config.v1";
export const DERIVED_CHANGE_DIAGNOSTIC_UNAVAILABLE_HOST_CONFIG_SCHEMA_V1 =
	"pointbreak.derived-change-diagnostic-unavailable-host-config.v1";
export const DERIVED_CHANGE_DIAGNOSTIC_MERGE_CONFIG_SCHEMA_V1 =
	"pointbreak.derived-change-diagnostic-merge-config.v1";
export const DERIVED_CHANGE_DIAGNOSTIC_HOST_FRAGMENT_BASENAME_V1 =
	"host-fragment.json";

const CAMPAIGN_MODULE = fileURLToPath(import.meta.url);
const HOST_PROBE_ENV = "POINTBREAK_DERIVED_CHANGE_HOST_PROBE";
const BINARY_PREFLIGHT_ENV = "POINTBREAK_DERIVED_CHANGE_BINARY_PREFLIGHT";
const CONTROL_CASE_ENV = "POINTBREAK_DERIVED_CHANGE_CONTROL_CASE";
const BOUND_REQUEST_ENV = "POINTBREAK_DERIVED_CHANGE_BOUND_REQUEST";

const DERIVED_POLICY_TESTS = Object.freeze([
	"bodyless_schema_names_allow_hash_metadata_and_refuse_body_material",
	"bound_change_read_receipt_crosses_the_fragment_boundary",
	"bound_smoke_fragment_assembles_into_a_verified_incomplete_evidence_package",
	"bounded_capability_pair_uses_exactly_two_point_reads",
	"candidate_open_preserves_admitted_truth_and_accounts_for_governed_namespaces",
	"change_fixture_witnesses_are_deterministic_and_bind_public_authority",
	"change_fixture_witnesses_name_duplicate_removal_and_fault_carriers_without_prose",
	"change_fixtures_exercise_their_declared_derived_outcomes",
	"change_read_summaries_require_raw_receipt_authority",
	"derived_access_evaluator_rejects_incomplete_or_ambiguous_rows",
	"derived_change_carrier_work_is_page_proportional_and_hydrates_required_support",
	"derived_change_compact_proposal_mismatches_fail_closed",
	"derived_change_selected_proposal_failures_are_typed_and_fail_closed",
	"diagnostic_documents_are_rejected_by_fragment_and_package_evidence_boundaries",
	"diagnostic_lifecycle_collection_continues_after_an_isolated_vector_failure",
	"exact_control_parser_requires_one_named_passing_test",
	"forbidden_probe_detection_marks_a_selected_carrier",
	"incomplete_and_cyclic_fixtures_have_their_declared_change_shapes",
	"native_diagnostic_result_exposes_only_the_admitted_root",
	"native_pair_verifier_rejects_diagnostic_documents_by_schema_and_reserved_path",
	"ordinary_bodyless_change_pages_refuse_summary_search_and_stale_continuations",
	"previous_and_last_keep_typed_lens_query_tamper_and_stale_refusals",
	"published_generation_witness_is_hash_only_and_deterministic",
	"qualification_typed_document_freezes_the_complete_direct_or_page_document",
	"ready_retry_may_preserve_the_current_projection_stamp",
	"stable_snapshot_refuses_a_carrier_created_between_reads",
	"stale_token_oracle_uses_a_distinct_governed_append_instead_of_ready_retry",
	"topology_witness_binds_the_complete_authoritative_inventory",
	"topology_witness_preflight_binds_classification_and_exact_current_revisions",
]);

const SCRIPT_POLICY_FILES = Object.freeze([
	"scripts/change-inspector-browser-diagnostics.selftest.mjs",
	"scripts/derived-change-diagnostic-report.selftest.mjs",
	"scripts/derived-change-diagnostic.selftest.mjs",
	"scripts/derived-change-diagnostic-native.selftest.mjs",
	"scripts/derived-change-diagnostic-browser.selftest.mjs",
	"scripts/derived-change-diagnostic-change-read.selftest.mjs",
	"scripts/derived-change-diagnostic-campaign.selftest.mjs",
]);

const PLATFORM_CONTROL_TESTS = Object.freeze([
	{
		name: "path-identity",
		testName: "existing_path_identity_ignores_equivalent_lexical_spellings",
		checkpoint: "canonical-path-identity",
	},
	{
		name: "carrier-mutation",
		testName:
			"candidate_open_preserves_admitted_truth_and_accounts_for_governed_namespaces",
		checkpoint: "authoritative-carrier-mutation",
	},
	{
		name: "carrier-missing",
		testName: "change_fixtures_exercise_their_declared_derived_outcomes",
		checkpoint: "missing-carrier-fixtures",
	},
	{
		name: "busy-writer",
		testName: "stable_authority_successor_does_not_wait_for_a_busy_writer",
		checkpoint: "busy-writer-continuation",
	},
]);

const NTFS_CONTROL_TEST = Object.freeze({
	name: "volume-churn",
	testName: "native_ntfs_stable_continuation_persists_unrelated_volume_churn",
	checkpoint: "irrelevant-volume-churn",
});

const CHANGE_READ_PROGRAM_NAMES = Object.freeze([
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
]);
const DERIVED_ACCESS_CONTRACT_SCHEMA_V1 =
	"pointbreak.qualification-derived-access-contract.v1";
const DERIVED_ACCESS_CONTRACT_SHA256_V1 =
	"c29fd0b862cfd3594c02b88f159477adb9b8666b8dfeebd868e766f8cf025ab8";
const CHANGE_READY_STORE = "tests/support/assets/change-ready-store";
const CHANGE_READY_ACTIVATION =
	"5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json";
const CHANGE_READY_COMPLETION =
	"f31956c2b820926adc74d4d03cb03820d13c9ed2739b5f7ada81611a6f8bcff1.json";

function requireObject(value, label) {
	if (value === null || typeof value !== "object" || Array.isArray(value)) {
		throw new Error(`${label} must be an object`);
	}
}

function requireText(value, label) {
	if (typeof value !== "string" || value.trim() === "") {
		throw new Error(`${label} must be non-empty text`);
	}
}

function requireAbsolutePath(value, label) {
	requireText(value, label);
	if (!isAbsolute(value)) throw new Error(`${label} must be absolute`);
}

function requireOutsideSourceRoot(root, sourceRoot, label) {
	requireAbsolutePath(root, label);
	requireAbsolutePath(sourceRoot, "diagnostic source checkout");
	const relation = relative(resolve(sourceRoot), resolve(root));
	if (
		relation === "" ||
		(relation !== ".." && !relation.startsWith(`..${sep}`))
	) {
		throw new Error(`${label} must be outside the source checkout`);
	}
}

function publicFixtureCheckpoint(campaign, checkpoint) {
	return {
		fixture: campaign.fixture.authoritySha256,
		checkpoint,
	};
}

function fixtureWitnessAuthority(campaign, fixtureId) {
	const authority = campaign.fixture.document.witnesses.find(
		(candidate) => candidate.fixtureId === fixtureId,
	);
	if (!authority) {
		throw new Error(`public fixture authority is missing ${fixtureId}`);
	}
	return authority;
}

function fixtureSourceAuthority(campaign, path) {
	const authority = campaign.fixture.document.sourceFiles.find(
		(candidate) => candidate.path === path,
	);
	if (!authority) {
		throw new Error(`public fixture source authority is missing ${path}`);
	}
	return authority;
}

function nativeChildDescriptors(platformId) {
	const descriptors = [];
	for (const tier of ["D0-128", "L1", "L7"]) {
		const native = `${platformId}.native-${tier}`;
		const setup = `${platformId}.lifecycle-${tier}-setup`;
		descriptors.push({
			id: native,
			lane: "native",
			dependsOn: [`${platformId}.native-stateful`],
		});
		descriptors.push({
			id: setup,
			lane: "native",
			dependsOn: [`${platformId}.native-stateful`, native],
		});
		for (const criterion of DERIVED_CHANGE_NATIVE_LIFECYCLE_CRITERIA_V1) {
			descriptors.push({
				id: `${platformId}.lifecycle-${tier}-${criterion}`,
				lane: "native",
				dependsOn: [`${platformId}.native-stateful`, setup],
			});
		}
	}
	return descriptors;
}

function browserChildDescriptors(platformId, iterations) {
	return [
		{
			id: `${platformId}.browser-bootstrap`,
			lane: "browser",
			dependsOn: [`${platformId}.browser-transition`],
		},
		...Array.from({ length: iterations }, (_, index) => ({
			id: `${platformId}.browser-widen-${index + 1}`,
			lane: "browser",
			dependsOn: [`${platformId}.browser-transition`],
		})),
		{
			id: `${platformId}.browser-runtime-pageerror`,
			lane: "browser",
			dependsOn: [`${platformId}.browser-transition`],
		},
		{
			id: `${platformId}.browser-runtime-console`,
			lane: "browser",
			dependsOn: [`${platformId}.browser-transition`],
		},
	];
}

function changeReadChildDescriptors(platformId) {
	const launcher = `${platformId}.change-read-stateful`;
	return derivedChangeChangeReadChildDescriptors().map(
		({ id, lane, dependsOn }) => ({
			id: `${platformId}.${id}`,
			lane,
			dependsOn: [
				launcher,
				...dependsOn.map((dependency) => `${platformId}.${dependency}`),
			],
		}),
	);
}

export function derivedChangeDiagnosticCaseDescriptors(
	platform,
	{ browserIterations = 0 } = {},
) {
	requireObject(platform, "diagnostic platform");
	requireText(platform.id, "diagnostic platform id");
	if (
		!Number.isInteger(browserIterations) ||
		browserIterations < 0 ||
		browserIterations > 32
	) {
		throw new Error(
			"diagnostic browser iterations must be an integer from 0 through 32",
		);
	}
	if (browserIterations > 0 && platform.operatingSystem !== "macos") {
		throw new Error(
			"the focused real-browser diagnostic is supported only on macOS",
		);
	}
	const id = platform.id;
	const preflight = `${id}.preflight`;
	const binaryPreflight = `${id}.product-version`;
	const descriptors = [
		{ id: preflight, lane: "preflight", dependsOn: [] },
		{ id: binaryPreflight, lane: "preflight", dependsOn: [preflight] },
		{
			id: `${id}.compile-all-targets`,
			lane: "compile",
			dependsOn: [preflight, binaryPreflight],
		},
		{
			id: `${id}.policy-derived-access`,
			lane: "policy",
			dependsOn: [preflight, binaryPreflight],
		},
		{
			id: `${id}.change-read-stateful`,
			lane: "native",
			dependsOn: [preflight, binaryPreflight],
		},
	];
	descriptors.push(...changeReadChildDescriptors(id));
	for (const control of [
		...PLATFORM_CONTROL_TESTS,
		...(platform.filesystem === "ntfs" ? [NTFS_CONTROL_TEST] : []),
	]) {
		descriptors.push({
			id: `${id}.platform-${control.name}`,
			lane: "platform",
			dependsOn: [preflight, binaryPreflight],
		});
	}
	descriptors.push({
		id: `${id}.native-stateful`,
		lane: "native",
		dependsOn: [preflight, binaryPreflight],
	});
	descriptors.push(...nativeChildDescriptors(id));
	if (platform.operatingSystem === "macos") {
		descriptors.push(
			{
				id: `${id}.policy-scripts`,
				lane: "policy",
				dependsOn: [preflight, binaryPreflight],
			},
			{
				id: `${id}.policy-web`,
				lane: "policy",
				dependsOn: [preflight, binaryPreflight],
			},
		);
	}
	if (browserIterations > 0) {
		descriptors.push({
			id: `${id}.browser-transition`,
			lane: "browser",
			dependsOn: [preflight, binaryPreflight],
		});
		descriptors.push(...browserChildDescriptors(id, browserIterations));
	}
	descriptors.push({
		id: `${id}.postflight`,
		lane: "preflight",
		dependsOn: [preflight, binaryPreflight],
	});
	return descriptors;
}

function browserIterationsFromCampaign(campaign, platformId) {
	const prefix = `${platformId}.browser-widen-`;
	const numbers = campaign.requiredCaseIds
		.filter((id) => id.startsWith(prefix))
		.map((id) => Number(id.slice(prefix.length)))
		.sort((left, right) => left - right);
	for (const [index, number] of numbers.entries()) {
		if (number !== index + 1)
			throw new Error(
				"diagnostic browser iteration authority is not contiguous",
			);
	}
	return numbers.length;
}

export function createDerivedChangeDiagnosticCampaign(seed) {
	requireObject(seed, "diagnostic authority seed");
	if (seed.schema !== DERIVED_CHANGE_DIAGNOSTIC_AUTHORITY_SEED_SCHEMA_V1) {
		throw new Error("unsupported diagnostic authority seed schema");
	}
	requireObject(
		seed.browserIterationsByPlatform,
		"diagnostic browser iteration authority",
	);
	if (!Array.isArray(seed.platforms) || seed.platforms.length === 0) {
		throw new Error("diagnostic authority seed requires platforms");
	}
	for (const platform of seed.platforms) {
		requireText(platform.architecture, "diagnostic platform architecture");
		if (!["macos", "windows"].includes(platform.operatingSystem)) {
			throw new Error("diagnostic platform operating system is unsupported");
		}
		if (!["apfs", "ntfs"].includes(platform.filesystem)) {
			throw new Error("diagnostic platform filesystem is unsupported");
		}
	}
	const campaign = {
		id: seed.id,
		rootComponent: DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1,
		source: structuredClone(seed.source),
		fixture: structuredClone(seed.fixture),
		requiredPlatformIds: seed.platforms.map(({ id }) => id).sort(),
		platforms: structuredClone(seed.platforms).sort((left, right) =>
			left.id.localeCompare(right.id),
		),
		product: structuredClone(seed.product),
		harness: structuredClone(seed.harness),
		control: structuredClone(seed.control),
		requiredCaseIds: [],
	};
	for (const platform of campaign.platforms) {
		const iterations = seed.browserIterationsByPlatform[platform.id] ?? 0;
		campaign.requiredCaseIds.push(
			...derivedChangeDiagnosticCaseDescriptors(platform, {
				browserIterations: iterations,
			}).map(({ id }) => id),
		);
	}
	campaign.requiredCaseIds.sort();
	validateDerivedChangeDiagnosticCampaign(campaign);
	return campaign;
}

function validateProgramInventory(programs, platform) {
	requireObject(programs, "diagnostic host programs");
	const required = [
		"node",
		"cargo",
		"git",
		"filesystemProbe",
		"bash",
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
	];
	if (platform.operatingSystem === "macos") {
		required.push("npm", "playwrightCli", "shasum", "chmod");
	} else {
		required.push("cygpath");
	}
	for (const name of required) {
		requireAbsolutePath(programs[name], `diagnostic ${name} program`);
	}
	for (const [name, program] of Object.entries(programs)) {
		requireAbsolutePath(program, `diagnostic ${name} program`);
	}
	return [...new Set(Object.values(programs))];
}

function campaignBinarySha256(campaign, identity, platformId, role) {
	const binary = campaign[identity].binaries.find(
		(candidate) =>
			candidate.platformId === platformId &&
			(role === undefined || candidate.role === role),
	);
	if (!binary) {
		throw new Error(
			`diagnostic ${identity} binary authority is missing ${platformId}${
				role === undefined ? "" : ` ${role}`
			}`,
		);
	}
	return binary.binarySha256;
}

function changeReadProgramIdentities(config) {
	requireObject(
		config.changeRead.programSha256,
		"diagnostic Change-read program hashes",
	);
	const identities = Object.fromEntries(
		CHANGE_READ_PROGRAM_NAMES.map((name) => [
			name,
			{
				program: config.programs[name],
				binarySha256: config.changeRead.programSha256[name],
			},
		]),
	);
	if (config.programs.cygpath !== undefined) {
		identities.cygpath = {
			program: config.programs.cygpath,
			binarySha256: config.changeRead.programSha256.cygpath,
		};
	}
	return identities;
}

function createChangeReadDiagnosticConfig(config, platform) {
	requireObject(config.changeRead, "diagnostic Change-read configuration");
	requireObject(
		config.changeRead.execution,
		"diagnostic Change-read execution identity",
	);
	requireObject(
		config.changeRead.product,
		"diagnostic Change-read product identity",
	);
	requireObject(
		config.changeRead.controls,
		"diagnostic Change-read control identity",
	);
	if (
		!Array.isArray(config.changeRead.harnessArgsPrefix) ||
		config.changeRead.harnessArgsPrefix.some(
			(argument) => typeof argument !== "string",
		)
	) {
		throw new Error("diagnostic Change-read harness arguments must be strings");
	}
	const campaign = config.campaign;
	const programIdentities = changeReadProgramIdentities(config);
	const topologyMaterializer = fixtureSourceAuthority(
		campaign,
		"scripts/materialize-inspector-decision-matrix.sh",
	);
	const activation = fixtureSourceAuthority(
		campaign,
		`${CHANGE_READY_STORE}/${CHANGE_READY_ACTIVATION}`,
	);
	const completion = fixtureSourceAuthority(
		campaign,
		`${CHANGE_READY_STORE}/${CHANGE_READY_COMPLETION}`,
	);
	const harnessProgram = config.identityPaths.harness;
	const harnessArgsPrefix = [...config.changeRead.harnessArgsPrefix];
	const harnessBinarySha256 = campaignBinarySha256(
		campaign,
		"harness",
		platform.id,
	);
	const execution = {
		platform: platform.id,
		sourceCommit: campaign.source.commit,
		sourceTree: campaign.source.tree,
		cargoLockSha256: config.changeRead.execution.cargoLockSha256,
		binarySha256: harnessBinarySha256,
		contractSchema: DERIVED_ACCESS_CONTRACT_SCHEMA_V1,
		contractSha256: DERIVED_ACCESS_CONTRACT_SHA256_V1,
		rootProvenanceSha256: campaign.fixture.authoritySha256,
		commandSha256: createHash("sha256")
			.update(
				JSON.stringify([
					harnessProgram,
					...harnessArgsPrefix,
					"--derived-change-read-diagnostic",
				]),
			)
			.digest("hex"),
		operatingSystem: platform.operatingSystem,
		architecture: platform.architecture,
		filesystem: platform.filesystem,
		hostIdentitySha256: platform.hostIdentitySha256,
		sourceDirty: false,
		privateCorpusConfigured: false,
	};
	const changeReadConfig = {
		schema: DERIVED_CHANGE_CHANGE_READ_DIAGNOSTIC_CONFIG_SCHEMA_V1,
		campaignId: campaign.id,
		rootAuthoritySha256: campaign.fixture.authoritySha256,
		caseRoot: caseRoot(config, "change-read-stateful"),
		sourceCheckout: config.sourceCheckout,
		execution,
		product: {
			program: config.identityPaths.product,
			binarySha256: campaignBinarySha256(campaign, "product", platform.id),
			platform: platform.id,
			sourceCommit: campaign.source.commit,
			sourceTree: campaign.source.tree,
			cargoLockSha256: execution.cargoLockSha256,
			versionSha256: config.changeRead.product.versionSha256,
			buildProfile: config.changeRead.product.buildProfile,
			enabledFeatures: structuredClone(
				config.changeRead.product.enabledFeatures,
			),
			buildCommandSha256: config.changeRead.product.buildCommandSha256,
			operatingSystem: platform.operatingSystem,
			architecture: platform.architecture,
			sourceDirty: false,
		},
		harness: {
			program: harnessProgram,
			binarySha256: harnessBinarySha256,
			argsPrefix: harnessArgsPrefix,
		},
		controls: {
			library: {
				program: config.identityPaths.control,
				binarySha256: campaignBinarySha256(
					campaign,
					"control",
					platform.id,
					"library",
				),
				buildCommandSha256:
					config.changeRead.controls.libraryBuildCommandSha256,
			},
			cli: {
				program: config.identityPaths.controlCli,
				binarySha256: campaignBinarySha256(
					campaign,
					"control",
					platform.id,
					"cli",
				),
				buildCommandSha256: config.changeRead.controls.cliBuildCommandSha256,
			},
		},
		fixtureAuthority: {
			path: config.identityPaths.fixtureAuthority,
			sha256: campaign.fixture.authoritySha256,
			readyStore: join(config.sourceCheckout, CHANGE_READY_STORE),
			activationSha256: activation.sha256,
			completionSha256: completion.sha256,
		},
		programs: {
			...programIdentities,
			topologyMaterializer: {
				program: join(
					config.sourceCheckout,
					"scripts",
					"materialize-inspector-decision-matrix.sh",
				),
				binarySha256: topologyMaterializer.sha256,
			},
			hash: {
				...programIdentities.hash,
				mode: config.changeRead.hashMode,
			},
		},
		summaryQuery: config.changeRead.summaryQuery,
	};
	return validateDerivedChangeChangeReadDiagnosticConfig(changeReadConfig);
}

function validateHostConfig(config) {
	requireObject(config, "diagnostic host config");
	if (config.schema !== DERIVED_CHANGE_DIAGNOSTIC_HOST_CONFIG_SCHEMA_V1) {
		throw new Error("unsupported diagnostic host config schema");
	}
	validateDerivedChangeDiagnosticCampaign(config.campaign);
	requireText(config.platformId, "diagnostic host platform id");
	const platform = config.campaign.platforms.find(
		({ id }) => id === config.platformId,
	);
	if (!platform)
		throw new Error(
			"diagnostic host platform is absent from campaign authority",
		);
	requireAbsolutePath(config.sourceCheckout, "diagnostic source checkout");
	requireOutsideSourceRoot(
		config.outputRoot,
		config.sourceCheckout,
		"diagnostic host output root",
	);
	if (
		basename(resolve(config.outputRoot)) !==
		DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1
	) {
		throw new Error(
			`diagnostic host output root must end in ${DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1}`,
		);
	}
	requireObject(config.identityPaths, "diagnostic identity paths");
	for (const name of [
		"product",
		"harness",
		"control",
		"controlCli",
		"fixtureAuthority",
	]) {
		requireAbsolutePath(
			config.identityPaths[name],
			`diagnostic ${name} identity path`,
		);
	}
	if (config.allowedSignersPath !== undefined) {
		requireAbsolutePath(
			config.allowedSignersPath,
			"diagnostic allowed signers path",
		);
	}
	const requiredExecutables = validateProgramInventory(
		config.programs,
		platform,
	);
	const observedIterations = browserIterationsFromCampaign(
		config.campaign,
		config.platformId,
	);
	if (config.browserIterations !== observedIterations) {
		throw new Error(
			"diagnostic browser iteration count differs from campaign authority",
		);
	}
	const descriptors = derivedChangeDiagnosticCaseDescriptors(platform, {
		browserIterations: observedIterations,
	});
	const observedIds = descriptors.map(({ id }) => id).sort();
	const requiredIds = config.campaign.requiredCaseIds
		.filter((id) => id.startsWith(`${config.platformId}.`))
		.sort();
	if (JSON.stringify(observedIds) !== JSON.stringify(requiredIds)) {
		throw new Error(
			"diagnostic host case inventory differs from campaign authority",
		);
	}
	const changeReadConfig = createChangeReadDiagnosticConfig(config, platform);
	return { platform, descriptors, requiredExecutables, changeReadConfig };
}

function caseRoot(config, name) {
	return join(config.outputRoot, "cases", name);
}

function executableCase({
	id,
	lane,
	dependsOn,
	program,
	args,
	root,
	campaign,
	failureClass = "case_failure",
	phase,
	checkpoint = phase,
	cwd,
	env,
	mutatesRoot = true,
	artifactPaths,
	collection,
}) {
	return {
		id,
		lane,
		required: true,
		dependsOn,
		failureClass,
		phase,
		fixtureCheckpoint: publicFixtureCheckpoint(campaign, checkpoint),
		program,
		args,
		root,
		...(cwd ? { cwd } : {}),
		...(env ? { env } : {}),
		mutatesRoot,
		...(artifactPaths ? { artifactPaths } : {}),
		...(collection ? { collection } : {}),
	};
}

function policyExpression() {
	return DERIVED_POLICY_TESTS.map((name) => `test(${name})`).join(" | ");
}

function nativeExpectedCaseIds() {
	return nativeChildDescriptors("platform")
		.map(({ id }) => id.slice("platform.".length))
		.sort();
}

function changeReadExpectedCaseIds() {
	return derivedChangeChangeReadChildDescriptors()
		.map(({ id }) => id)
		.sort();
}

function browserExpectedCaseIds(iterations) {
	return browserChildDescriptors("platform", iterations)
		.map(({ id }) => id.slice("platform.".length))
		.sort();
}

function browserArtifacts() {
	return [
		"logs/harness.json",
		"logs/pointbreak-version.json",
		"logs/fixture-witness.json",
		"logs/fixture-materialize.log",
		"logs/derived-build.json",
		"logs/derived-build.log",
		"logs/inspect-startup.json",
		"logs/inspect-server.log",
		"logs/browser-program.mjs",
		"logs/browser-open.log",
		"logs/browser.log",
		"logs/browser-result.json",
		"logs/browser-close.log",
		"harness/scripts/derived-change-diagnostic-browser.sh",
		"harness/scripts/derived-change-diagnostic-browser.mjs",
		"harness/scripts/change-inspector-browser-diagnostics.mjs",
		"harness/scripts/materialize-inspector-decision-matrix.sh",
		"harness/tests/support/assets/change-ready-store/5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json",
		"harness/tests/support/assets/change-ready-store/f31956c2b820926adc74d4d03cb03820d13c9ed2739b5f7ada81611a6f8bcff1.json",
		"harness/pointbreak",
	];
}

function bindingRequest(request) {
	return {
		campaign: request.campaign,
		platformId: request.platformId,
		sourcePreflight: request.sourcePreflight,
		identityPaths: request.identityPaths,
		requiredExecutables: request.requiredExecutables,
		cases: request.cases.map(
			({ program, unavailableReason, unknownReason }) => ({
				program,
				...(unavailableReason ? { unavailableReason } : {}),
				...(unknownReason ? { unknownReason } : {}),
			}),
		),
	};
}

export function createDerivedChangeDiagnosticHostRequest(config) {
	const { platform, requiredExecutables, changeReadConfig } =
		validateHostConfig(config);
	const campaign = config.campaign;
	const prefix = config.platformId;
	const preflight = `${prefix}.preflight`;
	const binaryPreflight = `${prefix}.product-version`;
	const sourcePreflight = {
		sourceRoot: config.sourceCheckout,
		gitProgram: config.programs.git,
		...(config.allowedSignersPath
			? { allowedSignersPath: config.allowedSignersPath }
			: {}),
	};
	const sourceScript = (name) => join(config.sourceCheckout, "scripts", name);
	const campaignModule = sourceScript("derived-change-diagnostic-campaign.mjs");
	const nativeModule = sourceScript("derived-change-diagnostic-native.mjs");
	const changeReadModule = sourceScript(
		"derived-change-diagnostic-change-read.mjs",
	);
	const browserScript = sourceScript("derived-change-diagnostic-browser.sh");
	const cases = [];
	cases.push(
		executableCase({
			id: preflight,
			lane: "preflight",
			dependsOn: [],
			program: config.programs.node,
			args: [campaignModule, "probe-host"],
			root: "preflight",
			campaign,
			failureClass: "global_invalid",
			phase: "host-platform-preflight",
			checkpoint: "platform-identity",
			env: {
				[HOST_PROBE_ENV]: JSON.stringify({
					platform,
					filesystemProbeProgram: config.programs.filesystemProbe,
				}),
			},
			mutatesRoot: false,
		}),
	);
	cases.push(
		executableCase({
			id: binaryPreflight,
			lane: "preflight",
			dependsOn: [preflight],
			program: config.programs.node,
			args: [campaignModule, "binary-preflight"],
			root: "product-version",
			campaign,
			failureClass: "global_invalid",
			phase: "product-self-identity",
			env: {
				[BINARY_PREFLIGHT_ENV]: JSON.stringify({
					product: config.identityPaths.product,
					harness: config.identityPaths.harness,
					control: config.identityPaths.control,
					controlCli: config.identityPaths.controlCli,
					sourceCheckout: config.sourceCheckout,
					source: campaign.source,
				}),
			},
			mutatesRoot: false,
		}),
	);
	cases.push(
		executableCase({
			id: `${prefix}.compile-all-targets`,
			lane: "compile",
			dependsOn: [preflight, binaryPreflight],
			program: config.programs.cargo,
			args: [
				"+stable",
				"build",
				"--locked",
				"--workspace",
				"--all-targets",
				"--all-features",
				"--keep-going",
			],
			root: "compile-all-targets",
			campaign,
			phase: "compile-all-diagnostic-targets",
			cwd: config.sourceCheckout,
			env: {
				CARGO_TARGET_DIR: join(
					caseRoot(config, "compile-all-targets"),
					"target",
				),
			},
		}),
	);
	cases.push(
		executableCase({
			id: `${prefix}.policy-derived-access`,
			lane: "policy",
			dependsOn: [preflight, binaryPreflight],
			program: config.programs.cargo,
			args: [
				"+stable",
				"nextest",
				"run",
				"--locked",
				"--all-features",
				"--no-fail-fast",
				"-E",
				policyExpression(),
			],
			root: "policy-derived-access",
			campaign,
			phase: "affected-policy-suite",
			cwd: config.sourceCheckout,
			env: {
				CARGO_TARGET_DIR: join(
					caseRoot(config, "policy-derived-access"),
					"target",
				),
			},
		}),
	);
	const changeReadRoot = "change-read-stateful";
	cases.push(
		executableCase({
			id: `${prefix}.change-read-stateful`,
			lane: "native",
			dependsOn: [preflight, binaryPreflight],
			program: config.programs.node,
			args: [changeReadModule, "--config-env"],
			root: changeReadRoot,
			campaign,
			failureClass: "global_invalid",
			phase: "derived-change-read-stateful-collection",
			env: {
				POINTBREAK_DERIVED_CHANGE_CHANGE_READ_CONFIG:
					JSON.stringify(changeReadConfig),
			},
			collection: {
				schema: DERIVED_CHANGE_DIAGNOSTIC_COLLECTION_SCHEMA_V1,
				source: "stdout",
				idPrefix: `${prefix}.`,
				expectedCaseIds: changeReadExpectedCaseIds(),
			},
		}),
	);
	for (const control of [
		...PLATFORM_CONTROL_TESTS,
		...(platform.filesystem === "ntfs" ? [NTFS_CONTROL_TEST] : []),
	]) {
		cases.push(
			executableCase({
				id: `${prefix}.platform-${control.name}`,
				lane: "platform",
				dependsOn: [preflight, binaryPreflight],
				program: config.programs.node,
				args: [campaignModule, "control-test"],
				root: `platform-${control.name}`,
				campaign,
				phase: "platform-policy-case",
				checkpoint: control.checkpoint,
				env: {
					[CONTROL_CASE_ENV]: JSON.stringify({
						program: config.identityPaths.control,
						testName: control.testName,
						cwd: config.sourceCheckout,
					}),
				},
			}),
		);
	}
	const nativeRoot = "native-stateful";
	cases.push(
		executableCase({
			id: `${prefix}.native-stateful`,
			lane: "native",
			dependsOn: [preflight, binaryPreflight],
			program: config.programs.node,
			args: [nativeModule, "--config-env"],
			root: nativeRoot,
			campaign,
			failureClass: "lane_invalid",
			phase: "native-stateful-collection",
			env: {
				POINTBREAK_DERIVED_CHANGE_NATIVE_CONFIG: JSON.stringify({
					schema: DERIVED_CHANGE_NATIVE_DIAGNOSTIC_CONFIG_SCHEMA_V1,
					campaignId: campaign.id,
					rootAuthoritySha256: campaign.fixture.authoritySha256,
					caseRoot: caseRoot(config, nativeRoot),
					sourceCheckout: config.sourceCheckout,
					gitProgram: config.programs.git,
					source: campaign.source,
					platform,
					harness: { program: config.identityPaths.harness, argsPrefix: [] },
				}),
			},
			collection: {
				schema: DERIVED_CHANGE_DIAGNOSTIC_COLLECTION_SCHEMA_V1,
				source: "stdout",
				idPrefix: `${prefix}.`,
				expectedCaseIds: nativeExpectedCaseIds(),
			},
		}),
	);
	if (platform.operatingSystem === "macos") {
		cases.push(
			executableCase({
				id: `${prefix}.policy-scripts`,
				lane: "policy",
				dependsOn: [preflight, binaryPreflight],
				program: config.programs.node,
				args: ["--test", ...SCRIPT_POLICY_FILES],
				root: "policy-scripts",
				campaign,
				phase: "diagnostic-script-policy",
				cwd: config.sourceCheckout,
			}),
			executableCase({
				id: `${prefix}.policy-web`,
				lane: "policy",
				dependsOn: [preflight, binaryPreflight],
				program: config.programs.npm,
				args: [
					"exec",
					"vitest",
					"run",
					"test/change-inspector-interaction-lifecycle.test.ts",
				],
				root: "policy-web",
				campaign,
				phase: "browser-lifecycle-policy",
				cwd: join(config.sourceCheckout, "src/cli/inspect/web"),
				env: {
					npm_config_cache: join(caseRoot(config, "policy-web"), "npm-cache"),
				},
			}),
		);
	}
	if (config.browserIterations > 0) {
		const browserRoot = "browser-transition";
		const topologyFixtureAuthority = fixtureWitnessAuthority(
			campaign,
			"topology-v1",
		);
		const topologyMaterializerAuthority = fixtureSourceAuthority(
			campaign,
			"scripts/materialize-inspector-decision-matrix.sh",
		);
		cases.push(
			executableCase({
				id: `${prefix}.browser-transition`,
				lane: "browser",
				dependsOn: [preflight, binaryPreflight],
				program: config.programs.bash,
				args: [
					browserScript,
					"--root",
					caseRoot(config, browserRoot),
					"--campaign-id",
					campaign.id,
					"--iterations",
					String(config.browserIterations),
				],
				root: browserRoot,
				campaign,
				failureClass: "lane_invalid",
				phase: "focused-real-browser-collection",
				env: {
					POINTBREAK_BINARY: config.identityPaths.product,
					POINTBREAK_EXPECTED_SOURCE_COMMIT: campaign.source.commit,
					POINTBREAK_EXPECTED_SOURCE_TREE: campaign.source.tree,
					PLAYWRIGHT_CLI: config.programs.playwrightCli,
					POINTBREAK_GIT_PROGRAM: config.programs.git,
					POINTBREAK_JQ_PROGRAM: config.programs.jq,
					POINTBREAK_NODE_PROGRAM: config.programs.node,
					POINTBREAK_SHASUM_PROGRAM: config.programs.shasum,
					POINTBREAK_FIND_PROGRAM: config.programs.find,
					POINTBREAK_SORT_PROGRAM: config.programs.sort,
					POINTBREAK_WC_PROGRAM: config.programs.wc,
					POINTBREAK_TR_PROGRAM: config.programs.tr,
					POINTBREAK_CP_PROGRAM: config.programs.cp,
					POINTBREAK_HEAD_PROGRAM: config.programs.head,
					POINTBREAK_DIRNAME_PROGRAM: config.programs.dirname,
					POINTBREAK_MKDIR_PROGRAM: config.programs.mkdir,
					POINTBREAK_RM_PROGRAM: config.programs.rm,
					POINTBREAK_CHMOD_PROGRAM: config.programs.chmod,
					POINTBREAK_AWK_PROGRAM: config.programs.awk,
					POINTBREAK_CYGPATH_PROGRAM: config.programs.cygpath ?? "absent",
					POINTBREAK_EXPECTED_FIXTURE_ID: "topology-v1",
					POINTBREAK_EXPECTED_AUTHORITATIVE_INVENTORY_SHA256:
						topologyFixtureAuthority.authoritativeInventorySha256,
					POINTBREAK_EXPECTED_FIXTURE_WITNESS_SHA256:
						topologyFixtureAuthority.witnessSha256,
					POINTBREAK_EXPECTED_TOPOLOGY_MATERIALIZER_SHA256:
						topologyMaterializerAuthority.sha256,
					...(config.allowedSignersPath
						? {
								POINTBREAK_ALLOWED_SIGNERS_PATH: join(
									config.outputRoot,
									"authority",
									"allowed-signers",
								),
							}
						: {}),
				},
				artifactPaths: browserArtifacts(),
				collection: {
					schema: DERIVED_CHANGE_DIAGNOSTIC_COLLECTION_SCHEMA_V1,
					source: "artifact",
					artifactPath: "logs/browser-result.json",
					idPrefix: `${prefix}.`,
					expectedCaseIds: browserExpectedCaseIds(config.browserIterations),
					completeExitCodes: [1],
				},
			}),
		);
	}
	const request = {
		schema: DERIVED_CHANGE_DIAGNOSTIC_REQUEST_SCHEMA_V1,
		campaign,
		platformId: config.platformId,
		outputRoot: config.outputRoot,
		sourcePreflight,
		identityPaths: structuredClone(config.identityPaths),
		requiredExecutables: [
			...new Set([
				...requiredExecutables,
				config.identityPaths.product,
				config.identityPaths.harness,
				config.identityPaths.control,
				config.identityPaths.controlCli,
			]),
		],
		cases,
	};
	const postflight = executableCase({
		id: `${prefix}.postflight`,
		lane: "preflight",
		dependsOn: [preflight, binaryPreflight],
		program: config.programs.node,
		args: [campaignModule, "verify-bindings"],
		root: "postflight",
		campaign,
		failureClass: "global_invalid",
		phase: "source-and-binary-postflight",
		checkpoint: "exact-bound-state-after-cases",
		env: {},
		mutatesRoot: false,
	});
	cases.push(postflight);
	postflight.env[BOUND_REQUEST_ENV] = JSON.stringify(bindingRequest(request));
	return request;
}

async function requireEmptyRoot(root) {
	try {
		const stat = await lstat(root);
		if (stat.isSymbolicLink() || !stat.isDirectory()) {
			throw new Error("diagnostic root must be a real directory");
		}
		if ((await readdir(root)).length !== 0) {
			throw new Error("diagnostic root must be empty");
		}
	} catch (error) {
		if (error?.code !== "ENOENT") throw error;
		await mkdir(root);
	}
}

async function writeExclusiveJson(path, value) {
	const file = await open(path, "wx");
	try {
		await file.writeFile(`${JSON.stringify(value)}\n`);
	} finally {
		await file.close();
	}
}

export async function runDerivedChangeDiagnosticHost(config) {
	const request = createDerivedChangeDiagnosticHostRequest(config);
	const fragment = await executeDerivedChangeDiagnosticCases(request);
	const fragmentPath = join(
		config.outputRoot,
		DERIVED_CHANGE_DIAGNOSTIC_HOST_FRAGMENT_BASENAME_V1,
	);
	await writeExclusiveJson(fragmentPath, fragment);
	return { fragment, fragmentPath };
}

function unavailableRecord(descriptor, reason) {
	return {
		id: descriptor.id,
		lane: descriptor.lane,
		required: true,
		attempted: false,
		status: "unavailable",
		dependsOn: [...descriptor.dependsOn],
		unavailableReason: reason,
	};
}

export async function writeUnavailableDerivedChangeDiagnosticHost(config) {
	requireObject(config, "unavailable diagnostic host config");
	if (
		config.schema !==
		DERIVED_CHANGE_DIAGNOSTIC_UNAVAILABLE_HOST_CONFIG_SCHEMA_V1
	) {
		throw new Error("unsupported unavailable diagnostic host config schema");
	}
	validateDerivedChangeDiagnosticCampaign(config.campaign);
	requireText(config.platformId, "unavailable diagnostic platform id");
	requireText(config.reason, "unavailable diagnostic reason");
	requireOutsideSourceRoot(
		config.outputRoot,
		config.sourceCheckout,
		"unavailable diagnostic output root",
	);
	if (
		basename(resolve(config.outputRoot)) !==
		DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1
	) {
		throw new Error(
			`unavailable diagnostic root must end in ${DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1}`,
		);
	}
	const platform = config.campaign.platforms.find(
		({ id }) => id === config.platformId,
	);
	if (!platform)
		throw new Error(
			"unavailable diagnostic platform is absent from campaign authority",
		);
	const descriptors = derivedChangeDiagnosticCaseDescriptors(platform, {
		browserIterations: browserIterationsFromCampaign(
			config.campaign,
			config.platformId,
		),
	});
	const requiredIds = config.campaign.requiredCaseIds.filter((id) =>
		id.startsWith(`${config.platformId}.`),
	);
	if (
		JSON.stringify(descriptors.map(({ id }) => id).sort()) !==
		JSON.stringify([...requiredIds].sort())
	) {
		throw new Error(
			"unavailable diagnostic case inventory differs from campaign authority",
		);
	}
	await assertDerivedChangeDiagnosticOutputRootSafety(
		config.outputRoot,
		config.sourceCheckout,
	);
	await requireEmptyRoot(config.outputRoot);
	const fragment = finalizeDerivedChangeDiagnosticFragment({
		schema: DERIVED_CHANGE_DIAGNOSTIC_FRAGMENT_SCHEMA_V1,
		campaign: structuredClone(config.campaign),
		platform: structuredClone(platform),
		artifacts: [],
		cases: descriptors.map((descriptor) =>
			unavailableRecord(descriptor, config.reason),
		),
	});
	const fragmentPath = join(
		config.outputRoot,
		DERIVED_CHANGE_DIAGNOSTIC_HOST_FRAGMENT_BASENAME_V1,
	);
	await writeExclusiveJson(fragmentPath, fragment);
	return { fragment, fragmentPath };
}

export async function mergeDerivedChangeDiagnosticCampaign(config) {
	requireObject(config, "diagnostic merge config");
	if (config.schema !== DERIVED_CHANGE_DIAGNOSTIC_MERGE_CONFIG_SCHEMA_V1) {
		throw new Error("unsupported diagnostic merge config schema");
	}
	validateDerivedChangeDiagnosticCampaign(config.campaign);
	requireOutsideSourceRoot(
		config.outputRoot,
		config.sourceCheckout,
		"diagnostic report output root",
	);
	if (
		basename(resolve(config.outputRoot)) !==
		DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1
	) {
		throw new Error(
			`diagnostic report root must end in ${DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1}`,
		);
	}
	if (
		!Array.isArray(config.fragmentPaths) ||
		config.fragmentPaths.length !==
			config.campaign.requiredPlatformIds.length ||
		config.fragmentPaths.some((path) => !isAbsolute(path))
	) {
		throw new Error(
			"diagnostic merge requires one absolute fragment path per platform",
		);
	}
	const fragments = await Promise.all(
		config.fragmentPaths.map(async (path) =>
			JSON.parse(await readFile(path, "utf8")),
		),
	);
	const report = mergeDerivedChangeDiagnosticReport({
		campaign: config.campaign,
		fragments,
	});
	validateDerivedChangeDiagnosticReport(report);
	await assertDerivedChangeDiagnosticOutputRootSafety(
		config.outputRoot,
		config.sourceCheckout,
	);
	await requireEmptyRoot(config.outputRoot);
	const reportPath = join(
		config.outputRoot,
		DERIVED_CHANGE_DIAGNOSTIC_REPORT_BASENAME_V1,
	);
	await writeExclusiveJson(reportPath, report);
	return { report, reportPath };
}

async function runCommand(program, args, options = {}) {
	return await new Promise((resolvePromise, rejectPromise) => {
		const child = spawn(program, args, {
			...options,
			shell: false,
			stdio: ["ignore", "pipe", "pipe"],
		});
		const stdout = [];
		const stderr = [];
		child.stdout.on("data", (chunk) => stdout.push(chunk));
		child.stderr.on("data", (chunk) => stderr.push(chunk));
		child.once("error", rejectPromise);
		child.once("exit", (code, signal) =>
			resolvePromise({
				code,
				signal,
				stdout: Buffer.concat(stdout),
				stderr: Buffer.concat(stderr),
			}),
		);
	});
}

async function sha256File(path) {
	return createHash("sha256")
		.update(await readFile(path))
		.digest("hex");
}

function oneNamedControlPass(outcome, testName) {
	const output = outcome.stdout.toString("utf8");
	const namedRows = output
		.split(/\r?\n/u)
		.filter(
			(line) =>
				line.startsWith("test ") &&
				line.includes(testName) &&
				line.includes(" ..."),
		);
	return (
		outcome.code === 0 &&
		outcome.signal === null &&
		namedRows.length === 1 &&
		/test result: ok\. 1 passed; 0 failed;/u.test(output)
	);
}

async function runBinaryPreflight() {
	const config = JSON.parse(process.env[BINARY_PREFLIGHT_ENV] ?? "");
	requireObject(config, "diagnostic binary preflight");
	for (const name of [
		"product",
		"harness",
		"control",
		"controlCli",
		"sourceCheckout",
	]) {
		requireAbsolutePath(config[name], `diagnostic ${name} path`);
	}
	requireObject(config.source, "diagnostic binary preflight source");

	const productOutcome = await runCommand(
		config.product,
		["version", "--format", "json"],
		{ cwd: config.sourceCheckout, env: process.env },
	);
	if (productOutcome.code !== 0 || productOutcome.signal !== null) {
		throw new Error("diagnostic product identity command failed");
	}
	const product = JSON.parse(productOutcome.stdout.toString("utf8"));
	if (
		product?.schema !== "pointbreak.version" ||
		product?.version !== 1 ||
		product?.build?.source !== "git" ||
		product?.build?.commit !== config.source.commit ||
		product?.build?.dirty !== false
	) {
		throw new Error(
			"diagnostic product binary differs from exact clean source",
		);
	}

	const harnessOutcome = await runCommand(
		config.harness,
		["--derived-access-contract"],
		{ cwd: config.sourceCheckout, env: process.env },
	);
	if (harnessOutcome.code !== 0 || harnessOutcome.signal !== null) {
		throw new Error("diagnostic harness identity command failed");
	}
	const harness = JSON.parse(harnessOutcome.stdout.toString("utf8"));
	const derivation = harness?.contract?.derivation;
	if (
		harness?.schema !==
			"pointbreak.qualification-derived-access-contract-publication.v1" ||
		derivation?.pointbreakCommit !== config.source.commit ||
		derivation?.pointbreakTree !== config.source.tree ||
		derivation?.cargoLockSha256 !==
			(await sha256File(join(config.sourceCheckout, "Cargo.lock"))) ||
		derivation?.privateCorpusUsed !== false
	) {
		throw new Error(
			"diagnostic harness binary differs from exact public source",
		);
	}

	for (const [role, program, testName] of [
		[
			"library",
			config.control,
			"bench_support::derived_access::contract::tests::qualification_library_control_binary_attests_clean_source",
		],
		[
			"CLI",
			config.controlCli,
			"cli::inspect::server::tests::qualification_cli_control_binary_attests_clean_source",
		],
	]) {
		const outcome = await runCommand(
			program,
			["--exact", testName, "--nocapture", "--test-threads=1"],
			{
				cwd: config.sourceCheckout,
				env: {
					...process.env,
					POINTBREAK_QUALIFICATION_EXPECTED_CONTROL_COMMIT:
						config.source.commit,
				},
			},
		);
		if (!oneNamedControlPass(outcome, testName)) {
			throw new Error(
				`diagnostic ${role} control binary differs from exact clean source`,
			);
		}
	}
	process.stdout.write(
		`${JSON.stringify({
			productCommit: product.build.commit,
			harnessCommit: derivation.pointbreakCommit,
			libraryControlCommit: config.source.commit,
			cliControlCommit: config.source.commit,
		})}\n`,
	);
}

function observedArchitecture() {
	return (
		new Map([
			["arm64", "aarch64"],
			["x64", "x86_64"],
		]).get(nodeArchitecture()) ?? nodeArchitecture()
	);
}

function observedOperatingSystem() {
	return (
		new Map([
			["darwin", "macos"],
			["win32", "windows"],
		]).get(nodePlatform()) ?? nodePlatform()
	);
}

function observedHostIdentitySha256() {
	return createHash("sha256")
		.update(hostname().trim().toLowerCase())
		.digest("hex");
}

async function observedFilesystem(program, operatingSystem, root) {
	const args =
		operatingSystem === "macos"
			? ["-Y", root]
			: ["fsinfo", "volumeinfo", win32.parse(root).root];
	const outcome = await runCommand(program, args);
	if (outcome.code !== 0 || outcome.signal !== null) {
		throw new Error(
			`filesystem probe failed: ${outcome.stderr.toString("utf8")}`,
		);
	}
	const output = outcome.stdout.toString("utf8");
	if (operatingSystem === "macos") {
		const filesystem = output.split(/\r?\n/u)[1]?.trim().split(/\s+/u)[1];
		if (!filesystem)
			throw new Error("macOS filesystem probe output is invalid");
		return filesystem.toLowerCase();
	}
	const match = output.match(/File System Name\s*:\s*(\S+)/iu);
	if (!match) throw new Error("Windows filesystem probe output is invalid");
	return match[1].toLowerCase();
}

async function probeHost() {
	const config = JSON.parse(process.env[HOST_PROBE_ENV] ?? "");
	requireObject(config, "diagnostic host probe");
	requireObject(config.platform, "diagnostic host probe platform");
	requireAbsolutePath(
		config.filesystemProbeProgram,
		"diagnostic filesystem probe program",
	);
	const observed = {
		operatingSystem: observedOperatingSystem(),
		architecture: observedArchitecture(),
		filesystem: await observedFilesystem(
			config.filesystemProbeProgram,
			config.platform.operatingSystem,
			process.env.POINTBREAK_DIAGNOSTIC_CASE_ROOT,
		),
		hostIdentitySha256: observedHostIdentitySha256(),
	};
	for (const field of [
		"operatingSystem",
		"architecture",
		"filesystem",
		"hostIdentitySha256",
	]) {
		if (observed[field] !== config.platform[field]) {
			throw new Error(
				`diagnostic host ${field} differs from campaign authority`,
			);
		}
	}
	process.stdout.write(`${JSON.stringify(observed)}\n`);
}

async function runControlTest() {
	const config = JSON.parse(process.env[CONTROL_CASE_ENV] ?? "");
	requireObject(config, "diagnostic control case");
	requireAbsolutePath(config.program, "diagnostic control binary");
	requireText(config.testName, "diagnostic control test name");
	requireAbsolutePath(config.cwd, "diagnostic control working directory");
	const outcome = await runCommand(
		config.program,
		["--exact", config.testName, "--nocapture", "--test-threads=1"],
		{ cwd: config.cwd, env: process.env },
	);
	process.stdout.write(outcome.stdout);
	process.stderr.write(outcome.stderr);
	if (!oneNamedControlPass(outcome, config.testName)) {
		throw new Error(
			`diagnostic control test did not produce one named pass: ${config.testName}`,
		);
	}
}

async function verifyBindings() {
	const request = JSON.parse(process.env[BOUND_REQUEST_ENV] ?? "");
	const failure = await verifyDerivedChangeDiagnosticBindings(request);
	if (failure) throw new Error(JSON.stringify(failure));
	process.stdout.write('{"boundState":"unchanged"}\n');
}

async function readConfig(path, schema) {
	requireAbsolutePath(path, "diagnostic config path");
	const config = JSON.parse(await readFile(path, "utf8"));
	if (config.schema !== schema)
		throw new Error("diagnostic config schema differs from command mode");
	return config;
}

function allRequiredPassed(fragment) {
	return fragment.cases.every(
		(record) => !record.required || record.status === "passed",
	);
}

async function cli() {
	const [mode, configPath] = process.argv.slice(2);
	if (mode === "probe-host") return await probeHost();
	if (mode === "binary-preflight") return await runBinaryPreflight();
	if (mode === "control-test") return await runControlTest();
	if (mode === "verify-bindings") return await verifyBindings();
	if (mode === "create-authority") {
		const config = await readConfig(
			configPath,
			DERIVED_CHANGE_DIAGNOSTIC_AUTHORITY_SEED_SCHEMA_V1,
		);
		requireAbsolutePath(
			config.outputPath,
			"diagnostic campaign authority output",
		);
		requireOutsideSourceRoot(
			config.outputPath,
			config.sourceCheckout,
			"diagnostic campaign authority output",
		);
		const campaign = createDerivedChangeDiagnosticCampaign(config);
		await writeExclusiveJson(config.outputPath, campaign);
		return;
	}
	if (mode === "run-host") {
		const config = await readConfig(
			configPath,
			DERIVED_CHANGE_DIAGNOSTIC_HOST_CONFIG_SCHEMA_V1,
		);
		const { fragment } = await runDerivedChangeDiagnosticHost(config);
		if (!allRequiredPassed(fragment)) process.exitCode = 1;
		return;
	}
	if (mode === "unavailable-host") {
		const config = await readConfig(
			configPath,
			DERIVED_CHANGE_DIAGNOSTIC_UNAVAILABLE_HOST_CONFIG_SCHEMA_V1,
		);
		await writeUnavailableDerivedChangeDiagnosticHost(config);
		process.exitCode = 1;
		return;
	}
	if (mode === "merge") {
		const config = await readConfig(
			configPath,
			DERIVED_CHANGE_DIAGNOSTIC_MERGE_CONFIG_SCHEMA_V1,
		);
		const { report } = await mergeDerivedChangeDiagnosticCampaign(config);
		if (report.verdict !== "green") process.exitCode = 1;
		return;
	}
	throw new Error(
		"usage: derived-change-diagnostic-campaign.mjs <create-authority|run-host|unavailable-host|merge> <absolute-config.json>",
	);
}

if (
	process.argv[1] &&
	import.meta.url === pathToFileURL(process.argv[1]).href
) {
	await cli();
}
