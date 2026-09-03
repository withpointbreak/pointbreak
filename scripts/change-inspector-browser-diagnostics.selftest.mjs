import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { setTimeout as delay } from "node:timers/promises";

import {
	BrowserDiagnosticFailure,
	createBrowserDiagnostics,
} from "./change-inspector-browser-diagnostics.mjs";
import { publishPassingManifest } from "./change-inspector-browser-manifest.mjs";

const context = () => ({
	route: "http://127.0.0.1:4173/#/changes",
	viewport: { width: 1440, height: 1000 },
	screenshot: "screenshots/wide-changes.png",
	log: "logs/browser-gate.log",
});

const currentDerivedAccessStatus = {
	schema: "pointbreak.inspect-derived-access-status",
	version: 1,
	active: true,
	servingCurrent: true,
	availability: "current",
	rebuildInFlight: false,
	rebuildPaused: false,
};

class FakePage {
	#listeners = new Map();
	#url;

	constructor(url = "http://127.0.0.1:4173/#/") {
		this.#url = url;
		this.main = new FakeFrame(() => this.#url);
	}

	on(name, listener) {
		const listeners = this.#listeners.get(name) ?? [];
		listeners.push(listener);
		this.#listeners.set(name, listeners);
	}

	emit(name, ...args) {
		for (const listener of this.#listeners.get(name) ?? []) listener(...args);
	}

	waitForEvent(name, predicate) {
		return new Promise((resolve) => {
			const listener = (...args) => {
				if (!predicate(...args)) return;
				const listeners = this.#listeners.get(name) ?? [];
				this.#listeners.set(
					name,
					listeners.filter((candidate) => candidate !== listener),
				);
				resolve(args[0]);
			};
			const listeners = this.#listeners.get(name) ?? [];
			listeners.push(listener);
			this.#listeners.set(name, listeners);
		});
	}

	mainFrame() {
		return this.main;
	}

	setUrl(url) {
		this.#url = url;
	}

	url() {
		return this.#url;
	}
}

class FakeFrame {
	constructor(url) {
		this.currentUrl = url;
	}

	url() {
		return this.currentUrl();
	}
}

class FakeRequest {
	constructor({
		url = "http://127.0.0.1:4173/api/v2/profile",
		method = "GET",
		resourceType = "fetch",
		frame,
		frameUnavailable = false,
		navigation = false,
		redirectedFrom = null,
		failure = "net::ERR_ABORTED",
	} = {}) {
		this.requestUrl = url;
		this.requestMethod = method;
		this.requestResourceType = resourceType;
		this.requestFrame = frame;
		this.frameUnavailable = frameUnavailable;
		this.navigation = navigation;
		this.redirectSource = redirectedFrom;
		this.failureText = failure;
	}

	failure() {
		return this.failureText === null ? null : { errorText: this.failureText };
	}

	frame() {
		if (this.frameUnavailable) throw new Error("request has no frame");
		return this.requestFrame;
	}

	isNavigationRequest() {
		return this.navigation;
	}

	method() {
		return this.requestMethod;
	}

	redirectedFrom() {
		return this.redirectSource;
	}

	resourceType() {
		return this.requestResourceType;
	}

	url() {
		return this.requestUrl;
	}
}

class FakeResponse {
	constructor(request, status = 200) {
		this.sourceRequest = request;
		this.responseStatus = status;
	}

	request() {
		return this.sourceRequest;
	}

	status() {
		return this.responseStatus;
	}

	url() {
		return this.sourceRequest.url();
	}
}

class ManualTimers {
	#nextId = 1;
	#timers = new Map();

	constructor() {
		this.now = 0;
	}

	clearTimeout = (id) => {
		this.#timers.delete(id);
	};

	setTimeout = (callback, delayMs) => {
		const id = this.#nextId;
		this.#nextId += 1;
		this.#timers.set(id, { at: this.now + delayMs, callback });
		return id;
	};

	advanceTo(value, { run = true } = {}) {
		this.now = value;
		if (run) this.flushDue();
	}

