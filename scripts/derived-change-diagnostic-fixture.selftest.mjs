import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFile } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { promisify } from "node:util";

import {
	DERIVED_CHANGE_PUBLIC_FIXTURE_AUTHORITY_SCHEMA_V2,
	DERIVED_CHANGE_TOPOLOGY_FIXTURE_CHECKPOINT_SCHEMA_V1,
	deriveTopologyCheckpointV1,
	topologyFixtureWitnessReportV1,
} from "./derived-change-diagnostic-fixture.mjs";

const execFileAsync = promisify(execFile);
const digest = (digit) => digit.repeat(64);
const revision = (digit) => `rev:sha256:${digest(digit)}`;
const change = (digit) => `change:sha256:${digest(digit)}`;
const event = (digit) => `evt:sha256:${digest(digit)}`;
const observation = (digit) => `obs:sha256:${digest(digit)}`;
const factPort = (digit) => `fact-port:sha256:${digest(digit)}`;
const artifact = (digit) => `sha256:${digest(digit)}`;
const commit = (digit) => digit.repeat(40);

const storageForbiddenProbeHashes = {
	proposalSummarySha256:
		"21f749c5f166ae819a99a8ff0e303297a43685fd14cc7f1b86a90751989b167c",
	proseSha256:
		"da79cc8c9b04f41616275f4a6bd027acf6d0358f3605dac74ccadfeea92945a4",
	payloadDocumentSha256:
		"20dfd0d4e1ce81bfb753001a61c0394914d4711e84f90fb745a659dba1ff11bf",
};

const topologyWitness = ({ dynamic = "a", inventory = "b" } = {}) => {
	const primary = revision(dynamic);
	const root = revision("c");
	const topologyRoot = revision("2");
	const shared = revision("d");
	const peer = revision("e");
	const consolidated = revision("f");
	const initialChange = change("1");
	const replacementChange = change("2");
	const divergentChange = change("3");
	const parallelChange = change("4");
	const consolidationChange = change("5");
	return {
		schema: "pointbreak.qualification-derived-change-fixture-witness.v1",
		fixtureId: "topology-v1",
		authoritativeInventorySha256: digest(inventory),
		storageForbiddenProbeHashes,
		primary_revision: primary,
		fact_port: {
			port_id: factPort(dynamic),
			event_id: event(dynamic),
			origin: {
				revision: shared,
				artifact: artifact("6"),
				observation: observation(dynamic),
			},
		},
		live_revision: revision("7"),
		unassessed_revision: revision("8"),
		superseded_revision: revision("9"),
		ambiguous_assessment_revision: revision("a"),
		competing_revision: revision("b"),
		range_revision: revision("c"),
		root_revision: root,
		staged_revision: revision("d"),
		unstaged_revision: revision("e"),
		detached_revision: revision("f"),
		missing_change: change("6"),
		missing_revision: revision("1"),
		missing_artifact: artifact("7"),
		topology: {
			initial: {
				change: initialChange,
				current: { revision: primary, artifact: artifact("1") },
			},
			replacement: {
				change: replacementChange,
				current: { revision: shared, artifact: artifact("6") },
				predecessor: { revision: topologyRoot, artifact: artifact("2") },
			},
			parallel_current: {
				change: parallelChange,
				current: [
					{ revision: shared, artifact: artifact("6") },
					{ revision: peer, artifact: artifact("3") },
				],
			},
			replacement_divergent: {
				change: divergentChange,
				current: [
					{ revision: shared, artifact: artifact("6") },
					{ revision: peer, artifact: artifact("3") },
				],
			},
			consolidation: {
				change: consolidationChange,
				current: { revision: consolidated, artifact: artifact("4") },
				predecessors: [
					{ revision: shared, artifact: artifact("6") },
					{ revision: peer, artifact: artifact("3") },
				],
			},
		},
		shared_revision: {
			revision: shared,
			artifact: artifact("6"),
			changes: [
				replacementChange,
				divergentChange,
				parallelChange,
				consolidationChange,
			],
		},
		base_commit: commit(dynamic),
		first_landing: commit("1"),
		second_landing: commit("2"),
		live_landing: commit("3"),
	};
};

