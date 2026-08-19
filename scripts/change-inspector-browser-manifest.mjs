import { createHash, randomUUID } from "node:crypto";
import { constants } from "node:fs";
import { link, lstat, open, readdir, readFile, unlink } from "node:fs/promises";
import { dirname, isAbsolute, join, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";
import { isDeepStrictEqual } from "node:util";

const stableStatFields = ["dev", "ino", "size", "mtimeNs", "ctimeNs"];
const derivedChangeDiagnosticSchemas = new Set([
	"pointbreak.derived-change-diagnostic-report.v1",
	"pointbreak.derived-change-diagnostic-fragment.v1",
	"pointbreak.derived-change-diagnostic-collection.v1",
]);
const derivedChangeDiagnosticModes = new Set([
	"--derived-access-lifecycle-diagnostic",
	"--derived-change-diagnostic-identity",
	"--derived-change-diagnostic-native",
	"--derived-change-read-diagnostic",
]);
const derivedChangeDiagnosticRootComponent = "derived-change-diagnostic";
const derivedChangeDiagnosticReportBasename =
	"derived-change-diagnostic-report.json";
const derivedChangeDiagnosticBoundaryError =
	"derived Change diagnostic output is never browser completion evidence";

function rejectDerivedChangeDiagnosticDocument(value) {
	if (
		derivedChangeDiagnosticSchemas.has(value?.schema) ||
		derivedChangeDiagnosticModes.has(value?.mode)
	) {
		throw new Error(derivedChangeDiagnosticBoundaryError);
	}
}

function rejectDerivedChangeDiagnosticBytes(bytes) {
	let value;
	try {
		value = JSON.parse(bytes.toString("utf8"));
	} catch {
		return;
	}
	rejectDerivedChangeDiagnosticDocument(value);
}

function rejectDerivedChangeDiagnosticPath(path) {
	const components = String(path).split(/[\\/]/u);
	if (
		components.some(
			(component) =>
				component.toLowerCase() === derivedChangeDiagnosticRootComponent ||
				component.toLowerCase() === derivedChangeDiagnosticReportBasename,
		)
	) {
		throw new Error(derivedChangeDiagnosticBoundaryError);
	}
}

const sameFileVersion = (left, right) =>
	stableStatFields.every((field) => left[field] === right[field]);

async function openStableFile(path, label) {
	const pathStat = await lstat(path, { bigint: true });
	if (pathStat.isSymbolicLink() || !pathStat.isFile()) {
		throw new Error(`${label} is not a retained regular file`);
	}
	const handle = await open(
		path,
		constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0),
	);
	try {
		const openedStat = await handle.stat({ bigint: true });
		if (!openedStat.isFile() || !sameFileVersion(pathStat, openedStat)) {
			throw new Error(`${label} changed while it was opened`);
		}
		const bytes = await handle.readFile();
		const readStat = await handle.stat({ bigint: true });
		const currentPathStat = await lstat(path, { bigint: true });
		if (
			!sameFileVersion(openedStat, readStat) ||
			!sameFileVersion(readStat, currentPathStat)
		) {
			throw new Error(`${label} changed while it was read`);
		}
		return { bytes, handle, label, path, stat: readStat };
	} catch (error) {
		await handle.close();
		throw error;
	}
}

async function assertStableFile(snapshot) {
	const openedStat = await snapshot.handle.stat({ bigint: true });
	const pathStat = await lstat(snapshot.path, { bigint: true });
	if (
		!openedStat.isFile() ||
		pathStat.isSymbolicLink() ||
		!pathStat.isFile() ||
		!sameFileVersion(snapshot.stat, openedStat) ||
		!sameFileVersion(openedStat, pathStat)
	) {
		throw new Error(`${snapshot.label} changed before completion publication`);
	}
}