	flushDue() {
		for (;;) {
			const due = [...this.#timers.entries()]
				.filter(([, timer]) => timer.at <= this.now)
				.sort(([leftId, left], [rightId, right]) =>
					left.at === right.at ? leftId - rightId : left.at - right.at,
				)[0];
			if (!due) return;
			const [id, timer] = due;
			this.#timers.delete(id);
			timer.callback();
		}
	}
}

async function runD70BrowserSelftest(hook) {
	let source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	for (const [marker, replacement] of [
		[
			"__POINTBREAK_BROWSER_DIAGNOSTIC_FAILURE__",
			BrowserDiagnosticFailure.toString(),
		],
		["__POINTBREAK_BROWSER_DIAGNOSTICS__", createBrowserDiagnostics.toString()],
		[
			"__POINTBREAK_CHANGE_BROWSER_CONFIG__",
			'({mode: "full", server: {baseUrl: "http://127.0.0.1:4173"}, __pointbreakD70Selftest: selftestHook})',
		],
	]) {
		assert.ok(source.includes(marker), `missing browser marker ${marker}`);
		source = source.replace(marker, replacement);
	}
	const programOrResult = new Function("selftestHook", `return (${source}\n);`)(
		hook,
	);
	return typeof programOrResult === "function"
		? await programOrResult(new FakePage())
		: await programOrResult;
}

function createBoundProfileTransition({
	createProfileRequestLifecycle,
	timers = new ManualTimers(),
	onRequestFailure = () => {},
	sourceUrl =
		"http://127.0.0.1:4173/#/changes?limit=100&order=change_id_asc",
	targetHash = "#/timeline?limit=100&order=desc",
} = {}) {
	const page = new FakePage(sourceUrl);
	const lifecycleFailures = [];
	const requestFailures = [];
	const lifecycle = createProfileRequestLifecycle({
		page,
		primaryBaseUrl: "http://127.0.0.1:4173",
		now: () => timers.now,
		setTimer: timers.setTimeout,
		clearTimer: timers.clearTimeout,
		onLifecycleFailure: (failure) => lifecycleFailures.push(failure),
		onRequestFailure: (failure) => {
			requestFailures.push(failure);
			onRequestFailure(failure);
		},
	});
	const visit = lifecycle.createRouteVisitIntent(
		"#/changes?limit=100&order=change_id_asc",
	);
	lifecycle.activateRouteVisit(visit);
	const request = new FakeRequest({ frame: page.mainFrame() });
	page.emit("request", request);
	assert.equal(lifecycle.certifyChangesVisit(visit, visit.intendedHash), true);
	const arm = lifecycle.snapshotProfileArm({
		arm: "optional",
		targetHash,
	});
	assert.equal(arm.candidates.length, 1);
	assert.ok(arm.transition);
	arm.transition.destinationSucceeded = true;
	return {
		arm,
		lifecycle,
		lifecycleFailures,
		page,
		request,
		requestFailures,
		timers,
	};
}

async function processTable() {
	const child = spawn("ps", ["-ax", "-o", "pid=", "-o", "ppid=", "-o", "pgid="]);
	let stdout = "";
	let stderr = "";
	child.stdout.on("data", (chunk) => {
		stdout += chunk;
	});
	child.stderr.on("data", (chunk) => {
		stderr += chunk;
	});
	const code = await new Promise((resolve, reject) => {
		child.once("error", reject);
		child.once("exit", resolve);
	});
	assert.equal(code, 0, `ps process-table query failed: ${stderr}`);
	return stdout
		.trim()
		.split("\n")
		.filter(Boolean)
		.map((line) => {
			const [pid, parentPid, processGroupId] = line.trim().split(/\s+/).map(Number);
			return { parentPid, pid, processGroupId };
		});
}

async function runD70ShellSelftest(caseName, { signal = null } = {}) {
	const root = await mkdtemp(join(tmpdir(), `pointbreak-d70-${caseName}-`));
	const script = new URL(
		"./change-inspector-browser-verify.sh",
		import.meta.url,
	).pathname;
	const child = spawn(
		process.env.POINTBREAK_BROWSER_STAGE_SELFTEST_BASH ?? "bash",
		[script],
		{
			env: {
				...process.env,
				POINTBREAK_BROWSER_STAGE_SELFTEST: "1",
				POINTBREAK_BROWSER_STAGE_SELFTEST_CASE: caseName,
				POINTBREAK_BROWSER_STAGE_SELFTEST_ROOT: root,
			},
			stdio: ["ignore", "pipe", "pipe"],
		},
	);
	let stdout = "";
	let stderr = "";
	child.stdout.on("data", (chunk) => {
		stdout += chunk;
	});
	child.stderr.on("data", (chunk) => {
		stderr += chunk;
	});
	const startPath = join(root, "logs", "browser-stage-start.json");
	const descendantPath = join(root, "fake-descendant.pid");
	let start;
	let launched = false;
	for (let attempt = 0; attempt < 200; attempt += 1) {
		try {
			start = JSON.parse(await readFile(startPath, "utf8"));
			await readFile(descendantPath);
			launched = true;
			break;
		} catch {
			await delay(10);
		}
	}
	assert.equal(
		launched,
		true,
		`${caseName} did not launch its recorded process tree`,
	);
	let workerPids = [];
	let workerTimerPids = [];
	for (let attempt = 0; attempt < 100; attempt += 1) {
		const liveProcesses = await processTable();
		workerPids = liveProcesses
			.filter(
				(process) =>
					process.parentPid === child.pid && process.pid !== start.childPid,
			)
			.map((process) => process.pid);
		workerTimerPids = liveProcesses
			.filter((process) => workerPids.includes(process.parentPid))
			.map((process) => process.pid);
		if (workerPids.length >= 2 && workerTimerPids.length >= 2) break;
		await delay(10);
	}
	assert.ok(workerPids.length >= 2, `${caseName} did not expose both stage workers`);
	assert.ok(
		workerTimerPids.length >= 2,
		`${caseName} did not expose both worker timers`,
	);
	const waitForMarker = async (name) => {
		for (let attempt = 0; attempt < 400; attempt += 1) {
			try {
				return await readFile(join(root, name), "utf8");
			} catch {
				await delay(10);
			}
		}
		assert.fail(`${caseName} did not publish ${name}`);
	};
	if (caseName === "precedence-exit-timeout") {
		await writeFile(join(root, "child-release"), "release\n");
		assert.equal((await waitForMarker("winner-observed")).trim(), "exit");
		await writeFile(join(root, "watchdog-release"), "release\n");
		await waitForMarker("watchdog-fired");
		await writeFile(join(root, "supervisor-release"), "release\n");
	} else if (caseName === "precedence-timeout-exit") {
		await writeFile(join(root, "watchdog-release"), "release\n");
		assert.equal((await waitForMarker("winner-observed")).trim(), "timeout");
		await writeFile(join(root, "child-release"), "release\n");
		await waitForMarker("fake-child-exited");
		await writeFile(join(root, "supervisor-release"), "release\n");
	} else if (signal !== null) {
		child.kill(signal);
	}
	const exit = await Promise.race([
		new Promise((resolve, reject) => {
			child.once("error", reject);
			child.once("exit", (code, exitSignal) =>
				resolve({ code, signal: exitSignal }),
			);
		}),
		delay(6_000, undefined, { ref: false }).then(() => {
			child.kill("SIGKILL");
			throw new Error(`${caseName} shell selftest exceeded six seconds`);
		}),
	]);
	return { exit, root, stderr, stdout, workerPids, workerTimerPids };
}

async function processExists(pid) {
	if (!Number.isInteger(pid) || pid <= 0) return false;
	return (await processTable()).some((process) => process.pid === pid);
}

test("browser program remains one expression for the Playwright runner", async () => {
	let source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	for (const [marker, replacement] of [
		[
			"__POINTBREAK_BROWSER_DIAGNOSTIC_FAILURE__",
			BrowserDiagnosticFailure.toString(),
		],
		["__POINTBREAK_BROWSER_DIAGNOSTICS__", createBrowserDiagnostics.toString()],
		["__POINTBREAK_CHANGE_BROWSER_CONFIG__", "{}"],
	]) {
		assert.ok(source.includes(marker), `missing browser marker ${marker}`);
		source = source.replace(marker, replacement);
	}

	assert.doesNotThrow(
		() => new Function(`return (${source}\n)`),
		"playwright-cli run-code parses the file as one function expression",
	);
});

test("shakedown mode owns its root and exits after one shared representative case", async () => {
	const shell = await readFile(
		new URL("./change-inspector-browser-verify.sh", import.meta.url),
		"utf8",
	);
	const browser = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const readme = await readFile(
		new URL("./README.md", import.meta.url),
		"utf8",
	);

	assert.match(shell, /--shakedown\) mode="shakedown"; shift ;;/);
	assert.match(
		shell,
		/shakedown_root="\$\(mktemp -d "\$shakedown_parent\/pointbreak-change-inspector-shakedown\.XXXXXX"\)"/,
	);
	assert.match(shell, /cleanup_shakedown_root/);
	assert.match(shell, /--arg mode "\$mode"/);
	assert.match(shell, /mode: \$mode/);
	assert.equal(
		(shell.match(/run_pw run-code --filename=/g) ?? []).length,
		1,
		"both modes must use the same browser runner",
	);

	const materialize = shell.indexOf('"$matrix_materializer" "$fixture_repo"');
	const modeSplit = shell.indexOf('if [ "$mode" = "full" ]; then', materialize);
	const primaryServer = shell.indexOf(
		'"$pointbreak_binary" inspect --repo "$fixture_repo"',
		modeSplit,
	);
	const browserRun = shell.indexOf(
		"run_pw run-code --filename=",
		primaryServer,
	);
	assert.ok(
		materialize >= 0 &&
			modeSplit > materialize &&
			primaryServer > modeSplit &&
			browserRun > primaryServer,
		"shakedown must share materialization, primary server, and browser launch paths",
	);

	const shakedown = browser.indexOf('if (config.mode === "shakedown")');
	const fullMatrix = browser.indexOf(
		'await diagnostics.section("Reader readiness"',
	);
	assert.ok(
		shakedown >= 0 && fullMatrix > shakedown,
		"the shakedown branch must precede the full matrix",
	);
	const branch = browser.slice(shakedown, fullMatrix);
	assert.equal(
		(
			branch.match(
				/diagnostics\.section\("Shakedown exact reading and quiet polling"/g,
			) ?? []
		).length,
		1,
		"shakedown must run exactly one named representative section",
	);
	assert.match(branch, /data-browser-poll-sentinel/);
	assert.match(branch, /\/api\/v2\/profile/);
	assert.match(branch, /profileCompletions/);
	assert.match(branch, /return shakedownResult/);
	assert.match(readme, /--shakedown/);
});

test("single-journey focused shakedowns are literal, root-owned, and exit before the full matrix", async () => {
	const shell = await readFile(
		new URL("./change-inspector-browser-verify.sh", import.meta.url),
		"utf8",
	);
	const browser = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const readme = await readFile(
		new URL("./README.md", import.meta.url),
		"utf8",
	);
	const modes = [
		{
			flag: "--shakedown-timeline-boundary",
			mode: "shakedown-timeline-boundary",
			section: "Shakedown Timeline boundary and quiet polling",
		},
		{
			flag: "--shakedown-exact-history-focus",
			mode: "shakedown-exact-history-focus",
			section: "Shakedown exact history and focus",
		},
	];

	for (const { flag, mode, section } of modes) {
		assert.match(
			shell,
			new RegExp(`${flag.replaceAll("-", "\\-")}\\) mode="${mode}"; shift ;;`),
			`${flag} must have one explicit parser arm`,
		);
		assert.match(readme, new RegExp(flag.replaceAll("-", "\\-")));
		const branchStart = browser.indexOf(`if (config.mode === "${mode}")`);
		const fullMatrix = browser.indexOf(
			'await diagnostics.section("Reader readiness"',
		);
		assert.ok(
			branchStart >= 0 && branchStart < fullMatrix,
			`${mode} must branch before the full matrix`,
		);
		const branch = browser.slice(branchStart, fullMatrix);
		assert.equal(
			(
				branch.match(
					new RegExp(`diagnostics\\.section\\(\\s*"${section}"`, "g"),
				) ?? []
			).length,
			1,
			`${mode} must run exactly one named journey`,
		);
		assert.match(branch, /return focusedShakedownResult/);
	}

	assert.match(
		shell,
		/case "\$mode" in[\s\S]*shakedown\|shakedown-timeline-boundary\|shakedown-exact-history-focus\|shakedown-return-destinations\)/,
		"all four shakedowns must share the self-owned root path",
	);
	assert.match(
		shell,
		/creates its own root and cannot use --root/,
		"a caller root must be rejected for every shakedown",
	);
	assert.doesNotMatch(
		shell,
		/--(?:section|shakedown-section|mode)[= )]/,
		"the harness must not expose a generic section or mode selector",
	);
	assert.match(
		browser,
		/config\.mode !== "full"[\s\S]*config\.mode !== "shakedown"[\s\S]*config\.mode !== "shakedown-timeline-boundary"[\s\S]*config\.mode !== "shakedown-exact-history-focus"[\s\S]*config\.mode !== "shakedown-return-destinations"/,
		"the browser program must reject every mode outside the closed set",
	);
	assert.doesNotMatch(
		browser,
		/new URL\(/,
		"focused modes must remain executable in the Playwright run-code sandbox",
	);
});

test("exact-history shakedown selects the primary Revision resource action", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const branchStart = source.indexOf(
		'if (config.mode === "shakedown-exact-history-focus")',
	);
	const fullMatrix = source.indexOf(
		'await diagnostics.section("Reader readiness"',
		branchStart,
	);
	assert.ok(
		branchStart >= 0 && fullMatrix > branchStart,
		"missing exact-history shakedown branch",
	);
	const branch = source.slice(branchStart, fullMatrix);
	assert.match(
		branch,
		/page\s*\.locator\("#detail-body > \.detail-actions"\)\s*\.getByRole\("button", \{[\s\S]*name: "Open authoritative captured diff",[\s\S]*exact: true,[\s\S]*\}\)/,
		"the journey must distinguish the primary Revision action from association-card actions",
	);
	assert.doesNotMatch(
		branch,
		/const resourceAction = page\.getByRole\(/,
		"the exact-history journey must not count same-named actions across the full page",
	);
});

test("Changes G waits for the terminal destination route, page key, and selected card", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const helperStart = source.indexOf(
		"const waitForChangesTerminalDestination =",
	);
	const sectionStart = source.indexOf(
		'await diagnostics.section("Changes keyboard and filters"',
	);
	const sectionEnd = source.indexOf(
		'await diagnostics.section("Change topology cards"',
		sectionStart,
	);
	assert.notEqual(helperStart, -1, "missing Changes terminal destination wait");
	assert.ok(sectionStart >= 0 && sectionEnd > sectionStart);
	const helper = source.slice(
		helperStart,
		source.indexOf("\n\tconst ", helperStart + 1),
	);
	assert.match(helper, /data-change-page=["']last["']/);
	assert.match(helper, /data-change-target-route/);
	assert.match(helper, /page\.keyboard\.press\("G"\)/);
	assert.match(
		helper,
		/location\.hash !== expectedHash/,
		"the wait must reject every route except the terminal destination",
	);
	assert.match(
		helper,
		/dataset\.changeListKey[\s\S]*JSON\.parse\(rawKey\)/,
		"the wait must bind the painted page key",
	);
	assert.match(
		helper,
		/change-card-selected[\s\S]*unit-card\[data-change-id\][\s\S]*key\.changes\.at\(-1\)/,
		"the destination wait must require its terminal card to be selected",
	);
	const section = source.slice(sectionStart, sectionEnd);
	assert.match(
		section,
		/const changesTraversal = await traverseChangesTerminalAndFirst\(\);[\s\S]*"G boundary"[\s\S]*"g boundary"/,
		"the full Changes journey must await the shared terminal and first-page destinations",
	);
});

test("D69 preserves the 17 untouched full sections, scale producer, and reduced-motion D68 body", async () => {
	const browser = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const shell = await readFile(
		new URL("./change-inspector-browser-verify.sh", import.meta.url),
		"utf8",
	);
	const fullSectionNames = [
		"Reader readiness",
		"Timeline overview and chronology",
		"Timeline search and correlation",
		"Timeline preferences",
		"Timeline keyboard and exact detail",
		"Timeline follow and stale continuation",
		"Changes and Attention paging",
		"Attention guidance",
		"Changes keyboard and filters",
		"Change topology cards",
		"Change relationship graph",
		"Exact Revision selection and history",
		"Shared Revision membership",
		"Split, preferences, and dialogs",
		"Fact relationship graph",
		"Annotated diff",
		"Exact detail and reading",
		"Exact resource availability",
		"Polling retention and reduced motion",
		"Browser runtime",
	];
	const frozenSectionNames = fullSectionNames.filter(
		(name) =>
			name !== "Changes keyboard and filters" &&
			name !== "Polling retention and reduced motion" &&
			name !== "Browser runtime",
	);
	const observedFullSectionNames = [
		...browser.matchAll(/^\tawait diagnostics\.section\("([^"]+)"/gm),
	]
		.map((match) => match[1])
		.slice(-fullSectionNames.length);
	assert.deepEqual(
		observedFullSectionNames,
		fullSectionNames,
		"the complete D68 full-section ledger must remain explicit",
	);
	assert.equal(frozenSectionNames.length, 17);

	const sectionSource = (name) => {
		const start = browser.indexOf(`\tawait diagnostics.section("${name}"`);
		assert.notEqual(start, -1, `missing full section ${name}`);
		const next = browser.indexOf(
			'\n\tawait diagnostics.section("',
			start + 1,
		);
		assert.notEqual(next, -1, `missing boundary after full section ${name}`);
		return browser.slice(start, next + 1);
	};
	const preservation = createHash("sha256");
	for (const name of frozenSectionNames) {
		const source = sectionSource(name);
		preservation.update(
			`section\0${name}\0${Buffer.byteLength(source)}\0`,
		);
		preservation.update(source);
	}
	const scaleStartMarker =
		'if [ "$mode" = "full" ]; then\nprintf \'pub const BROWSER_SCALE: u32 = 0;\\n\' >"$fixture_repo/src/browser-scale.rs"';
	const scaleStart = shell.indexOf(scaleStartMarker);
	const scaleEnd = shell.indexOf(
		"\n# An initial Change capture mints",
		scaleStart,
	);
	assert.ok(
		scaleStart >= 0 && scaleEnd > scaleStart,
		"missing exact full scale-producer slice",
	);
	const scaleSource = shell.slice(scaleStart, scaleEnd);
	preservation.update(
		`shell\0full-scale-producer\0${Buffer.byteLength(scaleSource)}\0`,
	);
	preservation.update(scaleSource);
	assert.equal(
		preservation.digest("hex"),
		"084bdb3253de950ce3072b4d459c9f8ac0af16c71ad264facc25270a701de056",
		"17 passing full sections and the complete scale producer changed from D68",
	);

	const reducedStart = browser.indexOf(
		'\tawait diagnostics.section("Polling retention and reduced motion"',
	);
	const reducedEnd = browser.indexOf(
		"\n\tawait settleResponseInspections();",
		reducedStart,
	);
	assert.ok(
		reducedStart >= 0 && reducedEnd > reducedStart,
		"missing complete reduced-motion section boundary",
	);
	const reducedSource = browser.slice(reducedStart, reducedEnd);
	const optionalArmLiteral =
		'\t\t\t\t{ profileSupersessionArm: "optional" },\n';
	const armOccurrences = reducedSource.split(optionalArmLiteral).length - 1;
	assert.ok(
		armOccurrences === 0 || armOccurrences === 1,
		"reduced-motion may contain at most the one exact optional arm literal",
	);
	const canonicalReducedSource = reducedSource.replace(optionalArmLiteral, "");
	assert.equal(
		createHash("sha256").update(canonicalReducedSource).digest("hex"),
		"57532687ce2b0b6009a0ded69a3b739f4e42b41fadf861d052409d6393f93b67",
		"reduced-motion differs from D68 beyond its one exact optional arm literal",
	);
});

test("D69 shares one semantic Changes terminal-to-first journey across focused and full paths", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const firstStart = source.indexOf("const waitForChangesFirstDestination =");
	assert.notEqual(firstStart, -1, "missing Changes first-page destination wait");
	const firstEnd = source.indexOf("\n\tconst ", firstStart + 1);
	const firstHelper = source.slice(firstStart, firstEnd);
	assert.match(firstHelper, /location\.hash !== expectedHash/);
	assert.match(
		firstHelper,
		/dataset\.changeListKey[\s\S]*JSON\.parse\(rawKey\)/,
		"the first-page wait must bind the painted list key",
	);
	assert.match(
		firstHelper,
		/JSON\.stringify\(keyQuery\) === JSON\.stringify\(expectedQuery\)/,
		"the first-page wait must bind the exact rendered query",
	);
	assert.match(
		firstHelper,
		/change-card-selected[\s\S]*cards\[0\][\s\S]*key\.changes\[0\]/,
		"the first-page wait must require the first keyed card to be selected",
	);

	const journeyStart = source.indexOf(
		"const traverseChangesTerminalAndFirst =",
	);
	assert.notEqual(journeyStart, -1, "missing shared Changes G-to-g journey");
	const journeyEnd = source.indexOf("\n\tconst ", journeyStart + 1);
	const journey = source.slice(journeyStart, journeyEnd);
	assert.match(
		journey,
		/await waitForChangesTerminalDestination\(\)[\s\S]*page\.keyboard\.press\("g"\)[\s\S]*waitForChangesFirstDestination\(/,
	);

	const focusedStart = source.indexOf(
		'await diagnostics.section("Shakedown Changes terminal return"',
	);
	const focusedEnd = source.indexOf(
		'await diagnostics.section("Shakedown parallel-current exact history"',
		focusedStart,
	);
	const fullStart = source.indexOf(
		'await diagnostics.section("Changes keyboard and filters"',
	);
	const fullEnd = source.indexOf(
		'await diagnostics.section("Change topology cards"',
		fullStart,
	);
	for (const [label, section] of [
		["focused", source.slice(focusedStart, focusedEnd)],
		["full", source.slice(fullStart, fullEnd)],
	]) {
		assert.equal(
			(section.match(/traverseChangesTerminalAndFirst\(/g) ?? []).length,
			1,
			`${label} Changes must call the shared G-to-g journey exactly once`,
		);
	}
});

test("D69 profile supersession accounting is exact-object, explicit-arm, and fail-closed", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const classifierStart = source.indexOf(
		"function isAdmissibleProfileSupersessionFailure(",
	);
	assert.notEqual(
		classifierStart,
		-1,
		"missing pure profile-supersession classifier",
	);
	const classifierEnd = source.indexOf("\n\tconst ", classifierStart);
	const classify = new Function(
		`${source.slice(classifierStart, classifierEnd)}\nreturn isAdmissibleProfileSupersessionFailure;`,
	)();
	const request = {};
	const transition = {
		arm: "optional",
		sourceHash: "#/changes?limit=100&order=change_id_asc",
		targetHash: "#/timeline?limit=100&order=desc",
		profileRequestsBeforeNavigation: [request],
		destinationSucceeded: true,
	};
	const record = {
		request,
		transition,
		method: "GET",
		resourceType: "fetch",
		url: "http://127.0.0.1:4173/api/v2/profile",
		error: "net::ERR_ABORTED",
	};
	assert.equal(
		classify(record, "http://127.0.0.1:4173"),
		true,
		"the exact successful pre-navigation profile supersession must be admissible",
	);
	for (const [label, candidate] of [
		["unarmed", { ...record, transition: { ...transition, arm: null } }],
		["new request", { ...record, request: {} }],
		["wrong method", { ...record, method: "POST" }],
		["wrong resource", { ...record, resourceType: "document" }],
		["wrong origin", { ...record, url: "http://127.0.0.1:4174/api/v2/profile" }],
		["wrong endpoint", { ...record, url: "http://127.0.0.1:4173/api/v2/changes" }],
		["wrong error", { ...record, error: "net::ERR_FAILED" }],
		[
			"multiple profile requests",
			{
				...record,
				transition: {
					...transition,
					profileRequestsBeforeNavigation: [request, {}],
				},
			},
		],
		[
			"unsuccessful destination",
			{
				...record,
				transition: { ...transition, destinationSucceeded: false },
			},
		],
		[
			"wrong source lens",
			{
				...record,
				transition: { ...transition, sourceHash: "#/attention" },
			},
		],
		[
			"wrong destination lens",
			{
				...record,
				transition: { ...transition, targetHash: "#/attention" },
			},
		],
	]) {
		assert.equal(
			classify(candidate, "http://127.0.0.1:4173"),
			false,
			`${label} must remain fatal`,
		);
	}

	assert.match(source, /function createProfileRequestLifecycle\(/);
	assert.match(source, /page\.on\("request", requestStarted\)/);
	assert.match(
		source,
		/page\.on\("response",[\s\S]*requestTerminated\(response\.request\(\), "response"\)/,
	);
	assert.match(
		source,
		/page\.on\("requestfinished",[\s\S]*requestTerminated\(request, "requestfinished"\)/,
	);
	assert.match(
		source,
		/page\.on\("requestfailed",[\s\S]*requestTerminated\(request, "requestfailed"\)/,
	);
	assert.match(
		source,
		/profileRequestsBeforeNavigation[\s\S]*page\.goto\(targetUrl/,
		"the exact Request-object snapshot must precede navigation",
	);
	assert.equal(
		(source.match(/\{ profileSupersessionArm: "optional" \}/g) ?? []).length,
		1,
		"full reduced-motion must own exactly one optional arm argument",
	);
	assert.equal(
		(source.match(/\{ profileSupersessionArm: "required" \}/g) ?? []).length,
		1,
		"the fourth focused section must own exactly one required arm argument",
	);
	assert.match(
		source,
		/admissibleProfileSupersessionFailures\.length <= 1/,
		"the whole invocation may admit at most one exact supersession failure",
	);
});

test("D70 production lifecycle binds committed documents and exact accepted route visits", async () => {
	const result = await runD70BrowserSelftest(
		async ({ createProfileRequestLifecycle, settlementTimeoutMs }) => {
			assert.equal(settlementTimeoutMs, 30_000);
			const page = new FakePage(
				"http://127.0.0.1:4173/#/?token=bootstrap-secret",
			);
			const timers = new ManualTimers();
			const lifecycleFailures = [];
			const requestFailures = [];
			const lifecycle = createProfileRequestLifecycle({
				page,
				primaryBaseUrl: "http://127.0.0.1:4173",
				now: () => timers.now,
				setTimer: timers.setTimeout,
				clearTimer: timers.clearTimeout,
				onLifecycleFailure: (failure) => lifecycleFailures.push(failure),
				onRequestFailure: (failure) => requestFailures.push(failure),
			});

			const staleBootstrap = new FakeRequest({ frame: page.mainFrame() });
			page.emit("request", staleBootstrap);
			assert.equal(
				lifecycle.requestRecord(staleBootstrap).sourceHash,
				"#/",
				"request-start ownership must not retain the bootstrap capability",
			);
			const navigationRoot = new FakeRequest({
				frame: page.mainFrame(),
				navigation: true,
				resourceType: "document",
				url: "http://127.0.0.1:4173/redirect-one",
			});
			const navigationRedirect = new FakeRequest({
				frame: page.mainFrame(),
				navigation: true,
				resourceType: "document",
				redirectedFrom: navigationRoot,
				url: "http://127.0.0.1:4173/redirect-two",
			});
			const navigationFinal = new FakeRequest({
				frame: page.mainFrame(),
				navigation: true,
				resourceType: "document",
				redirectedFrom: navigationRedirect,
				url: "http://127.0.0.1:4173/",
			});
			for (const request of [
				navigationRoot,
				navigationRedirect,
				navigationFinal,
			]) {
				page.emit("request", request);
				page.emit("response", new FakeResponse(request));
				page.emit("requestfinished", request);
				assert.equal(lifecycle.state().committedDocumentGeneration, 0);
				assert.equal(
					lifecycle.state().pendingNavigationRoot,
					navigationRoot,
					"successful Request settlement must not erase the redirect-root token",
				);
			}
			page.setUrl(
				"http://127.0.0.1:4173/#/changes?limit=100&order=change_id_asc",
			);
			page.emit("domcontentloaded");
			assert.equal(lifecycle.state().committedDocumentGeneration, 1);
			assert.equal(lifecycle.state().pendingNavigationRoot, null);
			assert.equal(lifecycle.requestRecord(staleBootstrap).status, "retired");
			page.emit("domcontentloaded");
			assert.equal(
				lifecycle.state().committedDocumentGeneration,
				1,
				"a DCL without a pending redirect root must not advance generation",
			);
			page.emit("requestfailed", staleBootstrap);
			assert.equal(requestFailures.at(-1).transition, null);
			assert.equal(requestFailures.at(-1).retired, true);

			const acceptedVisit = lifecycle.createRouteVisitIntent(
				"#/changes?limit=100&order=change_id_asc",
			);
			lifecycle.activateRouteVisit(acceptedVisit);
			const currentPoll = new FakeRequest({ frame: page.mainFrame() });
			page.emit("request", currentPoll);
			assert.equal(
				lifecycle.requestRecord(currentPoll).routeVisitId,
				acceptedVisit.id,
				"a poll begun after route intent but before readiness must retain the visit",
			);
			assert.equal(
				lifecycle.certifyChangesVisit(acceptedVisit, acceptedVisit.intendedHash),
				true,
			);
			assert.deepEqual(lifecycle.eligibleProfileRequests(acceptedVisit), [
				currentPoll,
			]);
			assert.equal(
				await lifecycle.awaitRequiredProfileRequest(acceptedVisit),
				currentPoll,
				"the required arm must reuse an already-outstanding eligible poll",
			);
			const arm = lifecycle.snapshotProfileArm({
				arm: "optional",
				targetHash: "#/timeline?limit=100&order=desc",
			});
			assert.deepEqual(arm.candidates, [currentPoll]);
			assert.equal(lifecycle.state().currentRouteVisit, null);
			assert.deepEqual(
				lifecycle.eligibleProfileRequests(acceptedVisit),
				[],
				"a consumed visit must stay closed after the outgoing arm",
			);
			assert.ok(
				lifecycle.requestRecord(currentPoll).ordinal >
					lifecycle.requestRecord(staleBootstrap).ordinal,
				"request ordinals must be monotonic and invocation-local",
			);
			assert.equal(lifecycle.requestRecord(currentPoll).documentGeneration, 1);
			assert.equal(lifecycle.requestRecord(currentPoll).initiator, "main-frame");

			const sameHashVisit = lifecycle.createRouteVisitIntent(
				acceptedVisit.intendedHash,
			);
			lifecycle.activateRouteVisit(sameHashVisit);
			assert.equal(
				lifecycle.certifyChangesVisit(sameHashVisit, sameHashVisit.intendedHash),
				true,
			);
			assert.deepEqual(
				lifecycle.eligibleProfileRequests(sameHashVisit),
				[],
				"an earlier identical-hash visit must not regain ownership",
			);
			const generationBeforeSameDocumentRoute =
				lifecycle.state().committedDocumentGeneration;
			page.emit("framenavigated", page.mainFrame());
			assert.equal(
				lifecycle.state().currentRouteVisit,
				null,
				"an unowned main-frame route event must invalidate even an identical hash",
			);
			assert.equal(
				lifecycle.state().committedDocumentGeneration,
				generationBeforeSameDocumentRoute,
				"a same-document route event must not advance document generation",
			);

			const acceptedLifecycle = ({
				url = "http://127.0.0.1:4173/#/changes?limit=100&order=change_id_asc",
			} = {}) => {
				const candidatePage = new FakePage(url);
				const candidateFailures = [];
				const candidateLifecycle = createProfileRequestLifecycle({
					page: candidatePage,
					primaryBaseUrl: "http://127.0.0.1:4173",
					now: () => timers.now,
					setTimer: timers.setTimeout,
					clearTimer: timers.clearTimeout,
					onLifecycleFailure: (failure) =>
						candidateFailures.push(failure),
					onRequestFailure: () => {},
				});
				const candidateVisit = candidateLifecycle.createRouteVisitIntent(
					"#/changes?limit=100&order=change_id_asc",
				);
				candidateLifecycle.activateRouteVisit(candidateVisit);
				return {
					failures: candidateFailures,
					lifecycle: candidateLifecycle,
					page: candidatePage,
					visit: candidateVisit,
				};
			};

			const zero = acceptedLifecycle();
			assert.equal(
				zero.lifecycle.certifyChangesVisit(
					zero.visit,
					zero.visit.intendedHash,
				),
				true,
			);
			const zeroArm = zero.lifecycle.snapshotProfileArm({
				arm: "optional",
				targetHash: "#/timeline?limit=100&order=desc",
			});
			assert.deepEqual(zeroArm.candidates, []);
			assert.equal(zeroArm.transition, null);
			assert.equal(zero.lifecycle.currentAcceptedRouteVisit(), null);

			const future = acceptedLifecycle();
			assert.equal(
				future.lifecycle.certifyChangesVisit(
					future.visit,
					future.visit.intendedHash,
				),
				true,
			);
			const futureRequired = future.lifecycle.awaitRequiredProfileRequest(
				future.visit,
			);
			const futurePoll = new FakeRequest({ frame: future.page.mainFrame() });
			future.page.emit("request", futurePoll);
			assert.equal(await futureRequired, futurePoll);

			const completedFuture = acceptedLifecycle();
			assert.equal(
				completedFuture.lifecycle.certifyChangesVisit(
					completedFuture.visit,
					completedFuture.visit.intendedHash,
				),
				true,
			);
			const completedRequired =
				completedFuture.lifecycle.awaitRequiredProfileRequest(
					completedFuture.visit,
				);
			const completedFuturePoll = new FakeRequest({
				frame: completedFuture.page.mainFrame(),
			});
			completedFuture.page.emit("request", completedFuturePoll);
			completedFuture.page.emit(
				"response",
				new FakeResponse(completedFuturePoll),
			);
			assert.equal(await completedRequired, completedFuturePoll);
			const completedRequiredArm =
				completedFuture.lifecycle.snapshotProfileArm({
					arm: "required",
					targetHash: "#/timeline?limit=100&order=desc",
				});
			assert.deepEqual(completedRequiredArm.candidates, []);
			assert.equal(completedRequiredArm.transition, null);
			assert.equal(
				completedFuture.lifecycle.currentAcceptedRouteVisit(),
				completedFuture.visit,
				"a completed required poll must retain the accepted source visit for retry",
			);

			const multiple = acceptedLifecycle();
			const firstConcurrent = new FakeRequest({
				frame: multiple.page.mainFrame(),
			});
			const secondConcurrent = new FakeRequest({
				frame: multiple.page.mainFrame(),
			});
			multiple.page.emit("request", firstConcurrent);
			multiple.page.emit("request", secondConcurrent);
			assert.equal(
				multiple.lifecycle.certifyChangesVisit(
					multiple.visit,
					multiple.visit.intendedHash,
				),
				true,
			);
			assert.equal(
				await multiple.lifecycle.awaitRequiredProfileRequest(multiple.visit),
				firstConcurrent,
				"the subsequent snapshot, not the wait helper, owns multiple-candidate refusal",
			);
			const multipleArm = multiple.lifecycle.snapshotProfileArm({
				arm: "optional",
				targetHash: "#/timeline?limit=100&order=desc",
			});
			assert.deepEqual(multipleArm.candidates, [
				firstConcurrent,
				secondConcurrent,
			]);
			assert.equal(multipleArm.transition, null);
			assert.equal(multiple.lifecycle.currentAcceptedRouteVisit(), null);

			const ineligible = acceptedLifecycle();
			const completed = new FakeRequest({ frame: ineligible.page.mainFrame() });
			for (const request of [
				new FakeRequest({ frame: new FakeFrame(() => "about:blank") }),
				new FakeRequest({ frameUnavailable: true }),
				new FakeRequest({ frame: ineligible.page.mainFrame(), method: "POST" }),
				new FakeRequest({
					frame: ineligible.page.mainFrame(),
					resourceType: "document",
				}),
				new FakeRequest({
					frame: ineligible.page.mainFrame(),
					url: "http://127.0.0.1:4173/api/v2/history",
				}),
				completed,
			]) {
				ineligible.page.emit("request", request);
			}
			ineligible.page.emit("response", new FakeResponse(completed));
			assert.equal(
				ineligible.lifecycle.certifyChangesVisit(
					ineligible.visit,
					ineligible.visit.intendedHash,
				),
				true,
			);
			assert.deepEqual(
				ineligible.lifecycle.eligibleProfileRequests(ineligible.visit),
				[],
				"subframe, unavailable-frame, non-GET/fetch/path, and completed requests must be ineligible",
			);
			assert.deepEqual(ineligible.failures, []);

			const wrongSource = acceptedLifecycle({
				url: "http://127.0.0.1:4173/#/attention",
			});
			const wrongSourceRequest = new FakeRequest({
				frame: wrongSource.page.mainFrame(),
			});
			wrongSource.page.emit("request", wrongSourceRequest);
			wrongSource.page.setUrl(
				"http://127.0.0.1:4173/#/changes?limit=100&order=change_id_asc",
			);
			assert.equal(
				wrongSource.lifecycle.certifyChangesVisit(
					wrongSource.visit,
					wrongSource.visit.intendedHash,
				),
				true,
			);
			assert.deepEqual(
				wrongSource.lifecycle.eligibleProfileRequests(wrongSource.visit),
				[],
			);

			const staleGeneration = acceptedLifecycle();
			const stalePoll = new FakeRequest({
				frame: staleGeneration.page.mainFrame(),
			});
			staleGeneration.page.emit("request", stalePoll);
			assert.equal(
				staleGeneration.lifecycle.certifyChangesVisit(
					staleGeneration.visit,
					staleGeneration.visit.intendedHash,
				),
				true,
			);
			const replacementRoot = new FakeRequest({
				frame: staleGeneration.page.mainFrame(),
				navigation: true,
				resourceType: "document",
			});
			staleGeneration.page.emit("request", replacementRoot);
			staleGeneration.page.emit("domcontentloaded");
			assert.equal(
				staleGeneration.lifecycle.requestRecord(stalePoll).status,
				"retired",
			);
			assert.deepEqual(
				staleGeneration.lifecycle.eligibleProfileRequests(
					staleGeneration.visit,
				),
				[],
			);

			const negativePage = new FakePage();
			const negativeFailures = [];
			const negative = createProfileRequestLifecycle({
				page: negativePage,
				primaryBaseUrl: "http://127.0.0.1:4173",
				now: () => timers.now,
				setTimer: timers.setTimeout,
				clearTimer: timers.clearTimeout,
				onLifecycleFailure: (failure) => negativeFailures.push(failure),
				onRequestFailure: () => {},
			});
			const failedNavigation = new FakeRequest({
				frame: negativePage.mainFrame(),
				navigation: true,
				resourceType: "document",
			});
			negativePage.emit("request", failedNavigation);
			negativePage.emit("requestfailed", failedNavigation);
			negativePage.emit("domcontentloaded");
			assert.equal(negative.state().committedDocumentGeneration, 0);
			const overlapOne = new FakeRequest({
				frame: negativePage.mainFrame(),
				navigation: true,
				resourceType: "document",
			});
			const overlapTwo = new FakeRequest({
				frame: negativePage.mainFrame(),
				navigation: true,
				resourceType: "document",
			});
			negativePage.emit("request", overlapOne);
			negativePage.emit("request", overlapTwo);
			negativePage.emit("domcontentloaded");
			assert.equal(negative.state().committedDocumentGeneration, 0);
			const subframe = new FakeFrame(() => "http://127.0.0.1:4173/frame");
			negativePage.emit(
				"request",
				new FakeRequest({ frame: subframe, navigation: true }),
			);
			negativePage.emit("domcontentloaded");
			assert.equal(negative.state().committedDocumentGeneration, 0);
			negativePage.emit(
				"request",
				new FakeRequest({ frameUnavailable: true, navigation: false }),
			);
			negativePage.emit(
				"request",
				new FakeRequest({ frame: null, navigation: false }),
			);
			negativePage.emit(
				"request",
				new FakeRequest({ frame: null, navigation: true }),
			);
			negativePage.emit(
				"request",
				new FakeRequest({ frameUnavailable: true, navigation: true }),
			);
			assert.equal(
				negativeFailures.filter((failure) =>
					/frame.*unavailable.*navigation/i.test(failure.detail),
				).length,
				2,
				"null and throwing navigation-frame lookups must both fail fatally",
			);
			assert.ok(
				negativeFailures.some((failure) => /overlap|unrelated/i.test(failure.detail)),
			);
			return true;
		},
	);
	assert.equal(result, true);
});

test("D70 production settlement is bounded, settle-once, and capability-redacted", async () => {
	const result = await runD70BrowserSelftest(
		async ({
			createProfileRequestLifecycle,
			createDiagnostics,
			recordProfileSettlementTimeout,
			settlementTimeoutMs,
		}) => {
			const beforeJoin = createBoundProfileTransition({
				createProfileRequestLifecycle,
			});
			beforeJoin.timers.advanceTo(100_000);
			const beforeJoinPromise = beforeJoin.lifecycle.enterSettlementJoin(
				beforeJoin.arm.transition,
			);
			beforeJoin.timers.advanceTo(129_999);
			beforeJoin.page.emit("requestfinished", beforeJoin.request);
			assert.equal((await beforeJoinPromise).outcome, "requestfinished");
			beforeJoin.timers.advanceTo(130_000);
			assert.equal(beforeJoin.arm.transition.terminalHistory.length, 1);

			const timedOut = createBoundProfileTransition({
				createProfileRequestLifecycle,
				sourceUrl:
					"http://127.0.0.1:4173/#/changes?limit=100&order=change_id_asc&token=bootstrap-secret",
				targetHash:
					"#/timeline?limit=100&order=desc&token=destination-secret",
			});
			const timeoutPromise = timedOut.lifecycle.enterSettlementJoin(
				timedOut.arm.transition,
			);
			timedOut.timers.advanceTo(settlementTimeoutMs, { run: false });
			timedOut.timers.flushDue();
			const timeout = await timeoutPromise;
			assert.equal(timeout.outcome, "timeout");
			assert.equal(
				timedOut.lifecycle.transitionForRequest(timedOut.request),
				null,
				"timeout must detach exact Request ownership before resolution",
			);
			timedOut.page.emit("requestfailed", timedOut.request);
			assert.equal(timedOut.requestFailures.length, 1);
			assert.equal(timedOut.requestFailures[0].transition, null);
			assert.equal(timedOut.arm.transition.terminalHistory.length, 1);

			const diagnostics = createDiagnostics();
			await diagnostics.section("D70 settlement", async () => {
				recordProfileSettlementTimeout(diagnostics, "profile settlement", timeout);
			});
			const report = diagnostics.result({ screenshotCount: 0 });
			assert.equal(report.status, "failed");
			assert.equal(report.globalInvalid, false);
			assert.equal(report.failures.length, 1);
			assert.deepEqual(Object.keys(timeout.diagnostic).sort(), [
				"candidateCount",
				"destinationOutcome",
				"documentGeneration",
				"method",
				"outcome",
				"path",
				"requestAgeMs",
				"requestOrdinal",
				"resourceType",
				"routeVisitId",
				"sourceHash",
				"targetHash",
				"terminalHistory",
				"timeoutMs",
			]);
			const serialized = JSON.stringify(report.failures[0]);
			assert.doesNotMatch(
				serialized,
				/bootstrap-secret|destination-secret|authorization|bearer|actor:|store content|response body/i,
			);
			assert.match(serialized, /requestOrdinal/);
			assert.match(serialized, /documentGeneration/);
			assert.match(serialized, /routeVisitId/);
			assert.match(serialized, /terminalHistory/);

			const terminalFirst = createBoundProfileTransition({
				createProfileRequestLifecycle,
			});
			const terminalFirstPromise = terminalFirst.lifecycle.enterSettlementJoin(
				terminalFirst.arm.transition,
			);
			terminalFirst.timers.advanceTo(settlementTimeoutMs, { run: false });
			terminalFirst.page.emit("requestfailed", terminalFirst.request);
			terminalFirst.timers.flushDue();
			assert.equal((await terminalFirstPromise).outcome, "requestfailed");
			assert.equal(terminalFirst.arm.transition.terminalHistory.length, 1);

			const timeoutFirst = createBoundProfileTransition({
				createProfileRequestLifecycle,
			});
			const timeoutFirstPromise = timeoutFirst.lifecycle.enterSettlementJoin(
				timeoutFirst.arm.transition,
			);
			timeoutFirst.timers.advanceTo(settlementTimeoutMs, { run: false });
			timeoutFirst.timers.flushDue();
			timeoutFirst.page.emit("requestfailed", timeoutFirst.request);
			assert.equal((await timeoutFirstPromise).outcome, "timeout");
			assert.equal(timeoutFirst.arm.transition.terminalHistory.length, 1);

			for (const [event, expected] of [
				["response", "response"],
				["requestfinished", "requestfinished"],
				["requestfailed", "requestfailed"],
			]) {
				const terminal = createBoundProfileTransition({
					createProfileRequestLifecycle,
				});
				const promise = terminal.lifecycle.enterSettlementJoin(
					terminal.arm.transition,
				);
				terminal.page.emit(
					event,
					event === "response"
						? new FakeResponse(terminal.request)
						: terminal.request,
				);
				assert.equal((await promise).outcome, expected);
			}

			const replaced = createBoundProfileTransition({
				createProfileRequestLifecycle,
			});
			const replacementNavigation = new FakeRequest({
				frame: replaced.page.mainFrame(),
				navigation: true,
				resourceType: "document",
			});
			replaced.page.emit("request", replacementNavigation);
			replaced.page.emit("domcontentloaded");
			assert.equal(
				(
					await replaced.lifecycle.enterSettlementJoin(replaced.arm.transition)
				).outcome,
				"document-replaced",
			);
			return true;
		},
	);
	assert.equal(result, true);
});

test("D70 shell stage helper publishes live terminal evidence and reaps its process group", async () => {
	const expectations = [
		{
			caseName: "exit-0",
			expectedCode: 0,
			terminalCode: 0,
			winner: "exit",
			cleanup: "complete",
		},
		{
			caseName: "exit-23",
			expectedCode: 23,
			terminalCode: 23,
			winner: "exit",
			cleanup: "complete",
		},
		{
			caseName: "timeout",
			expectedCode: 124,
			terminalCode: 124,
			winner: "timeout",
			cleanup: "complete",
		},
		{
			caseName: "signal-int",
			signal: "SIGINT",
			expectedCode: 130,
			terminalCode: 130,
			terminalSignal: "INT",
			winner: "signal",
			cleanup: "complete",
		},
		{
			caseName: "signal-term",
			signal: "SIGTERM",
			expectedCode: 143,
			terminalCode: 143,
			terminalSignal: "TERM",
			winner: "signal",
			cleanup: "complete",
		},
		{
			caseName: "precedence-exit-timeout",
			expectedCode: 0,
			terminalCode: 0,
			winner: "exit",
			cleanup: "complete",
		},
		{
			caseName: "precedence-timeout-exit",
			expectedCode: 124,
			terminalCode: 124,
			winner: "timeout",
			cleanup: "complete",
		},
		{
			caseName: "heartbeat-failed",
			expectedCode: 125,
			terminalCode: 125,
			winner: "internal",
			cleanup: "complete",
		},
		{
			caseName: "heartbeat-late",
			expectedCode: 125,
			terminalCode: 125,
			winner: "internal",
			cleanup: "complete",
		},
		{
			caseName: "heartbeat-render-hung",
			expectedCode: 0,
			terminalCode: 0,
			winner: "exit",
			cleanup: "complete",
		},
		{
			caseName: "ps-failed",
			expectedCode: 125,
			terminalCode: 0,
			winner: "exit",
			cleanup: "failed",
		},
	];
	for (const {
		caseName,
		cleanup,
		expectedCode,
		signal = null,
		terminalCode,
		terminalSignal = null,
		winner,
	} of expectations) {
		const execution = await runD70ShellSelftest(caseName, { signal });
		assert.equal(
			execution.exit.code,
			expectedCode,
			`${caseName} exit mismatch: ${execution.stderr}`,
		);
		assert.equal(execution.exit.signal, null);
		const startBytes = await readFile(
			join(execution.root, "logs", "browser-stage-start.json"),
			"utf8",
		);
		const heartbeatBytes = await readFile(
			join(execution.root, "logs", "browser-stage-heartbeat.log"),
			"utf8",
		);
		const terminalPath = join(
			execution.root,
			"logs",
			"browser-stage-terminal.json",
		);
		const terminalBytes = await readFile(terminalPath, "utf8");
		const start = JSON.parse(startBytes);
		const terminal = JSON.parse(terminalBytes);
		assert.equal(start.schema, "pointbreak.browser-stage-start");
		assert.equal(start.version, 1);
		assert.equal(start.mode, "selftest");
		assert.equal(terminal.schema, "pointbreak.browser-stage-terminal");
		assert.equal(terminal.version, 1);
		assert.equal(terminal.winner, winner);
		assert.equal(terminal.exitCode, terminalCode);
		assert.equal(terminal.signal, terminalSignal);
		assert.equal(terminal.runCodeGroupCleanup, cleanup);
		assert.equal(terminal.browserSessionCleanup, "pending");
		assert.match(execution.stdout, /pointbreak\.browser-stage-terminal/);
		const heartbeats = heartbeatBytes
			.trim()
			.split("\n")
			.filter(Boolean)
			.map((line) => JSON.parse(line));
		assert.ok(heartbeats.length >= 1);
		for (const heartbeat of heartbeats) {
			assert.deepEqual(Object.keys(heartbeat).sort(), [
				"childAlive",
				"elapsedSeconds",
				"gateLogBytes",
				"latestScreenshot",
				"mode",
				"screenshotCount",
			]);
		}
		assert.doesNotMatch(
			`${startBytes}${heartbeatBytes}${terminalBytes}`,
			/token|authorization|bearer|actor:|store content|response body/i,
		);
		assert.equal(await processExists(terminal.childPid), false);
		const descendantPath = join(execution.root, "fake-descendant.pid");
		const descendantPid = Number(
			(await readFile(descendantPath, "utf8")).trim(),
		);
		assert.equal(await processExists(descendantPid), false);
		for (const workerPid of [
			...execution.workerPids,
			...execution.workerTimerPids,
		]) {
			assert.equal(await processExists(workerPid), false);
		}
		let renderPidBytes = null;
		try {
			renderPidBytes = await readFile(
				join(execution.root, "heartbeat-render.pid"),
				"utf8",
			);
		} catch (error) {
			if (error?.code !== "ENOENT") throw error;
		}
		if (renderPidBytes !== null) {
			assert.equal(await processExists(Number(renderPidBytes.trim())), false);
		}
		assert.deepEqual(
			(await processTable())
				.filter(
					(process) => process.processGroupId === terminal.processGroupId,
				)
				.map((process) => process.pid),
			[],
			`${caseName} left members in the recorded process group`,
		);
		await delay(100);
		assert.equal(await readFile(terminalPath, "utf8"), terminalBytes);
		assert.equal(
			await readFile(
				join(execution.root, "logs", "browser-stage-heartbeat.log"),
				"utf8",
			),
			heartbeatBytes,
		);
		await assert.rejects(
			readFile(join(execution.root, "logs", "browser-result.json")),
			/ENOENT/,
		);
		await assert.rejects(
			readFile(join(execution.root, "manifest.json")),
			/ENOENT/,
		);
	}
});

test("D70 stage contracts are production-bounded, inventoried, and documented", async () => {
	const shell = await readFile(
		new URL("./change-inspector-browser-verify.sh", import.meta.url),
		"utf8",
	);
	const browser = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const readme = await readFile(new URL("./README.md", import.meta.url), "utf8");
	assert.match(browser, /PROFILE_SUPERSESSION_SETTLEMENT_TIMEOUT_MS\s*=\s*30_000/);
	assert.match(browser, /__pointbreakD70Selftest/);
	assert.match(
		browser,
		/profileSupersessionArm === "required"[\s\S]*profileRequestsBeforeNavigation\.length === 0[\s\S]*navigationPerformed: false[\s\S]*profileSupersessionOutcome: "completed-before-navigation"/,
	);
	assert.match(shell, /BROWSER_PROGRAM_TIMEOUT_SECONDS=600/);
	assert.match(shell, /BROWSER_STAGE_HEARTBEAT_SECONDS=15/);
	assert.match(shell, /BROWSER_STAGE_GROUP_CLEANUP_SECONDS=10/);
	assert.match(
		shell,
		/set -m[\s\S]*\) >"\$stage_gate_log" 2>&1 &[\s\S]*child_pid=\$![\s\S]*set \+m/,
	);
	assert.match(shell, /command in[\s\S]*\bps\b/);
	const helper = shell.slice(
		shell.indexOf("run_browser_program_stage()"),
		shell.indexOf("browser_stage_selftest_child()"),
	);
	assert.match(
		helper,
		/shell_group_id=""[\s\S]*read_process_group_id "\$\$"[\s\S]*is_positive_process_id "\$shell_group_id"/,
	);
	assert.ok(
		helper.indexOf("trap 'observe_browser_stage_terminal signal 130 INT") <
			helper.indexOf('atomic_publish_stage_json "$stage_start"'),
		"handled stage signals must be owned before the start receipt is published",
	);
	assert.ok(
		helper.lastIndexOf("trap - INT TERM USR1") >
			helper.lastIndexOf('atomic_publish_stage_json "$stage_terminal"'),
		"handled stage signals must remain owned through terminal publication",
	);
	const browserStageCall = shell.indexOf("run_browser_program_stage ", 1);
	const browserStageStatusCheck = shell.indexOf(
		'if [ "$browser_gate_status" -ne 0 ]',
		browserStageCall,
	);
	const browserResultWrite = shell.indexOf(
		'printf \'%s\\n\' "$browser_result_line" >"$browser_result"',
		browserStageCall,
	);
	assert.ok(
		browserStageStatusCheck > browserStageCall &&
			browserStageStatusCheck < browserResultWrite,
		"a failed browser-program stage must stop before browser-result publication",
	);
	assert.doesNotMatch(
		shell.slice(shell.indexOf("usage()"), shell.indexOf("EOF", shell.indexOf("usage()"))),
		/POINTBREAK_BROWSER_STAGE_SELFTEST/,
	);
	for (const path of [
		"logs/browser-stage-start.json",
		"logs/browser-stage-heartbeat.log",
		"logs/browser-stage-terminal.json",
	]) {
		assert.match(shell, new RegExp(path.replaceAll("/", "\\/")));
		assert.match(readme, new RegExp(path.replaceAll("/", "\\/")));
	}
	assert.match(readme, /run-code[^.]*600 seconds/i);
	assert.match(readme, /heartbeat[^.]*15 seconds/i);
	assert.match(readme, /process group[\s\S]*10 seconds/i);
	assert.match(readme, /browser session cleanup[\s\S]*pending/i);
	assert.match(readme, /timeout[\s\S]*browser-result\.json[\s\S]*manifest\.json/i);
});

test("retained-master Back waits for the complete destination state", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const helperStart = source.indexOf(
		"const waitForRetainedMasterDestination =",
	);
	assert.notEqual(helperStart, -1, "missing retained-master destination wait");
	const helper = source.slice(
		helperStart,
		source.indexOf("\n\tconst ", helperStart + 1),
	);
	assert.match(
		helper,
		/URLSearchParams[\s\S]*normalize\(location\.hash\)[\s\S]*split-closed[\s\S]*detail\?\.inert[\s\S]*aria-hidden[\s\S]*master\?\.contains\(document\.activeElement\)/,
		"Back readiness must normalize the route and combine it with closed, hidden, retained-master state",
	);
	const sectionStart = source.indexOf(
		'await diagnostics.section("Exact detail and reading"',
	);
	const sectionEnd = source.indexOf(
		'await diagnostics.section("Exact resource availability"',
		sectionStart,
	);
	const section = source.slice(sectionStart, sectionEnd);
	const click = section.indexOf('await page.locator("#detail-back").click();');
	const wait = section.indexOf(
		"await waitForRetainedMasterDestination(",
		click,
	);
	const assertion = section.indexOf("const narrowDetailClosed", click);
	assert.ok(
		click >= 0 && wait > click && assertion > wait,
		"the complete Back wait must precede every sampled destination assertion",
	);
});

test("full return journeys await complete semantic destinations", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const helperStart = source.indexOf(
		"const waitForRetainedMasterDestination =",
	);
	assert.notEqual(
		helperStart,
		-1,
		"the retained-master predicate must have a viewport-neutral name",
	);
	const helper = source.slice(
		helperStart,
		source.indexOf("\n\tconst ", helperStart + 1),
	);
	assert.match(
		helper,
		/URLSearchParams[\s\S]*normalize\(location\.hash\)[\s\S]*split-closed[\s\S]*detail\?\.inert[\s\S]*aria-hidden[\s\S]*master\?\.contains\(document\.activeElement\)/,
		"retained-master readiness must preserve route, closed, hidden, inert, and focus predicates",
	);

	const timelineStart = source.indexOf(
		'await diagnostics.section("Timeline keyboard and exact detail"',
	);
	const timelineEnd = source.indexOf(
		'await diagnostics.section("Timeline follow and stale continuation"',
		timelineStart,
	);
	assert.ok(timelineStart >= 0 && timelineEnd > timelineStart);
	const timeline = source.slice(timelineStart, timelineEnd);
	const timelineClick = timeline.lastIndexOf(
		'await page.locator("#detail-back").click();',
	);
	const timelineRoute = timeline.indexOf(
		"await waitForTimelineRoute(narrowTimelineHash);",
		timelineClick,
	);
	const timelineDestination = timeline.indexOf(
		"await waitForRetainedMasterDestination(",
		timelineRoute,
	);
	const timelineAssertion = timeline.indexOf(
		'"narrow Timeline event return"',
		timelineDestination,
	);
	assert.ok(
		timelineClick >= 0 &&
			timelineRoute > timelineClick &&
			timelineDestination > timelineRoute &&
			timelineAssertion > timelineDestination,
		"full Timeline return must keep its route/dataset wait, then await retained-master completion before sampling inertness",
	);

	const changesStart = source.indexOf(
		'await diagnostics.section("Changes keyboard and filters"',
	);
	const changesEnd = source.indexOf(
		'await diagnostics.section("Change topology cards"',
		changesStart,
	);
	assert.ok(changesStart >= 0 && changesEnd > changesStart);
	const changes = source.slice(changesStart, changesEnd);
	const changesEscape = changes.indexOf('await page.keyboard.press("Escape");');
	const changesDestination = changes.indexOf(
		"await waitForRetainedMasterDestination(",
		changesEscape,
	);
	const viewFocus = changes.indexOf("await viewToggle.focus();", changesEscape);
	const changesTraversal = changes.indexOf(
		"await traverseChangesTerminalAndFirst();",
		changesEscape,
	);
	assert.ok(
		changesEscape >= 0 &&
			changesDestination > changesEscape &&
			viewFocus > changesDestination &&
		changesTraversal > viewFocus,
		"full Changes return must complete its retained master before View and shared G-to-g traversal",
	);

	const exactStart = source.indexOf(
		'await diagnostics.section("Exact Revision selection and history"',
	);
	const exactEnd = source.indexOf(
		'await diagnostics.section("Shared Revision membership"',
		exactStart,
	);
	assert.ok(exactStart >= 0 && exactEnd > exactStart);
	const exact = source.slice(exactStart, exactEnd);
	const lateJourney = exact.slice(exact.indexOf("const revisionHash ="));
	assert.match(
		lateJourney,
		/const revisionHash = await hash\(\);[\s\S]*const revisionRoute = revisionHash\.slice\(2\);/,
		"the full exact journey must retain its accepted Revision identity",
	);
	assert.match(
		lateJourney,
		/const resourceReady = await page\.waitForFunction\([\s\S]*isAcceptedExactReadingInPage,[\s\S]*expectedHash: resourceHash,[\s\S]*expectedRoute: resourceRoute,[\s\S]*priorKeys: \{ reading: revisionReadingKey \}[\s\S]*resourceReadiness\.state === "refused"[\s\S]*exact route focus[\s\S]*await page\.goBack\(\)/,
		"resource focus and Back must follow exact accepted resource readiness",
	);
	assert.match(
		lateJourney,
		/await page\.goBack\(\);[\s\S]*const revisionReady = await page\.waitForFunction\([\s\S]*isAcceptedExactReadingInPage,[\s\S]*expectedHash: revisionHash,[\s\S]*expectedRoute: revisionRoute,[\s\S]*priorKeys: \{ reading: resourceReadingKey \}[\s\S]*revisionReadiness\.state === "refused"[\s\S]*await page\.keyboard\.press\("3"\)/,
		"later history must follow exact accepted Back-target Revision readiness",
	);
});

test("return-destinations shakedown closes every executable dependency", async () => {
	const shell = await readFile(
		new URL("./change-inspector-browser-verify.sh", import.meta.url),
		"utf8",
	);
	const browser = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const readme = await readFile(
		new URL("./README.md", import.meta.url),
		"utf8",
	);
	const flag = "--shakedown-return-destinations";
	const mode = "shakedown-return-destinations";
	const sections = [
		"Shakedown retained Timeline return",
		"Shakedown Changes terminal return",
		"Shakedown parallel-current exact history",
		"Shakedown poll supersession request accounting",
	];
	const screenshots = [
		"shakedown-retained-timeline-return",
		"shakedown-changes-terminal-return",
		"shakedown-parallel-current-exact-history",
		"shakedown-poll-supersession-request-accounting",
	];

	assert.match(
		shell,
		/--shakedown-return-destinations\) mode="shakedown-return-destinations"; shift ;;/,
		"the semantic mode must have one explicit parser arm",
	);
	assert.match(
		shell,
		/shakedown\|shakedown-timeline-boundary\|shakedown-exact-history-focus\|shakedown-return-destinations\)/,
		"the semantic mode must share the self-owned shakedown root path",
	);
	assert.match(readme, /--shakedown-return-destinations/);
	const branchStart = browser.indexOf(`if (config.mode === "${mode}")`);
	const fullMatrix = browser.indexOf(
		'await diagnostics.section("Reader readiness"',
	);
	assert.ok(
		branchStart >= 0 && branchStart < fullMatrix,
		"the semantic mode must branch before the full matrix",
	);
	const branchEnd = browser.indexOf(
		'if (config.mode === "shakedown")',
		branchStart,
	);
	assert.ok(
		branchEnd > branchStart && branchEnd < fullMatrix,
		"the semantic mode must remain a closed literal branch",
	);
	const branch = browser.slice(branchStart, branchEnd);
	assert.equal(
		(branch.match(/await diagnostics\.section\(/g) ?? []).length,
		4,
		"the semantic mode must run exactly four independent journeys",
	);
	const sectionStarts = sections.map((name) =>
		branch.indexOf(`await diagnostics.section("${name}"`),
	);
	assert.ok(
		sectionStarts.every((start) => start >= 0) &&
			sectionStarts.every(
				(start, index) => index === 0 || start > sectionStarts[index - 1],
			),
		"the four exact sections must run in ledger order",
	);
	for (const [index, name] of sections.entries()) {
		const end =
			sectionStarts[index + 1] ??
			branch.indexOf(
				"const focusedShakedownResult = diagnostics.result",
				sectionStarts[index],
			);
		const source = branch.slice(sectionStarts[index], end);
		assert.match(source, /setup:/, `${name} must own setup`);
		assert.match(source, /run:/, `${name} must own its transition`);
		assert.match(
			source,
			/teardown: teardownSection/,
			`${name} must own teardown`,
		);
		assert.match(
			source,
			new RegExp(`await screenshot\\("${screenshots[index]}"\\)`),
			`${name} must write its distinct screenshot`,
		);
	}
	const resultIndex = branch.indexOf(
		"const focusedShakedownResult = diagnostics.result",
	);
	assert.ok(
		resultIndex > sectionStarts.at(-1),
		"the aggregate report must be emitted only after all four sections",
	);
	assert.match(branch, /narrow Timeline return/);
	assert.match(branch, /return destinations Changes G/);
	assert.match(branch, /parallel-current resource readiness/);

	const changesSection = branch.slice(sectionStarts[1], sectionStarts[2]);
	assert.match(
		changesSection,
		/const changesQuery = "limit=1&order=change_id_asc";/,
		"the focused Changes section must use the guaranteed pagination query",
	);
	assert.match(
		changesSection,
		/page\.request\.get\([\s\S]*\/api\/v2\/changes\?\$\{changesQuery\}[\s\S]*Authorization:\s*`Bearer \$\{config\.server\.token\}`/,
		"the typed Changes preflight must use the exact Inspector bearer credential",
	);
	assert.match(changesSection, /changesPageResponse\.status\(\)[\s\S]*200/);
	assert.match(
		changesSection,
		/changesPage\.schema === "pointbreak\.inspect-changes-page"/,
	);
	assert.match(changesSection, /changesPage\.version === 1/);
	assert.match(changesSection, /changesPage\.changes\.length === 1/);
	assert.match(
		changesSection,
		/typeof changesPage\.next === "string"[\s\S]*changesPage\.next\.length > 0/,
	);
	assert.match(
		changesSection,
		/typeof changesPage\.last === "string"[\s\S]*changesPage\.last\.length > 0/,
	);
	assert.match(
		changesSection,
		/listKey\.projectionStamp === expected\.projectionStamp[\s\S]*listKey\.last === expected\.last[\s\S]*targetAfter === expected\.last[\s\S]*targetRoute !== location\.hash/,
		"the preflight response must be bound to the exact rendered list and terminal capability",
	);
	const changesOpen = branch.indexOf('"return destinations Changes setup",');
	const changesCursorFocus = branch.indexOf(
		'await page.locator("#master").focus();',
		changesOpen,
	);
	const changesCursorDown = branch.indexOf(
		'await page.keyboard.press("j");',
		changesOpen,
	);
	const selectedChangeRead = branch.indexOf(
		"const selectedChange = await selected().getAttribute",
		changesOpen,
	);
	const selectedChangeEnter = branch.indexOf(
		'await page.keyboard.press("Enter");',
		changesOpen,
	);
	assert.ok(
		changesOpen >= 0 &&
			changesCursorFocus > changesOpen &&
			changesCursorDown > changesCursorFocus &&
			selectedChangeRead > changesCursorDown &&
			selectedChangeEnter > selectedChangeRead,
		"the focused Changes journey must establish its local cursor before reading and opening the selected Change",
	);
	const parallelSection = branch.slice(sectionStarts[2], sectionStarts[3]);
	assert.match(
		parallelSection,
		/`changes\?limit=100&order=change_id_asc&topology=parallel_current&q=\$\{encodeURIComponent\(parallel\.change\)\}`/,
		"parallel-current exact history must keep its filtered limit=100 route",
	);
	const pollSection = branch.slice(sectionStarts[3], resultIndex);
	assert.match(
		pollSection,
		/PROFILE_SUPERSESSION_OBSERVATION_MAX_OPPORTUNITIES\s*=\s*3/,
		"poll supersession must cap natural opportunities inside one invocation",
	);
	assert.match(
		pollSection,
		/profileSupersessionArm:\s*"required"/,
		"poll supersession must explicitly arm only its Changes-to-Timeline open",
	);
	assert.doesNotMatch(
		pollSection,
		/waitForTimeout|route\.abort|intercept|retry/i,
		"poll supersession must use natural opportunities without delay, interception, or invocation retry",
	);
	assert.match(
		pollSection,
		/profileSupersessionObserved[\s\S]*Shakedown poll supersession request accounting/,
		"the fourth section must prove one exact admitted supersession",
	);
	assert.match(branch, /return focusedShakedownResult/);
	assert.match(
		shell,
		/\.sections == \[\s*\{name: "Shakedown retained Timeline return", status: "passed", failureCount: 0\},\s*\{name: "Shakedown Changes terminal return", status: "passed", failureCount: 0\},\s*\{name: "Shakedown parallel-current exact history", status: "passed", failureCount: 0\},\s*\{name: "Shakedown poll supersession request accounting", status: "passed", failureCount: 0\}\s*\]/,
		"the shell must pin the exact ordered passing-section array",
	);
	assert.match(
		shell,
		/shakedown-return-destinations[\s\S]*\.sectionCount == 4[\s\S]*\.screenshotCount == 4/,
		"the shell must special-case the aggregate report contract",
	);
	assert.match(
		shell,
		/shakedown-return-destinations[\s\S]*screenshot_count[^\n]*-eq 4/,
		"the shell must independently require four PNG files",
	);
	assert.match(
		shell,
		/expected_screenshot_names="\$\(printf '%s\\n' \\\n\s*'shakedown-changes-terminal-return\.png' \\\n\s*'shakedown-parallel-current-exact-history\.png' \\\n\s*'shakedown-poll-supersession-request-accounting\.png' \\\n\s*'shakedown-retained-timeline-return\.png'\)"[\s\S]*\[ "\$screenshot_names" = "\$expected_screenshot_names" \]/,
		"the shell must pin and compare the exact sorted PNG-name set",
	);
	assert.match(readme, /four independent diagnostics sections/);
	for (const screenshot of screenshots) {
		assert.match(readme, new RegExp(`${screenshot}\\.png`));
	}
	assert.match(
		browser,
		/config\.mode !== "shakedown-exact-history-focus"[\s\S]*config\.mode !== "shakedown-return-destinations"/,
		"the semantic mode must be a member of the closed browser set",
	);
	assert.doesNotMatch(
		shell,
		/--(?:section|shakedown-section|mode)[= )]/,
		"the semantic mode must not expose a generic selector",
	);
	assert.equal(flag, `--${mode}`);
});

test("exact readiness requires the requested route and accepted reading body", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const start = source.indexOf("function isAcceptedExactReadingInPage(");
	const end = source.indexOf("\n\tconst ", start);
	assert.notEqual(start, -1, "missing semantic exact-reading predicate");
	assert.ok(end > start, "missing semantic exact-reading predicate boundary");
	const classify = new Function(
		`${source.slice(start, end)}\nreturn isAcceptedExactReadingInPage;`,
	)();
	const route =
		"changes/change%3Asha256%3Aaa/revisions/rev%3Asha256%3Abb?artifactHash=sha256%3Acc";
	const expectedHash = `#/${route}`;
	const acceptedDetail = {
		dataset: { changeReadingKey: `${route}:sha256:projection` },
		textContent: "Exact Revision Matrix fact",
		querySelector: (selector) =>
			selector === ":scope > h2" ? { textContent: "Exact Revision" } : null,
	};
	const originalDocument = globalThis.document;
	const originalLocation = globalThis.location;
	const documentFor = (detail) => ({
		querySelector: (selector) => {
			if (selector === "#detail-body") return detail;
			if (selector === "#stat-hash") return { textContent: "sha256:stamp" };
			if (selector === "#master h1") return {};
			if (selector === "#master") return { textContent: "Changes" };
			return null;
		},
	});
	try {
		globalThis.location = { hash: expectedHash };
		globalThis.document = documentFor(acceptedDetail);
		assert.deepEqual(classify({ expectedHash, expectedRoute: route }), {
			state: "ready",
		});
		assert.equal(
			classify({
				expectedHash,
				expectedRoute: route,
				priorKeys: { reading: acceptedDetail.dataset.changeReadingKey },
				reload: false,
			}),
			false,
			"a retained key is not a replacement reading for ordinary exact navigation",
		);
		assert.deepEqual(
			classify({
				expectedHash,
				expectedRoute: route,
				priorKeys: { reading: acceptedDetail.dataset.changeReadingKey },
				reload: true,
			}),
			{ state: "ready" },
			"a deliberate reload may accept the same route-bound reading key",
		);
		const resourceRoute = `${route.replace("?", "/resource?")}`;
		globalThis.location = { hash: `#/${resourceRoute}` };
		globalThis.document = documentFor({
			dataset: { changeReadingKey: `${resourceRoute}:sha256:projection` },
			textContent: "Authoritative captured diff available",
			querySelector: (selector) =>
				selector === ":scope > h2"
					? { textContent: "Authoritative captured diff" }
					: null,
		});
		assert.deepEqual(
			classify({
				expectedHash: `#/${resourceRoute}`,
				expectedRoute: resourceRoute,
			}),
			{ state: "ready" },
			"accepted exact resource bodies use the same semantic readiness contract",
		);

		globalThis.location = { hash: expectedHash };
		globalThis.document = documentFor(null);
		assert.equal(
			classify({ expectedHash, expectedRoute: route }),
			false,
			"a matching hash without a reading is not ready",
		);
		globalThis.document = documentFor({
			...acceptedDetail,
			textContent: "Loading exact Revision…",
			querySelector: () => null,
		});
		assert.equal(
			classify({ expectedHash, expectedRoute: route }),
			false,
			"a loading presentation is not ready",
		);
		globalThis.document = documentFor({
			...acceptedDetail,
			querySelector: () => null,
		});
		assert.equal(
			classify({ expectedHash, expectedRoute: route }),
			false,
			"a merely changed reading key without accepted body is not ready",
		);
		globalThis.document = documentFor({
			...acceptedDetail,
			dataset: {
				changeReadingKey: "changes/other:sha256:projection",
			},
		});
		assert.equal(
			classify({ expectedHash, expectedRoute: route }),
			false,
			"an accepted body for another exact route is not ready",
		);
	} finally {
		if (originalDocument === undefined) delete globalThis.document;
		else globalThis.document = originalDocument;
		if (originalLocation === undefined) delete globalThis.location;
		else globalThis.location = originalLocation;
	}
	assert.match(
		source,
		/page\.waitForFunction\(isAcceptedExactReadingInPage,[\s\S]*expectedRoute:/,
		"the generic exact open path must use semantic readiness",
	);
});

test("empty ready L2 recovery is explicit, authenticated, and retained before browser readiness", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.sh", import.meta.url),
		"utf8",
	);
	const retry = source.indexOf("retry_empty_ready_l2() {");
	const emptyStart = source.indexOf(
		'start_reader_state_server "empty-ready-l2" "$reader_empty_l2_repo"',
	);
	const retryCall = source.indexOf("retry_empty_ready_l2", emptyStart);
	const l0Start = source.indexOf(
		'start_reader_state_server "l0" "$reader_l0_repo"',
	);
	assert.ok(retry >= 0, "missing empty-ready-l2 retry helper");
	assert.ok(
		emptyStart >= 0 && retryCall > emptyStart && l0Start > retryCall,
		"empty-ready-l2 must recover before another reader fixture starts",
	);
	const helper = source.slice(retry, emptyStart);
	assert.match(
		helper,
		/-X POST[\s\\]+-H "Authorization: Bearer \$token"[\s\\]+"\$base_url\/api\/derived-access\/retry"/,
	);
	assert.match(helper, /browser-empty-ready-l2-retry\.json/);
	assert.match(helper, /browser-empty-ready-l2-ready\.json/);
	assert.match(
		source,
		/logs\/browser-empty-ready-l2-retry\.json[\s\\]+logs\/browser-empty-ready-l2-ready\.json/,
		"completion must require both retained recovery records",
	);
	assert.match(
		helper,
		/servingCurrent == true[\s\S]*availability == "current"[\s\S]*rebuildInFlight == false/,
		"the ready record must wait for the recovered derived generation",
	);
});

test("browser servers force active derived access and retain the primary current-state witness", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.sh", import.meta.url),
		"utf8",
	);
	const ambientGuard = source.indexOf(
		"POINTBREAK_DERIVED_ACCESS must be unset or sqlite-wal-bodyless-v1",
	);
	const readerLaunch = source.indexOf(
		'POINTBREAK_DERIVED_ACCESS=sqlite-wal-bodyless-v1 \\\n    POINTBREAK_HOME="$reader_state_home"',
	);
	const primaryLaunch = source.indexOf(
		'POINTBREAK_DERIVED_ACCESS=sqlite-wal-bodyless-v1 \\\n  POINTBREAK_HOME="$pointbreak_home"',
	);
	const primaryStartup = source.indexOf('>"$log_dir/inspect-startup.json"');
	const primaryStatus = source.indexOf(
		"retain_primary_derived_access_status",
		primaryStartup,
	);
	const browserConfig = source.indexOf('browser_config="$(jq -cn');
	assert.ok(ambientGuard >= 0, "missing ambient derived-access guard");
	assert.ok(
		readerLaunch > ambientGuard,
		"reader servers must force active access",
	);
	assert.ok(
		primaryLaunch > ambientGuard,
		"primary server must force active access",
	);
	assert.ok(
		primaryStatus > primaryStartup && browserConfig > primaryStatus,
		"the primary status witness must be retained before browser configuration",
	);
	assert.match(
		source,
		/logs\/browser-primary-derived-access-status\.json/,
		"completion must require the retained primary status",
	);
	assert.match(
		source,
		/primaryDerivedAccessStatus: \$primaryDerivedAccess\[0\]/,
		"the completion candidate must bind the typed status document",
	);
});

test("reports failures from independently recoverable sections together", async () => {
	const diagnostics = createBrowserDiagnostics({ context });

	await diagnostics.section("Timeline", async () => {
		diagnostics.expect(
			false,
			"Timeline chronology",
			"newest-first label was absent",
			{
				expected: "Newest first",
				actual: "Oldest first",
			},
		);
	});
	await diagnostics.section("Changes", async () => {
		diagnostics.expect(false, "Change ordering", "card order drifted", {
			expected: ["change:a", "change:b"],
			actual: ["change:b", "change:a"],
		});
	});

	const report = diagnostics.report();
	assert.equal(report.failures.length, 2);
	assert.deepEqual(
		report.failures.map((failure) => failure.section),
		["Timeline", "Changes"],
	);
	assert.deepEqual(report.failures[0], {
		kind: "assertion",
		section: "Timeline",
		label: "Timeline chronology",
		detail: "newest-first label was absent",
		expected: "Newest first",
		actual: "Oldest first",
		route: context().route,
		viewport: context().viewport,
		screenshot: context().screenshot,
		log: context().log,
	});
	assert.match(report.text, /Timeline.*Timeline chronology/s);
	assert.match(report.text, /Changes.*Change ordering/s);
});

test("production-style checks retain semantic expected and observed values", async () => {
	const diagnostics = createBrowserDiagnostics({ context });

	await diagnostics.section("Timeline", async () => {
		diagnostics.expect(
			false,
			"bounded Timeline window",
			"expected 1-79 live events, observed 0",
		);
	});

	const [failure] = diagnostics.report().failures;
	assert.deepEqual(failure.expected, {
		condition: "bounded Timeline window",
		outcome: "satisfied",
	});
	assert.deepEqual(failure.actual, {
		condition: "bounded Timeline window",
		outcome: "failed",
		detail: "expected 1-79 live events, observed 0",
	});
});

test("explicit null and undefined comparison values remain diagnostic evidence", async () => {
	const diagnostics = createBrowserDiagnostics({ context });

	await diagnostics.section("Exact Revision", async () => {
		diagnostics.expect(false, "exact route", "exact identity was absent", {
			expected: null,
			actual: undefined,
		});
	});

	const [failure] = diagnostics.report().failures;
	assert.equal(failure.expected, null);
	assert.deepEqual(failure.actual, { valueType: "undefined" });
	assert.deepEqual(JSON.parse(JSON.stringify(failure)), failure);
});

test("non-finite numeric observations remain diagnostic evidence on the JSON wire", async () => {
	const diagnostics = createBrowserDiagnostics({ context });

	await diagnostics.section("Timeline", async () => {
		diagnostics.expect(false, "recorded event count", "count was unavailable", {
			expected: Number.POSITIVE_INFINITY,
			actual: {
				metrics: {
					svgWidth: Number.NaN,
					heights: [Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY],
				},
				optional: undefined,
			},
		});
	});

	const [failure] = diagnostics.report().failures;
	assert.deepEqual(failure.expected, {
		valueType: "number",
		value: "Infinity",
	});
	assert.deepEqual(failure.actual, {
		metrics: {
			svgWidth: { valueType: "number", value: "NaN" },
			heights: [
				{ valueType: "number", value: "Infinity" },
				{ valueType: "number", value: "-Infinity" },
			],
		},
		optional: { valueType: "undefined" },
	});
	assert.deepEqual(JSON.parse(JSON.stringify(failure)), failure);
});

test("stops only an invalid section and continues later independent setup", async () => {
	const diagnostics = createBrowserDiagnostics({ context });
	const visited = [];

	await diagnostics.section("Timeline", {
		setup: async () => {
			visited.push("Timeline setup");
			throw new Error("exact event transition never settled");
		},
		run: async () => {
			visited.push("Timeline body");
		},
	});
	await diagnostics.section("Attention", {
		setup: async () => {
			visited.push("Attention setup");
		},
		run: async () => {
			visited.push("Attention body");
			diagnostics.expect(false, "Attention guidance", "reason was absent");
		},
	});

	assert.deepEqual(visited, [
		"Timeline setup",
		"Attention setup",
		"Attention body",
	]);
	const report = diagnostics.report();
	assert.deepEqual(report.sections, [
		{ name: "Timeline", status: "stopped", failureCount: 1 },
		{ name: "Attention", status: "failed", failureCount: 1 },
	]);
	assert.equal(report.failures[0].kind, "section");
	assert.match(
		report.failures[0].detail,
		/exact event transition never settled/,
	);
});

test("a failed prerequisite stops its section without duplicating diagnostics", async () => {
	const diagnostics = createBrowserDiagnostics({ context });
	const visited = [];

	await diagnostics.section("Exact Revision", async () => {
		diagnostics.requireCondition(
			false,
			"exact Revision authority",
			"exact Change membership was absent",
			{ expected: "one exact member", actual: null },
		);
		visited.push("dependent transition");
	});
	await diagnostics.section("Timeline", async () => {
		visited.push("independent section");
	});

	const report = diagnostics.report();
	assert.deepEqual(visited, ["independent section"]);
	assert.equal(report.failures.length, 1);
	assert.deepEqual(
		{
			kind: report.failures[0].kind,
			label: report.failures[0].label,
			expected: report.failures[0].expected,
			actual: report.failures[0].actual,
		},
		{
			kind: "assertion",
			label: "exact Revision authority",
			expected: "one exact member",
			actual: null,
		},
	);
	assert.deepEqual(report.sections, [
		{ name: "Exact Revision", status: "stopped", failureCount: 1 },
		{ name: "Timeline", status: "passed", failureCount: 0 },
	]);
});

test("globally invalid browser state skips every later section", async () => {
	const diagnostics = createBrowserDiagnostics({ context });
	let laterSetupRan = false;

	await diagnostics.section("Timeline", async () => {
		diagnostics.abort("primary Inspector connection was lost", {
			expected: "authenticated primary Inspector remains available",
			actual: "net::ERR_CONNECTION_REFUSED",
		});
	});
	await diagnostics.section("Changes", {
		setup: async () => {
			laterSetupRan = true;
		},
		run: async () => {},
	});

	const report = diagnostics.report();
	assert.equal(laterSetupRan, false);
	assert.equal(report.globalInvalid, true);
	assert.equal(report.failures[0].kind, "fatal");
	assert.deepEqual(report.sections, [
		{ name: "Timeline", status: "stopped", failureCount: 1 },
		{ name: "Changes", status: "skipped", failureCount: 0 },
	]);
});

test("refuses successful completion whenever any assertion failed", async () => {
	const diagnostics = createBrowserDiagnostics({ context });
	let completionPublished = false;

	await diagnostics.section("Changes", async () => {
		diagnostics.expect(false, "bounded cards", "saw 101 cards", {
			expected: "at most 100",
			actual: 101,
		});
	});

	assert.throws(
		() => {
			const completion = diagnostics.complete({ screenshotCount: 7 });
			completionPublished = completion.status === "passed";
		},
		(error) => {
			assert.ok(error instanceof BrowserDiagnosticFailure);
			assert.equal(error.report.failures.length, 1);
			assert.match(error.message, /bounded cards/);
			return true;
		},
	);
	assert.equal(completionPublished, false);
});

test("returns a serializable failed result while completion remains green-only", async () => {
	const diagnostics = createBrowserDiagnostics({ context });

	await diagnostics.section("Timeline", async () => {
		diagnostics.expect(
			false,
			"chronological order",
			"newest event was not first",
			{
				expected: "newest first",
				actual: "oldest first",
			},
		);
	});
	await diagnostics.section("Changes", async () => {
		diagnostics.expect(true, "change cards", "change cards were absent");
	});

	const result = diagnostics.result({ screenshotCount: 4 });
	assert.deepEqual(result, {
		schema: "pointbreak.change-inspector-browser-report",
		version: 1,
		status: "failed",
		assertionCount: 2,
		screenshotCount: 4,
		sectionCount: 2,
		globalInvalid: false,
		sections: [
			{ name: "Timeline", status: "failed", failureCount: 1 },
			{ name: "Changes", status: "passed", failureCount: 0 },
		],
		failures: diagnostics.report().failures,
	});
	assert.deepEqual(JSON.parse(JSON.stringify(result)), result);
	assert.throws(
		() => diagnostics.complete({ screenshotCount: 4 }),
		BrowserDiagnosticFailure,
	);
});

test("returns a compact passing completion only after every section succeeds", async () => {
	const diagnostics = createBrowserDiagnostics({ context });

	await diagnostics.section("Reader readiness", async () => {
		diagnostics.expect(true, "ready L2", "reader did not start");
	});
	await diagnostics.section("Timeline", async () => {
		diagnostics.expect(true, "event rows", "no events rendered");
	});

	assert.deepEqual(diagnostics.complete({ screenshotCount: 3 }), {
		schema: "pointbreak.change-inspector-browser-report",
		version: 1,
		status: "passed",
		assertionCount: 2,
		screenshotCount: 3,
		sectionCount: 2,
		globalInvalid: false,
		sections: [
			{ name: "Reader readiness", status: "passed", failureCount: 0 },
			{ name: "Timeline", status: "passed", failureCount: 0 },
		],
		failures: [],
	});
});

test("rendered browser program logs and returns failed diagnostics normally", async () => {
	let source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	for (const [marker, replacement] of [
		[
			"__POINTBREAK_BROWSER_DIAGNOSTIC_FAILURE__",
			BrowserDiagnosticFailure.toString(),
		],
		["__POINTBREAK_BROWSER_DIAGNOSTICS__", createBrowserDiagnostics.toString()],
		["__POINTBREAK_CHANGE_BROWSER_CONFIG__", "{}"],
	]) {
		assert.ok(source.includes(marker), `missing browser marker ${marker}`);
		source = source.replace(marker, replacement);
	}

	assert.equal(
		typeof new Function(`return (${source}\n)`)(),
		"function",
		"rendered browser program remains executable by the runner",
	);
	assert.match(
		source,
		/const completion = diagnostics\.result\(\{ screenshotCount: screenshots \}\);/,
		"the browser program obtains failed and passing reports from diagnostics",
	);
	assert.match(
		source,
		/console\.log\(`POINTBREAK_BROWSER_RESULT=\$\{JSON\.stringify\(completion\)\}`\);\s*return completion;/,
		"the logged browser report is returned to the runner",
	);
	const completionTransport = source.slice(
		source.lastIndexOf(
			"const completion = diagnostics.result({ screenshotCount: screenshots });",
		),
	);
	assert.doesNotMatch(
		completionTransport,
		/throw new BrowserDiagnosticFailure\(/,
		"a failed browser report returns normally after logging",
	);
});

test("reduced-motion evidence waits for a semantically painted retained Changes surface", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const start = source.indexOf(
		'await diagnostics.section("Polling retention and reduced motion"',
	);
	const end = source.indexOf(
		'await diagnostics.section("Browser runtime"',
		start,
	);
	assert.notEqual(start, -1, "missing reduced-motion browser section");
	assert.notEqual(
		end,
		-1,
		"reduced-motion section must precede Browser runtime",
	);
	const section = source.slice(start, end);

	const pollWait = section.indexOf("await page.waitForTimeout(3500);");
	const retainedCardAssertion = section.indexOf(
		"locator('.unit-card[data-browser-retention=\"same-generation\"]')",
	);
	const semanticPaintWait = section.indexOf(
		"const semanticChangeSurface = await page.waitForFunction",
	);
	const screenshot = section.indexOf(
		'await screenshot("wide-reduced-motion");',
	);
	assert.ok(
		pollWait >= 0,
		"reduced-motion evidence must observe a poll interval",
	);
	assert.ok(
		retainedCardAssertion > pollWait,
		"the same-generation DOM-retention assertion must remain after polling",
	);
	assert.ok(
		semanticPaintWait > retainedCardAssertion,
		"the screenshot must wait for semantic Change paint after retaining the card node",
	);
	assert.ok(
		screenshot > semanticPaintWait,
		"the reduced-motion screenshot must follow the semantic Change-paint wait",
	);
	assert.match(
		section.slice(semanticPaintWait, screenshot),
		/#master[\s\S]*data-change-list-key[\s\S]*\.unit-card\[data-change-id\][\s\S]*change-card-primary/,
		"semantic paint must require a live Changes generation, exact Change identity, and a primary review action",
	);
});

test("fact-graph evidence checks painted labels against server-sized node frames", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const start = source.indexOf(
		'await diagnostics.section("Fact relationship graph"',
	);
	const end = source.indexOf(
		'await diagnostics.section("Annotated diff"',
		start,
	);
	assert.notEqual(start, -1, "missing fact relationship graph section");
	assert.notEqual(end, -1, "fact graph section must precede annotated diff");
	const section = source.slice(start, end);
	assert.match(
		section,
		/nodeLabelGeometry:[\s\S]*?getBBox\(\)[\s\S]*?clippedFactGraphLabels[\s\S]*?!Number\.isFinite\(node\.frameWidth\)[\s\S]*?node\.frameWidth <= 0[\s\S]*?node\.labelWidth <= 0[\s\S]*?node\.labelLeft < node\.frameLeft[\s\S]*?node\.labelRight > node\.frameRight/,
		"browser evidence must refuse a fact label painted outside its server-sized frame",
	);
});

