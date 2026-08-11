import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import {
	cp,
	lstat,
	mkdir,
	readFile,
	readdir,
	writeFile,
} from "node:fs/promises";
import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

export const DERIVED_CHANGE_CHANGE_READ_DIAGNOSTIC_CONFIG_SCHEMA_V1 =
	"pointbreak.derived-change-read-diagnostic-config.v1";
export const DERIVED_CHANGE_DIAGNOSTIC_CASE_COLLECTION_SCHEMA_V1 =
	"pointbreak.derived-change-diagnostic-collection.v1";

const SHA256 = /^[0-9a-f]{64}$/;
const COMMIT = /^[0-9a-f]{40}$/;
const READ_CASES = [
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
const READY_READ_CASES = READ_CASES.slice(0, 8);
const FAILURE_READ_CASES = [
	"profile",
	"changes_bare",
	"changes_bounded",
	"attention_bare",
	"attention_bounded",
	"summary_query",
];
const CONTROL_CASES = [
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
const CLI_CONTROLS = new Set([
	"direct_ready_call_graph_refusal",
	"automatic_error_call_graph_refusal",
	"explicit_off_strict_reader",
]);
const PREFLIGHT_KINDS = [
	"source",
	"fixture",
	"library_control",
	"cli_control",
	"template_postflight",
];
const TOOL_ENV = {
	git: "POINTBREAK_GIT_PROGRAM",
	jq: "POINTBREAK_JQ_PROGRAM",
	find: "POINTBREAK_FIND_PROGRAM",
	sort: "POINTBREAK_SORT_PROGRAM",
	wc: "POINTBREAK_WC_PROGRAM",
	tr: "POINTBREAK_TR_PROGRAM",
	awk: "POINTBREAK_AWK_PROGRAM",
	cp: "POINTBREAK_CP_PROGRAM",
	head: "POINTBREAK_HEAD_PROGRAM",
	dirname: "POINTBREAK_DIRNAME_PROGRAM",
	mkdir: "POINTBREAK_MKDIR_PROGRAM",
	rm: "POINTBREAK_RM_PROGRAM",
	hash: "POINTBREAK_HASH_PROGRAM",
};
const ACTIVATION_RECORD =
	"tests/support/assets/change-ready-store/5a1f8bbdea0db6199064bb2b75dfa89382b23398c71c640f7ca3268e48e3afaf.json";
const COMPLETION_RECORD =
	"tests/support/assets/change-ready-store/f31956c2b820926adc74d4d03cb03820d13c9ed2739b5f7ada81611a6f8bcff1.json";
const AUTHORITY_SOURCE_PATHS = [
	"scripts/materialize-inspector-decision-matrix.sh",
	"src/bench_support/derived_access/materializer.rs",
	ACTIVATION_RECORD,
	COMPLETION_RECORD,
];
const FIXTURES = [
	["topology-v1", null, READ_CASES, CONTROL_CASES, ["initial", "post_append"]],
	["duplicate-equal-v1", "duplicate_equal", READY_READ_CASES, [], ["initial"]],
	[
		"duplicate-conflict-v1",
		"duplicate_conflicting",
		FAILURE_READ_CASES,
		[],
		["initial"],
	],
	["removal-v1", "operative_removal", READY_READ_CASES, [], ["initial"]],
	[
		"missing-carrier-v1",
		"missing_selected_carrier",
		FAILURE_READ_CASES,
		[],
		["initial"],
	],
	[
		"mutated-carrier-v1",
		"mutated_selected_carrier",
		FAILURE_READ_CASES,
		[],
		["initial"],
	],
	[
		"wrong-family-carrier-v1",
		"wrong_family_selected_carrier",
		FAILURE_READ_CASES,
		[],
		["initial"],
	],
	["incomplete-v1", "incomplete_change", READY_READ_CASES, [], ["initial"]],
	[
		"cycle-conflicted-v1",
		"cycle_conflicted_change",
		READY_READ_CASES,
		[],
		["initial"],
	],
];

function object(value, label) {
	if (!value || typeof value !== "object" || Array.isArray(value))
		throw new Error(`${label} must be an object`);
}
function text(value, label) {
	if (typeof value !== "string" || !value.trim())
		throw new Error(`${label} must be non-empty text`);
}
function absolute(value, label) {
	text(value, label);
	if (!isAbsolute(value)) throw new Error(`${label} must be absolute`);
}
function digest(value, label) {
	if (!SHA256.test(value ?? "")) throw new Error(`${label} must be SHA-256`);
}
function pathInside(root, path) {
	const result = relative(resolve(root), resolve(path));
	if (!result || result === ".." || result.startsWith(`..${sep}`))
		throw new Error("diagnostic artifact escaped its case root");
	return result.split(sep).join("/");
}
function sameSet(actual, expected) {
	return (
		JSON.stringify([...actual].sort()) === JSON.stringify([...expected].sort())
	);
}
function fixturePreflights(fixture) {
	return fixture === "topology-v1"
		? PREFLIGHT_KINDS
		: ["source", "fixture", "template_postflight"];
}

export function derivedChangeChangeReadChildDescriptors() {
	const children = [];
	for (const [fixture, , rows, controls, storage] of FIXTURES) {
		const template = `${fixture}.template`;
		children.push({ id: template, lane: "native", dependsOn: [] });
		for (const kind of fixturePreflights(fixture))
			children.push({
				id: `${fixture}.preflight.${kind}`,
				lane: kind.includes("control") ? "control" : "native",
				dependsOn: [template],
			});
		const readDeps = [
			`${fixture}.preflight.source`,
			`${fixture}.preflight.fixture`,
		];
		for (const name of rows)
			children.push({
				id: `${fixture}.read.${name}`,
				lane: "native",
				dependsOn: readDeps,
			});
		for (const name of controls) {
			const kind = CLI_CONTROLS.has(name) ? "cli_control" : "library_control";
			children.push({
				id: `${fixture}.control.${name}`,
				lane: "control",
				dependsOn: [
					`${fixture}.preflight.source`,
					`${fixture}.preflight.${kind}`,
				],
			});
		}
		for (const name of storage)
			children.push({
				id: `${fixture}.storage.${name}`,
				lane: "native",
				dependsOn: readDeps,
			});
	}
	children.push({
		id: "change-read.identity-postflight",
		lane: "native",
		dependsOn: [],
	});
	return children.sort((a, b) => a.id.localeCompare(b.id));
}

function validateBinary(entry, label, buildCommand = false) {
	object(entry, label);
	absolute(entry.program, `${label} program`);
	digest(entry.binarySha256, `${label} binary hash`);
	if (buildCommand)
		digest(entry.buildCommandSha256, `${label} build command hash`);
}
export function validateDerivedChangeChangeReadDiagnosticConfig(config) {
	object(config, "change read diagnostic config");
	if (config.schema !== DERIVED_CHANGE_CHANGE_READ_DIAGNOSTIC_CONFIG_SCHEMA_V1)
		throw new Error("unsupported change read diagnostic config schema");
	text(config.campaignId, "change read diagnostic campaign id");
	digest(config.rootAuthoritySha256, "change read diagnostic root authority");
	absolute(config.caseRoot, "change read diagnostic case root");
	absolute(config.sourceCheckout, "change read diagnostic source checkout");
	object(config.execution, "change read diagnostic execution");
	for (const field of ["sourceCommit", "sourceTree"])
		if (!COMMIT.test(config.execution[field] ?? ""))
			throw new Error(`change read diagnostic execution ${field} is invalid`);
	for (const field of [
		"cargoLockSha256",
		"binarySha256",
		"contractSha256",
		"rootProvenanceSha256",
		"commandSha256",
		"hostIdentitySha256",
	])
		digest(
			config.execution[field],
			`change read diagnostic execution ${field}`,
		);
	for (const field of [
		"platform",
		"contractSchema",
		"operatingSystem",
		"architecture",
		"filesystem",
	])
		text(config.execution[field], `change read diagnostic execution ${field}`);
	if (config.execution.sourceDirty || config.execution.privateCorpusConfigured)
		throw new Error(
			"change read diagnostic execution must be clean and public",
		);
	if (
		!(
			(config.execution.platform === "macos_apfs" &&
				config.execution.operatingSystem === "macos" &&
				config.execution.filesystem === "apfs") ||
			(config.execution.platform === "windows_ntfs" &&
				config.execution.operatingSystem === "windows" &&
				config.execution.filesystem === "ntfs")
		) ||
		!["aarch64", "x86_64"].includes(config.execution.architecture)
	)
		throw new Error(
			"change read diagnostic execution platform normalization is invalid",
		);
	if (config.execution.rootProvenanceSha256 !== config.rootAuthoritySha256)
		throw new Error(
			"change read diagnostic root provenance differs from fixture authority",
		);
	validateBinary(config.product, "change read diagnostic product");
	for (const field of [
		"platform",
		"sourceCommit",
		"sourceTree",
		"cargoLockSha256",
		"versionSha256",
		"buildProfile",
		"buildCommandSha256",
		"operatingSystem",
		"architecture",
	]) {
		if (field.endsWith("Sha256"))
			digest(config.product[field], `change read diagnostic product ${field}`);
		else text(config.product[field], `change read diagnostic product ${field}`);
	}
	if (
		!COMMIT.test(config.product.sourceCommit) ||
		!COMMIT.test(config.product.sourceTree) ||
		config.product.sourceDirty ||
		!Array.isArray(config.product.enabledFeatures) ||
		config.product.enabledFeatures.some(
			(feature) =>
				typeof feature !== "string" ||
				!feature.trim() ||
				feature.trim() !== feature,
		) ||
		!config.product.enabledFeatures.every(
			(feature, index, all) => index === 0 || all[index - 1] < feature,
		)
	)
		throw new Error("change read diagnostic product identity is invalid");
	if (
		config.product.platform !== config.execution.platform ||
		config.product.sourceCommit !== config.execution.sourceCommit ||
		config.product.sourceTree !== config.execution.sourceTree ||
		config.product.cargoLockSha256 !== config.execution.cargoLockSha256 ||
		config.product.operatingSystem !== config.execution.operatingSystem ||
		config.product.architecture !== config.execution.architecture
	)
		throw new Error(
			"change read diagnostic product is not exact execution source",
		);
	validateBinary(config.harness, "change read diagnostic harness");
	if (
		!Array.isArray(config.harness.argsPrefix) ||
		config.harness.argsPrefix.some((value) => typeof value !== "string")
	)
		throw new Error("change read diagnostic harness arguments must be strings");
	object(config.controls, "change read diagnostic controls");
	validateBinary(
		config.controls.library,
		"change read diagnostic library control",
		true,
	);
	validateBinary(
		config.controls.cli,
		"change read diagnostic CLI control",
		true,
	);
	if (config.execution.binarySha256 !== config.harness.binarySha256)
		throw new Error(
			"change read diagnostic execution must bind the harness binary",
		);
	if (
		new Set([
			config.product.program,
			config.harness.program,
			config.controls.library.program,
			config.controls.cli.program,
		]).size !== 4 ||
		new Set([
			config.product.binarySha256,
			config.harness.binarySha256,
			config.controls.library.binarySha256,
			config.controls.cli.binarySha256,
		]).size !== 4
	)
		throw new Error(
			"change read diagnostic product, harness, and control binaries must be distinct",
		);
	object(config.fixtureAuthority, "change read diagnostic fixture authority");
	absolute(
		config.fixtureAuthority.path,
		"change read diagnostic fixture authority path",
	);
	digest(
		config.fixtureAuthority.sha256,
		"change read diagnostic fixture authority hash",
	);
	if (config.rootAuthoritySha256 !== config.fixtureAuthority.sha256)
		throw new Error(
			"change read diagnostic root authority differs from fixture authority",
		);
	absolute(
		config.fixtureAuthority.readyStore,
		"change read diagnostic public fixture store",
	);
	digest(
		config.fixtureAuthority.activationSha256,
		"change read diagnostic public activation fixture",
	);
	digest(
		config.fixtureAuthority.completionSha256,
		"change read diagnostic public completion fixture",
	);
	object(config.programs, "change read diagnostic programs");
	for (const name of ["bash", "topologyMaterializer", ...Object.keys(TOOL_ENV)])
		validateBinary(
			config.programs[name],
			`change read diagnostic ${name} program`,
		);
	if (config.programs.cygpath !== undefined)
		validateBinary(
			config.programs.cygpath,
			"change read diagnostic cygpath program",
		);
	if (!["shasum", "sha256sum"].includes(config.programs.hash.mode))
		throw new Error("change read diagnostic hash program mode is invalid");
	text(config.summaryQuery, "change read diagnostic summary query");
	if (
		config.summaryQuery.length > 256 ||
		config.summaryQuery.trim() !== config.summaryQuery
	)
		throw new Error("change read diagnostic summary query is invalid");
	return config;
}

async function verifyFile(entry, label) {
	const stat = await lstat(entry.program ?? entry.path);
	const path = entry.program ?? entry.path;
	const expected = entry.binarySha256 ?? entry.sha256;
	if (
		stat.isSymbolicLink() ||
		!stat.isFile() ||
		createHash("sha256")
			.update(await readFile(path))
			.digest("hex") !== expected
	)
		throw new Error(`${label} identity differs`);
}
async function sha256File(path) {
	return createHash("sha256")
		.update(await readFile(path))
		.digest("hex");
}
async function parseFixtureAuthority(config) {
	await verifyFile(
		config.fixtureAuthority,
		"change read diagnostic fixture authority",
	);
	const document = JSON.parse(
		await readFile(config.fixtureAuthority.path, "utf8"),
	);
	object(document, "change read diagnostic fixture authority document");
	if (
		document.schema !==
			"pointbreak.derived-change-public-fixture-authority.v1" ||
		document.sourceCommit !== config.execution.sourceCommit ||
		document.sourceTree !== config.execution.sourceTree
	)
		throw new Error("change read diagnostic fixture authority source differs");
	if (
		!Array.isArray(document.sourceFiles) ||
		!sameSet(
			document.sourceFiles.map((entry) => entry?.path),
			AUTHORITY_SOURCE_PATHS,
		) ||
		new Set(document.sourceFiles.map((entry) => entry.path)).size !==
			AUTHORITY_SOURCE_PATHS.length
	)
		throw new Error(
			"change read diagnostic fixture authority source inventory differs",
		);
	for (const entry of document.sourceFiles) {
		digest(
			entry.sha256,
			"change read diagnostic fixture authority source hash",
		);
		if (
			(await sha256File(join(config.sourceCheckout, entry.path))) !==
			entry.sha256
		)
			throw new Error(
				`change read diagnostic authority source differs: ${entry.path}`,
			);
	}
	const sourceFiles = new Map(
		document.sourceFiles.map((entry) => [entry.path, entry.sha256]),
	);
	if (
		sourceFiles.get(ACTIVATION_RECORD) !==
			config.fixtureAuthority.activationSha256 ||
		sourceFiles.get(COMPLETION_RECORD) !==
			config.fixtureAuthority.completionSha256 ||
		sourceFiles.get("scripts/materialize-inspector-decision-matrix.sh") !==
			config.programs.topologyMaterializer.binarySha256
	)
		throw new Error("change read diagnostic fixture authority binding differs");
	if (
		!Array.isArray(document.witnesses) ||
		!sameSet(
			document.witnesses.map((entry) => entry?.fixtureId),
			FIXTURES.map(([fixture]) => fixture),
		) ||
		new Set(document.witnesses.map((entry) => entry.fixtureId)).size !==
			FIXTURES.length
	)
		throw new Error(
			"change read diagnostic fixture authority witness inventory differs",
		);
	for (const witness of document.witnesses) {
		digest(
			witness.authoritativeInventorySha256,
			"change read diagnostic fixture authority inventory",
		);
		digest(
			witness.witnessSha256,
			"change read diagnostic fixture authority witness",
		);
	}
	return {
		document,
		witnesses: new Map(
			document.witnesses.map((entry) => [entry.fixtureId, entry]),
		),
	};
}
async function verifyBoundIdentities(config) {
	for (const [label, entry] of [
		["product", config.product],
		["harness", config.harness],
		["library control", config.controls.library],
		["CLI control", config.controls.cli],
		...Object.entries(config.programs).map(([name, entry]) => [
			`${name} program`,
			entry,
		]),
	])
		await verifyFile(entry, `change read diagnostic ${label}`);
	for (const [relativePath, expected] of [
		[ACTIVATION_RECORD, config.fixtureAuthority.activationSha256],
		[COMPLETION_RECORD, config.fixtureAuthority.completionSha256],
	]) {
		const path = join(
			config.fixtureAuthority.readyStore,
			relativePath.split("/").at(-1),
		);
		const stat = await lstat(path);
		if (
			stat.isSymbolicLink() ||
			!stat.isFile() ||
			(await sha256File(path)) !== expected
		)
			throw new Error(
				"change read diagnostic public fixture authority differs",
			);
	}
	await parseFixtureAuthority(config);
}
async function empty(root) {
	try {
		if ((await readdir(root)).length)
			throw new Error("change read diagnostic case root must be empty");
	} catch (error) {
		if (error?.code !== "ENOENT") throw error;
		await mkdir(root, { recursive: true });
	}
}
function cleanEnv(config, root, additions = {}) {
	const exactPrograms = [
		config.product.program,
		config.harness.program,
		config.controls.library.program,
		config.controls.cli.program,
		...Object.values(config.programs).map(({ program }) => program),
	];
	return {
		PATH: [...new Set(exactPrograms.map(dirname))].join(
			sep === "\\" ? ";" : ":",
		),
		HOME: root,
		USERPROFILE: root,
		POINTBREAK_GIT_PROGRAM: config.programs.git.program,
		POINTBREAK_DIAGNOSTIC_CASE_ROOT: root,
		TMPDIR: root,
		TMP: root,
		TEMP: root,
		...(process.env.SystemRoot ? { SystemRoot: process.env.SystemRoot } : {}),
		...(process.env.SYSTEMROOT ? { SYSTEMROOT: process.env.SYSTEMROOT } : {}),
		...additions,
	};
}
async function command(program, args, cwd, env) {
	return await new Promise((done) => {
		const child = spawn(program, args, {
			cwd,
			env,
			shell: false,
			stdio: ["ignore", "pipe", "pipe"],
		});
		const stdout = [];
		const stderr = [];
		child.stdout.on("data", (v) => stdout.push(v));
		child.stderr.on("data", (v) => stderr.push(v));
		child.once("error", (error) =>
			done({
				code: null,
				signal: null,
				spawnError: String(error),
				stdout: Buffer.concat(stdout),
				stderr: Buffer.concat(stderr),
			}),
		);
		child.once("exit", (code, signal) =>
			done({
				code,
				signal,
				stdout: Buffer.concat(stdout),
				stderr: Buffer.concat(stderr),
			}),
		);
	});
}
async function retain(root, path, bytes) {
	await mkdir(resolve(path, ".."), { recursive: true });
	await writeFile(path, bytes);
	return pathInside(root, path);
}
function statusRow(
	id,
	lane,
	status,
	dependsOn,
	fixture,
	phase,
	detail,
	failureClass = "case_failure",
) {
	if (!["passed", "failed", "skipped"].includes(status))
		throw new Error("diagnostic row status is invalid");
	if (status === "failed" && !detail)
		throw new Error("failed diagnostic row lacks detail");
	return {
		id,
		lane,
		required: true,
		attempted: status !== "skipped",
		status,
		dependsOn: [...new Set(dependsOn)].sort(),
		phase,
		fixtureCheckpoint: { fixture, checkpoint: phase },
		...(status === "failed"
			? { failureClass, expected: "passed", actual: String(detail) }
			: {}),
		...(status === "skipped"
			? { skipReason: String(detail ?? "dependency did not pass") }
			: {}),
	};
}
function expectedIds(fixture, rows, controls, storage) {
	return [
		...rows.map((name) => `${fixture}.read.${name}`),
		...controls.map((name) => `${fixture}.control.${name}`),
		...storage.map((name) => `${fixture}.storage.${name}`),
	];
}
function outputStatus(row, fallback) {
	if (!["passed", "failed", "skipped"].includes(row?.status))
		throw new Error("change read diagnostic row status is invalid");
	if (row.status === "failed" && !row.failureDetail)
		throw new Error("change read diagnostic failure lacks detail");
	return fallback ? "skipped" : row.status;
}
function validateExactRows(rows, expected, label) {
	if (
		!Array.isArray(rows) ||
		!sameSet(
			rows.map((row) => row?.case),
			expected,
		) ||
		new Set(rows.map((row) => row.case)).size !== expected.length
	)
		throw new Error(`change read diagnostic ${label} inventory differs`);
	return [...rows].sort((a, b) => a.case.localeCompare(b.case));
}
function validateOutput(value, fixture, rows, controls, storage) {
	object(value, "change read diagnostic output");
	if (
		"schema" in value ||
		value.mode !== "--derived-change-read-diagnostic" ||
		typeof value.sourceUnchanged !== "boolean"
	)
		throw new Error("change read diagnostic output identity differs");
	const expectedPreflight = fixturePreflights(fixture);
	if (
		!Array.isArray(value.preflight) ||
		!sameSet(
			value.preflight.map((row) => row?.kind),
			expectedPreflight,
		) ||
		new Set(value.preflight.map((row) => row.kind)).size !==
			expectedPreflight.length
	)
		throw new Error("change read diagnostic preflight inventory differs");
	const preflight = [...value.preflight].sort((a, b) =>
		a.kind.localeCompare(b.kind),
	);
	for (const row of preflight)
		if (
			!["passed", "failed", "skipped"].includes(row.status) ||
			(row.status === "failed" && !row.failureDetail)
		)
			throw new Error("change read diagnostic preflight is invalid");
	if (!value.sourceUnchanged) {
		const postflight = preflight.find(
			({ kind }) => kind === "template_postflight",
		);
		postflight.status = "failed";
		postflight.failureDetail ??=
			"derived Change diagnostic mutated its immutable fixture template";
	}
	return {
		preflight,
		rows: validateExactRows(value.rows, rows, "read"),
		controls: validateExactRows(value.controls, controls, "control"),
		storage: validateExactRows(value.storage, storage, "storage"),
	};
}
function addSkippedFixture(
	cases,
	fixture,
	rows,
	controls,
	storage,
	setupId,
	phase,
	detail,
) {
	for (const kindName of fixturePreflights(fixture))
		cases.push(
			statusRow(
				`${fixture}.preflight.${kindName}`,
				kindName.includes("control") ? "control" : "native",
				"skipped",
				[setupId],
				fixture,
				phase,
				detail,
			),
		);
	for (const id of expectedIds(fixture, rows, controls, storage))
		cases.push(
			statusRow(
				id,
				id.includes(".control.") ? "control" : "native",
				"skipped",
				[setupId],
				fixture,
				phase,
				detail,
			),
		);
}
function verifyWitness(bytes, fixture, expected) {
	let witness;
	try {
		witness = JSON.parse(bytes);
	} catch {
		throw new Error("fixture witness is not JSON");
	}
	if (
		witness.schema !==
			"pointbreak.qualification-derived-change-fixture-witness.v1" ||
		witness.fixtureId !== fixture ||
		witness.authoritativeInventorySha256 !==
			expected.authoritativeInventorySha256 ||
		createHash("sha256").update(bytes).digest("hex") !== expected.witnessSha256
	)
		throw new Error("fixture witness differs from public authority");
}
function materializerEnv(config, template) {
	const additions = {
		POINTBREAK_BINARY: config.product.program,
		POINTBREAK_CHANGE_READY_FIXTURE_DIR: config.fixtureAuthority.readyStore,
		POINTBREAK_HOME: join(template, ".git", "pointbreak-home"),
		POINTBREAK_HASH_PROGRAM: config.programs.hash.program,
		POINTBREAK_HASH_PROGRAM_MODE: config.programs.hash.mode,
		POINTBREAK_CYGPATH_PROGRAM: config.programs.cygpath?.program ?? "absent",
	};
	for (const [name, envName] of Object.entries(TOOL_ENV))
		if (name !== "hash") additions[envName] = config.programs[name].program;
	return cleanEnv(config, template, additions);
}
function readRequest(
	config,
	fixture,
	template,
	witness,
	workspace,
	requestPath,
) {
	const witnessSha256 = createHash("sha256").update(witness).digest("hex");
	const execution = structuredClone(config.execution);
	execution.commandSha256 = createHash("sha256")
		.update(
			JSON.stringify([
				config.harness.program,
				...config.harness.argsPrefix,
				"--derived-change-read-diagnostic",
				`--derived-access-request=${requestPath}`,
			]),
		)
		.digest("hex");
	const product = structuredClone(config.product);
	delete product.program;
	return {
		schema: "pointbreak.derived-change-read-diagnostic-request.v1",
		workspaceRoot: workspace,
		readRequest: {
			schema: "pointbreak.qualification-derived-change-read-request.v1",
			purpose: "exact_source_qualification",
			sourceCheckout: config.sourceCheckout,
			execution,
			productSourceCheckout: config.sourceCheckout,
			product,
			fixture,
			fixtureWitness: witness,
			fixtureWitnessSha256: witnessSha256,
			repository: template,
			pointbreakHome: join(template, ".git", "pointbreak-home"),
			productBinary: config.product.program,
			controlTestBinary: config.controls.library.program,
			controlTestBinarySha256: config.controls.library.binarySha256,
			controlTestBuildCommandSha256: config.controls.library.buildCommandSha256,
			controlCliTestBinary: config.controls.cli.program,
			controlCliTestBinarySha256: config.controls.cli.binarySha256,
			controlCliTestBuildCommandSha256: config.controls.cli.buildCommandSha256,
			storageForbiddenProbes: {
				proposalSummary: "qualification storage summary sentinel v1",
				prose: "qualification storage prose sentinel v1",
				payloadDocument:
					"20dfd0d4e1ce81bfb753001a61c0394914d4711e84f90fb745a659dba1ff11bf",
				privatePath: template,
			},
			summaryQuery: config.summaryQuery,
		},
	};
}

export async function runDerivedChangeChangeReadDiagnostic(input) {
	const config = validateDerivedChangeChangeReadDiagnosticConfig(input);
	await empty(config.caseRoot);
	const authorityState = await parseFixtureAuthority(config);
	await verifyBoundIdentities(config);
	const authority = join(
		config.caseRoot,
		"authority",
		"fixture-authority.json",
	);
	await mkdir(join(config.caseRoot, "authority"), { recursive: true });
	await cp(config.fixtureAuthority.path, authority);
	const artifacts = [pathInside(config.caseRoot, authority)];
	const cases = [];
	for (const [fixture, kind, rows, controls, storage] of FIXTURES) {
		const template = join(config.caseRoot, "templates", fixture);
		const workspace = join(config.caseRoot, "workspaces", fixture);
		const requestRoot = join(config.caseRoot, "requests");
		const logRoot = join(config.caseRoot, "logs");
		await Promise.all([
			mkdir(requestRoot, { recursive: true }),
			mkdir(logRoot, { recursive: true }),
			mkdir(join(config.caseRoot, "templates"), { recursive: true }),
			mkdir(join(config.caseRoot, "workspaces"), { recursive: true }),
		]);
		const setupId = `${fixture}.template`;
		const materializeRequest = join(requestRoot, `${fixture}.materialize.json`);
		await writeFile(
			materializeRequest,
			JSON.stringify({
				sourceCheckout: config.sourceCheckout,
				root: template,
				...(kind ? { kind } : {}),
			}),
		);
		artifacts.push(pathInside(config.caseRoot, materializeRequest));
		const outcome =
			fixture === "topology-v1"
				? await command(
						config.programs.bash.program,
						[config.programs.topologyMaterializer.program, template],
						config.sourceCheckout,
						materializerEnv(config, template),
					)
				: await command(
						config.harness.program,
						[
							...config.harness.argsPrefix,
							"--derived-change-fixture-materialize",
							`--derived-access-request=${materializeRequest}`,
						],
						config.sourceCheckout,
						cleanEnv(config, template),
					);
		artifacts.push(
			await retain(
				config.caseRoot,
				join(logRoot, `${fixture}.materialize.stdout.log`),
				outcome.stdout,
			),
			await retain(
				config.caseRoot,
				join(logRoot, `${fixture}.materialize.stderr.log`),
				outcome.stderr,
			),
		);
		const setupFailed =
			outcome.code !== 0 || outcome.signal !== null || outcome.spawnError;
		cases.push(
			statusRow(
				setupId,
				"native",
				setupFailed ? "failed" : "passed",
				[],
				fixture,
				"template-materialization",
				setupFailed
					? JSON.stringify({
							code: outcome.code,
							signal: outcome.signal,
							spawnError: outcome.spawnError,
						})
					: undefined,
				"lane_invalid",
			),
		);
		if (setupFailed) {
			addSkippedFixture(
				cases,
				fixture,
				rows,
				controls,
				storage,
				setupId,
				"fixture-unavailable",
				`dependency ${setupId} did not pass`,
			);
			continue;
		}
		const witness = join(config.caseRoot, "witnesses", `${fixture}.json`);
		await mkdir(join(config.caseRoot, "witnesses"), { recursive: true });
		await writeFile(witness, outcome.stdout);
		artifacts.push(pathInside(config.caseRoot, witness));
		try {
			verifyWitness(
				outcome.stdout,
				fixture,
				authorityState.witnesses.get(fixture),
			);
		} catch (error) {
			cases[cases.length - 1] = statusRow(
				setupId,
				"native",
				"failed",
				[],
				fixture,
				"fixture-witness",
				String(error),
				"global_invalid",
			);
			addSkippedFixture(
				cases,
				fixture,
				rows,
				controls,
				storage,
				setupId,
				"fixture-witness",
				String(error),
			);
			continue;
		}
		const requestPath = join(requestRoot, `${fixture}.read.json`);
		await writeFile(
			requestPath,
			JSON.stringify(
				readRequest(config, fixture, template, witness, workspace, requestPath),
			),
		);
		artifacts.push(pathInside(config.caseRoot, requestPath));
		const result = await command(
			config.harness.program,
			[
				...config.harness.argsPrefix,
				"--derived-change-read-diagnostic",
				`--derived-access-request=${requestPath}`,
			],
			config.sourceCheckout,
			cleanEnv(config, workspace),
		);
		artifacts.push(
			await retain(
				config.caseRoot,
				join(logRoot, `${fixture}.read.stdout.log`),
				result.stdout,
			),
			await retain(
				config.caseRoot,
				join(logRoot, `${fixture}.read.stderr.log`),
				result.stderr,
			),
		);
		let output;
		try {
			if (result.code !== 0 || result.signal !== null || result.spawnError)
				throw new Error("diagnostic harness did not complete");
			output = validateOutput(
				JSON.parse(result.stdout.toString("utf8")),
				fixture,
				rows,
				controls,
				storage,
			);
		} catch (error) {
			addSkippedFixture(
				cases,
				fixture,
				rows,
				controls,
				storage,
				setupId,
				"diagnostic-output",
				String(error),
			);
			continue;
		}
		const preflight = new Map(output.preflight.map((row) => [row.kind, row]));
		for (const kind of fixturePreflights(fixture)) {
			const row = preflight.get(kind);
			cases.push(
				statusRow(
					`${fixture}.preflight.${kind}`,
					kind.includes("control") ? "control" : "native",
					row.status,
					[setupId],
					fixture,
					`preflight-${kind}`,
					row.failureDetail,
					row.status === "failed" ? "lane_invalid" : "case_failure",
				),
			);
		}
		const readDeps = [
			`${fixture}.preflight.source`,
			`${fixture}.preflight.fixture`,
		];
		const readReady = ["source", "fixture"].every(
			(kind) => preflight.get(kind).status === "passed",
		);
		for (const row of output.rows)
			cases.push(
				statusRow(
					`${fixture}.read.${row.case}`,
					"native",
					outputStatus(row, !readReady),
					readDeps,
					fixture,
					`read-${row.case}`,
					readReady ? row.failureDetail : "read preflight did not pass",
				),
			);
		for (const row of output.controls) {
			const controlKind = CLI_CONTROLS.has(row.case)
				? "cli_control"
				: "library_control";
			const ready =
				preflight.get("source").status === "passed" &&
				preflight.get(controlKind).status === "passed";
			cases.push(
				statusRow(
					`${fixture}.control.${row.case}`,
					"control",
					outputStatus(row, !ready),
					[
						`${fixture}.preflight.source`,
						`${fixture}.preflight.${controlKind}`,
					],
					fixture,
					`control-${row.case}`,
					ready ? row.failureDetail : "control preflight did not pass",
				),
			);
		}
		for (const row of output.storage)
			cases.push(
				statusRow(
					`${fixture}.storage.${row.case}`,
					"native",
					outputStatus(row, !readReady),
					readDeps,
					fixture,
					`storage-${row.case}`,
					readReady ? row.failureDetail : "storage preflight did not pass",
				),
			);
	}
	let postflight;
	try {
		await verifyBoundIdentities(config);
		postflight = statusRow(
			"change-read.identity-postflight",
			"native",
			"passed",
			[],
			"all-fixtures",
			"identity-postflight",
		);
	} catch (error) {
		postflight = statusRow(
			"change-read.identity-postflight",
			"native",
			"failed",
			[],
			"all-fixtures",
			"identity-postflight",
			String(error),
			"global_invalid",
		);
	}
	cases.push(postflight);
	return {
		schema: DERIVED_CHANGE_DIAGNOSTIC_CASE_COLLECTION_SCHEMA_V1,
		campaignId: config.campaignId,
		cases: cases.sort((a, b) => a.id.localeCompare(b.id)),
		artifactPaths: [...new Set(artifacts)].sort(),
	};
}

async function main() {
	if (process.argv.slice(2).join(" ") !== "--config-env")
		throw new Error(
			"usage: derived-change-diagnostic-change-read.mjs --config-env",
		);
	const encoded = process.env.POINTBREAK_DERIVED_CHANGE_CHANGE_READ_CONFIG;
	if (!encoded)
		throw new Error("POINTBREAK_DERIVED_CHANGE_CHANGE_READ_CONFIG is required");
	const config = JSON.parse(encoded);
	if (process.env.POINTBREAK_DIAGNOSTIC_CASE_ROOT)
		config.caseRoot = process.env.POINTBREAK_DIAGNOSTIC_CASE_ROOT;
	process.stdout.write(
		`${JSON.stringify(await runDerivedChangeChangeReadDiagnostic(config))}\n`,
	);
}

if (
	process.argv[1] &&
	resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
	main().catch((error) => {
		process.stderr.write(
			`${error instanceof Error ? error.message : String(error)}\n`,
		);
		process.exitCode = 1;
	});
}
