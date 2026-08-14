import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { constants, createReadStream } from "node:fs";
import {
	access,
	appendFile,
	copyFile,
	lstat,
	mkdir,
	open,
	readdir,
	readFile,
	readlink,
	realpath,
} from "node:fs/promises";
import {
	basename,
	dirname,
	isAbsolute,
	join,
	parse,
	posix,
	relative,
	resolve,
	sep,
	win32,
} from "node:path";
import { isDeepStrictEqual } from "node:util";

import {
	DERIVED_CHANGE_DIAGNOSTIC_FRAGMENT_SCHEMA_V1,
	DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1,
	finalizeDerivedChangeDiagnosticFragment,
	validateDerivedChangeDiagnosticCampaign,
} from "./derived-change-diagnostic-report.mjs";

export { DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1 } from "./derived-change-diagnostic-report.mjs";

export const DERIVED_CHANGE_DIAGNOSTIC_REQUEST_SCHEMA_V1 =
	"pointbreak.derived-change-diagnostic-request.v1";
const FAILURE_CLASSES = new Set([
	"global_invalid",
	"lane_invalid",
	"case_failure",
]);
const RESERVED_OUTPUTS = new Set([
	"manifest.json",
	"package.json",
	"evaluation.json",
	"receipt.json",
]);
const OWNER_STORE_ENV = new Set([
	"POINTBREAK_HOME",
	"POINTBREAK_STORE",
	"POINTBREAK_QUALIFICATION_CORPUS",
	"POINTBREAK_DERIVED_ACCESS",
	"POINTBREAK_CHANGE_READY_FIXTURE_DIR",
]);
const OWNER_STORE_COMPONENTS = new Set([
	".git",
	".pointbreak",
	"pointbreak-home",
	"pointbreak_home",
	"pointbreak-store",
]);
export const DERIVED_CHANGE_DIAGNOSTIC_COLLECTION_SCHEMA_V1 =
	"pointbreak.derived-change-diagnostic-collection.v1";

function isPortableAbsolutePath(value) {
	return (
		typeof value === "string" &&
		(isAbsolute(value) ||
			/^[A-Za-z]:[\\/]/u.test(value) ||
			/^\\\\[^\\]+\\[^\\]+/u.test(value))
	);
}

function ownerStoreEnvironmentEntries(environment = process.env) {
	return Object.entries(environment).filter(([key]) =>
		OWNER_STORE_ENV.has(key.toUpperCase()),
	);
}

function requireObject(value, label) {
	if (value === null || typeof value !== "object" || Array.isArray(value))
		throw new Error(`${label} must be an object`);
}
function requireText(value, label) {
	if (typeof value !== "string" || !value.trim())
		throw new Error(`${label} must be non-empty text`);
}
function requireNormalRelativePath(value, label) {
	requireText(value, label);
	if (
		isAbsolute(value) ||
		value.split(/[\\/]/u).some((part) => !part || part === "." || part === "..")
	)
		throw new Error(`${label} must be a normal relative path`);
	if (RESERVED_OUTPUTS.has(value.split(/[\\/]/u).at(-1).toLowerCase()))
		throw new Error(`${label} uses a terminal evidence filename`);
}

function platformFor(request) {
	const platform = request.campaign.platforms.find(
		(candidate) => candidate.id === request.platformId,
	);
	if (!platform)
		throw new Error("diagnostic request platform is not campaign-bound");
	return platform;
}

function validateFixtureCheckpoint(value, label) {
	requireObject(value, `${label} fixture checkpoint`);
	requireText(value.fixture, `${label} fixture`);
	requireText(value.checkpoint, `${label} checkpoint`);
}

function validateCollection(value, label) {
	requireObject(value, `${label} collection`);
	if (value.schema !== DERIVED_CHANGE_DIAGNOSTIC_COLLECTION_SCHEMA_V1)
		throw new Error(`${label} collection schema is unsupported`);
	if (value.source !== "stdout" && value.source !== "artifact")
		throw new Error(`${label} collection source must be stdout or artifact`);
	if (value.source === "artifact")
		requireNormalRelativePath(
			value.artifactPath,
			`${label} collection artifact path`,
		);
	if (value.idPrefix !== undefined)
		requireText(value.idPrefix, `${label} collection ID prefix`);
	if (
		value.completeExitCodes !== undefined &&
		(!Array.isArray(value.completeExitCodes) ||
			value.completeExitCodes.some((code) => !Number.isInteger(code)))
	)
		throw new Error(`${label} collection complete exit codes must be integers`);
	if (
		!Array.isArray(value.expectedCaseIds) ||
		value.expectedCaseIds.length === 0 ||
		value.expectedCaseIds.some((id) => typeof id !== "string" || !id.trim()) ||
		JSON.stringify(value.expectedCaseIds) !==
			JSON.stringify([...value.expectedCaseIds].sort()) ||
		new Set(value.expectedCaseIds).size !== value.expectedCaseIds.length
	) {
		throw new Error(
			`${label} collection expected case inventory must be sorted and unique`,
		);
	}
}

function validateSourcePreflight(value) {
	if (value === undefined) return;
	requireObject(value, "diagnostic source preflight");
	if (!isAbsolute(value.sourceRoot))
		throw new Error("diagnostic source preflight root must be absolute");
	if (!isPortableAbsolutePath(value.gitProgram))
		throw new Error("diagnostic source preflight Git program must be absolute");
	if (!isPortableAbsolutePath(value.gitExecPath))
		throw new Error(
			"diagnostic source preflight Git exec path must be absolute",
		);
	if (!isPortableAbsolutePath(value.sshKeygenProgram))
		throw new Error(
			"diagnostic source preflight ssh-keygen program must be absolute",
		);
	if (!/^[0-9a-f]{64}$/u.test(value.allowedSignersSha256 ?? ""))
		throw new Error(
			"diagnostic source preflight allowed signers SHA-256 is invalid",
		);
	if (
		value.allowedSignersPath !== undefined &&
		!isAbsolute(value.allowedSignersPath)
	)
		throw new Error(
			"diagnostic source preflight allowed signers path must be absolute",
		);
}

function validateIdentityPaths(value) {
	requireObject(value, "diagnostic identity paths");
	for (const name of [
		"product",
		"harness",
		"control",
		"controlCli",
		"fixtureAuthority",
	])
		if (!isAbsolute(value[name]))
			throw new Error(`diagnostic ${name} identity path must be absolute`);
}

function isWithin(candidate, parent) {
	const relation = relative(resolve(parent), resolve(candidate));
	return (
		relation === "" ||
		(!isAbsolute(relation) &&
			relation !== ".." &&
			!relation.startsWith(`..${sep}`))
	);
}

function pathComponents(path) {
	const absolute = resolve(path);
	return absolute
		.slice(parse(absolute).root.length)
		.split(/[\\/]/u)
		.filter(Boolean);
}

function pathsOverlap(left, right) {
	return isWithin(left, right) || isWithin(right, left);
}

async function assertNoSymlinkTraversal(path, label) {
	let current = parse(resolve(path)).root;
	for (const component of pathComponents(path)) {
		current = join(current, component);
		if ((await lstat(current)).isSymbolicLink())
			throw new Error(`${label} must not traverse symbolic links`);
	}
}