test("Change-graph evidence checks painted labels against server-sized node frames", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const start = source.indexOf(
		'await diagnostics.section("Change relationship graph"',
	);
	const end = source.indexOf(
		'await diagnostics.section("Fact relationship graph"',
		start,
	);
	assert.notEqual(start, -1, "missing Change relationship graph section");
	assert.notEqual(end, -1, "Change graph section must precede fact graph");
	const section = source.slice(start, end);
	assert.match(
		section,
		/nodeLabelGeometry:[\s\S]*?getBBox\(\)[\s\S]*?clippedChangeGraphLabels[\s\S]*?!Number\.isFinite\(node\.frameWidth\)[\s\S]*?node\.frameWidth <= 0[\s\S]*?node\.labelWidth <= 0[\s\S]*?node\.labelLeft < node\.frameLeft[\s\S]*?node\.labelRight > node\.frameRight/,
		"browser evidence must refuse a Change graph label painted outside its server-sized frame",
	);
});

test("annotated-diff focus assertions wait for routed hydration and active focus", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const helperStart = source.indexOf("const waitForRoutedDiffFocus =");
	const sectionStart = source.indexOf(
		'await diagnostics.section("Annotated diff"',
	);
	const sectionEnd = source.indexOf(
		'await diagnostics.section("Exact detail and reading"',
		sectionStart,
	);
	assert.notEqual(helperStart, -1, "missing routed diff focus wait helper");
	assert.notEqual(sectionStart, -1, "missing annotated diff section");
	assert.notEqual(
		sectionEnd,
		-1,
		"annotated diff section must precede exact reading",
	);
	const helper = source.slice(
		helperStart,
		source.indexOf("\n\tconst ", helperStart + 1),
	);
	assert.match(
		helper,
		/#diff-page:not\(\.hidden\)[\s\S]*getClientRects\(\)\.length > 0[\s\S]*dataset\.exactFocus === "true"[\s\S]*document\.activeElement === target/,
		"focus readiness must require a visible diff, the exact-focus marker, and active DOM focus",
	);
	const section = source.slice(sectionStart, sectionEnd);
	const fileRouteWait = section.indexOf(
		'new URLSearchParams(location.hash.split("?", 2)[1] ?? "").get(\n\t\t\t\t\t\t"file"',
	);
	const fileFocusWait = section.indexOf(
		'await waitForRoutedDiffFocus("file", diffFilePaths[0]);',
	);
	const fileAssertion = section.indexOf("const focusedDiffFile = await page");
	assert.ok(
		fileRouteWait >= 0 &&
			fileFocusWait > fileRouteWait &&
			fileAssertion > fileFocusWait,
		"file focus must wait for hydrated routed focus after the route assertion",
	);
	const factRouteWait = section.indexOf(
		'new URLSearchParams(location.hash.split("?", 2)[1] ?? "").get(\n\t\t\t\t\t\t"fact"',
		fileAssertion,
	);
	const factFocusWait = section.indexOf(
		'await waitForRoutedDiffFocus("fact", firstDiffFact);',
	);
	const factAssertion = section.indexOf("const focusedDiffFact = await page");
	assert.ok(
		factRouteWait >= 0 &&
			factFocusWait > factRouteWait &&
			factAssertion > factFocusWait,
		"fact focus must wait for hydrated routed focus after the route assertion",
	);
});

