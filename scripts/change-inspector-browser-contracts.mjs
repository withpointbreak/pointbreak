export const BROWSER_PROGRAM_MARKERS = Object.freeze([
	"__POINTBREAK_BROWSER_DIAGNOSTIC_FAILURE__",
	"__POINTBREAK_BROWSER_DIAGNOSTICS__",
	"__POINTBREAK_CHANGE_BROWSER_CONFIG__",
]);

export function renderBrowserProgram({ source, diagnosticFailure, diagnostics, configLiteral }) {
	const replacements = new Map([
		[BROWSER_PROGRAM_MARKERS[0], diagnosticFailure],
		[BROWSER_PROGRAM_MARKERS[1], diagnostics],
		[BROWSER_PROGRAM_MARKERS[2], configLiteral],
	]);
	let rendered = source;
	for (const [marker, value] of replacements) {
		if (typeof value !== "string") throw new TypeError(`${marker} replacement must be a string`);
		const occurrences = rendered.split(marker).length - 1;
		if (occurrences !== 1) throw new Error(`browser program marker ${marker} occurred ${occurrences} times`);
		// A replacement callback keeps JavaScript replacement metasequences such as
		// $&, $$, $`, and $' byte-for-byte literal.
		rendered = rendered.replace(marker, () => value);
	}
	return rendered;
}

export function parseBrowserResultLog(log, exitCode = 0) {
	if (!Number.isSafeInteger(exitCode) || exitCode < 0) throw new Error("browser exit code must be a non-negative integer");
	if (exitCode !== 0) throw new Error(`browser program exited with status ${exitCode}`);
	const lines = log.split(/\n/).map((line) => line.replace(/\r$/, ""));
	if (lines.some((line) => line.startsWith("### Error"))) {
		throw new Error("browser output contains an Error section");
	}
	const resultMarkers = lines.flatMap((line, index) => line === "### Result" ? [index] : []);
	if (resultMarkers.length !== 1) throw new Error(`browser output must contain exactly one Result section; observed ${resultMarkers.length}`);
	const payload = lines[resultMarkers[0] + 1];
	if (!payload) throw new Error("browser Result section has no JSON payload");
	let report;
	try { report = JSON.parse(payload); }
	catch (error) { throw new Error(`browser Result payload is malformed JSON: ${error.message}`); }
	const count = (value) => typeof value === "number" && Number.isFinite(value) && value >= 0;
	if (report === null || typeof report !== "object" || Array.isArray(report) ||
		report.schema !== "pointbreak.change-inspector-browser-report" || report.version !== 1 ||
		!["passed", "failed"].includes(report.status) || !count(report.assertionCount) ||
		!count(report.screenshotCount) || !count(report.sectionCount) || report.sectionCount <= 0 ||
		typeof report.globalInvalid !== "boolean" || !Array.isArray(report.sections) ||
		report.sections.length !== report.sectionCount || !Array.isArray(report.failures)) {
		throw new Error("browser Result payload does not match the production report schema");
	}
	return report;
}

export function timelineReadyInPage() {
	const list = document.querySelector("#timeline");
	const stamp = document.querySelector("#stat-hash")?.textContent?.trim() ?? "";
	const mounted = list?.dataset.timelineRoute ?? null;
	const rows = document.querySelectorAll("#timeline [data-event-id]").length;
	const active = globalThis.__pointbreakBrowserTransportObserver?.snapshot?.().activeCount ?? null;
	const acceptedHash = location.hash === "#/" || location.hash === "#/timeline" || location.hash.startsWith("#/timeline?");
	const acceptedMount = mounted === "#/timeline" || Boolean(mounted?.startsWith("#/timeline?"));
	return acceptedHash && acceptedMount && Boolean(list) && rows > 0 && stamp !== "" && stamp !== "—" && active === 0;
}

export function installBrowserTransportObserver(scope) {
	const originalFetch = scope.fetch;
	let ordinal = 0;
	const active = new Set();
	const records = [];
	scope.fetch = function(input, options) {
		const request = input instanceof scope.Request ? input : null;
		const hasExplicitSignal = options !== undefined && Object.prototype.hasOwnProperty.call(options, "signal");
		const signal = hasExplicitSignal ? options.signal ?? null : request?.signal ?? null;
		const url = new scope.URL(request?.url ?? input, scope.location.href);
		const id = ++ordinal;
		const record = { id, endpoint: `${url.pathname}${url.search}`, terminal: "pending",
			signalNotifications: [], signalObservationOnly: true, requestFailureBinding: "unavailable" };
		records.push(record); active.add(id);
		const onAbort = () => record.signalNotifications.push({
			reason: typeof signal?.reason === "string" ? signal.reason : null,
			terminalAtNotification: record.terminal,
		});
		signal?.addEventListener("abort", onAbort);
		let result;
		try { result = originalFetch.call(this, input, options); }
		catch (error) { active.delete(id); record.terminal = "fetch-throw"; throw error; }
		void result.then((response) => {
			record.status = response.status;
			const originalText = response.text.bind(response);
			let observed = false;
			response.text = function() {
				const body = originalText();
				if (!observed) {
					observed = true;
					void body.then(
						() => { active.delete(id); record.terminal = "response-body"; },
						() => { active.delete(id); record.terminal = "body-reject"; },
					);
				}
				return body;
			};
		}, () => { active.delete(id); record.terminal = signal?.aborted ? "fetch-reject-after-signal" : "fetch-reject"; });
		return result;
	};
	scope.__pointbreakBrowserTransportObserver = {
		snapshot: () => ({ activeCount: active.size, records: records.map((record) => structuredClone(record)) }),
	};
}