async function canonicalPath(path, label) {
	let existing = resolve(path);
	while (true) {
		try {
			await lstat(existing);
			break;
		} catch (error) {
			if (error?.code !== "ENOENT") throw error;
			const parent = dirname(existing);
			if (parent === existing)
				throw new Error(`${label} has no existing ancestor`);
			existing = parent;
		}
	}
	return resolve(await realpath(existing), relative(existing, resolve(path)));
}

function validateTemporaryRootShape(temporaryRoot, outputRoot, sourceRoot) {
	if (typeof temporaryRoot !== "string" || !isAbsolute(temporaryRoot))
		throw new Error("diagnostic temporary root must be absolute");
	if (pathsOverlap(temporaryRoot, outputRoot))
		throw new Error(
			"diagnostic temporary root must be disjoint from the diagnostic output root",
		);
	if (pathsOverlap(temporaryRoot, sourceRoot))
		throw new Error(
			"diagnostic temporary root must be disjoint from the source checkout",
		);
	const components = pathComponents(temporaryRoot).map((component) =>
		component.toLowerCase(),
	);
	if (
		components.includes(DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1) ||
		components.some((component) => OWNER_STORE_COMPONENTS.has(component))
	)
		throw new Error(
			"diagnostic temporary root cannot enter a diagnostic or owner-store component",
		);
}

async function assertDerivedChangeDiagnosticTemporaryRootIdentity(
	temporaryRoot,
	outputRoot,
	sourceRoot,
) {
	validateTemporaryRootShape(temporaryRoot, outputRoot, sourceRoot);
	const temporaryStat = await lstat(temporaryRoot).catch((error) => {
		if (error?.code === "ENOENT")
			throw new Error("diagnostic temporary root must already exist");
		throw error;
	});
	if (temporaryStat.isSymbolicLink())
		throw new Error(
			"diagnostic temporary root must not traverse symbolic links",
		);
	if (!temporaryStat.isDirectory())
		throw new Error(
			"diagnostic temporary root must be an empty real directory",
		);
	await assertNoSymlinkTraversal(temporaryRoot, "diagnostic temporary root");
	const [canonicalTemporary, canonicalOutput, canonicalSource] =
		await Promise.all([
			realpath(temporaryRoot),
			canonicalPath(outputRoot, "diagnostic output root"),
			realpath(sourceRoot),
		]);
	if (pathsOverlap(canonicalTemporary, canonicalOutput))
		throw new Error(
			"diagnostic temporary root must be disjoint from the diagnostic output root",
		);
	if (pathsOverlap(canonicalTemporary, canonicalSource))
		throw new Error(
			"diagnostic temporary root must be disjoint from the source checkout",
		);
	for (const [key, ownerPath] of ownerStoreEnvironmentEntries()) {
		if (!ownerPath || !isAbsolute(ownerPath)) continue;
		const canonicalOwner = await realpath(ownerPath).catch(() =>
			resolve(ownerPath),
		);
		if (pathsOverlap(canonicalTemporary, canonicalOwner))
			throw new Error(`diagnostic temporary root cannot enter ${key}`);
	}
}

async function assertDerivedChangeDiagnosticTemporaryRootSafety(
	temporaryRoot,
	outputRoot,
	sourceRoot,
) {
	await assertDerivedChangeDiagnosticTemporaryRootIdentity(
		temporaryRoot,
		outputRoot,
		sourceRoot,
	);
	if ((await readdir(temporaryRoot)).length)
		throw new Error("diagnostic temporary root must be empty");
}

export async function assertDerivedChangeDiagnosticOutputRootSafety(
	outputRoot,
	sourceRoot,
) {
	if (
		!isAbsolute(outputRoot) ||
		basename(resolve(outputRoot)) !==
			DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1
	) {
		throw new Error(
			`diagnostic output root must end in ${DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1}`,
		);
	}
	if (!isAbsolute(sourceRoot))
		throw new Error("diagnostic source root must be absolute");
	const absoluteRoot = resolve(outputRoot);
	const parsedRoot = parse(absoluteRoot).root;
	const components = absoluteRoot
		.slice(parsedRoot.length)
		.split(/[\\/]/u)
		.filter(Boolean);
	if (
		components.some((component) =>
			OWNER_STORE_COMPONENTS.has(component.toLowerCase()),
		)
	) {
		throw new Error(
			"diagnostic output root cannot enter a source or owner-store component",
		);
	}
	for (const [key, ownerPath] of ownerStoreEnvironmentEntries()) {
		if (ownerPath && isAbsolute(ownerPath) && isWithin(outputRoot, ownerPath)) {
			throw new Error(`diagnostic output root cannot enter ${key}`);
		}
	}
	let existing = absoluteRoot;
	let existingStat;
	while (true) {
		try {
			existingStat = await lstat(existing);
			break;
		} catch (error) {
			if (error?.code !== "ENOENT") throw error;
			const parent = dirname(existing);
			if (parent === existing)
				throw new Error("diagnostic output root has no existing ancestor");
			existing = parent;
		}
	}
	if (existing === absoluteRoot && existingStat.isSymbolicLink()) {
		throw new Error("diagnostic output root must not be a symbolic link");
	}
	const [canonicalSource, canonicalExisting] = await Promise.all([
		realpath(sourceRoot),
		realpath(existing),
	]);
	const canonicalRoot = resolve(
		canonicalExisting,
		relative(existing, absoluteRoot),
	);
	const canonicalComponents = canonicalRoot
		.slice(parse(canonicalRoot).root.length)
		.split(/[\\/]/u)
		.filter(Boolean);
	if (
		canonicalComponents.some((component) =>
			OWNER_STORE_COMPONENTS.has(component.toLowerCase()),
		)
	) {
		throw new Error(
			"diagnostic output root cannot resolve into an owner-store component",
		);
	}
	for (const [key, ownerPath] of ownerStoreEnvironmentEntries()) {
		if (!ownerPath || !isAbsolute(ownerPath)) continue;
		const canonicalOwner = await realpath(ownerPath).catch(() =>
			resolve(ownerPath),
		);
		if (isWithin(canonicalRoot, canonicalOwner)) {
			throw new Error(`diagnostic output root cannot enter ${key}`);
		}
	}
	if (isWithin(canonicalRoot, canonicalSource)) {
		throw new Error(
			"diagnostic output root must resolve outside the source checkout",
		);
	}
}

function validateRequiredExecutables(value) {
	if (!Array.isArray(value) || value.length === 0)
		throw new Error(
			"diagnostic required executables must be a non-empty array",
		);
	if (
		value.some((program) => !isPortableAbsolutePath(program)) ||
		new Set(value).size !== value.length
	) {
		throw new Error(
			"diagnostic required executables must be absolute and unique",
		);
	}
}

