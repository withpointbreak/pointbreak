import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { chmod, mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
	DERIVED_CHANGE_CHANGE_READ_DIAGNOSTIC_CONFIG_SCHEMA_V1,
	derivedChangeChangeReadChildDescriptors,
	runDerivedChangeChangeReadDiagnostic,
	validateDerivedChangeChangeReadDiagnosticConfig,
} from "./derived-change-diagnostic-change-read.mjs";

const digest = (digit) => digit.repeat(64);
const commit = (digit) => digit.repeat(40);
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

async function fixture() {
	const root = await mkdtemp(
		join(tmpdir(), "pointbreak-change-read-diagnostic-"),
	);
	const source = join(root, "source");
	const caseRoot = join(root, "case");
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
	const authority = join(root, "fixture-authority.json");
	await mkdir(join(source, "src", "bench_support", "derived_access"), {
		recursive: true,
	});
	await mkdir(readyStore, { recursive: true });
	await mkdir(join(source, "scripts"), { recursive: true });
	await writeFile(
		fake,
		`#!${process.execPath}
import { readFile, mkdir } from "node:fs/promises";
import { createHash } from "node:crypto";
if(!process.env.POINTBREAK_GIT_PROGRAM) throw new Error("exact Git program is required");
const requestArg=process.argv.find((value)=>value.startsWith("--derived-access-request="));
const request=JSON.parse(await readFile(requestArg.slice(requestArg.indexOf("=")+1),"utf8"));
const ids={duplicate_equal:"duplicate-equal-v1",duplicate_conflicting:"duplicate-conflict-v1",operative_removal:"removal-v1",missing_selected_carrier:"missing-carrier-v1",mutated_selected_carrier:"mutated-carrier-v1",wrong_family_selected_carrier:"wrong-family-carrier-v1",incomplete_change:"incomplete-v1",cycle_conflicted_change:"cycle-conflicted-v1"};
const witness=(fixtureId)=>JSON.stringify({schema:"pointbreak.qualification-derived-change-fixture-witness.v1",fixtureId,authoritativeInventorySha256:"${digest("f")}",storageForbiddenProbeHashes:{proposalSummarySha256:"${digest("1")}",proseSha256:"${digest("2")}",payloadDocumentSha256:"${digest("3")}"}});
if(process.argv.includes("--derived-change-fixture-materialize")){if(request.kind === "mutated_selected_carrier" && process.argv.includes("--test-fail-mutated")){process.stderr.write("fixture fail");process.exit(7)}await mkdir(request.root,{recursive:true});console.log(witness(ids[request.kind]) + (request.kind === "mutated_selected_carrier" && process.argv.includes("--test-witness-mismatch") ? " " : ""));process.exit(0)}
const fixture=request.readRequest.fixture; const typed=new Set(["duplicate-conflict-v1","missing-carrier-v1","mutated-carrier-v1","wrong-family-carrier-v1"]); const rows=(fixture === "topology-v1" ? ${JSON.stringify(readCases)} : typed.has(fixture) ? ${JSON.stringify(["profile", "changes_bare", "changes_bounded", "attention_bare", "attention_bounded", "summary_query"])} : ${JSON.stringify(readCases.slice(0, 8))}).map((caseName)=>({case:caseName,status:"passed"}));
const commandSha256=createHash("sha256").update(JSON.stringify([process.argv[1],...process.argv.slice(2)])).digest("hex"); if(request.readRequest.execution.commandSha256 !== commandSha256) throw new Error("per-fixture command identity differs");
if(process.argv.includes("--test-invalid-output")){console.log("{}"),process.exit(0)}
const topology=fixture === "topology-v1"; const preflight=(topology?["source","fixture","library_control","cli_control","template_postflight"]:["source","fixture","template_postflight"]).map((kind)=>({kind,status:"passed"}));
console.log(JSON.stringify({mode:"--derived-change-read-diagnostic",sourceUnchanged:!process.argv.includes("--test-template-mutation"),preflight,rows,controls:topology?${JSON.stringify(controls)}.map((caseName)=>({case:caseName,status:"passed"})):[],storage:[{case:"initial",status:"passed"},...(topology?[{case:"post_append",status:"passed"}]:[])]}));
`,
	);
	const topologyWitness = JSON.stringify({
		schema: "pointbreak.qualification-derived-change-fixture-witness.v1",
		fixtureId: "topology-v1",
		authoritativeInventorySha256: digest("f"),
		storageForbiddenProbeHashes: {
			proposalSummarySha256: digest("1"),
			proseSha256: digest("2"),
			payloadDocumentSha256: digest("3"),
		},
	});
	await writeFile(
		materializer,
		`#!/bin/sh\ncase "$POINTBREAK_CYGPATH_PROGRAM" in absent|/*) ;; *) exit 18 ;; esac\nmkdir -p "$1"\nprintf '%s\\n' '${topologyWitness}'\n`,
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
		"topology-v1",
		"duplicate-equal-v1",
		"duplicate-conflict-v1",
		"removal-v1",
		"missing-carrier-v1",
		"mutated-carrier-v1",
		"wrong-family-carrier-v1",
		"incomplete-v1",
		"cycle-conflicted-v1",
	];
	const witnessBytes = (fixtureId) =>
		`${fixtureId === "topology-v1" ? topologyWitness : JSON.stringify({ schema: "pointbreak.qualification-derived-change-fixture-witness.v1", fixtureId, authoritativeInventorySha256: digest("f"), storageForbiddenProbeHashes: { proposalSummarySha256: digest("1"), proseSha256: digest("2"), payloadDocumentSha256: digest("3") } })}\n`;
	const authorityDocument = {
		schema: "pointbreak.derived-change-public-fixture-authority.v1",
		sourceCommit: commit("1"),
		sourceTree: commit("2"),
		sourceFiles: [
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
	};
	await writeFile(authority, `${JSON.stringify(authorityDocument)}\n`);
	const authoritySha256 = await sha256(authority);
	return {
		root,
		caseRoot,
		config: {
			schema: DERIVED_CHANGE_CHANGE_READ_DIAGNOSTIC_CONFIG_SCHEMA_V1,
			campaignId: "diagnostic-test",
			rootAuthoritySha256: authoritySha256,
			caseRoot,
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

test("passes an exact optional cygpath binding to topology materialization", async () => {
	const input = await fixture();
	input.config.programs.cygpath = structuredClone(input.config.programs.hash);
	const result = await runDerivedChangeChangeReadDiagnostic(input.config);
	assert.equal(
		result.cases.find(({ id }) => id === "topology-v1.template").status,
		"passed",
	);
});

test("exports a deterministic complete child inventory", () => {
	const first = derivedChangeChangeReadChildDescriptors();
	assert.deepEqual(first, derivedChangeChangeReadChildDescriptors());
	assert.equal(first.filter(({ id }) => id.includes(".read.")).length, 71);
	assert.ok(first.some(({ id }) => id === "change-read.identity-postflight"));
});

test("classifies a witness mismatch without dropping independent fixture rows", async () => {
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
		"passed",
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
});

test("CLI reads its config from the diagnostic environment and emits only collection JSON", async () => {
	const input = await fixture();
	const env = {
		...process.env,
		POINTBREAK_DERIVED_CHANGE_CHANGE_READ_CONFIG: JSON.stringify(input.config),
		POINTBREAK_DIAGNOSTIC_CASE_ROOT: input.caseRoot,
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
