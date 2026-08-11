// Internal real-browser program injected by derived-change-diagnostic-browser.sh.
((config) => async (page) => {
	const narrow = { width: 390, height: 844 };
	const wide = { width: 1440, height: 1000 };
	const ordinarySplitPaneSelectors = [
		"#topbar",
		"#toolbar",
		"#master-rail",
		"#master",
		".divider",
	];
	const cases = [];
	const pageErrors = [];
	const consoleErrors = [];
	page.on("pageerror", (error) => pageErrors.push(error.message));
	page.on("console", (message) => {
		if (message.type() === "error") consoleErrors.push(message.text());
	});
	const fixtureCheckpoint = (checkpoint) => ({
		fixture: config.fixture.authoritativeInventorySha256,
		fixtureId: config.fixture.id,
		witnessSha256: config.fixture.witnessSha256,
		topologyMaterializerSha256: config.fixture.topologyMaterializerSha256,
		checkpoint,
	});
	const observedFailure = (phase, error) => ({
		phase,
		detail: error instanceof Error ? error.message : String(error),
		url: page.url(),
		viewport: page.viewportSize(),
	});
	const transitionExpectation = () => ({
		wideCss: "#detail-back has display none",
		exactActionFocus: "the exact action is document.activeElement",
		ordinarySplitPaneSurfaces: ordinarySplitPaneSelectors.map((selector) => ({
			selector,
			inert: false,
		})),
	});
	const recordCaseFailure = (
		caseResult,
		phase,
		error,
		expected = transitionExpectation(),
		failureClass = "case_failure",
	) => {
		const observation = observedFailure(phase, error);
		if (Array.isArray(caseResult.observations)) {
			caseResult.observations.push(observation);
		}
		if (caseResult.status === "failed") {
			caseResult.actual.observations.push(observation);
			return;
		}
		caseResult.status = "failed";
		caseResult.failureClass = failureClass;
		caseResult.phase = phase;
		caseResult.expected = expected;
		caseResult.actual = { observations: [observation] };
	};
	const timelineUrl = `${config.server.baseUrl}/#/timeline?limit=100&order=desc`;
	const waitForTimeline = () =>
		page.waitForFunction(() => {
			const rows = document.querySelectorAll("#timeline [data-event-id]");
			return (
				rows.length > 0 &&
				document.querySelector("#detail")?.inert === true &&
				document.querySelector("#master")?.inert === false
			);
		});
	const waitForNarrowExactEvent = (eventId) =>
		page.waitForFunction(
			({ expectedEventId, selectors }) => {
				const selected = document.querySelector(
					'#timeline [aria-selected="true"]',
				);
				const detailEvent = document.querySelector(
					"#detail-body [data-event-id]",
				);
				return (
					getComputedStyle(document.querySelector("#detail-back")).display !==
						"none" &&
					document.querySelector("#detail")?.inert === false &&
					detailEvent?.dataset.eventId === expectedEventId &&
					selected?.dataset.eventId === expectedEventId &&
					selectors.every(
						(selector) => document.querySelector(selector)?.inert === true,
					)
				);
			},
			{ expectedEventId: eventId, selectors: ordinarySplitPaneSelectors },
		);
	const waitForWideExactEvent = () =>
		page.waitForFunction((selectors) => {
			const exactAction = document.querySelector(
				"#detail-body [data-exact-diff-activation], #detail-body [data-event-diff-refusal]",
			);
			return (
				getComputedStyle(document.querySelector("#detail-back")).display ===
					"none" &&
				exactAction !== null &&
				document.activeElement === exactAction &&
				selectors.every(
					(selector) => document.querySelector(selector)?.inert === false,
				)
			);
		}, ordinarySplitPaneSelectors);
	const returnToTimeline = async () => {
		await page.setViewportSize(narrow);
		await page.waitForFunction(
			(selectors) =>
				getComputedStyle(document.querySelector("#detail-back")).display !==
					"none" &&
				selectors.every(
					(selector) => document.querySelector(selector)?.inert === true,
				),
			ordinarySplitPaneSelectors,
		);
		await page.locator("#detail-back").click();
		await waitForTimeline();
	};

	const bootstrapId = "browser-bootstrap";
	const bootstrap = {
		id: bootstrapId,
		lane: "browser",
		required: true,
		attempted: true,
		status: "passed",
		dependsOn: [],
		phase: "authenticated-browser-bootstrap",
		fixtureCheckpoint: fixtureCheckpoint("browser-bootstrap"),
		artifactPaths: [],
	};
	try {
		await page.goto(
			`${config.server.baseUrl}/#/?token=${encodeURIComponent(config.server.token)}`,
			{ waitUntil: "domcontentloaded" },
		);
	} catch (error) {
		recordCaseFailure(
			bootstrap,
			"authenticated-browser-bootstrap",
			error,
			{ authenticatedNavigation: "completed" },
			"lane_invalid",
		);
	}
	cases.push(bootstrap);
	for (let iteration = 1; iteration <= config.iterations; iteration += 1) {
		if (bootstrap.status !== "passed") {
			cases.push({
				id: `browser-widen-${iteration}`,
				lane: "browser",
				required: true,
				attempted: false,
				status: "skipped",
				dependsOn: [bootstrapId],
				skipReason: `dependency ${bootstrapId} did not pass`,
				phase: "narrow-exact-event-to-wide-split-pane",
				fixtureCheckpoint: fixtureCheckpoint(`iteration-${iteration}`),
				artifactPaths: [],
			});
			continue;
		}
		const screenshotPath = `browser-artifacts/browser-widen-${iteration}.png`;
		const caseResult = {
			id: `browser-widen-${iteration}`,
			lane: "browser",
			required: true,
			attempted: true,
			status: "passed",
			dependsOn: [bootstrapId],
			phase: "narrow-exact-event-to-wide-split-pane",
			fixtureCheckpoint: fixtureCheckpoint(`iteration-${iteration}`),
			artifactPaths: [screenshotPath],
		};
		let opened = false;
		let screenshotCaptured = false;
		try {
			await page.setViewportSize(narrow);
			await page.goto(timelineUrl, { waitUntil: "domcontentloaded" });
			await waitForTimeline();
			const event = page.locator("#timeline [data-event-id]").first();
			const eventId = await event.getAttribute("data-event-id");
			if (!eventId)
				throw new Error("Timeline fixture did not expose an exact event ID");
			await event.click();
			opened = true;
			await waitForNarrowExactEvent(eventId);
			await page.setViewportSize(wide);
			await waitForWideExactEvent();
			await page.screenshot({
				path: `${config.artifactDir}/browser-widen-${iteration}.png`,
				type: "png",
				fullPage: false,
			});
			screenshotCaptured = true;
		} catch (error) {
			recordCaseFailure(caseResult, "narrow-open-or-wide-settle", error);
		} finally {
			if (!screenshotCaptured) {
				try {
					await page.screenshot({
						path: `${config.artifactDir}/browser-widen-${iteration}.png`,
						type: "png",
						fullPage: false,
					});
				} catch (error) {
					recordCaseFailure(caseResult, "diagnostic-screenshot", error);
				}
			}
			if (opened) {
				try {
					await returnToTimeline();
				} catch (error) {
					recordCaseFailure(caseResult, "narrow-return", error);
				}
			}
		}
		cases.push(caseResult);
	}
	const pageErrorCase = {
		id: "browser-runtime-pageerror",
		lane: "browser",
		required: true,
		attempted: true,
		status: "passed",
		dependsOn: [],
		phase: "browser-pageerror",
		fixtureCheckpoint: fixtureCheckpoint("browser-runtime"),
		artifactPaths: [],
		observations: [],
	};
	for (const error of pageErrors) {
		recordCaseFailure(pageErrorCase, "browser-pageerror", error, {
			pageErrors: [],
		});
	}
	cases.push(pageErrorCase);
	const consoleCase = {
		id: "browser-runtime-console",
		lane: "browser",
		required: true,
		attempted: true,
		status: "passed",
		dependsOn: [],
		phase: "browser-console",
		fixtureCheckpoint: fixtureCheckpoint("browser-runtime"),
		artifactPaths: [],
		observations: [],
	};
	for (const error of consoleErrors) {
		recordCaseFailure(consoleCase, "browser-console", error, {
			consoleErrors: [],
		});
	}
	cases.push(consoleCase);
	const result = {
		schema: "pointbreak.derived-change-diagnostic-collection.v1",
		campaignId: config.campaignId,
		status: cases.every((caseResult) => caseResult.status === "passed")
			? "passed"
			: "failed",
		iterations: config.iterations,
		cases,
	};
	console.log(
		`POINTBREAK_DERIVED_CHANGE_DIAGNOSTIC_BROWSER_RESULT=${JSON.stringify(result)}`,
	);
	return result;
})(__POINTBREAK_DERIVED_CHANGE_DIAGNOSTIC_BROWSER_CONFIG__);
