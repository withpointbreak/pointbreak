import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import {
	access,
	lstat,
	mkdir,
	readdir,
	readFile,
	writeFile,
} from "node:fs/promises";
import {
	dirname,
	isAbsolute,
	join,
	relative,
	resolve,
	sep,
} from "node:path";
import { pathToFileURL } from "node:url";

export const DERIVED_CHANGE_NATIVE_DIAGNOSTIC_CONFIG_SCHEMA_V1 =
	"pointbreak.derived-change-native-diagnostic-config.v1";
export const DERIVED_CHANGE_DIAGNOSTIC_CASE_COLLECTION_SCHEMA_V1 =
	"pointbreak.derived-change-diagnostic-collection.v1";

export const DERIVED_CHANGE_NATIVE_LIFECYCLE_CRITERIA_V1 = Object.freeze([
	"open_bootstrap_reopen_replay_equality",
	"concurrent_writers_long_lived_reader",
	"unique_equal_conflict_cursor_sequence",
	"crash_before_intent_commit",
	"crash_after_intent_before_event",
	"crash_after_event_before_receipt",
	"crash_after_receipt_before_head",
	"crash_after_head_before_intent_retirement",
	"crash_during_bootstrap_staging",
	"crash_during_quarantine_epoch_publication",
	"derived_transaction_interruption",
	"backup_without_derived_then_rebuild",
	"wrong_root",
	"wrong_schema",
	"wrong_profile",
	"corruption_quarantine_new_epoch",
	"reader_handle_release_retirement",
	"independent_package_verification",
]);

const TIERS = Object.freeze(["D0-128", "L1", "L7"]);
const SHA256 = /^[0-9a-f]{64}$/;
const COMMIT = /^[0-9a-f]{40}$/;

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

function pathsOverlap(left, right) {
	const relation = relative(resolve(left), resolve(right));
	if (
		relation === "" ||
		(relation !== ".." && !relation.startsWith(`..${sep}`))
	)
		return true;
	const reverse = relative(resolve(right), resolve(left));
	return reverse !== ".." && !reverse.startsWith(`..${sep}`);
}

function requireSha256(value, label) {
	if (typeof value !== "string" || !SHA256.test(value)) {
		throw new Error(`${label} must be a lowercase SHA-256`);
	}
}

function validateConfig(config) {
	requireObject(config, "native diagnostic config");
	if (config.schema !== DERIVED_CHANGE_NATIVE_DIAGNOSTIC_CONFIG_SCHEMA_V1) {
		throw new Error("unsupported native diagnostic config schema");
	}
	requireText(config.campaignId, "native diagnostic campaign id");
	requireSha256(config.rootAuthoritySha256, "native diagnostic root authority");
	requireAbsolutePath(config.caseRoot, "native diagnostic case root");
	requireAbsolutePath(config.workRoot, "native diagnostic work root");
	requireAbsolutePath(
		config.sourceCheckout,
		"native diagnostic source checkout",
	);
	if (
		pathsOverlap(config.workRoot, config.caseRoot) ||
		pathsOverlap(config.workRoot, config.sourceCheckout)
	) {
		throw new Error(
			"native diagnostic work root must be disjoint from retained and source roots",
		);
	}
	requireAbsolutePath(config.gitProgram, "native diagnostic Git program");
	requireObject(config.source, "native diagnostic source identity");
	for (const field of ["commit", "tree", "rangeBaseCommit"]) {
		if (!COMMIT.test(config.source[field] ?? "")) {
			throw new Error(`native diagnostic source ${field} is invalid`);
		}
	}
	requireSha256(config.source.rangeSha256, "native diagnostic source range");
	requireObject(config.platform, "native diagnostic platform");
	for (const field of ["id", "operatingSystem", "architecture", "filesystem"]) {
		requireText(config.platform[field], `native diagnostic platform ${field}`);
	}
	requireSha256(
		config.platform.hostIdentitySha256,
		"native diagnostic host identity",
	);
	requireObject(config.harness, "native diagnostic harness");
	requireAbsolutePath(
		config.harness.program,
		"native diagnostic harness program",
	);
	if (
		!Array.isArray(config.harness.argsPrefix) ||
		config.harness.argsPrefix.some((argument) => typeof argument !== "string")
	) {
		throw new Error("native diagnostic harness arguments must be strings");
	}
	const criteria =
		config.lifecycleCriteria ?? DERIVED_CHANGE_NATIVE_LIFECYCLE_CRITERIA_V1;
	if (
		!Array.isArray(criteria) ||
		criteria.length === 0 ||
		new Set(criteria).size !== criteria.length ||
		criteria.some(
			(criterion) => typeof criterion !== "string" || criterion === "",
		)
	) {
		throw new Error("native diagnostic lifecycle inventory is invalid");
	}
	return { ...config, lifecycleCriteria: [...criteria] };
}

