import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import {
	chmod,
	appendFile,
	lstat,
	mkdir,
	mkdtemp,
	readFile,
	symlink,
	writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
	DERIVED_CHANGE_CHANGE_READ_DIAGNOSTIC_CONFIG_SCHEMA_V1,
	derivedChangeChangeReadChildDescriptors,
	runDerivedChangeChangeReadDiagnostic,
	validateDerivedChangeChangeReadDiagnosticConfig,
} from "./derived-change-diagnostic-change-read.mjs";
import {
	DERIVED_CHANGE_PUBLIC_FIXTURE_AUTHORITY_SCHEMA_V2,
	DERIVED_CHANGE_TOPOLOGY_FIXTURE_CHECKPOINT_SCHEMA_V1,
	deriveTopologyCheckpointV1,
} from "./derived-change-diagnostic-fixture.mjs";
import {
	DERIVED_CHANGE_DIAGNOSTIC_FRAGMENT_SCHEMA_V1,
	DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1,
	finalizeDerivedChangeDiagnosticFragment,
	mergeDerivedChangeDiagnosticReport,
} from "./derived-change-diagnostic-report.mjs";

const digest = (digit) => digit.repeat(64);
const commit = (digit) => digit.repeat(40);
const revision = (digit) => `rev:sha256:${digest(digit)}`;
const change = (digit) => `change:sha256:${digest(digit)}`;
const event = (digit) => `evt:sha256:${digest(digit)}`;
const observation = (digit) => `obs:sha256:${digest(digit)}`;
const factPort = (digit) => `fact-port:sha256:${digest(digit)}`;
const artifact = (digit) => `sha256:${digest(digit)}`;
const sha256 = async (path) =>
	createHash("sha256")
		.update(await readFile(path))
		.digest("hex");
const readCases = [
	"profile",
	"changes_bare",
	"changes_bounded",
	"attention_bare",
	"attention_bounded",
	"bodyless_filter_suite",
	"summary_query",
	"summary_filter_suite",
	"page_token_suite",
	"concurrent_readers",
	"fresh_process_suite",
	"warm_reuse_suite",
	"stale_page_token",
	"post_append_suite",
	"post_append_fresh_process_suite",
];
const controls = [
	"l0_no_generation",
	"m1_preactivation_invalidation",
	"l2_current_profile",
	"authority_failure_axes",
	"incompatible_reader",
	"absent_v3",
	"stale_v3",
	"corrupt_v3",
	"checkpoint_authority_mismatch",
	"checkpoint_stamp_mismatch",
	"checkpoint_anchor_mismatch",
	"interrupted_publication",
	"interrupted_catch_up_resume",
	"catch_up_maintenance_attribution",
	"current_read_no_maintenance",
	"moving_checkpoint",
	"moving_publication",
	"generation_lease_overlap",
	"n_plus_one_publication",
	"generation_reclamation",
	"direct_ready_call_graph_refusal",
	"automatic_error_call_graph_refusal",
	"capability_classifier_authority_only",
	"explicit_off_isolation",
	"explicit_off_strict_reader",
	"concurrent_writers_and_readers",
	"busy_writer_nonblocking",
];
const activationRecord =
	"5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json";

const storageForbiddenProbeHashes = {
	proposalSummarySha256:
		"21f749c5f166ae819a99a8ff0e303297a43685fd14cc7f1b86a90751989b167c",
	proseSha256:
		"da79cc8c9b04f41616275f4a6bd027acf6d0358f3605dac74ccadfeea92945a4",
	payloadDocumentSha256:
		"20dfd0d4e1ce81bfb753001a61c0394914d4711e84f90fb745a659dba1ff11bf",
};
const ordinaryStorageForbiddenProbeHashes = {
	proposalSummarySha256:
		"c28dcb78bb4ccee57a2c6af8c1496b9fc8a14dd4860404907cc8607077ef4fc7",
	proseSha256:
		"50598e3fd911558ba8a903c07689d5128156d63db94dbcce8deda237e8bc73aa",
};

