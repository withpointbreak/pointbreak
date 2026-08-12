import { createHash } from "node:crypto";

import {
	DERIVED_CHANGE_DETERMINISTIC_FIXTURE_IDS_V2,
	DERIVED_CHANGE_PUBLIC_FIXTURE_AUTHORITY_SCHEMA_V2,
	DERIVED_CHANGE_PUBLIC_FIXTURE_SOURCE_PATHS_V2,
	DERIVED_CHANGE_TOPOLOGY_FIXTURE_CHECKPOINT_SCHEMA_V1,
} from "./derived-change-diagnostic-fixture.mjs";

export const DERIVED_CHANGE_DIAGNOSTIC_REPORT_SCHEMA_V1 =
	"pointbreak.derived-change-diagnostic-report.v1";
export const DERIVED_CHANGE_DIAGNOSTIC_FRAGMENT_SCHEMA_V1 =
	"pointbreak.derived-change-diagnostic-fragment.v1";
export const DERIVED_CHANGE_DIAGNOSTIC_REPORT_BASENAME_V1 =
	"derived-change-diagnostic-report.json";
export const DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1 =
	"derived-change-diagnostic";

const CASE_STATUSES = new Set([
	"passed",
	"failed",
	"skipped",
	"unavailable",
	"unknown",
]);
const FAILURE_CLASSES = new Set([
	"global_invalid",
	"lane_invalid",
	"case_failure",
]);
const SHA256 = /^[0-9a-f]{64}$/;
const COMMIT = /^[0-9a-f]{40}$/;

const copy = (value) => structuredClone(value);
const equal = (left, right) => JSON.stringify(left) === JSON.stringify(right);
const own = (value, key) => Object.hasOwn(value, key);
const compareText = (left, right) => (left < right ? -1 : left > right ? 1 : 0);

