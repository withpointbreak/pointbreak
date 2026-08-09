export class BrowserDiagnosticFailure extends Error {
	constructor(report) {
		super(report.text);
		this.name = "BrowserDiagnosticFailure";
		this.report = report;
	}
}

export function createBrowserDiagnostics({
	context = () => ({}),
	isFatal = () => false,
} = {}) {
	let assertionCount = 0;
	let currentSection = null;
	let globalInvalid = false;
	const failures = [];
	const sections = [];
	const normalizeDiagnosticValue = (value, ancestors = new WeakSet()) => {
		if (value === undefined) return { valueType: "undefined" };
		if (typeof value === "number" && !Number.isFinite(value)) {
			return { valueType: "number", value: String(value) };
		}
		if (typeof value === "bigint") {
			return { valueType: "bigint", value: String(value) };
		}
		const isPlainObject =
			value !== null &&
			typeof value === "object" &&
			(Object.getPrototypeOf(value) === Object.prototype ||
				Object.getPrototypeOf(value) === null);
		if (!Array.isArray(value) && !isPlainObject) return value;
		if (ancestors.has(value)) return { valueType: "circular" };
		ancestors.add(value);
		const normalized = Array.isArray(value)
			? value.map((entry) => normalizeDiagnosticValue(entry, ancestors))
			: Object.fromEntries(
					Object.entries(value).map(([entryKey, entry]) => [
						entryKey,
						normalizeDiagnosticValue(entry, ancestors),
					]),
				);
		ancestors.delete(value);
		return normalized;
	};
	const diagnosticValue = (source, key, fallback) => {
		if (!source || !Object.hasOwn(source, key)) return fallback;
		return normalizeDiagnosticValue(source[key]);
	};

	const currentContext = () => {
		try {
			const value = context();
			return value && typeof value === "object" ? value : {};
		} catch (error) {
			return {
				contextError: error instanceof Error ? error.message : String(error),
			};
		}
	};

	const record = ({ kind, label, detail, expected, actual }) => {
		const evidence = currentContext();
		failures.push({
			kind,
			section: currentSection ?? "Unsectioned",
			label,
			detail,
			expected,
			actual,
			route: evidence.route ?? null,
			viewport: evidence.viewport ?? null,
			screenshot: evidence.screenshot ?? null,
			log: evidence.log ?? null,
		});
	};

	const expect = (condition, label, detail, comparison = {}) => {
		assertionCount += 1;
		if (condition) return true;
		record({
			kind: "assertion",
			label,
			detail,
			expected: diagnosticValue(comparison, "expected", {
				condition: label,
				outcome: "satisfied",
			}),
			actual: diagnosticValue(comparison, "actual", {
				condition: label,
				outcome: "failed",
				detail,
			}),
		});
		return false;
	};

	const abort = (detail, comparison = {}) => {
		const error = new Error(detail);
		error.browserDiagnosticFatal = true;
		error.expected = diagnosticValue(
			comparison,
			"expected",
			"browser and primary Inspector remain usable",
		);
		error.actual = diagnosticValue(comparison, "actual", detail);
		throw error;
	};
	const requireCondition = (condition, label, detail, comparison = {}) => {
		if (expect(condition, label, detail, comparison)) return true;
		const error = new Error(detail);
		error.browserDiagnosticRecorded = true;
		throw error;
	};

	const recordLifecycleFailure = (name, phase, error) => {
		const fatal = error?.browserDiagnosticFatal === true || isFatal(error);
		if (fatal) globalInvalid = true;
		record({
			kind: fatal ? "fatal" : "section",
			label: `${name} ${phase}`,
			detail: error instanceof Error ? error.message : String(error),
			expected: diagnosticValue(
				error,
				"expected",
				fatal
					? "browser and primary Inspector remain usable"
					: `section ${phase} completes`,
			),
			actual: diagnosticValue(
				error,
				"actual",
				fatal
					? "browser or primary Inspector became unusable"
					: `section ${phase} stopped`,
			),
		});
		return fatal;
	};

	const section = async (name, operation) => {
		if (currentSection !== null) {
			throw new Error(
				`browser diagnostic sections cannot nest (${currentSection} -> ${name})`,
			);
		}
		if (globalInvalid) {
			sections.push({ name, status: "skipped", failureCount: 0 });
			return;
		}
		const lifecycle =
			typeof operation === "function" ? { run: operation } : (operation ?? {});
		const before = failures.length;
		let stopped = false;
		let setupValue;
		currentSection = name;
		try {
			try {
				setupValue = await (lifecycle.setup?.() ?? undefined);
			} catch (error) {
				stopped = true;
				if (error?.browserDiagnosticRecorded !== true) {
					recordLifecycleFailure(name, "setup", error);
				}
			}
			if (!stopped && !globalInvalid) {
				try {
					await lifecycle.run?.(setupValue);
				} catch (error) {
					stopped = true;
					if (error?.browserDiagnosticRecorded !== true) {
						recordLifecycleFailure(name, "transition", error);
					}
				}
			}
			if (!globalInvalid) {
				try {
					await lifecycle.teardown?.();
				} catch (error) {
					stopped = true;
					if (error?.browserDiagnosticRecorded !== true) {
						recordLifecycleFailure(name, "teardown", error);
					}
				}
			}
		} finally {
			const failureCount = failures.length - before;
			sections.push({
				name,
				status: stopped ? "stopped" : failureCount > 0 ? "failed" : "passed",
				failureCount,
			});
			currentSection = null;
		}
	};

	const report = () => {
		const text =
			failures.length === 0
				? "Browser diagnostics passed."
				: [
						`Browser diagnostics recorded ${failures.length} failure(s):`,
						...failures.map((failure, index) => {
							const viewport = failure.viewport
								? `${failure.viewport.width}x${failure.viewport.height}`
								: "unknown viewport";
							const artifacts =
								[failure.screenshot, failure.log].filter(Boolean).join(", ") ||
								"no artifact reference";
							return [
								`${index + 1}. [${failure.section}] ${failure.label}: ${failure.detail}`,
								`   expected=${JSON.stringify(failure.expected)} actual=${JSON.stringify(failure.actual)}`,
								`   route=${failure.route ?? "unknown route"} viewport=${viewport} artifacts=${artifacts}`,
							].join("\n");
						}),
					].join("\n");
		return {
			assertionCount,
			globalInvalid,
			sections: sections.map((entry) => ({ ...entry })),
			failures: failures.map((failure) => ({ ...failure })),
			text,
		};
	};

	const result = ({ screenshotCount }) => {
		const diagnosticReport = report();
		return {
			schema: "pointbreak.change-inspector-browser-report",
			version: 1,
			status: diagnosticReport.failures.length === 0 ? "passed" : "failed",
			assertionCount,
			screenshotCount,
			sectionCount: sections.length,
			globalInvalid,
			sections: sections.map((entry) => ({ ...entry })),
			failures: diagnosticReport.failures,
		};
	};

	const complete = ({ screenshotCount }) => {
		const completion = result({ screenshotCount });
		if (completion.status === "failed") {
			throw new BrowserDiagnosticFailure(report());
		}
		return completion;
	};

	return { abort, complete, expect, report, requireCondition, result, section };
}