async function sha256File(path) {
	const hash = createHash("sha256");
	await new Promise((resolvePromise, rejectPromise) => {
		const stream = createReadStream(path);
		stream.on("data", (chunk) => hash.update(chunk));
		stream.on("error", rejectPromise);
		stream.on("end", resolvePromise);
	});
	return hash.digest("hex");
}

const sha256Text = (value) => createHash("sha256").update(value).digest("hex");

async function requireEmptyRoot(path) {
	try {
		const stat = await lstat(path);
		if (stat.isSymbolicLink() || !stat.isDirectory()) {
			throw new Error("native diagnostic case root is not a real directory");
		}
		if ((await readdir(path)).length !== 0) {
			throw new Error("native diagnostic case root must be empty");
		}
	} catch (error) {
		if (error?.code !== "ENOENT") throw error;
		await mkdir(path, { recursive: false });
	}
}

function normalRelativePath(root, path) {
	const relation = relative(resolve(root), resolve(path));
	if (relation === "" || relation === ".." || relation.startsWith(`..${sep}`)) {
		throw new Error("native diagnostic artifact escaped its case root");
	}
	return relation.split(sep).join("/");
}

async function runCommand(program, args, { cwd, env } = {}) {
	await access(program);
	return new Promise((resolvePromise) => {
		const child = spawn(program, args, {
			cwd,
			env,
			shell: false,
			stdio: ["ignore", "pipe", "pipe"],
		});
		const stdout = [];
		const stderr = [];
		child.stdout.on("data", (chunk) => stdout.push(chunk));
		child.stderr.on("data", (chunk) => stderr.push(chunk));
		child.once("error", (error) =>
			resolvePromise({
				code: null,
				signal: null,
				spawnError: String(error),
				stdout: Buffer.concat(stdout),
				stderr: Buffer.concat(stderr),
			}),
		);
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

function commandArguments(config, mode, requestPath) {
	return [
		...config.harness.argsPrefix,
		mode,
		`--derived-access-request=${requestPath}`,
	];
}

function executionIdentity(
	config,
	contract,
	binarySha256,
	cargoLockSha256,
	args,
) {
	return {
		platform: config.platform.id,
		sourceCommit: config.source.commit,
		sourceTree: config.source.tree,
		cargoLockSha256,
		binarySha256,
		contractSchema: contract.contract.schema,
		contractSha256: contract.contractSha256,
		rootProvenanceSha256: config.rootAuthoritySha256,
		commandSha256: sha256Text(
			JSON.stringify([config.harness.program, ...args]),
		),
		operatingSystem: config.platform.operatingSystem,
		architecture: config.platform.architecture,
		filesystem: config.platform.filesystem,
		hostIdentitySha256: config.platform.hostIdentitySha256,
		sourceDirty: false,
		privateCorpusConfigured: false,
	};
}

function baseCase(id, status, dependsOn, phase, tier, artifactPaths) {
	return {
		id,
		lane: "native",
		required: true,
		attempted: status === "passed" || status === "failed",
		status,
		dependsOn,
		phase,
		fixtureCheckpoint: {
			fixture: `public-derived-access-${tier}`,
			checkpoint: phase,
		},
		artifactPaths,
	};
}

function failedCase(base, failureClass, expected, actual) {
	return { ...base, failureClass, expected, actual };
}

function skippedCase(base, blocker) {
	return {
		...base,
		attempted: false,
		status: "skipped",
		skipReason: `dependency ${blocker} did not pass`,
	};
}

async function retainCommand(root, name, outcome) {
	const stdoutPath = join(root, `${name}.stdout.log`);
	const stderrPath = join(root, `${name}.stderr.log`);
	await Promise.all([
		writeFile(stdoutPath, outcome.stdout),
		writeFile(stderrPath, outcome.stderr),
	]);
	return [
		normalRelativePath(root, stdoutPath),
		normalRelativePath(root, stderrPath),
	];
}

function parseJsonOutput(outcome, label) {
	if (outcome.code !== 0 || outcome.signal !== null || outcome.spawnError) {
		throw new Error(
			`${label} exited with ${JSON.stringify({
				code: outcome.code,
				signal: outcome.signal,
				spawnError: outcome.spawnError,
			})}`,
		);
	}
	try {
		return JSON.parse(outcome.stdout.toString("utf8"));
	} catch (error) {
		throw new Error(`${label} returned invalid JSON: ${error}`);
	}
}

function admittedNativeSource(result, tier, workspaceRoot) {
	if (
		result?.mode !== "--derived-change-diagnostic-native" ||
		result?.tier !== tier ||
		result?.sourceUnchanged !== true
	) {
		throw new Error("native diagnostic result is incomplete");
	}
	requireAbsolutePath(result.admittedRootPath, "admitted native root");
	requireSha256(result.admittedRootSha256, "admitted native root");
	const expectedPath = join(workspaceRoot, "root-a");
	if (resolve(result.admittedRootPath) !== resolve(expectedPath)) {
		throw new Error(
			"native diagnostic admitted root escaped its tier workspace",
		);
	}
	return { path: result.admittedRootPath, sha256: result.admittedRootSha256 };
}

function outcomeActual(outcome) {
	return {
		exitCode: outcome.code,
		signal: outcome.signal,
		spawnError: outcome.spawnError ?? null,
		stderrSha256: sha256Text(outcome.stderr),
	};
}

export async function runDerivedChangeNativeDiagnostic(input) {
	const config = validateConfig(input);
	await Promise.all([
		requireEmptyRoot(config.caseRoot),
		requireEmptyRoot(config.workRoot),
	]);
	const requestRoot = join(config.caseRoot, "requests");
	const logRoot = join(config.caseRoot, "logs");
	await Promise.all([mkdir(requestRoot), mkdir(logRoot)]);
	const artifactPaths = [];
	const cases = [];
	const isolatedEnvironmentNames = new Set([
		"HOME",
		"PATH",
		"TEMP",
		"TMP",
		"TMPDIR",
		"USERPROFILE",
	]);
	const cleanEnvironment = {
		...Object.fromEntries(
			Object.entries(process.env).filter(([key]) => {
				const normalizedKey = key.toUpperCase();
				return (
					!normalizedKey.startsWith("POINTBREAK_") &&
					!isolatedEnvironmentNames.has(normalizedKey)
				);
			}),
		),
		HOME: config.workRoot,
		USERPROFILE: config.workRoot,
		TMPDIR: config.workRoot,
		TMP: config.workRoot,
		TEMP: config.workRoot,
		POINTBREAK_DIAGNOSTIC_CASE_ROOT: config.workRoot,
		POINTBREAK_DIAGNOSTIC_WORK_ROOT: config.workRoot,
		POINTBREAK_GIT_PROGRAM: config.gitProgram,
		PATH: dirname(config.gitProgram),
	};

	const contractOutcome = await runCommand(
		config.harness.program,
		[...config.harness.argsPrefix, "--derived-access-contract"],
		{ cwd: config.sourceCheckout, env: cleanEnvironment },
	);
	const contractArtifacts = await retainCommand(
		logRoot,
		"contract",
		contractOutcome,
	);
	artifactPaths.push(...contractArtifacts.map((path) => `logs/${path}`));
	const contract = parseJsonOutput(
		contractOutcome,
		"derived-access contract diagnostic",
	);
	if (
		contract?.contract?.schema !==
			"pointbreak.qualification-derived-access-contract.v1" ||
		!SHA256.test(contract?.contractSha256 ?? "")
	) {
		throw new Error("derived-access contract diagnostic identity is invalid");
	}
	const [binarySha256, cargoLockSha256] = await Promise.all([
		sha256File(config.harness.program),
		sha256File(join(config.sourceCheckout, "Cargo.lock")),
	]);

	for (const tier of TIERS) {
		const nativeId = `native-${tier}`;
		const nativeRoot = join(config.workRoot, `native-${tier}`);
		const nativeRequestPath = join(requestRoot, `native-${tier}.json`);
		const nativeArgs = commandArguments(
			config,
			"--derived-change-diagnostic-native",
			nativeRequestPath,
		);
		const nativeRequest = {
			schema: "pointbreak.qualification-derived-access-native-smoke-request.v1",
			sourceCheckout: config.sourceCheckout,
			workspaceRoot: nativeRoot,
			execution: executionIdentity(
				config,
				contract,
				binarySha256,
				cargoLockSha256,
				nativeArgs,
			),
			tier,
		};
		await writeFile(nativeRequestPath, `${JSON.stringify(nativeRequest)}\n`, {
			flag: "wx",
		});
		artifactPaths.push(normalRelativePath(config.caseRoot, nativeRequestPath));
		const nativeOutcome = await runCommand(config.harness.program, nativeArgs, {
			cwd: config.sourceCheckout,
			env: cleanEnvironment,
		});
		const nativeLogs = await retainCommand(
			logRoot,
			`native-${tier}`,
			nativeOutcome,
		);
		const nativeArtifacts = nativeLogs.map((path) => `logs/${path}`);
		artifactPaths.push(...nativeArtifacts);
		let admitted;
		try {
			admitted = admittedNativeSource(
				parseJsonOutput(nativeOutcome, `${tier} native diagnostic`),
				tier,
				nativeRoot,
			);
			cases.push(
				baseCase(
					nativeId,
					"passed",
					[],
					"native-materialization",
					tier,
					nativeArtifacts,
				),
			);
		} catch (error) {
			cases.push(
				failedCase(
					baseCase(
						nativeId,
						"failed",
						[],
						"native-materialization",
						tier,
						nativeArtifacts,
					),
					"lane_invalid",
					{ exitCode: 0, validNativeCollection: true },
					{ ...outcomeActual(nativeOutcome), detail: String(error) },
				),
			);
			const setupId = `lifecycle-${tier}-setup`;
			cases.push(
				skippedCase(
					baseCase(setupId, "skipped", [nativeId], "lifecycle-setup", tier, []),
					nativeId,
				),
			);
			for (const criterion of config.lifecycleCriteria) {
				cases.push(
					skippedCase(
						baseCase(
							`lifecycle-${tier}-${criterion}`,
							"skipped",
							[setupId],
							"lifecycle-vector",
							tier,
							[],
						),
						setupId,
					),
				);
			}
			continue;
		}

		const lifecycleRequestPath = join(requestRoot, `lifecycle-${tier}.json`);
		const lifecycleArgs = commandArguments(
			config,
			"--derived-access-lifecycle-diagnostic",
			lifecycleRequestPath,
		);
		const lifecycleRequest = {
			schema:
				"pointbreak.qualification-derived-access-lifecycle-run-request.v1",
			sourceCheckout: config.sourceCheckout,
			execution: executionIdentity(
				config,
				contract,
				binarySha256,
				cargoLockSha256,
				lifecycleArgs,
			),
			rootAuthoritySha256: config.rootAuthoritySha256,
			sourceRoot: admitted.path,
			workspaceRoot: join(config.workRoot, `lifecycle-${tier}`),
			admittedRootSha256: admitted.sha256,
			platform: config.platform.id,
			tier,
		};
		await writeFile(
			lifecycleRequestPath,
			`${JSON.stringify(lifecycleRequest)}\n`,
			{ flag: "wx" },
		);
		artifactPaths.push(
			normalRelativePath(config.caseRoot, lifecycleRequestPath),
		);
		const lifecycleOutcome = await runCommand(
			config.harness.program,
			lifecycleArgs,
			{ cwd: config.sourceCheckout, env: cleanEnvironment },
		);
		const lifecycleLogs = await retainCommand(
			logRoot,
			`lifecycle-${tier}`,
			lifecycleOutcome,
		);
		const lifecycleArtifacts = lifecycleLogs.map((path) => `logs/${path}`);
		artifactPaths.push(...lifecycleArtifacts);
		const setupId = `lifecycle-${tier}-setup`;
		let collection;
		try {
			collection = parseJsonOutput(
				lifecycleOutcome,
				`${tier} lifecycle diagnostic`,
			);
			if (
				collection?.sourceUnchanged !== true ||
				!Array.isArray(collection.cases)
			) {
				throw new Error("lifecycle diagnostic collection is incomplete");
			}
			const observedCriteria = collection.cases.map(
				({ criterion }) => criterion,
			);
			if (
				new Set(observedCriteria).size !== observedCriteria.length ||
				observedCriteria.some(
					(criterion) => !config.lifecycleCriteria.includes(criterion),
				) ||
				collection.cases.some(
					(caseRecord) =>
						!["passed", "failed"].includes(caseRecord.status) ||
						(caseRecord.status === "passed" &&
							caseRecord.failureDetail !== null) ||
						(caseRecord.status === "failed" &&
							(typeof caseRecord.failureDetail !== "string" ||
								caseRecord.failureDetail === "")),
				)
			) {
				throw new Error("lifecycle diagnostic criterion inventory is invalid");
			}
			cases.push(
				baseCase(
					setupId,
					"passed",
					[nativeId],
					"lifecycle-setup",
					tier,
					lifecycleArtifacts,
				),
			);
		} catch (error) {
			cases.push(
				failedCase(
					baseCase(
						setupId,
						"failed",
						[nativeId],
						"lifecycle-setup",
						tier,
						lifecycleArtifacts,
					),
					"lane_invalid",
					{ exitCode: 0, completeCaseInventory: true },
					{ ...outcomeActual(lifecycleOutcome), detail: String(error) },
				),
			);
			for (const criterion of config.lifecycleCriteria) {
				cases.push(
					skippedCase(
						baseCase(
							`lifecycle-${tier}-${criterion}`,
							"skipped",
							[setupId],
							"lifecycle-vector",
							tier,
							[],
						),
						setupId,
					),
				);
			}
			continue;
		}

		const byCriterion = new Map(
			collection.cases.map((caseRecord) => [caseRecord.criterion, caseRecord]),
		);
		for (const criterion of config.lifecycleCriteria) {
			const observed = byCriterion.get(criterion);
			const base = baseCase(
				`lifecycle-${tier}-${criterion}`,
				observed ? observed.status : "unknown",
				[setupId],
				"lifecycle-vector",
				tier,
				lifecycleArtifacts,
			);
			if (!observed) {
				cases.push({
					...base,
					attempted: false,
					unknownReason: "required lifecycle vector was absent",
				});
			} else if (observed.status === "failed") {
				cases.push(
					failedCase(
						base,
						"case_failure",
						{ status: "passed" },
						{
							status: observed.status,
							failureDetail: observed.failureDetail,
						},
					),
				);
			} else if (observed.status === "passed") {
				cases.push(base);
			} else {
				throw new Error(
					`unsupported lifecycle diagnostic status: ${observed.status}`,
				);
			}
		}
	}

	return {
		schema: DERIVED_CHANGE_DIAGNOSTIC_CASE_COLLECTION_SCHEMA_V1,
		campaignId: config.campaignId,
		cases,
		artifactPaths: [...new Set(artifactPaths)].sort(),
	};
}

if (
	process.argv[1] &&
	import.meta.url === pathToFileURL(process.argv[1]).href
) {
	const [configInput] = process.argv.slice(2);
	if (!configInput) {
		throw new Error(
			"usage: derived-change-diagnostic-native.mjs <config.json|--config-env>",
		);
	}
	const config =
		configInput === "--config-env"
			? JSON.parse(process.env.POINTBREAK_DERIVED_CHANGE_NATIVE_CONFIG ?? "")
			: JSON.parse(await readFile(configInput, "utf8"));
	if (configInput === "--config-env") {
		config.caseRoot = process.env.POINTBREAK_DIAGNOSTIC_CASE_ROOT;
		config.workRoot = process.env.POINTBREAK_DIAGNOSTIC_WORK_ROOT;
	}
	const result = await runDerivedChangeNativeDiagnostic(config);
	process.stdout.write(`${JSON.stringify(result)}\n`);
}
