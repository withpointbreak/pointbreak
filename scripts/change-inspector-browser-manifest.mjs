import { access, readFile, rename } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { pathToFileURL } from "node:url";

export async function publishPassingManifest({
	candidatePath,
	manifestPath,
	browserResult,
}) {
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
			"browser manifest publication must use a same-directory atomic rename",
		);
	}
	try {
		await access(manifestPath);
		throw new Error("browser completion manifest already exists");
	} catch (error) {
		if (error?.code !== "ENOENT") throw error;
	}

	const candidate = JSON.parse(await readFile(candidatePath, "utf8"));
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
	await rename(candidatePath, manifestPath);
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
	await publishPassingManifest({ candidatePath, manifestPath, browserResult });
}