function canonicalJson(value) {
	if (value === null || typeof value !== "object") return JSON.stringify(value);
	if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
	return `{${Object.keys(value)
		.sort()
		.map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`)
		.join(",")}}`;
}

function canonicalSelfSha256(value, field) {
	const preimage = copy(value);
	delete preimage[field];
	return createHash("sha256").update(canonicalJson(preimage)).digest("hex");
}

function requireObject(value, label) {
	if (value === null || typeof value !== "object" || Array.isArray(value)) {
		throw new Error(`${label} must be an object`);
	}
}

function requireExactKeys(value, keys, label) {
	const observed = Object.keys(value).sort();
	const expected = [...keys].sort();
	if (!equal(observed, expected)) {
		throw new Error(`${label} has an invalid field inventory`);
	}
}

function requireText(value, label) {
	if (typeof value !== "string" || value.trim() === "") {
		throw new Error(`${label} must be non-empty text`);
	}
}

function requireSha256(value, label) {
	if (typeof value !== "string" || !SHA256.test(value)) {
		throw new Error(`${label} must be a lowercase SHA-256`);
	}
}

function requireCommit(value, label) {
	if (typeof value !== "string" || !COMMIT.test(value)) {
		throw new Error(`${label} must be a lowercase Git object ID`);
	}
}

function requireRelativePath(value, label) {
	requireText(value, label);
	if (
		value.startsWith("/") ||
		value.split("/").some((part) => !part || part === "." || part === "..")
	) {
		throw new Error(`${label} must be a normal relative path`);
	}
}

function validateSource(source, label) {
	requireObject(source, `${label} source`);
	requireCommit(source.commit, `${label} source commit`);
	requireCommit(source.tree, `${label} source tree`);
	requireCommit(source.rangeBaseCommit, `${label} source range base commit`);
	requireSha256(source.rangeSha256, `${label} source range SHA-256`);
}

function validateFixture(fixture, label, source) {
	requireObject(fixture, `${label} fixture`);
	requireExactKeys(
		fixture,
		["authoritySha256", "document"],
		`${label} fixture`,
	);
	requireSha256(fixture.authoritySha256, `${label} fixture authority SHA-256`);
	requireObject(fixture.document, `${label} public fixture authority`);
	requireExactKeys(
		fixture.document,
		[
			"schema",
			"sourceCommit",
			"sourceTree",
			"sourceFiles",
			"topologyCheckpoint",
			"witnesses",
		],
		`${label} public fixture authority`,
	);
	if (
		fixture.document.schema !==
			DERIVED_CHANGE_PUBLIC_FIXTURE_AUTHORITY_SCHEMA_V2 ||
		fixture.document.sourceCommit !== source.commit ||
		fixture.document.sourceTree !== source.tree
	) {
		throw new Error(
			`${label} public fixture authority differs from the exact source`,
		);
	}
	if (!Array.isArray(fixture.document.sourceFiles)) {
		throw new Error(
			`${label} public fixture source authority must be an array`,
		);
	}
	const sourcePaths = fixture.document.sourceFiles.map((entry) => {
		requireObject(entry, `${label} public fixture source authority entry`);
		requireExactKeys(
			entry,
			["path", "sha256"],
			`${label} public fixture source authority entry`,
		);
		requireRelativePath(entry.path, `${label} public fixture source path`);
		requireSha256(entry.sha256, `${label} public fixture source SHA-256`);
		return entry.path;
	});
	if (!equal(sourcePaths, DERIVED_CHANGE_PUBLIC_FIXTURE_SOURCE_PATHS_V2)) {
		throw new Error(`${label} public fixture source authority is incomplete`);
	}
	if (!Array.isArray(fixture.document.witnesses)) {
		throw new Error(
			`${label} public fixture witness inventory must be an array`,
		);
	}
	const fixtureIds = fixture.document.witnesses.map((entry) => {
		requireObject(entry, `${label} public fixture witness authority entry`);
		requireExactKeys(
			entry,
			["fixtureId", "authoritativeInventorySha256", "witnessSha256"],
			`${label} public fixture witness authority entry`,
		);
		requireText(entry.fixtureId, `${label} public fixture witness id`);
		requireSha256(
			entry.authoritativeInventorySha256,
			`${label} public fixture inventory SHA-256`,
		);
		requireSha256(
			entry.witnessSha256,
			`${label} public fixture witness SHA-256`,
		);
		return entry.fixtureId;
	});
	if (!equal(fixtureIds, DERIVED_CHANGE_DETERMINISTIC_FIXTURE_IDS_V2)) {
		throw new Error(`${label} public fixture witness inventory is incomplete`);
	}
	requireObject(
		fixture.document.topologyCheckpoint,
		`${label} public fixture topology checkpoint`,
	);
	requireExactKeys(
		fixture.document.topologyCheckpoint,
		["schema", "fixtureId", "checkpointSha256"],
		`${label} public fixture topology checkpoint`,
	);
	if (
		fixture.document.topologyCheckpoint.schema !==
			DERIVED_CHANGE_TOPOLOGY_FIXTURE_CHECKPOINT_SCHEMA_V1 ||
		fixture.document.topologyCheckpoint.fixtureId !== "topology-v1"
	) {
		throw new Error(`${label} public fixture topology checkpoint is invalid`);
	}
	requireSha256(
		fixture.document.topologyCheckpoint.checkpointSha256,
		`${label} public fixture topology checkpoint SHA-256`,
	);
}

function validatePlatform(platform, label) {
	requireObject(platform, `${label} platform`);
	requireText(platform.id, `${label} platform id`);
	requireText(platform.operatingSystem, `${label} platform operating system`);
	requireText(platform.architecture, `${label} platform architecture`);
	requireText(platform.filesystem, `${label} platform filesystem`);
	requireSha256(
		platform.hostIdentitySha256,
		`${label} platform host identity SHA-256`,
	);
	requireExactKeys(
		platform,
		[
			"id",
			"operatingSystem",
			"architecture",
			"filesystem",
			"hostIdentitySha256",
		],
		`${label} platform`,
	);
}

function validateSortedInventory(values, label) {
	if (!Array.isArray(values) || values.length === 0)
		throw new Error(`required ${label} inventory must be a non-empty array`);
	if (values.some((value) => typeof value !== "string" || !value.trim()))
		throw new Error(
			`required ${label} inventory contains an invalid identifier`,
		);
	if (
		!equal(values, [...values].sort()) ||
		new Set(values).size !== values.length
	) {
		throw new Error(`required ${label} inventory must be sorted and unique`);
	}
}

function validateBinaryInventory(
	identity,
	label,
	platformIds,
	{ roles = null } = {},
) {
	requireObject(identity, `${label} identity`);
	if (!Array.isArray(identity.binaries) || identity.binaries.length === 0) {
		throw new Error(`${label} binary inventory must be a non-empty array`);
	}
	const keys = [];
	for (const binary of identity.binaries) {
		requireObject(binary, `${label} binary inventory entry`);
		requireText(binary.platformId, `${label} binary platform id`);
		requireSha256(binary.binarySha256, `${label} binary SHA-256`);
		if (roles === null) {
			if (own(binary, "role")) {
				throw new Error(`${label} binary inventory forbids roles`);
			}
			keys.push(binary.platformId);
		} else {
			requireText(binary.role, `${label} binary role`);
			if (!roles.includes(binary.role)) {
				throw new Error(`${label} binary inventory has an unknown role`);
			}
			keys.push(`${binary.platformId}\u0000${binary.role}`);
		}
	}
	const expected = platformIds
		.flatMap((platformId) =>
			roles === null
				? [platformId]
				: roles.map((role) => `${platformId}\u0000${role}`),
		)
		.sort();
	if (
		!equal(keys, [...keys].sort()) ||
		new Set(keys).size !== keys.length ||
		!equal(keys, expected)
	) {
		throw new Error(
			`${label} binary inventory must be sorted and exactly platform-bound${
				roles === null ? "" : ` with roles ${roles.join(", ")}`
			}`,
		);
	}
}

export function validateDerivedChangeDiagnosticRootComponent(rootComponent) {
	if (rootComponent !== DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1) {
		throw new Error(
			`diagnostic root component must be ${DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1}`,
		);
	}
}

export function validateDerivedChangeDiagnosticCampaign(
	campaign,
	label = "diagnostic campaign",
) {
	requireObject(campaign, label);
	requireText(campaign.id, `${label} id`);
	validateDerivedChangeDiagnosticRootComponent(campaign.rootComponent);
	validateSource(campaign.source, label);
	validateFixture(campaign.fixture, label, campaign.source);
	validateSortedInventory(campaign.requiredCaseIds, "case");
	validateSortedInventory(campaign.requiredPlatformIds, "platform");
	if (!Array.isArray(campaign.platforms) || campaign.platforms.length === 0) {
		throw new Error(`${label} platforms must be a non-empty array`);
	}
	const platformIds = campaign.platforms.map((platform) => {
		validatePlatform(platform, label);
		return platform.id;
	});
	if (
		!equal(platformIds, [...platformIds].sort()) ||
		new Set(platformIds).size !== platformIds.length ||
		!equal(platformIds, campaign.requiredPlatformIds)
	) {
		throw new Error(
			"required platform inventory differs from campaign platforms",
		);
	}
	validateBinaryInventory(campaign.product, `${label} product`, platformIds);
	validateBinaryInventory(campaign.harness, `${label} harness`, platformIds);
	validateBinaryInventory(campaign.control, `${label} control`, platformIds, {
		roles: ["cli", "library"],
	});
}

function validateFragmentShape(fragment) {
	requireObject(fragment, "diagnostic fragment");
	if (fragment.schema !== DERIVED_CHANGE_DIAGNOSTIC_FRAGMENT_SCHEMA_V1)
		throw new Error("unsupported derived Change diagnostic fragment schema");
	validateDerivedChangeDiagnosticCampaign(
		fragment.campaign,
		"diagnostic fragment campaign",
	);
	validatePlatform(fragment.platform, "diagnostic fragment");
	if (!Array.isArray(fragment.artifacts))
		throw new Error("diagnostic fragment artifacts must be an array");
	if (!Array.isArray(fragment.cases) || fragment.cases.length === 0)
		throw new Error("diagnostic fragment cases must be a non-empty array");
}

function requireSameIdentity(fragment, campaign) {
	if (!equal(fragment.campaign, campaign))
		throw new Error("campaign identity differs");
	if (
		!campaign.platforms.some((platform) => equal(platform, fragment.platform))
	)
		throw new Error("platform identity differs");
}

function artifactKey(platform, path) {
	return `${platform}:${path}`;
}

function validateArtifacts(artifacts, label = "diagnostic artifact") {
	const seen = new Map();
	for (const artifact of artifacts) {
		requireObject(artifact, label);
		requireRelativePath(artifact.path, `${label} path`);
		requireSha256(artifact.sha256, `${label} SHA-256`);
		if (seen.has(artifact.path))
			throw new Error(`duplicate diagnostic artifact path: ${artifact.path}`);
		seen.set(artifact.path, artifact.sha256);
	}
	return seen;
}

function validateFailureContext(caseRecord) {
	if (!FAILURE_CLASSES.has(caseRecord.failureClass))
		throw new Error(
			`failed diagnostic case ${caseRecord.id} has an invalid failure class`,
		);
	requireText(
		caseRecord.phase,
		`failed diagnostic case ${caseRecord.id} phase`,
	);
	requireObject(
		caseRecord.fixtureCheckpoint,
		`failed diagnostic case ${caseRecord.id} fixture checkpoint`,
	);
	requireText(
		caseRecord.fixtureCheckpoint.fixture,
		`failed diagnostic case ${caseRecord.id} fixture`,
	);
	requireText(
		caseRecord.fixtureCheckpoint.checkpoint,
		`failed diagnostic case ${caseRecord.id} checkpoint`,
	);
}

function validateNoFailureFields(caseRecord) {
	if (
		["failureClass", "expected", "actual"].some((field) =>
			own(caseRecord, field),
		)
	)
		throw new Error(
			`failure class or failure payload is forbidden on nonfailed diagnostic case ${caseRecord.id}`,
		);
	if (own(caseRecord, "phase"))
		requireText(caseRecord.phase, `diagnostic case ${caseRecord.id} phase`);
	if (own(caseRecord, "fixtureCheckpoint")) {
		requireObject(
			caseRecord.fixtureCheckpoint,
			`diagnostic case ${caseRecord.id} fixture checkpoint`,
		);
		requireText(
			caseRecord.fixtureCheckpoint.fixture,
			`diagnostic case ${caseRecord.id} fixture`,
		);
		requireText(
			caseRecord.fixtureCheckpoint.checkpoint,
			`diagnostic case ${caseRecord.id} checkpoint`,
		);
	}
}

function validateCaseShape(caseRecord, artifacts) {
	requireObject(caseRecord, "diagnostic case");
	requireText(caseRecord.id, "diagnostic case id");
	requireText(caseRecord.lane, `diagnostic case ${caseRecord.id} lane`);
	if (
		typeof caseRecord.required !== "boolean" ||
		typeof caseRecord.attempted !== "boolean"
	)
		throw new Error(
			`diagnostic case ${caseRecord.id} required and attempted must be boolean`,
		);
	if (!CASE_STATUSES.has(caseRecord.status))
		throw new Error(`diagnostic case ${caseRecord.id} has an invalid status`);
	if (
		!Array.isArray(caseRecord.dependsOn) ||
		caseRecord.dependsOn.some(
			(dependency) => typeof dependency !== "string" || !dependency.trim(),
		) ||
		new Set(caseRecord.dependsOn).size !== caseRecord.dependsOn.length
	) {
		throw new Error(
			`diagnostic case ${caseRecord.id} has invalid dependencies`,
		);
	}
	if (caseRecord.log !== undefined) {
		requireObject(caseRecord.log, `diagnostic case ${caseRecord.id} log`);
		requireRelativePath(
			caseRecord.log.path,
			`diagnostic case ${caseRecord.id} log path`,
		);
		requireSha256(
			caseRecord.log.sha256,
			`diagnostic case ${caseRecord.id} log SHA-256`,
		);
		if (artifacts.get(caseRecord.log.path) !== caseRecord.log.sha256)
			throw new Error(
				`diagnostic case ${caseRecord.id} log SHA-256 is not retained`,
			);
	}
	if (caseRecord.artifactPaths !== undefined) {
		if (!Array.isArray(caseRecord.artifactPaths))
			throw new Error(
				`diagnostic case ${caseRecord.id} artifact paths must be an array`,
			);
		for (const path of caseRecord.artifactPaths) {
			requireRelativePath(
				path,
				`diagnostic case ${caseRecord.id} artifact path`,
			);
			if (!artifacts.has(path))
				throw new Error(
					`diagnostic case ${caseRecord.id} artifact is not retained: ${path}`,
				);
		}
	}
	if (caseRecord.status === "passed") {
		if (!caseRecord.attempted)
			throw new Error(
				`passed diagnostic case ${caseRecord.id} must be attempted`,
			);
		validateNoFailureFields(caseRecord);
	} else if (caseRecord.status === "failed") {
		if (
			!caseRecord.attempted ||
			!own(caseRecord, "expected") ||
			!own(caseRecord, "actual")
		)
			throw new Error(
				`failed diagnostic case ${caseRecord.id} must be attempted with expected and actual values`,
			);
		validateFailureContext(caseRecord);
	} else if (caseRecord.status === "skipped") {
		if (
			caseRecord.attempted ||
			!caseRecord.skipReason ||
			caseRecord.dependsOn.length === 0
		)
			throw new Error(
				`skipped diagnostic case ${caseRecord.id} must be unattempted and name a skip reason and dependency`,
			);
		validateNoFailureFields(caseRecord);
	} else if (caseRecord.status === "unavailable") {
		if (caseRecord.attempted || !caseRecord.unavailableReason)
			throw new Error(
				`unavailable diagnostic case ${caseRecord.id} must be unattempted with an unavailable reason`,
			);
		validateNoFailureFields(caseRecord);
	} else {
		if (caseRecord.attempted || !caseRecord.unknownReason)
			throw new Error(
				`unknown diagnostic case ${caseRecord.id} must be unattempted with an unknown reason`,
			);
		validateNoFailureFields(caseRecord);
	}
}

function validateDependencies(cases) {
	const byId = new Map(cases.map((caseRecord) => [caseRecord.id, caseRecord]));
	for (const caseRecord of cases)
		for (const dependency of caseRecord.dependsOn)
			if (!byId.has(dependency))
				throw new Error(
					`diagnostic case ${caseRecord.id} has unknown dependency ${dependency}`,
				);
	const visiting = new Set();
	const visited = new Set();
	const visit = (record) => {
		if (visited.has(record.id)) return;
		if (visiting.has(record.id))
			throw new Error(`diagnostic dependency cycle includes ${record.id}`);
		visiting.add(record.id);
		for (const dependency of record.dependsOn) visit(byId.get(dependency));
		visiting.delete(record.id);
		visited.add(record.id);
	};
	for (const record of cases) visit(record);
	for (const record of cases) {
		const statuses = record.dependsOn.map(
			(dependency) => byId.get(dependency).status,
		);
		if (
			record.status === "skipped" &&
			!statuses.some((status) => status !== "passed")
		)
			throw new Error(
				`skipped diagnostic case ${record.id} lacks an invalid dependency`,
			);
		if (
			(record.status === "passed" || record.status === "failed") &&
			statuses.some((status) => status !== "passed")
		)
			throw new Error(
				`attempted diagnostic case ${record.id} follows an invalid dependency`,
			);
	}
}

function canonicalCaseOrder(cases) {
	const byId = new Map(cases.map((record) => [record.id, record]));
	const remaining = new Map(
		cases.map((record) => [record.id, record.dependsOn.length]),
	);
	const children = new Map(cases.map((record) => [record.id, []]));
	for (const record of cases)
		for (const dependency of record.dependsOn)
			children.get(dependency).push(record.id);
	const key = (record) => `${record.platform}\u0000${record.id}`;
	const ready = cases
		.filter((record) => remaining.get(record.id) === 0)
		.sort((left, right) => compareText(key(left), key(right)));
	const result = [];
	while (ready.length) {
		const record = ready.shift();
		result.push(record);
		for (const child of children.get(record.id)) {
			remaining.set(child, remaining.get(child) - 1);
			if (remaining.get(child) === 0) {
				ready.push(byId.get(child));
				ready.sort((left, right) => compareText(key(left), key(right)));
			}
		}
	}
	return result;
}

function countCases(cases) {
	const counts = {
		required: 0,
		attempted: 0,
		passed: 0,
		failed: 0,
		skipped: 0,
		unavailable: 0,
		unknown: 0,
	};
	for (const record of cases) {
		if (record.required) counts.required += 1;
		if (record.attempted) counts.attempted += 1;
		counts[record.status] += 1;
	}
	return counts;
}

function laneSummaries(cases) {
	const lanes = new Map();
	for (const record of cases) {
		const lane = lanes.get(record.lane) ?? {
			name: record.lane,
			counts: {
				required: 0,
				attempted: 0,
				passed: 0,
				failed: 0,
				skipped: 0,
				unavailable: 0,
				unknown: 0,
			},
		};
		if (record.required) lane.counts.required += 1;
		if (record.attempted) lane.counts.attempted += 1;
		lane.counts[record.status] += 1;
		lanes.set(record.lane, lane);
	}
	return [...lanes.values()].sort((left, right) =>
		compareText(left.name, right.name),
	);
}

export function finalizeDerivedChangeDiagnosticFragment(fragment) {
	validateFragmentShape(fragment);
	const artifacts = validateArtifacts(fragment.artifacts);
	for (const record of fragment.cases) validateCaseShape(record, artifacts);
	const finalized = copy(fragment);
	finalized.fragmentSha256 = canonicalSelfSha256(finalized, "fragmentSha256");
	return finalized;
}

function validateFinalizedFragment(fragment) {
	validateFragmentShape(fragment);
	requireSha256(fragment.fragmentSha256, "diagnostic fragment SHA-256");
	if (
		fragment.fragmentSha256 !== canonicalSelfSha256(fragment, "fragmentSha256")
	)
		throw new Error("diagnostic fragment SHA-256 differs");
}

function validateArtifactInventory(artifacts, campaign) {
	if (!Array.isArray(artifacts))
		throw new Error("diagnostic report artifact inventory must be an array");
	const platformIds = new Set(campaign.requiredPlatformIds);
	const byPlatform = new Map();
	const keys = [];
	for (const artifact of artifacts) {
		requireObject(artifact, "diagnostic report artifact");
		if (!platformIds.has(artifact.platform))
			throw new Error(
				"diagnostic report artifact platform is not campaign-bound",
			);
		requireRelativePath(artifact.path, "diagnostic report artifact path");
		requireSha256(artifact.sha256, "diagnostic report artifact SHA-256");
		const key = artifactKey(artifact.platform, artifact.path);
		keys.push(key);
		if (byPlatform.get(artifact.platform)?.has(artifact.path))
			throw new Error(`duplicate diagnostic artifact identity: ${key}`);
		const local = byPlatform.get(artifact.platform) ?? new Map();
		local.set(artifact.path, artifact.sha256);
		byPlatform.set(artifact.platform, local);
	}
	if (!equal(keys, [...keys].sort()))
		throw new Error(
			"diagnostic report artifact inventory must be deterministically ordered",
		);
	return byPlatform;
}

function reportVerdict(cases) {
	return cases.every((record) => !record.required || record.status === "passed")
		? "green"
		: "red";
}

export function validateDerivedChangeDiagnosticReport(report) {
	requireObject(report, "derived Change diagnostic report");
	if (
		report.schema !== DERIVED_CHANGE_DIAGNOSTIC_REPORT_SCHEMA_V1 ||
		report.version !== 1 ||
		report.admissible !== false
	)
		throw new Error("derived Change diagnostic report shape is invalid");
	validateDerivedChangeDiagnosticCampaign(report.campaign);
	if (
		!Array.isArray(report.cases) ||
		!Array.isArray(report.lanes) ||
		!own(report, "counts") ||
		!own(report, "artifactInventory")
	)
		throw new Error("derived Change diagnostic report rows are invalid");
	requireSha256(report.reportSha256, "diagnostic report SHA-256");
	const artifactsByPlatform = validateArtifactInventory(
		report.artifactInventory,
		report.campaign,
	);
	const ids = new Set();
	for (const record of report.cases) {
		if (ids.has(record.id))
			throw new Error(`duplicate diagnostic case id: ${record.id}`);
		ids.add(record.id);
		if (!report.campaign.requiredPlatformIds.includes(record.platform))
			throw new Error(
				`diagnostic case ${record.id} platform is not campaign-bound`,
			);
		validateCaseShape(
			record,
			artifactsByPlatform.get(record.platform) ?? new Map(),
		);
	}
	const required = report.cases
		.filter((record) => record.required)
		.map((record) => record.id)
		.sort();
	if (!equal(required, report.campaign.requiredCaseIds))
		throw new Error("required case inventory differs from diagnostic cases");
	validateDependencies(report.cases);
	if (!equal(report.cases, canonicalCaseOrder(report.cases)))
		throw new Error(
			"diagnostic report cases must be deterministically ordered",
		);
	if (!equal(report.counts, countCases(report.cases)))
		throw new Error("diagnostic report counts differ from cases");
	if (!equal(report.lanes, laneSummaries(report.cases)))
		throw new Error("diagnostic report lanes differ from cases");
	if (report.verdict !== reportVerdict(report.cases))
		throw new Error("diagnostic report verdict differs from cases");
	if (report.reportSha256 !== canonicalSelfSha256(report, "reportSha256"))
		throw new Error("diagnostic report SHA-256 differs");
}

export function mergeDerivedChangeDiagnosticReport({ campaign, fragments }) {
	validateDerivedChangeDiagnosticCampaign(campaign);
	if (!Array.isArray(fragments) || fragments.length === 0)
		throw new Error("diagnostic report requires one or more host fragments");
	const cases = [];
	const artifacts = [];
	const caseIds = new Set();
	const platformIds = new Set();
	for (const fragment of fragments) {
		validateFinalizedFragment(fragment);
		requireSameIdentity(fragment, campaign);
		platformIds.add(fragment.platform.id);
		const localArtifacts = validateArtifacts(fragment.artifacts);
		for (const artifact of fragment.artifacts)
			artifacts.push({ platform: fragment.platform.id, ...copy(artifact) });
		for (const record of fragment.cases) {
			validateCaseShape(record, localArtifacts);
			if (caseIds.has(record.id))
				throw new Error(`duplicate diagnostic case id: ${record.id}`);
			caseIds.add(record.id);
			cases.push({ ...copy(record), platform: fragment.platform.id });
		}
	}
	if (!equal([...platformIds].sort(), campaign.requiredPlatformIds))
		throw new Error("required platform inventory differs from host fragments");
	const required = cases
		.filter((record) => record.required)
		.map((record) => record.id)
		.sort();
	if (!equal(required, campaign.requiredCaseIds))
		throw new Error("required case inventory differs from diagnostic cases");
	validateDependencies(cases);
	const orderedCases = canonicalCaseOrder(cases);
	const orderedArtifacts = artifacts.sort((left, right) =>
		compareText(
			artifactKey(left.platform, left.path),
			artifactKey(right.platform, right.path),
		),
	);
	const report = {
		schema: DERIVED_CHANGE_DIAGNOSTIC_REPORT_SCHEMA_V1,
		version: 1,
		admissible: false,
		campaign: copy(campaign),
		cases: orderedCases,
		lanes: laneSummaries(orderedCases),
		counts: countCases(orderedCases),
		artifactInventory: orderedArtifacts,
		verdict: reportVerdict(orderedCases),
	};
	report.reportSha256 = canonicalSelfSha256(report, "reportSha256");
	validateDerivedChangeDiagnosticReport(report);
	return report;
}
