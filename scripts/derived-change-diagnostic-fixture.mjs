import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

export const DERIVED_CHANGE_PUBLIC_FIXTURE_AUTHORITY_SCHEMA_V2 =
	"pointbreak.derived-change-public-fixture-authority.v2";
export const DERIVED_CHANGE_TOPOLOGY_FIXTURE_CHECKPOINT_SCHEMA_V1 =
	"pointbreak.derived-change-topology-fixture-checkpoint.v1";
export const TOPOLOGY_CHECKPOINT_REPORT_SCHEMA_V1 =
	"pointbreak.derived-change-topology-checkpoint-report.v1";
export const DERIVED_CHANGE_PUBLIC_FIXTURE_SOURCE_PATHS_V2 = Object.freeze([
	"scripts/derived-change-diagnostic-fixture.mjs",
	"scripts/materialize-inspector-decision-matrix.sh",
	"src/bench_support/derived_access/materializer.rs",
	"tests/support/assets/change-ready-store/5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json",
	"tests/support/assets/change-ready-store/f31956c2b820926adc74d4d03cb03820d13c9ed2739b5f7ada81611a6f8bcff1.json",
]);
export const DERIVED_CHANGE_DETERMINISTIC_FIXTURE_IDS_V2 = Object.freeze([
	"cycle-conflicted-v1",
	"duplicate-conflict-v1",
	"duplicate-equal-v1",
	"incomplete-v1",
	"missing-carrier-v1",
	"mutated-carrier-v1",
	"removal-v1",
	"wrong-family-carrier-v1",
]);

const WITNESS_SCHEMA_V1 =
	"pointbreak.qualification-derived-change-fixture-witness.v1";
const TOPOLOGY_FIXTURE_ID_V1 = "topology-v1";
const SHA256 = /^[0-9a-f]{64}$/u;
const COMMIT = /^[0-9a-f]{40}$/u;
const REVISION = /^rev:sha256:[0-9a-f]{64}$/u;
const CHANGE = /^change:sha256:[0-9a-f]{64}$/u;
const EVENT = /^evt:sha256:[0-9a-f]{64}$/u;
const OBSERVATION = /^obs:sha256:[0-9a-f]{64}$/u;
const FACT_PORT = /^fact-port:sha256:[0-9a-f]{64}$/u;
const ARTIFACT = /^sha256:[0-9a-f]{64}$/u;

const WITNESS_TOP_LEVEL_KEYS = [
	"ambiguous_assessment_revision",
	"authoritativeInventorySha256",
	"base_commit",
	"competing_revision",
	"detached_revision",
	"fact_port",
	"first_landing",
	"fixtureId",
	"live_landing",
	"live_revision",
	"missing_artifact",
	"missing_change",
	"missing_revision",
	"primary_revision",
	"range_revision",
	"root_revision",
	"schema",
	"second_landing",
	"shared_revision",
	"staged_revision",
	"storageForbiddenProbeHashes",
	"superseded_revision",
	"topology",
	"unassessed_revision",
	"unstaged_revision",
].sort();

const STORAGE_FORBIDDEN_PROBE_HASHES_V1 = {
	payloadDocumentSha256:
		"20dfd0d4e1ce81bfb753001a61c0394914d4711e84f90fb745a659dba1ff11bf",
	proposalSummarySha256:
		"21f749c5f166ae819a99a8ff0e303297a43685fd14cc7f1b86a90751989b167c",
	proseSha256:
		"da79cc8c9b04f41616275f4a6bd027acf6d0358f3605dac74ccadfeea92945a4",
};

function assertObject(value, label) {
	if (value === null || typeof value !== "object" || Array.isArray(value))
		throw new Error(`${label} must be an object`);
}

function assertExactKeys(value, expected, label) {
	assertObject(value, label);
	const actual = Object.keys(value).sort();
	if (
		actual.length !== expected.length ||
		actual.some((key, index) => key !== expected[index])
	)
		throw new Error(`${label} top-level shape differs`);
}

function assertMatches(value, expression, label) {
	if (typeof value !== "string" || !expression.test(value))
		throw new Error(`${label} differs`);
}

function assertRevision(value, label) {
	assertMatches(value, REVISION, label);
}

function assertArtifact(value, label) {
	assertMatches(value, ARTIFACT, label);
}