test("Timeline event widen waits for the complete split-pane transition", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const narrowDetailScreenshot = source.indexOf(
		'await screenshot("narrow-timeline-event-detail");',
	);
	assert.notEqual(
		narrowDetailScreenshot,
		-1,
		"missing narrow Timeline event detail checkpoint",
	);
	const postWidenWaitStart = source.indexOf(
		"await page.waitForFunction(",
		narrowDetailScreenshot,
	);
	const postWidenAssertion = source.indexOf(
		'"Timeline event widen",',
		postWidenWaitStart,
	);
	assert.ok(
		postWidenWaitStart > narrowDetailScreenshot,
		"missing post-widen wait",
	);
	assert.ok(
		postWidenAssertion > postWidenWaitStart,
		"missing post-widen diagnostic assertion",
	);
	const wait = source.slice(postWidenWaitStart, postWidenAssertion);
	assert.match(
		wait,
		/getComputedStyle\(document\.querySelector\("#detail-back"\)\)\.display ===\s*"none"/,
		"post-widen readiness must require the wide CSS state",
	);
	assert.match(
		wait,
		/exactAction !== null[\s\S]*document\.activeElement === exactAction/,
		"post-widen readiness must retain exact-action focus",
	);
	assert.match(
		wait,
		/\["#topbar", "#toolbar", "#master-rail", "#master", "\.divider"\]\.every\([\s\S]*?document\.querySelector\(selector\)\?\.inert === false/,
		"post-widen readiness must wait for every ordinary split-pane surface to become non-inert",
	);
});