async function assertStablePath(snapshot) {
	const pathStat = await lstat(snapshot.path, { bigint: true });
	if (
		pathStat.isSymbolicLink() ||
		!pathStat.isFile() ||
		!sameFileVersion(snapshot.stat, pathStat)
	) {
		throw new Error(`${snapshot.label} changed before completion publication`);
	}
}

async function snapshotDirectory(path, label) {
	const stat = await lstat(path, { bigint: true });
	if (stat.isSymbolicLink() || !stat.isDirectory()) {
		throw new Error(`${label} is not a retained directory`);
	}
	return { label, path, stat };
}

async function assertStableDirectory(snapshot) {
	const stat = await lstat(snapshot.path, { bigint: true });
	if (
		stat.isSymbolicLink() ||
		!stat.isDirectory() ||
		!sameFileVersion(snapshot.stat, stat)
	) {
		throw new Error(`${snapshot.label} changed before completion publication`);
	}
}

const comparePaths = (left, right) =>
	left < right ? -1 : left > right ? 1 : 0;

export async function publishPassingManifest({
	candidatePath,
	manifestPath,
	browserResult,
	browserResultPath,
	evidenceRoot,
}) {
	rejectDerivedChangeDiagnosticDocument(browserResult);
	for (const path of [
		candidatePath,
		manifestPath,
		...(browserResultPath ? [browserResultPath] : []),
		evidenceRoot ?? dirname(resolve(manifestPath)),
	]) {
		rejectDerivedChangeDiagnosticPath(path);
	}
	if (
		browserResult?.schema !== "pointbreak.change-inspector-browser-report" ||
		browserResult?.version !== 1
	) {
		throw new Error("browser result schema or version is invalid");
	}
	if (browserResult?.status !== "passed") {
		throw new Error("browser result did not declare a passing status");
	}
	if (!Array.isArray(browserResult.failures)) {
		throw new Error("browser result omitted its diagnostic failure list");
	}
	if (browserResult.failures.length > 0) {
		throw new Error(
			`browser result recorded ${browserResult.failures.length} browser diagnostic failure(s)`,
		);
	}
	for (const field of ["assertionCount", "screenshotCount"]) {
		if (
			!Number.isSafeInteger(browserResult[field]) ||
			browserResult[field] < 0
		) {
			throw new Error(`browser result ${field} is not a nonnegative integer`);
		}
	}
	if (
		browserResult.globalInvalid !== false ||
		!Array.isArray(browserResult.sections) ||
		browserResult.sections.length !== browserResult.sectionCount ||
		browserResult.sections.some(
			(section) => section.status !== "passed" || section.failureCount !== 0,
		)
	) {
		throw new Error("browser result retained an invalid or incomplete section");
	}
	if (dirname(resolve(candidatePath)) !== dirname(resolve(manifestPath))) {
		throw new Error(
			"browser manifest publication must use same-directory atomic publication",
		);
	}

	const candidateSnapshot = await openStableFile(
		candidatePath,
		"browser manifest candidate",
	);
	await candidateSnapshot.handle.close();
	const candidate = JSON.parse(candidateSnapshot.bytes.toString("utf8"));
	rejectDerivedChangeDiagnosticDocument(candidate);
	if (
		candidate?.gate !== "change-inspector-browser-verify" ||
		candidate?.status !== "passed"
	) {
		throw new Error(
			"browser manifest candidate is not a passing Change Inspector result",
		);
	}
	if (
		![candidate.assertionCount, candidate.screenshotCount].every(
			(value) => Number.isSafeInteger(value) && value >= 0,
		)
	) {
		throw new Error(
			"browser manifest candidate omitted assertionCount or screenshotCount",
		);
	}
	for (const field of ["assertionCount", "screenshotCount"]) {
		if (candidate[field] !== browserResult[field]) {
			throw new Error(
				`browser manifest ${field} ${candidate[field]} did not match browser result ${browserResult[field]}`,
			);
		}
	}
	const resolvedEvidenceRoot = resolve(
		evidenceRoot ?? dirname(resolve(manifestPath)),
	);
	const evidenceInventory = candidate.evidenceInventory;
	if (!Array.isArray(evidenceInventory) || evidenceInventory.length === 0) {
		throw new Error("browser manifest omitted its retained evidence inventory");
	}
	const inventoryPaths = evidenceInventory.map((entry) => entry?.path);
	if (
		inventoryPaths.some(
			(path) => typeof path !== "string" || path.length === 0,
		) ||
		new Set(inventoryPaths).size !== inventoryPaths.length
	) {
		throw new Error(
			"browser evidence inventory paths are invalid or duplicated",
		);
	}
	for (const path of inventoryPaths) rejectDerivedChangeDiagnosticPath(path);
	const sortedInventoryPaths = [...inventoryPaths].sort(comparePaths);
	if (
		inventoryPaths.some((path, index) => path !== sortedInventoryPaths[index])
	) {
		throw new Error("browser evidence inventory must be sorted by path");
	}
	const artifactDirectory = await snapshotDirectory(
		join(resolvedEvidenceRoot, "browser-artifacts"),
		"retained browser artifact directory",
	);
	const logDirectory = await snapshotDirectory(
		join(resolvedEvidenceRoot, "logs"),
		"retained browser log directory",
	);
	const artifactNames = (
		await readdir(artifactDirectory.path, {
			withFileTypes: true,
		})
	)
		.filter((entry) => entry.isFile() && entry.name.endsWith(".png"))
		.map((entry) => `browser-artifacts/${entry.name}`);
	const browserLogNames = (
		await readdir(logDirectory.path, { withFileTypes: true })
	)
		.filter(
			(entry) =>
				entry.isFile() &&
				entry.name.startsWith("browser-") &&
				/[.](?:json|log|mjs)$/.test(entry.name),
		)
		.map((entry) => `logs/${entry.name}`);
	const requiredEvidencePaths = [...artifactNames, ...browserLogNames].sort(
		comparePaths,
	);
	for (const requiredPath of [
		"logs/browser-result.json",
		"logs/browser-gate.log",
		"logs/browser-primary-derived-access-status.json",
		"logs/browser-program.mjs",
	]) {
		if (!requiredEvidencePaths.includes(requiredPath)) {
			throw new Error(`retained browser evidence is missing ${requiredPath}`);
		}
	}
	if (
		requiredEvidencePaths.length !== inventoryPaths.length ||
		requiredEvidencePaths.some((path, index) => path !== inventoryPaths[index])
	) {
		throw new Error(
			"browser evidence inventory does not cover every retained browser output",
		);
	}
	if (artifactNames.length !== candidate.screenshotCount) {
		throw new Error(
			`browser evidence inventory found ${artifactNames.length} screenshots for declared count ${candidate.screenshotCount}`,
		);
	}
	const evidenceSnapshots = [];
	let publisherPath;
	let publisherSnapshot;
	try {
		for (const entry of evidenceInventory) {
			if (
				typeof entry.sha256 !== "string" ||
				!/^[0-9a-f]{64}$/.test(entry.sha256)
			) {
				throw new Error(
					`browser evidence inventory has an invalid SHA-256 for ${entry.path}`,
				);
			}
			const absolutePath = resolve(resolvedEvidenceRoot, entry.path);
			const relativePath = relative(resolvedEvidenceRoot, absolutePath);
			if (
				relativePath === "" ||
				isAbsolute(relativePath) ||
				relativePath === ".." ||
				relativePath.startsWith(`..${sep}`)
			) {
				throw new Error(
					`browser evidence inventory path escapes its root: ${entry.path}`,
				);
			}
			const snapshot = await openStableFile(
				absolutePath,
				`browser evidence ${entry.path}`,
			);
			evidenceSnapshots.push(snapshot);
			rejectDerivedChangeDiagnosticBytes(snapshot.bytes);
			const actualSha256 = createHash("sha256")
				.update(snapshot.bytes)
				.digest("hex");
			if (actualSha256 !== entry.sha256) {
				throw new Error(`browser evidence SHA-256 mismatch for ${entry.path}`);
			}
		}
		const primaryStatusSnapshot = evidenceSnapshots.find(
			(snapshot) =>
				snapshot.path ===
				join(
					resolvedEvidenceRoot,
					"logs/browser-primary-derived-access-status.json",
				),
		);
		if (!primaryStatusSnapshot) {
			throw new Error("primary derived-access status evidence is missing");
		}
		let primaryStatus;
		try {
			primaryStatus = JSON.parse(primaryStatusSnapshot.bytes.toString("utf8"));
		} catch (error) {
			throw new Error(`primary derived-access status is invalid JSON: ${error}`);
		}
		if (
			primaryStatus?.schema !==
				"pointbreak.inspect-derived-access-status" ||
			primaryStatus?.version !== 1 ||
			primaryStatus?.active !== true ||
			primaryStatus?.servingCurrent !== true ||
			primaryStatus?.availability !== "current" ||
			primaryStatus?.rebuildInFlight !== false ||
			primaryStatus?.rebuildPaused !== false
		) {
			throw new Error(
				"primary derived-access status is not active/current",
			);
		}
		if (!isDeepStrictEqual(candidate.primaryDerivedAccessStatus, primaryStatus)) {
			throw new Error(
				"browser manifest primary derived-access status did not match retained evidence",
			);
		}

		await assertStablePath(candidateSnapshot);
		await assertStableDirectory(artifactDirectory);
		await assertStableDirectory(logDirectory);
		for (const snapshot of evidenceSnapshots) await assertStableFile(snapshot);

		publisherPath = join(
			dirname(resolve(manifestPath)),
			`.manifest.publisher-${process.pid}-${randomUUID()}.tmp`,
		);
		const publisher = await open(publisherPath, "wx", 0o644);
		try {
			await publisher.writeFile(candidateSnapshot.bytes);
			await publisher.sync();
		} finally {
			await publisher.close();
		}
		publisherSnapshot = await openStableFile(
			publisherPath,
			"publisher-owned browser completion manifest",
		);
		if (!publisherSnapshot.bytes.equals(candidateSnapshot.bytes)) {
			throw new Error("publisher-owned browser completion bytes changed");
		}
		await assertStablePath(candidateSnapshot);
		await unlink(candidatePath);

		await assertStableDirectory(artifactDirectory);
		await assertStableDirectory(logDirectory);
		for (const snapshot of evidenceSnapshots) await assertStableFile(snapshot);
		await assertStableFile(publisherSnapshot);
		try {
			await link(publisherPath, manifestPath);
		} catch (error) {
			if (error?.code === "EEXIST") {
				throw new Error("browser completion manifest already exists");
			}
			throw error;
		}
	} finally {
		for (const snapshot of evidenceSnapshots) {
			await snapshot.handle.close().catch(() => {});
		}
		if (publisherSnapshot)
			await publisherSnapshot.handle.close().catch(() => {});
		if (publisherPath) await unlink(publisherPath).catch(() => {});
	}
}

if (
	process.argv[1] &&
	import.meta.url === pathToFileURL(process.argv[1]).href
) {
	const [candidatePath, manifestPath, resultPath] = process.argv.slice(2);
	if (!candidatePath || !manifestPath || !resultPath) {
		throw new Error(
			"usage: change-inspector-browser-manifest.mjs <candidate> <manifest> <browser-result>",
		);
	}
	const browserResult = JSON.parse(await readFile(resultPath, "utf8"));
	await publishPassingManifest({
		candidatePath,
		manifestPath,
		browserResult,
		browserResultPath: resultPath,
		evidenceRoot: dirname(resolve(manifestPath)),
	});
}