export function validateDerivedChangeDiagnosticRequest(request) {
	requireObject(request, "diagnostic request");
	if (request.schema !== DERIVED_CHANGE_DIAGNOSTIC_REQUEST_SCHEMA_V1)
		throw new Error("unsupported derived Change diagnostic request schema");
	validateDerivedChangeDiagnosticCampaign(request.campaign);
	platformFor(request);
	if (
		!isAbsolute(request.outputRoot) ||
		basename(resolve(request.outputRoot)) !==
			DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1
	)
		throw new Error(
			`diagnostic output root must end in ${DERIVED_CHANGE_DIAGNOSTIC_ROOT_COMPONENT_V1}`,
		);
	validateSourcePreflight(request.sourcePreflight);
	validateIdentityPaths(request.identityPaths);
	validateRequiredExecutables(request.requiredExecutables);
	const campaignPrograms = new Set(
		request.campaign.programs
			.filter(({ platformId }) => platformId === request.platformId)
			.map(({ program }) => program),
	);
	if (request.sourcePreflight) {
		const platform = platformFor(request);
		const pathModule =
			platform.operatingSystem === "windows" ? win32 : posix;
		const programByName = new Map(
			request.campaign.programs
				.filter(({ platformId }) => platformId === request.platformId)
				.map((identity) => [identity.name, identity]),
		);
		const git = programByName.get("git");
		const expectedGitExecPath =
			platform.operatingSystem === "windows"
				? pathModule.join(
						git.treeRoot,
						platform.architecture === "aarch64"
							? "clangarm64"
							: "mingw64",
						"libexec",
						"git-core",
					)
				: pathModule.join(git.treeRoot, "libexec", "git-core");
		if (
			request.sourcePreflight.gitProgram !== git.program ||
			request.sourcePreflight.gitExecPath !== expectedGitExecPath ||
			request.sourcePreflight.sshKeygenProgram !==
				programByName.get("sshKeygen").program ||
			request.sourcePreflight.allowedSignersSha256 !==
				request.campaign.signatureAuthoritySha256
		) {
			throw new Error(
				"diagnostic source preflight tools differ from campaign authority",
			);
		}
		const outputRelation = relative(
			resolve(request.sourcePreflight.sourceRoot),
			resolve(request.outputRoot),
		);
		if (
			outputRelation === "" ||
			(outputRelation !== ".." && !outputRelation.startsWith(`..${sep}`))
		)
			throw new Error(
				"diagnostic output root must be outside the source checkout",
			);
	}
	validateTemporaryRootShape(
		request.temporaryRoot,
		request.outputRoot,
		request.sourcePreflight?.sourceRoot ?? process.cwd(),
	);
	if (!Array.isArray(request.cases) || !request.cases.length)
		throw new Error("diagnostic request requires cases");
	const ids = new Set();
	const mutableRoots = new Set();
	const indices = new Map();
	for (const [index, caseRequest] of request.cases.entries()) {
		requireObject(caseRequest, "diagnostic case request");
		requireText(caseRequest.id, "diagnostic case id");
		requireText(caseRequest.lane, `diagnostic case ${caseRequest.id} lane`);
		if (ids.has(caseRequest.id))
			throw new Error(`duplicate diagnostic case id: ${caseRequest.id}`);
		ids.add(caseRequest.id);
		indices.set(caseRequest.id, index);
		if (typeof caseRequest.required !== "boolean")
			throw new Error(
				`diagnostic case ${caseRequest.id} required must be boolean`,
			);
		if (
			!Array.isArray(caseRequest.dependsOn) ||
			caseRequest.dependsOn.some(
				(dependency) => typeof dependency !== "string" || !dependency.trim(),
			) ||
			new Set(caseRequest.dependsOn).size !== caseRequest.dependsOn.length
		)
			throw new Error(
				`diagnostic case ${caseRequest.id} dependencies are invalid`,
			);
		if (caseRequest.unavailableReason || caseRequest.unknownReason) {
			if (
				caseRequest.program !== undefined ||
				caseRequest.failureClass !== undefined ||
				caseRequest.alwaysAttempt !== undefined
			)
				throw new Error(
					`unattempted diagnostic case ${caseRequest.id} cannot name a command or failure class`,
				);
			continue;
		}
		if (!FAILURE_CLASSES.has(caseRequest.failureClass))
			throw new Error(
				`diagnostic case ${caseRequest.id} has an invalid failure class`,
			);
		requireText(caseRequest.phase, `diagnostic case ${caseRequest.id} phase`);
		if (
			caseRequest.alwaysAttempt !== undefined &&
			typeof caseRequest.alwaysAttempt !== "boolean"
		) {
			throw new Error(
				`diagnostic case ${caseRequest.id} always-attempt flag must be boolean`,
			);
		}
		if (
			caseRequest.alwaysAttempt &&
			(caseRequest.failureClass !== "global_invalid" ||
				index !== request.cases.length - 1)
		) {
			throw new Error(
				"diagnostic always-attempt case must be the final global-invalid case",
			);
		}
		validateFixtureCheckpoint(
			caseRequest.fixtureCheckpoint,
			`diagnostic case ${caseRequest.id}`,
		);
		if (!isPortableAbsolutePath(caseRequest.program))
			throw new Error(
				`diagnostic case ${caseRequest.id} program must be an absolute executable path`,
			);
		if (!campaignPrograms.has(caseRequest.program)) {
			throw new Error(
				`diagnostic case ${caseRequest.id} uses a program absent from campaign authority`,
			);
		}
		if (
			!Array.isArray(caseRequest.args) ||
			caseRequest.args.some((arg) => typeof arg !== "string")
		)
			throw new Error(
				`diagnostic case ${caseRequest.id} arguments must be strings`,
			);
		if (caseRequest.cwd !== undefined && !isAbsolute(caseRequest.cwd))
			throw new Error(
				`diagnostic case ${caseRequest.id} working directory must be absolute`,
			);
		if (caseRequest.env !== undefined) {
			requireObject(
				caseRequest.env,
				`diagnostic case ${caseRequest.id} environment`,
			);
			if (
				Object.entries(caseRequest.env).some(
					([key, value]) =>
						typeof value !== "string" || OWNER_STORE_ENV.has(key.toUpperCase()),
				)
			)
				throw new Error(
					`diagnostic case ${caseRequest.id} environment cannot supply owner-store state`,
				);
		}
		const root = caseRequest.root ?? caseRequest.id;
		requireNormalRelativePath(root, `diagnostic case ${caseRequest.id} root`);
		if (caseRequest.mutatesRoot) {
			if (mutableRoots.has(root))
				throw new Error(`duplicate mutable case root: ${root}`);
			mutableRoots.add(root);
		}
		for (const path of caseRequest.artifactPaths ?? [])
			requireNormalRelativePath(
				path,
				`diagnostic case ${caseRequest.id} artifact path`,
			);
		if (caseRequest.collection)
			validateCollection(
				caseRequest.collection,
				`diagnostic case ${caseRequest.id}`,
			);
	}
	for (const caseRequest of request.cases) {
		for (const dependency of caseRequest.dependsOn) {
			if (!ids.has(dependency))
				throw new Error(
					`diagnostic case ${caseRequest.id} has unknown dependency ${dependency}`,
				);
			if (indices.get(dependency) >= indices.get(caseRequest.id))
				throw new Error(
					`diagnostic case ${caseRequest.id} dependencies must be topologically ordered`,
				);
		}
		if (
			caseRequest.required &&
			!request.campaign.requiredCaseIds.includes(caseRequest.id)
		)
			throw new Error(
				`required diagnostic case ${caseRequest.id} is absent from campaign authority`,
			);
	}
	const expandedIds = new Set(ids);
	for (const caseRequest of request.cases) {
		if (!caseRequest.collection) continue;
		for (const childId of caseRequest.collection.expectedCaseIds) {
			const expandedId = `${caseRequest.collection.idPrefix ?? ""}${childId}`;
			if (expandedIds.has(expandedId))
				throw new Error(
					`duplicate diagnostic collection case id: ${expandedId}`,
				);
			expandedIds.add(expandedId);
		}
	}
	return request;
}