test("console 503 exemption accepts only typed primary Change transitions inside append", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const start = source.indexOf(
		"function isDeliberateChangeProjectionTransition(",
	);
	const end = source.indexOf("\n\tconst responseInspections", start);
	assert.notEqual(start, -1, "missing Change projection transition classifier");
	assert.ok(end > start, "missing classifier boundary");
	const classify = new Function(
		`${source.slice(start, end)}\nreturn isDeliberateChangeProjectionTransition;`,
	)();
	const baseUrl = "http://127.0.0.1:4173";
	const typed = {
		url: `${baseUrl}/api/v2/changes?limit=100`,
		status: 503,
		body: {
			schema: "pointbreak.inspect-change-projection-error",
			version: 1,
			code: "projection_unstable",
			retryable: true,
		},
		schema: "pointbreak.inspect-change-projection-error",
		insideAppendWindow: true,
	};
	assert.equal(classify(typed, baseUrl), true);
	assert.equal(
		classify(
			{ ...typed, url: `${baseUrl}/api/v2/attention?limit=100` },
			baseUrl,
		),
		true,
	);
	for (const rejected of [
		{ ...typed, insideAppendWindow: false },
		{ ...typed, status: 409 },
		{ ...typed, url: `${baseUrl}/api/v2/profile` },
		{ ...typed, url: `${baseUrl}/api/v2/changes-extra` },
		{ ...typed, url: `${baseUrl}/api/v2/changes/` },
		{ ...typed, url: `${baseUrl}.example/api/v2/changes` },
		{ ...typed, url: "http://127.0.0.1:4999/api/v2/changes" },
		{ ...typed, schema: "pointbreak.inspect-reader-profile" },
		{
			...typed,
			body: { ...typed.body, schema: "pointbreak.inspect-reader-profile" },
		},
		{ ...typed, body: { ...typed.body, code: "moving_journal" } },
		{ ...typed, body: { ...typed.body, version: 2 } },
		{ ...typed, body: { ...typed.body, retryable: false } },
	]) {
		assert.equal(classify(rejected, baseUrl), false, JSON.stringify(rejected));
	}
	const responseCapture = source.slice(
		source.indexOf('page.on("response"'),
		source.indexOf("\n\n\tconst layouts", source.indexOf('page.on("response"')),
	);
	for (const retained of [
		/response\.url\(\)/,
		/response\.status\(\)/,
		/response\.text\(\)/,
		/schema:/,
		/body,/,
	]) {
		assert.match(
			responseCapture,
			retained,
			"503 response evidence must retain URL, status, body, and schema",
		);
	}
	const runtimeAccounting = source.slice(
		source.indexOf("await settleResponseInspections();"),
		source.indexOf('await diagnostics.section("Browser runtime"'),
	);
	assert.match(
		runtimeAccounting,
		/error\.text !== genericServiceUnavailable[\s\S]*!error\.insideAppendWindow[\s\S]*error\.url === null[\s\S]*transitionResponsesByUrl\.get\(error\.url\)/,
		"generic console errors must consume only same-URL typed transitions observed in the append window",
	);
	assert.match(
		runtimeAccounting,
		/unexpectedServiceUnavailableResponses[\s\S]*!isDeliberateChangeProjectionTransition/,
		"every non-classified 503 response must remain a browser-runtime failure",
	);
});