test("topology checkpoint keeps only public stable topology authority", () => {
	const first = deriveTopologyCheckpointV1(topologyWitness());
	const second = deriveTopologyCheckpointV1(
		topologyWitness({ dynamic: "f", inventory: "e" }),
	);

	assert.equal(
		DERIVED_CHANGE_PUBLIC_FIXTURE_AUTHORITY_SCHEMA_V2,
		"pointbreak.derived-change-public-fixture-authority.v2",
	);
	assert.equal(
		DERIVED_CHANGE_TOPOLOGY_FIXTURE_CHECKPOINT_SCHEMA_V1,
		"pointbreak.derived-change-topology-fixture-checkpoint.v1",
	);
	assert.equal(
		first.checkpoint.schema,
		DERIVED_CHANGE_TOPOLOGY_FIXTURE_CHECKPOINT_SCHEMA_V1,
	);
	assert.deepEqual(first.checkpoint, second.checkpoint);
	assert.equal(first.sha256, second.sha256);
	assert.equal(first.checkpoint.artifactRoles.initialCurrent, artifact("1"));
	assert.equal(first.checkpoint.artifactRoles.shared, artifact("6"));
	assert.notEqual(
		topologyWitness().root_revision,
		topologyWitness().topology.replacement.predecessor.revision,
	);
	assert.deepEqual(
		first.checkpoint.storageForbiddenProbeHashes,
		storageForbiddenProbeHashes,
	);
	assert.equal(first.checkpoint.relations.sharedRevisionAcrossTopology, true);
	assert.equal(first.checkpoint.relations.sharedChangeMembership, true);
	assert.doesNotMatch(
		JSON.stringify(first.checkpoint),
		/rev:sha256|change:sha256|evt:sha256|fact-port:sha256/,
	);
	assert.doesNotMatch(
		JSON.stringify(first.checkpoint),
		new RegExp(digest("b")),
	);
});

test("topology checkpoint binds changed stable artifact roles and rejects malformed witness shape", () => {
	const changedArtifact = topologyWitness();
	changedArtifact.topology.replacement.current.artifact = artifact("f");
	changedArtifact.fact_port.origin.artifact = artifact("f");
	changedArtifact.shared_revision.artifact = artifact("f");
	changedArtifact.topology.parallel_current.current[0].artifact = artifact("f");
	changedArtifact.topology.replacement_divergent.current[0].artifact =
		artifact("f");
	changedArtifact.topology.consolidation.predecessors[0].artifact =
		artifact("f");
	assert.notEqual(
		deriveTopologyCheckpointV1(changedArtifact).sha256,
		deriveTopologyCheckpointV1(topologyWitness()).sha256,
	);
	const brokenSharedRelation = topologyWitness();
	brokenSharedRelation.topology.parallel_current.current[0].revision =
		revision("0");
	assert.throws(
		() => deriveTopologyCheckpointV1(brokenSharedRelation),
		/parallel shared artifact relation differs/,
	);
	const unexpected = topologyWitness();
	unexpected.extra = true;
	assert.throws(
		() => deriveTopologyCheckpointV1(unexpected),
		/top-level shape differs/,
	);
});

test("fixture checkpoint CLI reports raw retained identity beside normalized authority", async () => {
	const root = await mkdtemp(join(tmpdir(), "pointbreak-topology-checkpoint-"));
	const witnessPath = join(root, "witness.json");
	const bytes = `${JSON.stringify(topologyWitness())}\n`;
	await writeFile(witnessPath, bytes);
	try {
		const { stdout } = await execFileAsync(process.execPath, [
			new URL("./derived-change-diagnostic-fixture.mjs", import.meta.url)
				.pathname,
			"--witness",
			witnessPath,
		]);
		const result = JSON.parse(stdout);
		assert.equal(result.fixtureId, "topology-v1");
		assert.equal(
			result.rawWitnessSha256,
			createHash("sha256").update(bytes).digest("hex"),
		);
		assert.equal(result.authoritativeInventorySha256, digest("b"));
		assert.equal(
			result.topologyCheckpointSha256,
			deriveTopologyCheckpointV1(topologyWitness()).sha256,
		);
	} finally {
		await rm(root, { recursive: true, force: true });
	}
});