async function sha256File(path) {
	const hash = createHash("sha256");
	await new Promise((done, fail) => {
		const stream = createReadStream(path);
		stream.on("data", (chunk) => hash.update(chunk));
		stream.on("error", fail);
		stream.on("end", done);
	});
	return hash.digest("hex");
}

export async function sha256DerivedChangeDiagnosticTree(root) {
	if (!isAbsolute(root)) {
		throw new Error("diagnostic program tree root must be absolute");
	}
	const absoluteRoot = resolve(root);
	const rootStat = await lstat(absoluteRoot);
	if (rootStat.isSymbolicLink() || !rootStat.isDirectory()) {
		throw new Error("diagnostic program tree root must be a real directory");
	}
	const entries = [];
	const visit = async (directory, relativeDirectory = "") => {
		const children = (await readdir(directory, { withFileTypes: true })).sort(
			(left, right) =>
				left.name < right.name ? -1 : left.name > right.name ? 1 : 0,
		);
		for (const child of children) {
			const absolutePath = join(directory, child.name);
			const relativePath = relativeDirectory
				? `${relativeDirectory}/${child.name}`
				: child.name;
			const stat = await lstat(absolutePath);
			const mode = stat.mode & 0o777;
			if (stat.isDirectory()) {
				entries.push(["directory", relativePath, mode]);
				await visit(absolutePath, relativePath);
				continue;
			}
			if (stat.isFile()) {
				entries.push([
					"file",
					relativePath,
					mode,
					stat.size,
					await sha256File(absolutePath),
				]);
				continue;
			}
			if (stat.isSymbolicLink()) {
				const target = await readlink(absolutePath);
				const resolvedTarget = resolve(directory, target);
				if (isAbsolute(target) || !isWithin(resolvedTarget, absoluteRoot)) {
					throw new Error(
						`diagnostic program tree symlink escapes its root: ${relativePath}`,
					);
				}
				entries.push(["symlink", relativePath, mode, target]);
				continue;
			}
			throw new Error(
				`diagnostic program tree contains unsupported entry: ${relativePath}`,
			);
		}
	};
	await visit(absoluteRoot);
	return createHash("sha256")
		.update(
			JSON.stringify({
				schema: "pointbreak.derived-change-diagnostic-program-tree.v1",
				rootMode: rootStat.mode & 0o777,
				entries,
			}),
		)
		.digest("hex");
}

async function requireRetainedFile(path, root, label) {
	const absoluteRoot = resolve(root);
	const absolutePath = resolve(path);
	const relation = relative(absoluteRoot, absolutePath);
	if (
		!relation ||
		isAbsolute(relation) ||
		relation === ".." ||
		relation.startsWith(`..${sep}`)
	)
		throw new Error(`${label} escapes the diagnostic output root`);
	let current = absoluteRoot;
	let stat;
	for (const component of relation.split(sep)) {
		current = join(current, component);
		stat = await lstat(current);
		if (stat.isSymbolicLink())
			throw new Error(`${label} traverses a symbolic link`);
	}
	const [canonicalRoot, canonicalPath] = await Promise.all([
		realpath(absoluteRoot),
		realpath(absolutePath),
	]);
	if (!isWithin(canonicalPath, canonicalRoot))
		throw new Error(`${label} escapes the diagnostic output root`);
	if (!stat?.isFile())
		throw new Error(`${label} is not a retained regular file`);
	return {
		path: relation.split(sep).join("/"),
		sha256: await sha256File(absolutePath),
	};
}

function retainArtifact(artifacts, artifact) {
	const existing = artifacts.find(({ path }) => path === artifact.path);
	if (existing && existing.sha256 !== artifact.sha256)
		throw new Error(
			`diagnostic artifact changed while retained: ${artifact.path}`,
		);
	if (!existing) artifacts.push(artifact);
	return artifact;
}

function isIsolatedRustEnvironmentName(normalizedKey) {
	return (
		normalizedKey === "CARGO" ||
		normalizedKey.startsWith("CARGO_") ||
		normalizedKey === "NEXTEST" ||
		normalizedKey.startsWith("NEXTEST_") ||
		normalizedKey.startsWith("RUST")
	);
}

function isAmbientToolEnvironmentName(normalizedKey) {
	const exactNames = new Set([
		"AR",
		"BASH",
		"CC",
		"CFLAGS",
		"CL",
		"COMSPEC",
		"CPATH",
		"CPPFLAGS",
		"CXX",
		"CXXFLAGS",
		"C_INCLUDE_PATH",
		"CPLUS_INCLUDE_PATH",
		"DEVELOPER_DIR",
		"DYLD_LIBRARY_PATH",
		"INCLUDE",
		"LD",
		"LDFLAGS",
		"LD_LIBRARY_PATH",
		"LD_PRELOAD",
		"LIB",
		"LIBPATH",
		"LIBRARY_PATH",
		"LINK",
		"LLVM_CONFIG_PATH",
		"MACOSX_DEPLOYMENT_TARGET",
		"NODE_OPTIONS",
		"NODE_PATH",
		"PATH",
		"RANLIB",
		"SDKROOT",
		"SHELL",
		"UNIVERSALCRTSDKDIR",
		"UCRTVERSION",
		"VCINSTALLDIR",
		"VCTOOLSINSTALLDIR",
		"VSINSTALLDIR",
		"WINDOWSSDKDIR",
		"WINDOWSSDKVERSION",
		"_CL_",
	]);
	return (
		exactNames.has(normalizedKey) ||
		/^(HOST|TARGET)_(AR|CC|CXX|RANLIB)$/u.test(normalizedKey) ||
		normalizedKey.startsWith("AR_") ||
		normalizedKey.startsWith("BINDGEN_") ||
		normalizedKey.startsWith("CC_") ||
		normalizedKey.startsWith("CMAKE_") ||
		normalizedKey.startsWith("CXX_") ||
		normalizedKey.startsWith("DYLD_") ||
		normalizedKey.startsWith("GIT_") ||
		normalizedKey.startsWith("MAKE_") ||
		normalizedKey.startsWith("MESON_") ||
		normalizedKey.startsWith("NINJA_") ||
		normalizedKey.startsWith("NPM_CONFIG_") ||
		normalizedKey.startsWith("OPENSSL_") ||
		normalizedKey.startsWith("PKG_CONFIG_") ||
		normalizedKey.startsWith("PLAYWRIGHT_") ||
		normalizedKey.startsWith("RANLIB_")
	);
}