function assertRevisionArtifact(value, label) {
	assertExactKeys(value, ["artifact", "revision"], label);
	assertRevision(value.revision, `${label} revision`);
	assertArtifact(value.artifact, `${label} artifact`);
}

function assertSameRevisionArtifact(left, right, label) {
	if (left.revision !== right.revision || left.artifact !== right.artifact)
		throw new Error(`${label} relation differs`);
}

function assertSameSet(left, right, label) {
	if (
		!Array.isArray(left) ||
		!Array.isArray(right) ||
		left.length !== right.length ||
		new Set(left).size !== left.length ||
		new Set(right).size !== right.length ||
		left.some((value) => !right.includes(value))
	)
		throw new Error(`${label} relation differs`);
}

function validateTopologyWitnessV1(witness) {
	assertExactKeys(witness, WITNESS_TOP_LEVEL_KEYS, "topology witness");
	if (witness.schema !== WITNESS_SCHEMA_V1)
		throw new Error("topology witness schema differs");
	if (witness.fixtureId !== TOPOLOGY_FIXTURE_ID_V1)
		throw new Error("topology witness fixture differs");
	assertMatches(
		witness.authoritativeInventorySha256,
		SHA256,
		"topology witness authoritative inventory",
	);
	assertExactKeys(
		witness.storageForbiddenProbeHashes,
		Object.keys(STORAGE_FORBIDDEN_PROBE_HASHES_V1).sort(),
		"topology witness storage forbidden probes",
	);
	for (const [name, expected] of Object.entries(
		STORAGE_FORBIDDEN_PROBE_HASHES_V1,
	)) {
		if (witness.storageForbiddenProbeHashes[name] !== expected)
			throw new Error(
				`topology witness storage forbidden probe differs: ${name}`,
			);
	}

	for (const name of [
		"primary_revision",
		"live_revision",
		"unassessed_revision",
		"superseded_revision",
		"ambiguous_assessment_revision",
		"competing_revision",
		"range_revision",
		"root_revision",
		"staged_revision",
		"unstaged_revision",
		"detached_revision",
		"missing_revision",
	])
		assertRevision(witness[name], `topology witness ${name}`);
	assertMatches(
		witness.missing_change,
		CHANGE,
		"topology witness missing change",
	);
	assertArtifact(witness.missing_artifact, "topology witness missing artifact");
	for (const name of [
		"base_commit",
		"first_landing",
		"second_landing",
		"live_landing",
	])
		assertMatches(witness[name], COMMIT, `topology witness ${name}`);

	assertExactKeys(
		witness.fact_port,
		["event_id", "origin", "port_id"],
		"topology fact port",
	);
	assertMatches(witness.fact_port.port_id, FACT_PORT, "topology fact port id");
	assertMatches(witness.fact_port.event_id, EVENT, "topology fact port event");
	assertExactKeys(
		witness.fact_port.origin,
		["artifact", "observation", "revision"],
		"topology fact port origin",
	);
	assertRevision(
		witness.fact_port.origin.revision,
		"topology fact port origin revision",
	);
	assertArtifact(
		witness.fact_port.origin.artifact,
		"topology fact port origin artifact",
	);
	assertMatches(
		witness.fact_port.origin.observation,
		OBSERVATION,
		"topology fact port origin observation",
	);

	assertExactKeys(
		witness.shared_revision,
		["artifact", "changes", "revision"],
		"topology shared revision",
	);
	assertRevision(
		witness.shared_revision.revision,
		"topology shared revision id",
	);
	assertArtifact(
		witness.shared_revision.artifact,
		"topology shared revision artifact",
	);
	if (
		!Array.isArray(witness.shared_revision.changes) ||
		witness.shared_revision.changes.length !== 4
	)
		throw new Error("topology shared revision changes differ");
	for (const value of witness.shared_revision.changes)
		assertMatches(value, CHANGE, "topology shared revision change");

	assertExactKeys(
		witness.topology,
		[
			"consolidation",
			"initial",
			"parallel_current",
			"replacement",
			"replacement_divergent",
		].sort(),
		"topology witness topology",
	);
	const {
		initial,
		replacement,
		parallel_current: parallel,
		replacement_divergent: divergent,
		consolidation,
	} = witness.topology;
	assertExactKeys(initial, ["change", "current"], "topology initial");
	assertMatches(initial.change, CHANGE, "topology initial change");
	assertRevisionArtifact(initial.current, "topology initial current");
	assertExactKeys(
		replacement,
		["change", "current", "predecessor"],
		"topology replacement",
	);
	assertMatches(replacement.change, CHANGE, "topology replacement change");
	assertRevisionArtifact(replacement.current, "topology replacement current");
	assertRevisionArtifact(
		replacement.predecessor,
		"topology replacement predecessor",
	);
	for (const [name, value] of [
		["parallel", parallel],
		["divergent", divergent],
	]) {
		assertExactKeys(value, ["change", "current"], `topology ${name}`);
		assertMatches(value.change, CHANGE, `topology ${name} change`);
		if (!Array.isArray(value.current) || value.current.length !== 2)
			throw new Error(`topology ${name} current differs`);
		value.current.forEach((entry, index) =>
			assertRevisionArtifact(entry, `topology ${name} current ${index}`),
		);
	}
	assertExactKeys(
		consolidation,
		["change", "current", "predecessors"],
		"topology consolidation",
	);
	assertMatches(consolidation.change, CHANGE, "topology consolidation change");
	assertRevisionArtifact(
		consolidation.current,
		"topology consolidation current",
	);
	if (
		!Array.isArray(consolidation.predecessors) ||
		consolidation.predecessors.length !== 2
	)
		throw new Error("topology consolidation predecessors differ");
	consolidation.predecessors.forEach((entry, index) =>
		assertRevisionArtifact(
			entry,
			`topology consolidation predecessor ${index}`,
		),
	);

	if (witness.primary_revision !== initial.current.revision)
		throw new Error("topology primary revision relation differs");
	const shared = {
		revision: witness.shared_revision.revision,
		artifact: witness.shared_revision.artifact,
	};
	assertSameRevisionArtifact(
		witness.fact_port.origin,
		shared,
		"topology fact port shared artifact",
	);
	assertSameRevisionArtifact(
		replacement.current,
		shared,
		"topology replacement shared artifact",
	);
	assertSameRevisionArtifact(
		parallel.current[0],
		shared,
		"topology parallel shared artifact",
	);
	assertSameRevisionArtifact(
		divergent.current[0],
		shared,
		"topology divergent shared artifact",
	);
	assertSameRevisionArtifact(
		consolidation.predecessors[0],
		shared,
		"topology consolidation shared artifact",
	);
	assertSameRevisionArtifact(
		parallel.current[1],
		divergent.current[1],
		"topology divergent peer",
	);
	assertSameRevisionArtifact(
		parallel.current[1],
		consolidation.predecessors[1],
		"topology consolidation peer",
	);
	assertSameSet(
		witness.shared_revision.changes,
		[
			replacement.change,
			divergent.change,
			parallel.change,
			consolidation.change,
		],
		"topology shared changes",
	);
}