test("append-window profile exemption accepts only the exact primary stale tuple", async () => {
	const source = await readFile(
		new URL("./change-inspector-browser-verify.mjs", import.meta.url),
		"utf8",
	);
	const start = source.indexOf(
		"function isDeliberateProfileProjectionTransition(",
	);
	const end = source.indexOf("\n\tconst responseInspections", start);
	assert.notEqual(
		start,
		-1,
		"missing profile projection transition classifier",
	);
	assert.ok(end > start, "missing profile classifier boundary");
	const classify = new Function(
		`${source.slice(start, end)}\nreturn isDeliberateProfileProjectionTransition;`,
	)();
	const baseUrl = "http://127.0.0.1:4173";
	const typed = {
		url: `${baseUrl}/api/v2/profile`,
		status: 503,
		body: {
			schema: "pointbreak.inspect-change-projection-error",
			version: 1,
			code: "projection_stale",
			retryable: true,
		},
		schema: "pointbreak.inspect-change-projection-error",
		insideAppendWindow: true,
	};
	assert.equal(classify(typed, baseUrl), true);
	for (const rejected of [
		{ ...typed, insideAppendWindow: false },
		{ ...typed, status: 409 },
		{ ...typed, url: `${baseUrl}/api/v2/changes` },
		{ ...typed, url: `${baseUrl}/api/v2/profile/extra` },
		{ ...typed, url: `${baseUrl}.example/api/v2/profile` },
		{ ...typed, url: "http://127.0.0.1:4999/api/v2/profile" },
		{ ...typed, schema: "pointbreak.inspect-reader-profile" },
		{
			...typed,
			body: { ...typed.body, schema: "pointbreak.inspect-reader-profile" },
		},
		{ ...typed, body: { ...typed.body, version: 2 } },
		{ ...typed, body: { ...typed.body, code: "projection_unstable" } },
		{ ...typed, body: { ...typed.body, retryable: false } },
	]) {
		assert.equal(classify(rejected, baseUrl), false, JSON.stringify(rejected));
	}
	const runtimeAccounting = source.slice(
		source.indexOf("await settleResponseInspections();"),
		source.indexOf('await diagnostics.section("Browser runtime"'),
	);
	assert.match(
		runtimeAccounting,
		/isDeliberateChangeProjectionTransition[\s\S]*isDeliberateProfileProjectionTransition/,
		"runtime accounting must admit both closed typed transition classes",
	);
});