function sanitizedEnvironment(caseRequest, caseRoot, workRoot) {
	const environment = {};
	const isolatedNames = new Set([
		"APPDATA",
		"HOME",
		"LOCALAPPDATA",
		"NPM_CONFIG_CACHE",
		"POINTBREAK_DIAGNOSTIC_CASE_ROOT",
		"POINTBREAK_DIAGNOSTIC_WORK_ROOT",
		"TEMP",
		"TMP",
		"TMPDIR",
		"USERPROFILE",
		"XDG_CACHE_HOME",
		"XDG_CONFIG_HOME",
		"XDG_DATA_HOME",
		"XDG_STATE_HOME",
	]);
	for (const [key, value] of Object.entries(process.env)) {
		const normalizedKey = key.toUpperCase();
		if (
			normalizedKey.startsWith("POINTBREAK_") ||
			isolatedNames.has(normalizedKey) ||
			isIsolatedRustEnvironmentName(normalizedKey) ||
			isAmbientToolEnvironmentName(normalizedKey)
		)
			continue;
		environment[key] = value;
	}
	for (const [key, value] of Object.entries(caseRequest.env ?? {})) {
		const normalizedKey = key.toUpperCase();
		if (OWNER_STORE_ENV.has(normalizedKey) || isolatedNames.has(normalizedKey))
			continue;
		environment[key] = value;
	}
	return Object.assign(environment, {
		PATH: environment.PATH ?? "",
		HOME: workRoot,
		USERPROFILE: workRoot,
		APPDATA: join(workRoot, "app-data"),
		LOCALAPPDATA: join(workRoot, "local-app-data"),
		XDG_CACHE_HOME: join(workRoot, "xdg-cache"),
		XDG_CONFIG_HOME: join(workRoot, "xdg-config"),
		XDG_DATA_HOME: join(workRoot, "xdg-data"),
		XDG_STATE_HOME: join(workRoot, "xdg-state"),
		npm_config_cache: join(workRoot, "npm-cache"),
		CARGO_HOME: join(workRoot, "cargo-home"),
		RUSTUP_HOME: join(workRoot, "rustup-home"),
		POINTBREAK_DIAGNOSTIC_CASE_ROOT: caseRoot,
		POINTBREAK_DIAGNOSTIC_WORK_ROOT: workRoot,
		TMPDIR: workRoot,
		TMP: workRoot,
		TEMP: workRoot,
		CARGO_TARGET_DIR: join(workRoot, "target"),
	});
}

async function runCommand(
	caseRequest,
	caseRoot,
	workRoot,
	stdoutPath,
	stderrPath,
) {
	const stdout = await open(stdoutPath, "wx");
	const stderr = await open(stderrPath, "wx");
	try {
		return await new Promise((done, fail) => {
			const child = spawn(caseRequest.program, caseRequest.args, {
				cwd: caseRequest.cwd,
				env: sanitizedEnvironment(caseRequest, caseRoot, workRoot),
				stdio: ["ignore", stdout.fd, stderr.fd],
			});
			child.once("error", fail);
			child.once("exit", (code, signal) => done({ code, signal }));
		});
	} finally {
		await Promise.all([stdout.close(), stderr.close()]);
	}
}

async function rootMustBeEmpty(root) {
	try {
		if ((await readdir(root)).length)
			throw new Error("diagnostic output root must be empty");
	} catch (error) {
		if (error?.code !== "ENOENT") throw error;
		await mkdir(root, { recursive: true });
	}
}

async function assertRootEmptyOrAbsent(root, label) {
	try {
		if ((await readdir(root)).length) throw new Error(`${label} must be empty`);
	} catch (error) {
		if (error?.code !== "ENOENT") throw error;
	}
}