const topologyWitness = ({ dynamic = "a", inventory = "f" } = {}) => {
	const primary = revision(dynamic);
	const root = revision("c");
	const shared = revision("d");
	const peer = revision("e");
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
				change: change("1"),
				current: { revision: primary, artifact: artifact("1") },
			},
			replacement: {
				change: replacementChange,
				current: { revision: shared, artifact: artifact("6") },
				predecessor: { revision: root, artifact: artifact("2") },
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
				current: { revision: revision("f"), artifact: artifact("4") },
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

async function fixture({ topologyActual = topologyWitness() } = {}) {
	const root = await mkdtemp(
		join(tmpdir(), "pointbreak-change-read-diagnostic-"),
	);
	const source = join(root, "source");
	const caseRoot = join(root, "case");
	const workRoot = await mkdtemp(join(tmpdir(), "pbcw-"));
	const readyStore = join(
		source,
		"tests",
		"support",
		"assets",
		"change-ready-store",
	);
	const fake = join(root, "fake-harness.mjs");
	const product = join(root, "product.mjs");
	const library = join(root, "library-control.mjs");
	const cli = join(root, "cli-control.mjs");
	const materializer = join(
		source,
		"scripts",
		"materialize-inspector-decision-matrix.sh",
	);
	const fixtureModule = join(
		source,
		"scripts",
		"derived-change-diagnostic-fixture.mjs",
	);
	const authority = join(root, "fixture-authority.json");
	await mkdir(join(source, "src", "bench_support", "derived_access"), {
		recursive: true,
	});
	await mkdir(readyStore, { recursive: true });
	await mkdir(join(source, "scripts"), { recursive: true });
	await writeFile(
		fake,
		`#!${process.execPath}
import { appendFile, readFile, mkdir } from "node:fs/promises";
import { existsSync } from "node:fs";
import { createHash } from "node:crypto";
import { join } from "node:path";
if(!process.env.POINTBREAK_GIT_PROGRAM) throw new Error("exact Git program is required");
const requestArg=process.argv.find((value)=>value.startsWith("--derived-access-request="));
const request=JSON.parse(await readFile(requestArg.slice(requestArg.indexOf("=")+1),"utf8"));
const ids={duplicate_equal:"duplicate-equal-v1",duplicate_conflicting:"duplicate-conflict-v1",operative_removal:"removal-v1",missing_selected_carrier:"missing-carrier-v1",mutated_selected_carrier:"mutated-carrier-v1",wrong_family_selected_carrier:"wrong-family-carrier-v1",incomplete_change:"incomplete-v1",cycle_conflicted_change:"cycle-conflicted-v1"};
const witness=(fixtureId)=>JSON.stringify({schema:"pointbreak.qualification-derived-change-fixture-witness.v1",fixtureId,authoritativeInventorySha256:"${digest("f")}",storageForbiddenProbeHashes:{proposalSummarySha256:"${digest("1")}",proseSha256:"${digest("2")}",payloadDocumentSha256:"${digest("3")}"}});
if(process.argv.includes("--derived-change-fixture-materialize")){if(request.kind === "mutated_selected_carrier" && process.argv.includes("--test-fail-mutated")){process.stderr.write("fixture fail");process.exit(7)}await mkdir(request.root,{recursive:true});console.log(witness(ids[request.kind]) + (request.kind === "mutated_selected_carrier" && process.argv.includes("--test-witness-mismatch") ? " " : ""));process.exit(0)}
const fixture=request.readRequest.fixture; const typed=new Set(["duplicate-conflict-v1","missing-carrier-v1","mutated-carrier-v1","wrong-family-carrier-v1"]); const sourceFailure=process.argv.includes("--test-source-preflight-failure"); const caseStatus=sourceFailure?{status:"skipped",failureDetail:"synthetic source preflight failure"}:{status:"passed"}; const rows=(fixture === "topology-v1" ? ${JSON.stringify(readCases)} : typed.has(fixture) ? ${JSON.stringify(["profile", "changes_bare", "changes_bounded", "attention_bare", "attention_bounded", "summary_query"])} : ${JSON.stringify(readCases.slice(0, 8))}).map((caseName)=>({case:caseName,...caseStatus}));
const commandSha256=createHash("sha256").update(JSON.stringify([process.argv[1],...process.argv.slice(2)])).digest("hex"); if(request.readRequest.execution.commandSha256 !== commandSha256) throw new Error("per-fixture command identity differs");
const capture=process.argv.find((value)=>value.startsWith("--test-capture=")); if(capture){const activation=await readFile(join(request.readRequest.sourceCheckout,"tests","support","assets","change-ready-store",${JSON.stringify(activationRecord)}),"utf8"); const probes=request.readRequest.storageForbiddenProbes; const expected=fixture === "topology-v1" ? ["Decision continuity matrix","The matrix keeps evidence classes distinct."] : ["qualification storage summary sentinel v1","qualification storage prose sentinel v1"]; if(probes.proposalSummary !== expected[0] || probes.prose !== expected[1]) throw new Error("fixture probes must match public materializer authority"); if(probes.payloadDocument !== activation) throw new Error("payload probe must be authoritative activation bytes"); for(const key of ["HOME","TMP","TEMP"]){if(!existsSync(process.env[key])) throw new Error(key+" must name an existing isolated root")} await appendFile(capture.slice(capture.indexOf("=")+1),JSON.stringify({fixture,platform:request.readRequest.execution.platform,probes,environment:{HOME:process.env.HOME,TMP:process.env.TMP,TEMP:process.env.TEMP}})+"\\n")}
if(process.argv.includes("--test-invalid-output")){console.log("{}"),process.exit(0)}
const topology=fixture === "topology-v1"; const preflight=(topology?["source","fixture","library_control","cli_control","template_postflight"]:["source","fixture","template_postflight"]).map((kind)=>sourceFailure?{kind,status:kind === "source"?"failed":"skipped",failureDetail:"synthetic source preflight failure"}:{kind,status:"passed"});
for(const field of ["controlTestBinary","controlTestBinarySha256","controlTestBuildCommandSha256","controlCliTestBinary","controlCliTestBinarySha256","controlCliTestBuildCommandSha256"]){if(Object.hasOwn(request.readRequest,field) !== topology) throw new Error("control identity presence differs from fixture authority")}
if(process.argv.includes("--test-read-nonzero")){process.stderr.write("synthetic read failure");process.exit(9)}
console.log(JSON.stringify({mode:"--derived-change-read-diagnostic",sourceUnchanged:!process.argv.includes("--test-template-mutation"),preflight,rows,controls:topology?${JSON.stringify(controls)}.map((caseName)=>({case:caseName,...caseStatus})):[],storage:[{case:"initial",...caseStatus},...(topology?[{case:"post_append",...caseStatus}]:[])]}));
`,
	);
	const topologyWitnessBytes = JSON.stringify(topologyActual);
	await writeFile(
		materializer,
		`#!/bin/sh\ncase "$POINTBREAK_CYGPATH_PROGRAM" in absent|/*) ;; *) exit 18 ;; esac\nmkdir -p "$1"\nprintf '%s\\n' '${topologyWitnessBytes}'\n`,
	);
	await writeFile(
		fixtureModule,
		await readFile(
			new URL("./derived-change-diagnostic-fixture.mjs", import.meta.url),
		),
	);
	await writeFile(
		join(source, "src", "bench_support", "derived_access", "materializer.rs"),
		"public materializer\n",
	);
	await writeFile(
		join(
			readyStore,
			"5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json",
		),
		"activation\n",
	);
	await writeFile(
		join(
			readyStore,
			"f31956c2b820926adc74d4d03cb03820d13c9ed2739b5f7ada81611a6f8bcff1.json",
		),
		"completion\n",
	);
	await writeFile(product, `${await readFile(fake, "utf8")}\n// product\n`);
	await writeFile(library, `${await readFile(fake, "utf8")}\n// library\n`);
	await writeFile(cli, `${await readFile(fake, "utf8")}\n// cli\n`);
	await chmod(fake, 0o755);
	await chmod(product, 0o755);
	await chmod(library, 0o755);
	await chmod(cli, 0o755);
	await chmod(materializer, 0o755);
	const shell = "/bin/sh";
	const program = async (path) => ({
		program: path,
		binarySha256: await sha256(path),
	});
	const shellProgram = await program(shell);
	const fakeProgram = await program(fake);
	const activationSha256 = await sha256(
		join(
			readyStore,
			"5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json",
		),
	);
	const completionSha256 = await sha256(
		join(
			readyStore,
			"f31956c2b820926adc74d4d03cb03820d13c9ed2739b5f7ada81611a6f8bcff1.json",
		),
	);
	const fixtureIds = [
		"cycle-conflicted-v1",
		"duplicate-conflict-v1",
		"duplicate-equal-v1",
		"incomplete-v1",
		"missing-carrier-v1",
		"mutated-carrier-v1",
		"removal-v1",
		"wrong-family-carrier-v1",
	];
	const witnessBytes = (fixtureId) =>
		`${JSON.stringify({ schema: "pointbreak.qualification-derived-change-fixture-witness.v1", fixtureId, authoritativeInventorySha256: digest("f"), storageForbiddenProbeHashes: { proposalSummarySha256: digest("1"), proseSha256: digest("2"), payloadDocumentSha256: digest("3") } })}\n`;
	const topologyCheckpointSha256 = deriveTopologyCheckpointV1(
		topologyWitness(),
	).sha256;
	const authorityDocument = {
		schema: DERIVED_CHANGE_PUBLIC_FIXTURE_AUTHORITY_SCHEMA_V2,
		sourceCommit: commit("1"),
		sourceTree: commit("2"),
		sourceFiles: [
			{
				path: "scripts/derived-change-diagnostic-fixture.mjs",
				sha256: await sha256(fixtureModule),
			},
			{
				path: "scripts/materialize-inspector-decision-matrix.sh",
				sha256: await sha256(materializer),
			},
			{
				path: "src/bench_support/derived_access/materializer.rs",
				sha256: await sha256(
					join(
						source,
						"src",
						"bench_support",
						"derived_access",
						"materializer.rs",
					),
				),
			},
			{
				path: "tests/support/assets/change-ready-store/5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json",
				sha256: activationSha256,
			},
			{
				path: "tests/support/assets/change-ready-store/f31956c2b820926adc74d4d03cb03820d13c9ed2739b5f7ada81611a6f8bcff1.json",
				sha256: completionSha256,
			},
		],
		witnesses: fixtureIds.map((fixtureId) => ({
			fixtureId,
			authoritativeInventorySha256: digest("f"),
			witnessSha256: createHash("sha256")
				.update(witnessBytes(fixtureId))
				.digest("hex"),
		})),
		topologyCheckpoint: {
			schema: DERIVED_CHANGE_TOPOLOGY_FIXTURE_CHECKPOINT_SCHEMA_V1,
			fixtureId: "topology-v1",
			checkpointSha256: topologyCheckpointSha256,
		},
	};
	await writeFile(authority, `${JSON.stringify(authorityDocument)}\n`);
	const authoritySha256 = await sha256(authority);
	return {
		root,
		caseRoot,
		workRoot,
		config: {
			schema: DERIVED_CHANGE_CHANGE_READ_DIAGNOSTIC_CONFIG_SCHEMA_V1,
			campaignId: "diagnostic-test",
			rootAuthoritySha256: authoritySha256,
			caseRoot,
			workRoot,
			sourceCheckout: source,
			execution: {
				platform: "macos_apfs",
				sourceCommit: commit("1"),
				sourceTree: commit("2"),
				cargoLockSha256: digest("3"),
				binarySha256: fakeProgram.binarySha256,
				contractSchema: "pointbreak.qualification-derived-access-contract.v1",
				contractSha256: digest("4"),
				rootProvenanceSha256: authoritySha256,
				commandSha256: digest("5"),
				operatingSystem: "macos",
				architecture: "aarch64",
				filesystem: "apfs",
				hostIdentitySha256: digest("6"),
				sourceDirty: false,
				privateCorpusConfigured: false,
			},
			product: {
				...(await program(product)),
				platform: "macos_apfs",
				sourceCommit: commit("1"),
				sourceTree: commit("2"),
				cargoLockSha256: digest("3"),
				versionSha256: digest("7"),
				buildProfile: "release",
				enabledFeatures: ["longitudinal-counting"],
				buildCommandSha256: digest("8"),
				operatingSystem: "macos",
				architecture: "aarch64",
				sourceDirty: false,
			},
			harness: { ...fakeProgram, argsPrefix: [] },
			controls: {
				library: {
					...(await program(library)),
					buildCommandSha256: digest("9"),
				},
				cli: { ...(await program(cli)), buildCommandSha256: digest("b") },
			},
			fixtureAuthority: {
				path: authority,
				sha256: authoritySha256,
				readyStore,
				activationSha256,
				completionSha256,
			},
			programs: {
				bash: shellProgram,
				topologyMaterializer: await program(materializer),
				git: shellProgram,
				jq: shellProgram,
				find: shellProgram,
				sort: shellProgram,
				wc: shellProgram,
				tr: shellProgram,
				awk: shellProgram,
				cp: shellProgram,
				head: shellProgram,
				dirname: shellProgram,
				mkdir: shellProgram,
				rm: shellProgram,
				hash: { ...shellProgram, mode: "shasum" },
			},
			summaryQuery: "matrix",
		},
	};
}

test("collects exact named inventories from all nine public fixtures", async () => {
	const input = await fixture();
	const result = await runDerivedChangeChangeReadDiagnostic(input.config);
	assert.equal(
		result.cases.filter(({ id }) => id.includes(".read.")).length,
		71,
	);
	assert.equal(
		result.cases.filter(({ id }) => id.includes(".control.")).length,
		27,
	);
	assert.equal(
		result.cases.filter(({ id }) => id.includes(".storage.")).length,
		10,
	);
	assert.equal(
		result.cases.filter(({ id }) => id.includes(".preflight.")).length,
		29,
	);
	assert.equal(
		result.cases.find(({ id }) => id === "topology-v1.template").status,
		"passed",
	);
	assert.ok(
		result.cases.some(
			({ id }) => id === "topology-v1.read.post_append_fresh_process_suite",
		),
	);
	assert.ok(
		result.cases.some(
			({ id, dependsOn }) =>
				id === "topology-v1.control.direct_ready_call_graph_refusal" &&
				dependsOn.includes("topology-v1.preflight.cli_control"),
		),
	);
	assert.equal(
		result.schema,
		"pointbreak.derived-change-diagnostic-collection.v1",
	);
	assert.equal(
		result.cases.some(({ id }) => id.includes("receipt")),
		false,
	);
	assert.equal(
		(
			await lstat(join(input.workRoot, "templates", "topology-v1"))
		).isDirectory(),
		true,
	);
	await assert.rejects(
		() => lstat(join(input.caseRoot, "templates", "topology-v1")),
		{ code: "ENOENT" },
	);
	for (const retainedPath of result.artifactPaths)
		assert.equal(
			(await lstat(join(input.caseRoot, retainedPath))).isFile(),
			true,
		);
});

test("binds each read request to retained witness file bytes, not its path spelling", async () => {
	const input = await fixture();
	input.config.caseRoot = join(input.root, "C:\\pb17\\witness-root");
	const result = await runDerivedChangeChangeReadDiagnostic(input.config);
	assert.equal(
		result.cases.find(({ id }) => id === "duplicate-equal-v1.template").status,
		"passed",
	);
	const witness = join(
		input.config.caseRoot,
		"witnesses",
		"duplicate-equal-v1.json",
	);
	const request = JSON.parse(
		await readFile(
			join(input.config.caseRoot, "requests", "duplicate-equal-v1.read.json"),
			"utf8",
		),
	);
	const retainedWitnessSha256 = await sha256(witness);
	assert.equal(
		request.readRequest.fixtureWitnessSha256,
		retainedWitnessSha256,
	);
	assert.notEqual(
		request.readRequest.fixtureWitnessSha256,
		createHash("sha256").update(witness).digest("hex"),
	);
});

test("uses authority-bound activation bytes and existing isolated fixture roots on Windows", async () => {
	const input = await fixture();
	const capture = join(input.root, "read-request-capture.jsonl");
	input.config.harness.argsPrefix = [`--test-capture=${capture}`];
	input.config.execution.platform = "windows_ntfs";
	input.config.execution.operatingSystem = "windows";
	input.config.execution.filesystem = "ntfs";
	input.config.product.platform = "windows_ntfs";
	input.config.product.operatingSystem = "windows";

	const result = await runDerivedChangeChangeReadDiagnostic(input.config);
	assert.equal(
		result.cases.find(({ id }) => id === "topology-v1.preflight.fixture").status,
		"passed",
	);
	assert.equal(
		result.cases.find(({ id }) => id === "duplicate-equal-v1.preflight.fixture")
			.status,
		"passed",
	);
	const requests = (await readFile(capture, "utf8"))
		.trim()
		.split("\n")
		.map((line) => JSON.parse(line));
	assert.equal(requests.length, 9);
	assert.deepEqual(
		new Set(requests.map(({ fixture }) => fixture)),
		new Set([
			"topology-v1",
			"duplicate-equal-v1",
			"duplicate-conflict-v1",
			"removal-v1",
			"missing-carrier-v1",
			"mutated-carrier-v1",
			"wrong-family-carrier-v1",
			"incomplete-v1",
			"cycle-conflicted-v1",
		]),
	);
	for (const { environment, probes } of requests) {
		const expected =
			probes.proposalSummary === "Decision continuity matrix"
				? storageForbiddenProbeHashes
				: ordinaryStorageForbiddenProbeHashes;
		assert.equal(environment.HOME, environment.TMP);
		assert.equal(environment.HOME, environment.TEMP);
		assert.match(environment.HOME, /environments/);
		assert.equal(
			createHash("sha256").update(probes.payloadDocument).digest("hex"),
			input.config.fixtureAuthority.activationSha256,
		);
		assert.equal(
			createHash("sha256").update(probes.proposalSummary).digest("hex"),
			expected.proposalSummarySha256,
		);
		assert.equal(
			createHash("sha256").update(probes.prose).digest("hex"),
			expected.proseSha256,
		);
	}
});

test("rejects a same-byte symlinked jq binding as a global identity failure", async () => {
	const input = await fixture();
	const jqLink = join(input.root, "jq-link");
	await symlink(input.config.programs.jq.program, jqLink);
	input.config.programs.jq.program = jqLink;
	const result = await runDerivedChangeChangeReadDiagnostic(input.config);
	const preflight = result.cases.find(
		({ id }) => id === "change-read.global-preflight",
	);
	assert.equal(preflight.status, "failed");
	assert.equal(preflight.failureClass, "global_invalid");
	assert.ok(
		result.cases
			.filter(({ id }) => id !== preflight.id)
			.every(({ status }) => status === "skipped"),
	);
	assert.equal(
		result.cases.length,
		derivedChangeChangeReadChildDescriptors().length,
	);
});

test("records one failed setup and continues independent public fixtures", async () => {
	const input = await fixture();
	input.config.harness.argsPrefix = ["--test-fail-mutated"];
	const result = await runDerivedChangeChangeReadDiagnostic(input.config);
	assert.equal(
		result.cases.find(({ id }) => id === "mutated-carrier-v1.template").status,
		"failed",
	);
	assert.equal(
		result.cases.find(({ id }) => id === "mutated-carrier-v1.preflight.source")
			.status,
		"skipped",
	);
	assert.equal(
		result.cases.find(({ id }) => id === "mutated-carrier-v1.read.profile")
			.status,
		"skipped",
	);
	assert.equal(
		result.cases.find(({ id }) => id === "cycle-conflicted-v1.read.profile")
			.status,
		"passed",
	);
	assert.deepEqual(
		result.cases.find(({ id }) => id === "change-read.identity-postflight")
			.dependsOn,
		[],
	);
});

test("remains a diagnostic-only adapter", async () => {
	const source = await readFile(
		new URL("./derived-change-diagnostic-change-read.mjs", import.meta.url),
		"utf8",
	);
	assert.match(source, /--derived-change-read-diagnostic/);
	assert.match(source, /POINTBREAK_HASH_PROGRAM_MODE/);
	assert.match(source, /POINTBREAK_DIAGNOSTIC_WORK_ROOT/);
	assert.match(source, /HOME:\s*root/);
	assert.match(source, /USERPROFILE:\s*root/);
	assert.doesNotMatch(source, /qualification-derived-change-read-receipt/);
	assert.doesNotMatch(source, /derived-access-package/);
});

test("root provenance is the exact public fixture authority", async () => {
	const input = await fixture();
	input.config.execution.rootProvenanceSha256 = digest("0");
	assert.throws(
		() => validateDerivedChangeChangeReadDiagnosticConfig(input.config),
		/root provenance differs from fixture authority/,
	);
});

test("serializes initial authority drift as a complete global-invalid collection", async () => {
	const input = await fixture();
	await writeFile(input.config.fixtureAuthority.path, "{}\n");
	const result = await runDerivedChangeChangeReadDiagnostic(input.config);
	const preflight = result.cases.find(
		({ id }) => id === "change-read.global-preflight",
	);
	assert.equal(preflight.status, "failed");
	assert.equal(preflight.failureClass, "global_invalid");
	for (const id of [
		"topology-v1.template",
		"change-read.identity-postflight",
	]) {
		const row = result.cases.find((candidate) => candidate.id === id);
		assert.equal(row.status, "skipped");
		assert.deepEqual(row.dependsOn, [preflight.id]);
	}
	assert.equal(
		result.cases.length,
		derivedChangeChangeReadChildDescriptors().length,
	);
});

test("work roots must be disjoint from retained and source roots", async () => {
	const input = await fixture();
	input.config.workRoot = input.caseRoot;
	assert.throws(
		() => validateDerivedChangeChangeReadDiagnosticConfig(input.config),
		/work root must be disjoint/,
	);
});

test("passes an exact optional cygpath binding to topology materialization", async () => {
	const input = await fixture();
	input.config.programs.cygpath = structuredClone(input.config.programs.hash);
	const result = await runDerivedChangeChangeReadDiagnostic(input.config);
	assert.equal(
		result.cases.find(({ id }) => id === "topology-v1.template").status,
		"passed",
	);
});

test("accepts dynamic topology witness bytes only through the frozen normalized checkpoint", async () => {
	const input = await fixture({
		topologyActual: topologyWitness({ dynamic: "f", inventory: "e" }),
	});
	const result = await runDerivedChangeChangeReadDiagnostic(input.config);
	assert.equal(
		result.cases.find(({ id }) => id === "topology-v1.template").status,
		"passed",
	);
});

test("rejects a topology witness whose stable normalized checkpoint differs", async () => {
	const changed = topologyWitness();
	for (const entry of [
		changed.fact_port.origin,
		changed.shared_revision,
		changed.topology.replacement.current,
		changed.topology.parallel_current.current[0],
		changed.topology.replacement_divergent.current[0],
		changed.topology.consolidation.predecessors[0],
	])
		entry.artifact = artifact("f");
	const input = await fixture({ topologyActual: changed });
	const result = await runDerivedChangeChangeReadDiagnostic(input.config);
	const setup = result.cases.find(({ id }) => id === "topology-v1.template");
	assert.equal(setup.status, "failed");
	assert.equal(setup.failureClass, "global_invalid");
	assert.equal(setup.phase, "fixture-witness");
});

test("exports a deterministic complete child inventory", () => {
	const first = derivedChangeChangeReadChildDescriptors();
	assert.deepEqual(first, derivedChangeChangeReadChildDescriptors());
	assert.equal(first.filter(({ id }) => id.includes(".read.")).length, 71);
	assert.ok(first.some(({ id }) => id === "change-read.identity-postflight"));
});

test("keeps complete skipped inventory after a global witness mismatch", async () => {
	const input = await fixture();
	input.config.harness.argsPrefix = ["--test-witness-mismatch"];
	const result = await runDerivedChangeChangeReadDiagnostic(input.config);
	const setup = result.cases.find(
		({ id }) => id === "mutated-carrier-v1.template",
	);
	assert.equal(setup.status, "failed");
	assert.equal(setup.failureClass, "global_invalid");
	assert.equal(
		result.cases.find(({ id }) => id === "mutated-carrier-v1.preflight.source")
			.status,
		"skipped",
	);
	assert.equal(
		result.cases.find(({ id }) => id === "cycle-conflicted-v1.read.profile")
			.status,
		"skipped",
	);
	assert.ok(
		result.cases
			.find(({ id }) => id === "cycle-conflicted-v1.template")
			.dependsOn.includes(setup.id),
	);
});

test("converts a false sourceUnchanged output into template-postflight failure", async () => {
	const input = await fixture();
	input.config.harness.argsPrefix = ["--test-template-mutation"];
	const result = await runDerivedChangeChangeReadDiagnostic(input.config);
	assert.equal(
		result.cases.find(
			({ id }) => id === "topology-v1.preflight.template_postflight",
		).status,
		"failed",
	);
	assert.equal(
		result.cases.find(
			({ id }) => id === "topology-v1.preflight.template_postflight",
		).failureClass,
		"lane_invalid",
	);
	assert.equal(
		result.cases.find(({ id }) => id === "topology-v1.read.profile").status,
		"passed",
	);
	for (const artifact of result.artifactPaths)
		assert.equal(
			(
				await (
					await import("node:fs/promises")
				).lstat(join(input.caseRoot, artifact))
			).isDirectory(),
			false,
		);
});

test("keeps every preflight child when a completed harness output is invalid", async () => {
	const input = await fixture();
	input.config.harness.argsPrefix = ["--test-invalid-output"];
	const result = await runDerivedChangeChangeReadDiagnostic(input.config);
	for (const id of [
		"topology-v1.preflight.source",
		"topology-v1.preflight.fixture",
		"topology-v1.preflight.library_control",
		"topology-v1.preflight.cli_control",
		"topology-v1.preflight.template_postflight",
	])
		assert.equal(result.cases.find((row) => row.id === id).status, "skipped");
	const setup = result.cases.find((row) => row.id === "topology-v1.template");
	assert.equal(setup.status, "failed");
	assert.equal(setup.failureClass, "lane_invalid");
	assert.equal(setup.phase, "diagnostic-output");
	assert.deepEqual(
		result.cases.find((row) => row.id === "topology-v1.preflight.source")
			.dependsOn,
		[setup.id],
	);
});

test("a nonzero read harness preserves every row behind a failed setup authority", async () => {
	const input = await fixture();
	input.config.harness.argsPrefix = ["--test-read-nonzero"];
	const result = await runDerivedChangeChangeReadDiagnostic(input.config);
	const setup = result.cases.find((row) => row.id === "topology-v1.template");
	assert.equal(setup.status, "failed");
	assert.equal(setup.failureClass, "lane_invalid");
	assert.equal(setup.phase, "diagnostic-output");
	assert.equal(
		result.cases.find((row) => row.id === "topology-v1.read.profile").status,
		"skipped",
	);
	assert.equal(
		result.cases.length,
		derivedChangeChangeReadChildDescriptors().length,
	);
});

test("source-preflight failure remains a mergeable global Red collection", async () => {
	const input = await fixture();
	input.config.harness.argsPrefix = ["--test-source-preflight-failure"];
	const result = await runDerivedChangeChangeReadDiagnostic(input.config);
	const source = result.cases.find(
		({ id }) => id === "topology-v1.preflight.source",
	);
	assert.equal(source.status, "failed");
	assert.equal(source.failureClass, "global_invalid");
	for (const id of [
		"topology-v1.preflight.fixture",
		"topology-v1.preflight.library_control",
		"topology-v1.preflight.cli_control",
		"topology-v1.preflight.template_postflight",
	]) {
		const row = result.cases.find((candidate) => candidate.id === id);
		assert.equal(row.status, "skipped");
		assert.ok(row.dependsOn.includes(source.id));
	}
	const laterTemplate = result.cases.find(
		({ id }) => id === "duplicate-equal-v1.template",
	);
	assert.equal(laterTemplate.status, "skipped");
	assert.ok(laterTemplate.dependsOn.includes(source.id));
	const authorityDocument = JSON.parse(
		await readFile(input.config.fixtureAuthority.path, "utf8"),
	);
	const platform = {
		id: "macos_apfs",
		operatingSystem: "macos",
		architecture: "aarch64",
		filesystem: "apfs",
		hostIdentitySha256: input.config.execution.hostIdentitySha256,
	};
	const campaign = {
		id: input.config.campaignId,
		requiredCaseIds: result.cases.map(({ id }) => id).sort(),
		requiredPlatformIds: [platform.id],
		source: {
			commit: input.config.execution.sourceCommit,
			tree: input.config.execution.sourceTree,
			rangeBaseCommit: commit("3"),
			rangeSha256: digest("4"),
		},
		rootComponent: DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1,
		product: {
			binaries: [
				{
					platformId: platform.id,
					binarySha256: input.config.product.binarySha256,
				},
			],
		},
		harness: {
			binaries: [
				{
					platformId: platform.id,
					binarySha256: input.config.harness.binarySha256,
				},
			],
		},
		control: {
			binaries: [
				{
					platformId: platform.id,
					role: "cli",
					binarySha256: input.config.controls.cli.binarySha256,
				},
				{
					platformId: platform.id,
					role: "library",
					binarySha256: input.config.controls.library.binarySha256,
				},
			],
		},
		fixture: {
			authoritySha256: input.config.rootAuthoritySha256,
			document: authorityDocument,
		},
		platforms: [platform],
	};
	const fragment = finalizeDerivedChangeDiagnosticFragment({
		schema: DERIVED_CHANGE_DIAGNOSTIC_FRAGMENT_SCHEMA_V1,
		campaign,
		platform,
		artifacts: [],
		cases: result.cases,
	});
	assert.equal(
		mergeDerivedChangeDiagnosticReport({ campaign, fragments: [fragment] })
			.verdict,
		"red",
	);
});

test("CLI reads its config from the diagnostic environment and emits only collection JSON", async () => {
	const input = await fixture();
	input.config.workRoot = join(
		input.root,
		"configured-work-root-must-be-overridden",
	);
	const env = {
		...process.env,
		POINTBREAK_DERIVED_CHANGE_CHANGE_READ_CONFIG: JSON.stringify(input.config),
		POINTBREAK_DIAGNOSTIC_CASE_ROOT: input.caseRoot,
		POINTBREAK_DIAGNOSTIC_WORK_ROOT: input.workRoot,
	};
	const outcome = await new Promise((done) => {
		const child = spawn(
			process.execPath,
			[
				new URL("./derived-change-diagnostic-change-read.mjs", import.meta.url)
					.pathname,
				"--config-env",
			],
			{ env, stdio: ["ignore", "pipe", "pipe"] },
		);
		const stdout = [];
		const stderr = [];
		child.stdout.on("data", (chunk) => stdout.push(chunk));
		child.stderr.on("data", (chunk) => stderr.push(chunk));
		child.once("exit", (code) =>
			done({
				code,
				stdout: Buffer.concat(stdout).toString("utf8"),
				stderr: Buffer.concat(stderr).toString("utf8"),
			}),
		);
	});
	assert.equal(outcome.code, 0, outcome.stderr);
	assert.equal(
		JSON.parse(outcome.stdout).schema,
		"pointbreak.derived-change-diagnostic-collection.v1",
	);
});