test("an aggregate failure cannot publish a passing completion manifest", async () => {
	const root = await mkdtemp(join(tmpdir(), "pointbreak-browser-diagnostics-"));
	const candidatePath = join(root, ".manifest.json.tmp");
	const manifestPath = join(root, "manifest.json");
	const candidate = {
		gate: "change-inspector-browser-verify",
		status: "passed",
	};
	await writeFile(candidatePath, `${JSON.stringify(candidate)}\n`);

	await assert.rejects(
		publishPassingManifest({
			candidatePath,
			manifestPath,
			browserResult: {
				schema: "pointbreak.change-inspector-browser-report",
				version: 1,
				status: "passed",
				assertionCount: 2,
				screenshotCount: 1,
				sectionCount: 2,
				globalInvalid: false,
				sections: [
					{ name: "Timeline", status: "failed", failureCount: 1 },
					{ name: "Changes", status: "passed", failureCount: 0 },
				],
				failures: [{ section: "Timeline", label: "chronology" }],
			},
		}),
		/recorded 1 browser diagnostic failure/,
	);
	await assert.rejects(readFile(manifestPath), /ENOENT/);

	const passingResult = {
		schema: "pointbreak.change-inspector-browser-report",
		version: 1,
		status: "passed",
		assertionCount: 2,
		screenshotCount: 1,
		sectionCount: 2,
		globalInvalid: false,
		sections: [
			{ name: "Timeline", status: "passed", failureCount: 0 },
			{ name: "Changes", status: "passed", failureCount: 0 },
		],
		failures: [],
	};
	await assert.rejects(
		publishPassingManifest({
			candidatePath,
			manifestPath,
			browserResult: passingResult,
		}),
		/manifest candidate omitted assertionCount or screenshotCount/,
	);
	await writeFile(
		candidatePath,
		`${JSON.stringify({ ...candidate, assertionCount: 3, screenshotCount: 1 })}\n`,
	);
	await assert.rejects(
		publishPassingManifest({
			candidatePath,
			manifestPath,
			browserResult: passingResult,
		}),
		/assertionCount 3 did not match browser result 2/,
	);
	const passingCandidate = {
		...candidate,
		assertionCount: passingResult.assertionCount,
		screenshotCount: passingResult.screenshotCount,
		primaryDerivedAccessStatus: currentDerivedAccessStatus,
		evidenceInventory: [],
	};
	await mkdir(join(root, "browser-artifacts"), { recursive: true });
	await mkdir(join(root, "logs"), { recursive: true });
	const retainedFiles = new Map([
		["browser-artifacts/wide-timeline.png", "wide PNG bytes"],
		["logs/browser-gate.log", "browser gate log bytes"],
		[
			"logs/browser-primary-derived-access-status.json",
			`${JSON.stringify(currentDerivedAccessStatus)}\n`,
		],
		["logs/browser-program.mjs", "browser program bytes"],
		["logs/browser-result.json", '{"status":"passed"}\n'],
	]);
	for (const [path, bytes] of retainedFiles) {
		await writeFile(join(root, path), bytes);
	}
	passingCandidate.evidenceInventory = [...retainedFiles.entries()]
		.map(([path, bytes]) => ({
			path,
			sha256: createHash("sha256").update(bytes).digest("hex"),
		}))
		.sort((left, right) => left.path.localeCompare(right.path));
	await writeFile(candidatePath, `${JSON.stringify(passingCandidate)}\n`);
	await publishPassingManifest({
		candidatePath,
		manifestPath,
		browserResult: passingResult,
		evidenceRoot: root,
	});
	assert.deepEqual(
		JSON.parse(await readFile(manifestPath, "utf8")),
		passingCandidate,
	);
});