async function commandResult(program, args, options) {
	return await new Promise((done, fail) => {
		const child = spawn(program, args, {
			...options,
			stdio: ["ignore", "pipe", "pipe"],
		});
		const stdout = [];
		const stderr = [];
		child.stdout.on("data", (chunk) => stdout.push(chunk));
		child.stderr.on("data", (chunk) => stderr.push(chunk));
		child.once("error", fail);
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

export async function executeDerivedChangeDiagnosticReadinessCase(
	caseRequest,
	caseRoot,
	workRoot,
) {
	await Promise.all([
		assertRootEmptyOrAbsent(caseRoot, "diagnostic readiness case root"),
		assertRootEmptyOrAbsent(workRoot, "diagnostic readiness work root"),
	]);
	await Promise.all([
		mkdir(caseRoot, { recursive: true }),
		mkdir(workRoot, { recursive: true }),
	]);
	return await commandResult(caseRequest.program, caseRequest.args, {
		cwd: caseRequest.cwd,
		env: sanitizedEnvironment(caseRequest, caseRoot, workRoot),
	});
}

async function verifySourcePreflight(request) {
	const preflight = request.sourcePreflight;
	if (!preflight) return null;
	const git = preflight.gitProgram;
	const source = request.campaign.source;
	try {
		if (preflight.allowedSignersPath !== undefined) {
			const stat = await lstat(preflight.allowedSignersPath);
			if (stat.isSymbolicLink() || !stat.isFile())
				throw new Error("allowed signers authority is not a regular file");
			if (
				(await sha256File(preflight.allowedSignersPath)) !==
				preflight.allowedSignersSha256
			) {
				throw new Error("allowed signers authority differs from campaign");
			}
		}
		const environment = {
			GIT_CONFIG_GLOBAL: "/dev/null",
			GIT_CONFIG_NOSYSTEM: "1",
			GIT_EXEC_PATH: preflight.gitExecPath,
			GIT_TERMINAL_PROMPT: "0",
			HOME: request.temporaryRoot,
			LANG: "C",
			LC_ALL: "C",
			PATH: "",
			USERPROFILE: request.temporaryRoot,
			...(process.env.SystemRoot
				? { SystemRoot: process.env.SystemRoot }
				: {}),
			...(process.env.SYSTEMROOT
				? { SYSTEMROOT: process.env.SYSTEMROOT }
				: {}),
		};
		const options = { env: environment };
		const common = ["-C", preflight.sourceRoot];
		const headAtStart = await commandResult(
			git,
			[...common, "rev-parse", "HEAD"],
			options,
		);
		const porcelain = await commandResult(
			git,
			[...common, "status", "--porcelain=v1", "--untracked-files=all"],
			options,
		);
		const head = await commandResult(
			git,
			[...common, "rev-parse", `${source.commit}^{commit}`],
			options,
		);
		const tree = await commandResult(
			git,
			[...common, "rev-parse", `${source.commit}^{tree}`],
			options,
		);
		const range = await commandResult(
			git,
			[...common, "diff", "--binary", source.rangeBaseCommit, source.commit],
			options,
		);
		const verifyArgs =
			preflight.allowedSignersPath === undefined
				? [
						"-c",
						`gpg.ssh.program=${preflight.sshKeygenProgram}`,
						...common,
						"verify-commit",
						source.commit,
					]
				: [
						"-c",
						`gpg.ssh.allowedSignersFile=${preflight.allowedSignersPath}`,
						"-c",
						`gpg.ssh.program=${preflight.sshKeygenProgram}`,
						...common,
						"verify-commit",
						source.commit,
					];
		const signature = await commandResult(git, verifyArgs, options);
		const fixtureSourceResults = await Promise.all(
			request.campaign.fixture.document.sourceFiles.map(async (entry) => ({
				...entry,
				actualSha256: await sha256File(join(preflight.sourceRoot, entry.path)),
			})),
		);
		if (
			headAtStart.code !== 0 ||
			headAtStart.stdout.toString("utf8").trim() !== source.commit ||
			porcelain.code !== 0 ||
			porcelain.stdout.length !== 0 ||
			head.code !== 0 ||
			head.stdout.toString("utf8").trim() !== source.commit ||
			tree.code !== 0 ||
			tree.stdout.toString("utf8").trim() !== source.tree ||
			range.code !== 0 ||
			createHash("sha256").update(range.stdout).digest("hex") !==
				source.rangeSha256 ||
			signature.code !== 0 ||
			fixtureSourceResults.some(
				({ sha256, actualSha256 }) => sha256 !== actualSha256,
			)
		)
			throw new Error(
				"source identity, cleanliness, or signature verification failed",
			);
		return null;
	} catch (error) {
		return String(error);
	}
}

async function verifyBoundIdentities(request) {
	const expected = (name, role) => {
		const binary = request.campaign[name].binaries.find(
			(candidate) =>
				candidate.platformId === request.platformId &&
				(role === undefined || candidate.role === role),
		);
		if (!binary)
			throw new Error(`diagnostic ${name} binary identity is absent`);
		return binary.binarySha256;
	};
	const checks = [
		["product", request.identityPaths.product, expected("product")],
		["harness", request.identityPaths.harness, expected("harness")],
		[
			"library control",
			request.identityPaths.control,
			expected("control", "library"),
		],
		[
			"CLI control",
			request.identityPaths.controlCli,
			expected("control", "cli"),
		],
		[
			"fixture authority",
			request.identityPaths.fixtureAuthority,
			request.campaign.fixture.authoritySha256,
		],
	];
	const failures = [];
	for (const [name, path, digest] of checks) {
		try {
			const stat = await lstat(path);
			if (
				stat.isSymbolicLink() ||
				!stat.isFile() ||
				(await sha256File(path)) !== digest
			)
				throw new Error("not an exact regular-file identity");
		} catch (error) {
			failures.push({ name, path, error: String(error) });
		}
	}
	try {
		const fixtureAuthorityDocument = JSON.parse(
			await readFile(request.identityPaths.fixtureAuthority, "utf8"),
		);
		if (
			!isDeepStrictEqual(
				fixtureAuthorityDocument,
				request.campaign.fixture.document,
			)
		) {
			throw new Error("fixture authority document differs from the campaign");
		}
	} catch (error) {
		failures.push({
			name: "fixture authority document",
			path: request.identityPaths.fixtureAuthority,
			error: String(error),
		});
	}
	return failures;
}

async function verifyBoundPrograms(request) {
	const failures = [];
	const treeHashes = new Map();
	for (const identity of request.campaign.programs.filter(
		({ platformId }) => platformId === request.platformId,
	)) {
		try {
			const stat = await lstat(identity.program);
			if (
				stat.isSymbolicLink() ||
				!stat.isFile() ||
				(await sha256File(identity.program)) !== identity.binarySha256
			) {
				throw new Error("not an exact regular-file program identity");
			}
			if (identity.treeRoot !== undefined) {
				await Promise.all([
					assertNoSymlinkTraversal(
						identity.treeRoot,
						"diagnostic program dependency tree",
					),
					assertNoSymlinkTraversal(
						identity.program,
						"diagnostic program path",
					),
				]);
				const [canonicalTree, canonicalProgram] = await Promise.all([
					realpath(identity.treeRoot),
					realpath(identity.program),
				]);
				if (!isWithin(canonicalProgram, canonicalTree)) {
					throw new Error("tree-bound program escapes its dependency tree");
				}
				const prior = treeHashes.get(canonicalTree);
				if (prior && prior.expected !== identity.treeSha256) {
					throw new Error("shared program dependency tree authority differs");
				}
				const observed =
					prior?.observed ??
					(await sha256DerivedChangeDiagnosticTree(identity.treeRoot));
				treeHashes.set(canonicalTree, {
					expected: identity.treeSha256,
					observed,
				});
				if (observed !== identity.treeSha256) {
					throw new Error("program dependency tree identity differs");
				}
			}
		} catch (error) {
			failures.push({
				name: identity.name,
				program: identity.program,
				error: String(error),
			});
		}
	}
	return failures;
}

export async function verifyDerivedChangeDiagnosticBindings(request) {
	const programIdentityFailures = await verifyBoundPrograms(request);
	const sourcePreflightFailure = programIdentityFailures.length
		? null
		: await verifySourcePreflight(request);
	let temporaryRootFailure = null;
	try {
		await assertDerivedChangeDiagnosticTemporaryRootIdentity(
			request.temporaryRoot,
			request.outputRoot,
			request.sourcePreflight?.sourceRoot ?? process.cwd(),
		);
	} catch (error) {
		temporaryRootFailure = String(error);
	}
	const identityFailures = await verifyBoundIdentities(request);
	const requiredPrograms = [
		...new Set([
			...request.requiredExecutables,
			...request.cases
				.filter((entry) => !entry.unavailableReason && !entry.unknownReason)
				.map((entry) => entry.program),
		]),
	];
	const executableResults = await Promise.all(
		requiredPrograms.map(async (program) => {
			try {
				await access(program, constants.X_OK);
				return null;
			} catch (error) {
				return { program, error: String(error) };
			}
		}),
	);
	const executableFailures = executableResults.filter(Boolean);
	return sourcePreflightFailure ||
		temporaryRootFailure ||
		executableFailures.length ||
		identityFailures.length ||
		programIdentityFailures.length
		? {
				...(sourcePreflightFailure ? { sourcePreflightFailure } : {}),
				...(temporaryRootFailure ? { temporaryRootFailure } : {}),
				...(executableFailures.length ? { executableFailures } : {}),
				...(identityFailures.length ? { identityFailures } : {}),
				...(programIdentityFailures.length ? { programIdentityFailures } : {}),
			}
		: null;
}

function failureRow(
	caseRequest,
	dependsOn,
	log,
	actual,
	failureClass = caseRequest.failureClass,
	phase = caseRequest.phase,
) {
	return {
		id: caseRequest.id,
		lane: caseRequest.lane,
		required: caseRequest.required,
		attempted: true,
		status: "failed",
		dependsOn,
		failureClass,
		phase,
		fixtureCheckpoint: structuredClone(caseRequest.fixtureCheckpoint),
		log,
		artifactPaths: [],
		expected: { exitCode: caseRequest.expectedExitCode ?? 0, signal: null },
		actual,
	};
}

function skippedRow(caseRequest, blocker) {
	return {
		id: caseRequest.id,
		lane: caseRequest.lane,
		required: caseRequest.required,
		attempted: false,
		status: "skipped",
		dependsOn: caseRequest.dependsOn.includes(blocker)
			? [...caseRequest.dependsOn]
			: [...caseRequest.dependsOn, blocker],
		skipReason: `dependency ${blocker} did not pass`,
	};
}

async function expandCollection(
	caseRequest,
	caseRoot,
	stdoutPath,
	log,
	artifacts,
	outputRoot,
	launcherPassed,
	requiredCaseIds,
	campaignId,
) {
	const collection = caseRequest.collection;
	if (!collection) return [];
	const prefix = collection.idPrefix ?? "";
	const skip = (reason) =>
		collection.expectedCaseIds.map((id) => ({
			id: `${prefix}${id}`,
			lane: caseRequest.lane,
			required: requiredCaseIds.includes(`${prefix}${id}`),
			attempted: false,
			status: "skipped",
			dependsOn: [caseRequest.id],
			skipReason: reason,
		}));
	if (!launcherPassed)
		return skip(`collection launcher ${caseRequest.id} did not pass`);
	let records;
	try {
		const payload =
			collection.source === "stdout"
				? await readFile(stdoutPath, "utf8")
				: await readFile(join(caseRoot, collection.artifactPath), "utf8");
		const payloadObject = JSON.parse(payload);
		if (
			payloadObject?.schema !==
				DERIVED_CHANGE_DIAGNOSTIC_COLLECTION_SCHEMA_V1 ||
			!Array.isArray(payloadObject.cases)
		)
			throw new Error(
				"collection payload must be an internal diagnostic collection object",
			);
		if (
			payloadObject.campaignId !== undefined &&
			payloadObject.campaignId !== campaignId
		)
			throw new Error("collection payload campaign identity differs");
		if (
			payloadObject.artifactPaths !== undefined &&
			!Array.isArray(payloadObject.artifactPaths)
		)
			throw new Error("collection artifact inventory must be an array");
		for (const retainedPath of payloadObject.artifactPaths ?? []) {
			requireNormalRelativePath(
				retainedPath,
				`${caseRequest.id} collection artifact path`,
			);
			retainArtifact(
				artifacts,
				await requireRetainedFile(
					join(caseRoot, retainedPath),
					outputRoot,
					`${caseRequest.id} collection artifact`,
				),
			);
		}
		records = payloadObject.cases;
		const ids = records.map((record) => record?.id);
		if (
			new Set(ids).size !== ids.length ||
			JSON.stringify([...ids].sort()) !==
				JSON.stringify(collection.expectedCaseIds)
		)
			throw new Error(
				"collection payload identifiers differ from expected inventory",
			);
	} catch (error) {
		throw new Error(`collection payload was invalid: ${String(error)}`);
	}
	const expanded = [];
	for (const record of records) {
		requireObject(
			record,
			`diagnostic collection child ${record?.id ?? "unknown"}`,
		);
		requireText(record.lane, `diagnostic collection child ${record.id} lane`);
		if (
			typeof record.required !== "boolean" ||
			record.required !== requiredCaseIds.includes(`${prefix}${record.id}`)
		)
			throw new Error(
				`diagnostic collection child ${record.id} required identity differs from campaign authority`,
			);
		if (
			!["passed", "failed", "skipped", "unavailable", "unknown"].includes(
				record.status,
			)
		)
			throw new Error(
				`diagnostic collection child ${record.id} status is invalid`,
			);
		const artifactPaths = [];
		for (const childPath of record.artifactPaths ?? []) {
			requireNormalRelativePath(
				childPath,
				`diagnostic collection child ${record.id} artifact path`,
			);
			const artifact = await requireRetainedFile(
				join(caseRoot, childPath),
				outputRoot,
				`diagnostic collection child ${record.id} artifact`,
			);
			retainArtifact(artifacts, artifact);
			artifactPaths.push(artifact.path);
		}
		const dependencies = [
			...new Set([
				caseRequest.id,
				...(record.dependsOn ?? []).map((id) =>
					id === caseRequest.id ? id : `${prefix}${id}`,
				),
			]),
		];
		const child = {
			...structuredClone(record),
			id: `${prefix}${record.id}`,
			attempted: record.status === "passed" || record.status === "failed",
			dependsOn: dependencies,
			log,
			artifactPaths,
		};
		if (record.status === "failed")
			validateFixtureCheckpoint(
				record.fixtureCheckpoint,
				`diagnostic collection child ${record.id}`,
			);
		expanded.push(child);
	}
	return expanded;
}

export async function executeDerivedChangeDiagnosticCases(request) {
	validateDerivedChangeDiagnosticRequest(request);
	const sourceRoot = request.sourcePreflight?.sourceRoot ?? process.cwd();
	await assertDerivedChangeDiagnosticOutputRootSafety(
		request.outputRoot,
		sourceRoot,
	);
	await assertRootEmptyOrAbsent(request.outputRoot, "diagnostic output root");
	await assertDerivedChangeDiagnosticTemporaryRootSafety(
		request.temporaryRoot,
		request.outputRoot,
		sourceRoot,
	);
	await rootMustBeEmpty(request.outputRoot);
	const casesRoot = join(request.outputRoot, "cases");
	const logsRoot = join(request.outputRoot, "logs");
	await Promise.all([mkdir(casesRoot), mkdir(logsRoot)]);
	const artifacts = [];
	let effectiveRequest = request;
	if (request.sourcePreflight?.allowedSignersPath) {
		const authorityRoot = join(request.outputRoot, "authority");
		const retainedAuthority = join(authorityRoot, "allowed-signers");
		await mkdir(authorityRoot);
		await copyFile(
			request.sourcePreflight.allowedSignersPath,
			retainedAuthority,
		);
		retainArtifact(
			artifacts,
			await requireRetainedFile(
				retainedAuthority,
				request.outputRoot,
				"diagnostic allowed signers authority",
			),
		);
		effectiveRequest = structuredClone(request);
		effectiveRequest.sourcePreflight.allowedSignersPath = retainedAuthority;
	}
	const executableCases = request.cases.filter(
		(entry) => !entry.unavailableReason && !entry.unknownReason,
	);
	const globalPreflightFailure =
		await verifyDerivedChangeDiagnosticBindings(effectiveRequest);
	const globalPreflightCase = globalPreflightFailure
		? executableCases[0]
		: null;
	const rows = [];
	const rowById = new Map();
	let globalInvalidCase;
	for (const [caseIndex, caseRequest] of request.cases.entries()) {
		const dependencies = caseRequest.dependsOn.map((id) => rowById.get(id));
		const invalid = dependencies.find((record) => record.status !== "passed");
		if (!caseRequest.alwaysAttempt && (globalInvalidCase || invalid)) {
			const row = skippedRow(caseRequest, globalInvalidCase ?? invalid.id);
			rows.push(row);
			rowById.set(row.id, row);
			for (const child of await expandCollection(
				caseRequest,
				"",
				"",
				undefined,
				artifacts,
				request.outputRoot,
				false,
				request.campaign.requiredCaseIds,
				request.campaign.id,
			)) {
				rows.push(child);
				rowById.set(child.id, child);
			}
			continue;
		}
		if (caseRequest.unavailableReason || caseRequest.unknownReason) {
			const status = caseRequest.unavailableReason ? "unavailable" : "unknown";
			const row = {
				id: caseRequest.id,
				lane: caseRequest.lane,
				required: caseRequest.required,
				attempted: false,
				status,
				dependsOn: [...caseRequest.dependsOn],
				...(status === "unavailable"
					? { unavailableReason: caseRequest.unavailableReason }
					: { unknownReason: caseRequest.unknownReason }),
			};
			rows.push(row);
			rowById.set(row.id, row);
			continue;
		}
		const caseRoot = join(casesRoot, caseRequest.root ?? caseRequest.id);
		const workRoot = join(
			request.temporaryRoot,
			"w",
			caseIndex.toString().padStart(3, "0"),
		);
		const stdoutPath = join(logsRoot, `${caseRequest.id}.stdout.log`);
		const stderrPath = join(logsRoot, `${caseRequest.id}.stderr.log`);
		try {
			await Promise.all([
				mkdir(caseRoot),
				mkdir(workRoot, { recursive: true }),
			]);
		} catch (error) {
			const stdoutFile = await open(stdoutPath, "wx");
			await stdoutFile.close();
			const stderrFile = await open(stderrPath, "wx");
			try {
				await stderrFile.writeFile(`${String(error)}\n`);
			} finally {
				await stderrFile.close();
			}
			const stdout = await requireRetainedFile(
				stdoutPath,
				request.outputRoot,
				`diagnostic case ${caseRequest.id} stdout`,
			);
			const stderr = await requireRetainedFile(
				stderrPath,
				request.outputRoot,
				`diagnostic case ${caseRequest.id} stderr`,
			);
			retainArtifact(artifacts, stdout);
			retainArtifact(artifacts, stderr);
			const row = failureRow(caseRequest, [...caseRequest.dependsOn], stderr, {
				caseRoot,
				workRoot,
				setupError: String(error),
			});
			rows.push(row);
			rowById.set(row.id, row);
			for (const child of await expandCollection(
				caseRequest,
				caseRoot,
				stdoutPath,
				stderr,
				artifacts,
				request.outputRoot,
				false,
				request.campaign.requiredCaseIds,
				request.campaign.id,
			)) {
				rows.push(child);
				rowById.set(child.id, child);
			}
			if (row.failureClass === "global_invalid") globalInvalidCase = row.id;
			continue;
		}
		let outcome;
		if (globalPreflightFailure && caseRequest.id === globalPreflightCase.id)
			outcome = {
				code: null,
				signal: null,
				preflightError: globalPreflightFailure,
			};
		else {
			try {
				outcome = await runCommand(
					caseRequest,
					caseRoot,
					workRoot,
					stdoutPath,
					stderrPath,
				);
			} catch (error) {
				outcome = { code: null, signal: null, spawnError: String(error) };
			}
		}
		if (globalPreflightFailure && caseRequest.id === globalPreflightCase.id) {
			await open(stdoutPath, "wx").then((file) => file.close());
			await appendFile(
				stderrPath,
				`${JSON.stringify(globalPreflightFailure)}\n`,
			);
		}
		const stdout = await requireRetainedFile(
			stdoutPath,
			request.outputRoot,
			`diagnostic case ${caseRequest.id} stdout`,
		);
		const stderr = await requireRetainedFile(
			stderrPath,
			request.outputRoot,
			`diagnostic case ${caseRequest.id} stderr`,
		);
		retainArtifact(artifacts, stdout);
		retainArtifact(artifacts, stderr);
		const artifactPaths = [];
		const artifactErrors = [];
		for (const path of caseRequest.artifactPaths ?? []) {
			try {
				const artifact = await requireRetainedFile(
					join(caseRoot, path),
					request.outputRoot,
					`diagnostic case ${caseRequest.id} artifact`,
				);
				retainArtifact(artifacts, artifact);
				artifactPaths.push(artifact.path);
			} catch (error) {
				artifactErrors.push(String(error));
			}
		}
		if (artifactErrors.length)
			await appendFile(stderrPath, `${artifactErrors.join("\n")}\n`);
		const retainedStderr = artifactErrors.length
			? await requireRetainedFile(
					stderrPath,
					request.outputRoot,
					`diagnostic case ${caseRequest.id} stderr`,
				)
			: stderr;
		if (artifactErrors.length) {
			const index = artifacts.findIndex(
				(artifact) => artifact.path === stderr.path,
			);
			artifacts[index] = retainedStderr;
		}
		const expected = caseRequest.expectedExitCode ?? 0;
		const commandPassed =
			!globalPreflightFailure &&
			outcome.code === expected &&
			outcome.signal === null &&
			artifactErrors.length === 0;
		const collectionComplete =
			commandPassed ||
			(caseRequest.collection &&
				!globalPreflightFailure &&
				outcome.signal === null &&
				artifactErrors.length === 0 &&
				caseRequest.collection.completeExitCodes?.includes(outcome.code));
		const passed = Boolean(collectionComplete);
		let row = passed
			? {
					id: caseRequest.id,
					lane: caseRequest.lane,
					required: caseRequest.required,
					attempted: true,
					status: "passed",
					dependsOn: [...caseRequest.dependsOn],
					log: retainedStderr,
					artifactPaths,
					...(caseRequest.phase
						? {
								phase: caseRequest.phase,
								fixtureCheckpoint: structuredClone(
									caseRequest.fixtureCheckpoint,
								),
							}
						: {}),
				}
			: failureRow(
					caseRequest,
					[...caseRequest.dependsOn],
					retainedStderr,
					artifactErrors.length ? { ...outcome, artifactErrors } : outcome,
					globalPreflightFailure ? "global_invalid" : caseRequest.failureClass,
					globalPreflightFailure
						? "source-or-executable-preflight"
						: caseRequest.phase,
				);
		row.artifactPaths = artifactPaths;
		let children;
		try {
			children = await expandCollection(
				caseRequest,
				caseRoot,
				stdoutPath,
				retainedStderr,
				artifacts,
				request.outputRoot,
				collectionComplete,
				request.campaign.requiredCaseIds,
				request.campaign.id,
			);
		} catch (error) {
			row = failureRow(
				caseRequest,
				[...caseRequest.dependsOn],
				retainedStderr,
				{ ...outcome, collectionError: String(error) },
			);
			row.artifactPaths = artifactPaths;
			children = await expandCollection(
				caseRequest,
				caseRoot,
				stdoutPath,
				retainedStderr,
				artifacts,
				request.outputRoot,
				false,
				request.campaign.requiredCaseIds,
				request.campaign.id,
			);
		}
		rows.push(row);
		rowById.set(row.id, row);
		for (const child of children) {
			if (
				child.required &&
				!request.campaign.requiredCaseIds.includes(child.id)
			)
				throw new Error(
					`required diagnostic collection case ${child.id} is absent from campaign authority`,
				);
			rows.push(child);
			rowById.set(child.id, child);
		}
		const invalidChild = children.find(
			(child) =>
				child.status === "failed" && child.failureClass === "global_invalid",
		);
		if (row.failureClass === "global_invalid") globalInvalidCase = row.id;
		else if (invalidChild) globalInvalidCase = invalidChild.id;
	}
	return finalizeDerivedChangeDiagnosticFragment({
		schema: DERIVED_CHANGE_DIAGNOSTIC_FRAGMENT_SCHEMA_V1,
		campaign: structuredClone(request.campaign),
		platform: structuredClone(platformFor(request)),
		artifacts,
		cases: rows,
	});
}