export function canonicalJson(value) {
	if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
	if (value !== null && typeof value === "object") {
		return `{${Object.keys(value)
			.sort()
			.map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`)
			.join(",")}}`;
	}
	return JSON.stringify(value);
}

export function sha256CanonicalJson(value) {
	return createHash("sha256").update(canonicalJson(value)).digest("hex");
}

export function validateTopologyCheckpointV1(checkpoint) {
	assertExactKeys(
		checkpoint,
		[
			"artifactRoles",
			"fixtureId",
			"relations",
			"schema",
			"storageForbiddenProbeHashes",
			"witness",
		].sort(),
		"topology checkpoint",
	);
	if (
		checkpoint.schema !== DERIVED_CHANGE_TOPOLOGY_FIXTURE_CHECKPOINT_SCHEMA_V1
	)
		throw new Error("topology checkpoint schema differs");
	if (checkpoint.fixtureId !== TOPOLOGY_FIXTURE_ID_V1)
		throw new Error("topology checkpoint fixture differs");
	assertExactKeys(
		checkpoint.witness,
		["schema", "topLevelKeys"],
		"topology checkpoint witness",
	);
	if (checkpoint.witness.schema !== WITNESS_SCHEMA_V1)
		throw new Error("topology checkpoint witness schema differs");
	if (
		!Array.isArray(checkpoint.witness.topLevelKeys) ||
		checkpoint.witness.topLevelKeys.length !== WITNESS_TOP_LEVEL_KEYS.length ||
		checkpoint.witness.topLevelKeys.some(
			(key, index) => key !== WITNESS_TOP_LEVEL_KEYS[index],
		)
	)
		throw new Error("topology checkpoint witness shape differs");
	assertExactKeys(
		checkpoint.storageForbiddenProbeHashes,
		Object.keys(STORAGE_FORBIDDEN_PROBE_HASHES_V1).sort(),
		"topology checkpoint storage forbidden probes",
	);
	for (const [name, expected] of Object.entries(
		STORAGE_FORBIDDEN_PROBE_HASHES_V1,
	)) {
		if (checkpoint.storageForbiddenProbeHashes[name] !== expected)
			throw new Error(
				`topology checkpoint storage forbidden probe differs: ${name}`,
			);
	}
	assertExactKeys(
		checkpoint.artifactRoles,
		[
			"consolidationCurrent",
			"initialCurrent",
			"missing",
			"peer",
			"replacementPredecessor",
			"shared",
		].sort(),
		"topology checkpoint artifact roles",
	);
	for (const [role, value] of Object.entries(checkpoint.artifactRoles))
		assertArtifact(value, `topology checkpoint artifact role ${role}`);
	assertExactKeys(
		checkpoint.relations,
		[
			"factPortOriginIsShared",
			"primaryRevisionIsInitialCurrent",
			"sharedChangeMembership",
			"sharedRevisionAcrossTopology",
			"sharedRevisionPeerConsistency",
		].sort(),
		"topology checkpoint relations",
	);
	for (const [name, value] of Object.entries(checkpoint.relations)) {
		if (value !== true)
			throw new Error(`topology checkpoint relation differs: ${name}`);
	}
	return checkpoint;
}

export function deriveTopologyCheckpointV1(witness) {
	validateTopologyWitnessV1(witness);
	const { topology } = witness;
	const checkpoint = {
		schema: DERIVED_CHANGE_TOPOLOGY_FIXTURE_CHECKPOINT_SCHEMA_V1,
		fixtureId: TOPOLOGY_FIXTURE_ID_V1,
		witness: {
			schema: WITNESS_SCHEMA_V1,
			topLevelKeys: WITNESS_TOP_LEVEL_KEYS,
		},
		storageForbiddenProbeHashes: STORAGE_FORBIDDEN_PROBE_HASHES_V1,
		artifactRoles: {
			initialCurrent: topology.initial.current.artifact,
			replacementPredecessor: topology.replacement.predecessor.artifact,
			shared: witness.shared_revision.artifact,
			peer: topology.parallel_current.current[1].artifact,
			consolidationCurrent: topology.consolidation.current.artifact,
			missing: witness.missing_artifact,
		},
		relations: {
			primaryRevisionIsInitialCurrent: true,
			factPortOriginIsShared: true,
			sharedRevisionAcrossTopology: true,
			sharedRevisionPeerConsistency: true,
			sharedChangeMembership: true,
		},
	};
	validateTopologyCheckpointV1(checkpoint);
	return { checkpoint, sha256: sha256CanonicalJson(checkpoint) };
}

export function topologyFixtureWitnessReportV1(witnessBytes) {
	const rawWitnessSha256 = createHash("sha256")
		.update(witnessBytes)
		.digest("hex");
	let witness;
	try {
		witness = JSON.parse(witnessBytes);
	} catch {
		throw new Error("topology fixture witness is not JSON");
	}
	const { checkpoint, sha256 } = deriveTopologyCheckpointV1(witness);
	return {
		schema: TOPOLOGY_CHECKPOINT_REPORT_SCHEMA_V1,
		fixtureId: witness.fixtureId,
		rawWitnessSha256,
		authoritativeInventorySha256: witness.authoritativeInventorySha256,
		topologyCheckpointSha256: sha256,
		topologyCheckpoint: checkpoint,
	};
}

function usage() {
	return "usage: derived-change-diagnostic-fixture.mjs --witness <path>";
}

async function main(args) {
	if (args.length !== 2 || args[0] !== "--witness" || !args[1])
		throw new Error(usage());
	const bytes = await readFile(args[1]);
	process.stdout.write(
		`${JSON.stringify(topologyFixtureWitnessReportV1(bytes))}\n`,
	);
}

if (import.meta.url === pathToFileURL(resolve(process.argv[1] ?? "")).href) {
	main(process.argv.slice(2)).catch((error) => {
		process.stderr.write(`error: ${error.message}\n`);
		process.exitCode = 1;
	});
}