test("derived Change diagnostics are inadmissible to browser completion publication", async () => {
	const root = await mkdtemp(join(tmpdir(), "pointbreak-browser-boundary-"));
	const candidatePath = join(root, ".renamed-candidate.json.tmp");
	const manifestPath = join(root, "manifest.json");
	const passingResult = {
		schema: "pointbreak.change-inspector-browser-report",
		version: 1,
		status: "passed",
		assertionCount: 0,
		screenshotCount: 0,
		sectionCount: 1,
		globalInvalid: false,
		sections: [{ name: "Timeline", status: "passed", failureCount: 0 }],
		failures: [],
	};

	for (const schema of [
		"pointbreak.derived-change-diagnostic-report.v1",
		"pointbreak.derived-change-diagnostic-fragment.v1",
		"pointbreak.derived-change-diagnostic-collection.v1",
	]) {
		await assert.rejects(
			publishPassingManifest({
				candidatePath,
				manifestPath,
				browserResult: { ...passingResult, schema },
				evidenceRoot: root,
			}),
			/derived Change diagnostic.*never browser completion evidence/,
		);
		await writeFile(
			candidatePath,
			`${JSON.stringify({
				schema,
				gate: "change-inspector-browser-verify",
				status: "passed",
				assertionCount: 0,
				screenshotCount: 0,
			})}\n`,
		);
		await assert.rejects(
			publishPassingManifest({
				candidatePath,
				manifestPath,
				browserResult: passingResult,
				evidenceRoot: root,
			}),
			/derived Change diagnostic.*never browser completion evidence/,
		);
	}

	await assert.rejects(
		publishPassingManifest({
			candidatePath: join(root, "derived-change-diagnostic-report.json"),
			manifestPath,
			browserResult: passingResult,
			evidenceRoot: root,
		}),
		/derived Change diagnostic.*never browser completion evidence/,
	);
	await assert.rejects(
		publishPassingManifest({
			candidatePath,
			manifestPath,
			browserResult: passingResult,
			evidenceRoot: join(root, "derived-change-diagnostic", "browser"),
		}),
		/derived Change diagnostic.*never browser completion evidence/,
	);

	await writeFile(
		candidatePath,
		`${JSON.stringify({
			gate: "change-inspector-browser-verify",
			status: "passed",
			assertionCount: 0,
			screenshotCount: 0,
			evidenceInventory: [],
		})}\n`,
	);
	await assert.rejects(
		publishPassingManifest({
			candidatePath,
			manifestPath,
			browserResult: passingResult,
			browserResultPath: join(
				root,
				"derived-change-diagnostic",
				"browser-result.json",
			),
			evidenceRoot: root,
		}),
		/derived Change diagnostic.*never browser completion evidence/,
	);

	await writeFile(
		candidatePath,
		`${JSON.stringify({
			gate: "change-inspector-browser-verify",
			status: "passed",
			assertionCount: 0,
			screenshotCount: 0,
			evidenceInventory: [
				{
					path: "logs/derived-change-diagnostic/payload.json",
					sha256: "0".repeat(64),
				},
			],
		})}\n`,
	);
	await assert.rejects(
		publishPassingManifest({
			candidatePath,
			manifestPath,
			browserResult: passingResult,
			evidenceRoot: root,
		}),
		/derived Change diagnostic.*never browser completion evidence/,
	);

	const inventoryRoot = await mkdtemp(
		join(tmpdir(), "pointbreak-browser-inventory-boundary-"),
	);
	await mkdir(join(inventoryRoot, "browser-artifacts"));
	await mkdir(join(inventoryRoot, "logs"));
	const diagnosticBytes = `${JSON.stringify({
		schema: "pointbreak.derived-change-diagnostic-collection.v1",
		cases: [],
	})}\n`;
	const retainedFiles = new Map([
		["logs/browser-gate.log", "browser gate log bytes"],
		[
			"logs/browser-primary-derived-access-status.json",
			`${JSON.stringify(currentDerivedAccessStatus)}\n`,
		],
		["logs/browser-program.mjs", "browser program bytes"],
		["logs/browser-result.json", diagnosticBytes],
	]);
	for (const [path, bytes] of retainedFiles) {
		await writeFile(join(inventoryRoot, path), bytes);
	}
	const inventoryCandidate = {
		gate: "change-inspector-browser-verify",
		status: "passed",
		assertionCount: 0,
		screenshotCount: 0,
		primaryDerivedAccessStatus: currentDerivedAccessStatus,
		evidenceInventory: [...retainedFiles.entries()]
			.map(([path, bytes]) => ({
				path,
				sha256: createHash("sha256").update(bytes).digest("hex"),
			}))
			.sort((left, right) => left.path.localeCompare(right.path)),
	};
	const inventoryCandidatePath = join(inventoryRoot, ".candidate.json.tmp");
	const inventoryManifestPath = join(inventoryRoot, "manifest.json");
	await writeFile(
		inventoryCandidatePath,
		`${JSON.stringify(inventoryCandidate)}\n`,
	);
	await assert.rejects(
		publishPassingManifest({
			candidatePath: inventoryCandidatePath,
			manifestPath: inventoryManifestPath,
			browserResult: passingResult,
			evidenceRoot: inventoryRoot,
		}),
		/derived Change diagnostic.*never browser completion evidence/,
	);

	const schemaLessDiagnosticBytes = `${JSON.stringify({
		mode: "--derived-change-read-diagnostic",
		sourceUnchanged: true,
		preflight: [],
		rows: [],
		controls: [],
		storage: [],
	})}\n`;
	await writeFile(
		join(inventoryRoot, "logs", "browser-result.json"),
		schemaLessDiagnosticBytes,
	);
	inventoryCandidate.evidenceInventory = [...retainedFiles.entries()]
		.map(([path, bytes]) => ({
			path,
			sha256: createHash("sha256")
				.update(
					path === "logs/browser-result.json"
						? schemaLessDiagnosticBytes
						: bytes,
				)
				.digest("hex"),
		}))
		.sort((left, right) => left.path.localeCompare(right.path));
	await writeFile(
		inventoryCandidatePath,
		`${JSON.stringify(inventoryCandidate)}\n`,
	);
	await assert.rejects(
		publishPassingManifest({
			candidatePath: inventoryCandidatePath,
			manifestPath: inventoryManifestPath,
			browserResult: passingResult,
			evidenceRoot: inventoryRoot,
		}),
		/derived Change diagnostic.*never browser completion evidence/,
	);
});

test("completion manifests bind a sorted SHA-256 inventory of retained browser evidence", async () => {
	const root = await mkdtemp(join(tmpdir(), "pointbreak-browser-evidence-"));
	const evidenceRoot = join(root, "evidence");
	const artifactDir = join(evidenceRoot, "browser-artifacts");
	const logDir = join(evidenceRoot, "logs");
	await mkdir(artifactDir, { recursive: true });
	await mkdir(logDir, { recursive: true });

	const primaryDerivedAccessStatus = currentDerivedAccessStatus;
	const retainedFiles = new Map([
		["browser-artifacts/narrow-timeline.png", "narrow PNG bytes"],
		["browser-artifacts/wide-timeline.png", "wide PNG bytes"],
		["logs/browser-gate.log", "browser gate log bytes"],
		[
			"logs/browser-primary-derived-access-status.json",
			`${JSON.stringify(primaryDerivedAccessStatus)}\n`,
		],
		["logs/browser-program.mjs", "browser program bytes"],
		["logs/browser-result.json", '{"status":"passed"}\n'],
	]);
	for (const [path, bytes] of retainedFiles) {
		await writeFile(join(evidenceRoot, path), bytes);
	}
	const evidenceInventory = [...retainedFiles.entries()]
		.map(([path, bytes]) => ({
			path,
			sha256: createHash("sha256").update(bytes).digest("hex"),
		}))
		.sort((left, right) => left.path.localeCompare(right.path));
	const browserResult = {
		schema: "pointbreak.change-inspector-browser-report",
		version: 1,
		status: "passed",
		assertionCount: 2,
		screenshotCount: 2,
		sectionCount: 1,
		globalInvalid: false,
		sections: [{ name: "Timeline", status: "passed", failureCount: 0 }],
		failures: [],
	};
	const candidate = {
		gate: "change-inspector-browser-verify",
		status: "passed",
		assertionCount: browserResult.assertionCount,
		screenshotCount: browserResult.screenshotCount,
		primaryDerivedAccessStatus,
		evidenceInventory,
	};

	const missingPath = join(root, ".missing-manifest.json.tmp");
	await writeFile(
		missingPath,
		`${JSON.stringify({ ...candidate, evidenceInventory: evidenceInventory.slice(1) })}\n`,
	);
	await assert.rejects(
		publishPassingManifest({
			candidatePath: missingPath,
			manifestPath: join(root, "missing-manifest.json"),
			browserResult,
			evidenceRoot,
		}),
		/retained browser evidence|evidence inventory/i,
	);

	const unsortedPath = join(root, ".unsorted-manifest.json.tmp");
	await writeFile(
		unsortedPath,
		`${JSON.stringify({
			...candidate,
			evidenceInventory: [...evidenceInventory].reverse(),
		})}\n`,
	);
	await assert.rejects(
		publishPassingManifest({
			candidatePath: unsortedPath,
			manifestPath: join(root, "unsorted-manifest.json"),
			browserResult,
			evidenceRoot,
		}),
		/sorted.*evidence inventory|evidence inventory.*sorted/i,
	);

	const tamperedPath = join(root, ".tampered-manifest.json.tmp");
	await writeFile(tamperedPath, `${JSON.stringify(candidate)}\n`);
	await writeFile(
		join(logDir, "browser-gate.log"),
		"tampered browser gate log",
	);
	await assert.rejects(
		publishPassingManifest({
			candidatePath: tamperedPath,
			manifestPath: join(root, "tampered-manifest.json"),
			browserResult,
			evidenceRoot,
		}),
		/SHA-256|digest|evidence inventory/i,
	);

	await writeFile(
		join(logDir, "browser-gate.log"),
		retainedFiles.get("logs/browser-gate.log"),
	);

	const inactiveCandidatePath = join(root, ".inactive-manifest.json.tmp");
	await writeFile(
		inactiveCandidatePath,
		`${JSON.stringify({
			...candidate,
			primaryDerivedAccessStatus: {
				...primaryDerivedAccessStatus,
				active: false,
			},
		})}\n`,
	);
	await assert.rejects(
		publishPassingManifest({
			candidatePath: inactiveCandidatePath,
			manifestPath: join(root, "inactive-manifest.json"),
			browserResult,
			evidenceRoot,
		}),
		/primary derived-access status|active\/current/i,
	);

	const mismatchedStatusPath = join(
		root,
		".mismatched-status-manifest.json.tmp",
	);
	await writeFile(
		mismatchedStatusPath,
		`${JSON.stringify({
			...candidate,
			primaryDerivedAccessStatus: {
				...primaryDerivedAccessStatus,
				availability: "stale",
			},
		})}\n`,
	);
	await assert.rejects(
		publishPassingManifest({
			candidatePath: mismatchedStatusPath,
			manifestPath: join(root, "mismatched-status-manifest.json"),
			browserResult,
			evidenceRoot,
		}),
		/primary derived-access status|active\/current|match/i,
	);

	const validCandidatePath = join(root, ".valid-manifest.json.tmp");
	const validManifestPath = join(root, "valid-manifest.json");
	const validCandidateBytes = `${JSON.stringify(candidate)}\n`;
	await writeFile(validCandidatePath, validCandidateBytes);
	await publishPassingManifest({
		candidatePath: validCandidatePath,
		manifestPath: validManifestPath,
		browserResult,
		evidenceRoot,
	});
	assert.deepEqual(
		JSON.parse(await readFile(validManifestPath, "utf8")),
		candidate,
	);
	assert.equal(await readFile(validManifestPath, "utf8"), validCandidateBytes);

	const duplicateCandidatePath = join(root, ".duplicate-manifest.json.tmp");
	await writeFile(duplicateCandidatePath, `${JSON.stringify(candidate)}\n`);
	const publishedBytes = await readFile(validManifestPath);
	await assert.rejects(
		publishPassingManifest({
			candidatePath: duplicateCandidatePath,
			manifestPath: validManifestPath,
			browserResult,
			evidenceRoot,
		}),
		/already exists/i,
	);
	assert.deepEqual(await readFile(validManifestPath), publishedBytes);
});
