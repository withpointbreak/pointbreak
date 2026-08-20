// Internal browser program injected by change-inspector-browser-verify.sh.
// It only reads the disposable Inspector page and writes screenshots under its configured root.
// biome-ignore format: playwright-cli run-code wraps this file as one function expression.
((config) => async (page) => {
	// biome-ignore lint/correctness/noUnusedVariables: the rendered diagnostics closure uses this binding.
	const BrowserDiagnosticFailure = __POINTBREAK_BROWSER_DIAGNOSTIC_FAILURE__;
	const createBrowserDiagnostics = __POINTBREAK_BROWSER_DIAGNOSTICS__;
	let screenshots = 0;
	let lastScreenshot = null;
	const consoleErrors = [];
	const pageErrors = [];
	const requestFailures = [];
	const serviceUnavailableResponses = [];
	let insideAppendWindow = false;
	const bootstrapUrl = (server) =>
		`${server.baseUrl}/#/?token=${encodeURIComponent(server.token)}`;
	function isDeliberateChangeProjectionTransition(record, primaryBaseUrl) {
		if (!record.insideAppendWindow || record.status !== 503) return false;
		if (typeof record.url !== "string" || typeof primaryBaseUrl !== "string")
			return false;
		const primaryOrigin = primaryBaseUrl.endsWith("/")
			? primaryBaseUrl.slice(0, -1)
			: primaryBaseUrl;
		const isPrimaryChangeRoute = [
			"/api/v2/changes",
			"/api/v2/attention",
		].some((path) => {
			const route = `${primaryOrigin}${path}`;
			return record.url === route || record.url.startsWith(`${route}?`);
		});
		if (!isPrimaryChangeRoute) return false;
		const body = record.body;
		return (
			typeof body === "object" &&
			body !== null &&
			record.schema === "pointbreak.inspect-change-projection-error" &&
			body.schema === record.schema &&
			body.version === 1 &&
			body.code === "projection_unstable" &&
			body.retryable === true
		);
	}
	const responseInspections = new Set();
	const settleResponseInspections = async () => {
		while (responseInspections.size > 0) {
			await Promise.all(Array.from(responseInspections));
		}
	};
	page.on("console", (message) => {
		if (message.type() !== "error") return;
		consoleErrors.push({
			text: message.text(),
			url: message.location().url || null,
			insideAppendWindow,
		});
	});
	page.on("pageerror", (error) => pageErrors.push(error.message));
	page.on("requestfailed", (request) => {
		requestFailures.push({
			method: request.method(),
			resourceType: request.resourceType(),
			url: request.url(),
			error: request.failure()?.errorText ?? "unknown request failure",
		});
	});
	page.on("response", (response) => {
		if (response.status() !== 503) return;
		const responseWindow = insideAppendWindow;
		const inspection = (async () => {
			let bodyText;
			try {
				bodyText = await response.text();
			} catch (error) {
				bodyText = `response body unavailable: ${error instanceof Error ? error.message : String(error)}`;
			}
			let body = bodyText;
			try {
				body = JSON.parse(bodyText);
			} catch {
				// Preserve the original response text when the server did not return JSON.
			}
			serviceUnavailableResponses.push({
				url: response.url(),
				status: response.status(),
				body,
				schema:
					typeof body === "object" && body !== null
						? (body.schema ?? null)
						: null,
				insideAppendWindow: responseWindow,
			});
		})();
		responseInspections.add(inspection);
		void inspection.finally(() => responseInspections.delete(inspection));
	});

	const layouts = [
		{ name: "wide", width: 1440, height: 1000 },
		{ name: "narrow", width: 390, height: 844 },
	];
	const diagnostics = createBrowserDiagnostics({
		context: () => ({
			route: page.url(),
			viewport: page.viewportSize(),
			screenshot: lastScreenshot,
			log: "logs/browser-gate.log",
		}),
		isFatal: (error) => {
			const detail = error instanceof Error ? error.message : String(error);
			return (
				page.isClosed() ||
				(detail.includes(config.server.baseUrl) &&
					/ERR_CONNECTION_REFUSED|ECONNREFUSED|connection.*closed/i.test(
						detail,
					)) ||
				/target page, context or browser has been closed|browser has been closed/i.test(
					detail,
				)
			);
		},
	});
	await page.goto(bootstrapUrl(config.server), {
		waitUntil: "domcontentloaded",
	});
	const fail = (label, detail) => {
		throw new Error(`${label}: ${detail}`);
	};
	const expect = (condition, label, detail, comparison) =>
		diagnostics.expect(condition, label, detail, comparison);
	const compare = (condition, label, detail, expected, actual) =>
		diagnostics.expect(condition, label, detail, { expected, actual });
	const requireCondition = (condition, label, detail, expected, actual) =>
		diagnostics.requireCondition(condition, label, detail, {
			expected,
			actual,
		});
	const teardownSection = async () => {
		await page.evaluate(() => {
			if (document.activeElement instanceof HTMLElement)
				document.activeElement.blur();
		});
	};
	// The instrumented bootstrap has moved the one-time fragment capability into
	// origin-scoped sessionStorage. Route changes are same-document navigation
	// and therefore use only the strict, shareable Change route grammar.
	const url = (route) => `${config.server.baseUrl}/#/${route}`;
	// Playwright serializes this callback into the page. Query normalization must
	// stay there because the run-code sandbox does not expose URLSearchParams.
	const semanticRouteMatchesInPage = ({ expectedHash, source }) => {
		const normalize = (value) => {
			const raw = value.startsWith("#/")
				? value.slice(2)
				: value.startsWith("#")
					? value.slice(1)
					: value;
			const separator = raw.indexOf("?");
			const path = separator === -1 ? raw : raw.slice(0, separator);
			const search = separator === -1 ? "" : raw.slice(separator + 1);
			const entries = Array.from(new URLSearchParams(search).entries()).sort(
				([leftKey, leftValue], [rightKey, rightValue]) =>
					leftKey.localeCompare(rightKey) || leftValue.localeCompare(rightValue),
			);
			return JSON.stringify([path, entries]);
		};
		const actualHash =
			source === "timeline"
				? document.querySelector("#timeline")?.dataset.timelineRoute
				: location.hash;
		return (
			actualHash !== undefined &&
			normalize(actualHash) === normalize(expectedHash)
		);
	};
	const currentRouteMatches = (expectedHash) =>
		page.evaluate(semanticRouteMatchesInPage, {
			expectedHash,
			source: "location",
		});
	const waitForCurrentRoute = (expectedHash, source = "location") =>
		page.waitForFunction(semanticRouteMatchesInPage, { expectedHash, source });
	const waitForRoutedDiffFocus = (kind, identity) =>
		page.waitForFunction(
			({ kind, identity }) => {
				if (!document.querySelector("#diff-page:not(.hidden)")) return false;
				const candidates = Array.from(
					document.querySelectorAll(
						kind === "file"
							? "#diff-page-body .dfile"
							: "#diff-page-body [data-anno]",
					),
				);
				const matches = candidates.filter((candidate) =>
					kind === "file"
						? candidate.dataset.filePath === identity ||
							candidate.dataset.oldFilePath === identity ||
							candidate.dataset.newFilePath === identity
						: candidate.dataset.anno === identity,
				);
				const target =
					kind === "fact"
						? (matches.find((candidate) => candidate.classList.contains("anno")) ??
							matches[0])
						: matches[0];
				return (
					target !== undefined &&
					target.getClientRects().length > 0 &&
					target.dataset.exactFocus === "true" &&
					document.activeElement === target
				);
			},
			{ kind, identity },
		);
	const screenshot = async (name) => {
		screenshots += 1;
		const path = `${config.artifactDir}/${name}.png`;
		try {
			await page.screenshot({ path, type: "png", fullPage: false });
		} catch (error) {
			diagnostics.abort(
				`screenshot evidence sink failed: ${error instanceof Error ? error.message : String(error)}`,
				{
					expected: path,
					actual: error instanceof Error ? error.message : String(error),
				},
			);
		}
		lastScreenshot = `screenshots/${name}.png`;
	};
	const open = async (route, layout, label) => {
		await page.setViewportSize({ width: layout.width, height: layout.height });
		const targetUrl = url(route);
		const targetHash = `#/${route}`;
		const priorKeys = await page.evaluate(() => ({
			changeList:
				document.querySelector("#master")?.dataset.changeListKey ?? null,
			reading:
				document.querySelector("#detail-body")?.dataset.changeReadingKey ??
				null,
			route: location.hash,
		}));
		const reload =
			page.url().startsWith(`${config.server.baseUrl}/`) &&
			(await currentRouteMatches(targetHash));
		const [expectedPath] = route.split("?", 2);
		const eventPrefix = "timeline/events/";
		const expectedEventId = expectedPath.startsWith(eventPrefix)
			? decodeURIComponent(expectedPath.slice(eventPrefix.length))
			: null;
		const companionTimelineHash = expectedPath.startsWith(eventPrefix)
			? `#/timeline${route.includes("?") ? `?${route.split("?", 2)[1]}` : ""}`
			: expectedPath === "timeline"
				? targetHash
				: null;
		// A goto to the exact current fragment is a no-op. Force a document reload
		// so a deliberately refused reader-profile fixture cannot leak its DOM
		// into the real reader that follows it.
		if (reload) {
			await page.reload({ waitUntil: "domcontentloaded" });
		} else {
			await page.goto(targetUrl, { waitUntil: "domcontentloaded" });
		}
		await waitForCurrentRoute(targetHash);
		await page.waitForFunction(
			() =>
				document.querySelector("#connection-status")?.textContent ===
				"connected",
		);
		if (companionTimelineHash !== null) {
			await waitForCurrentRoute(companionTimelineHash, "timeline");
		}
		const readiness = await page.waitForFunction(
			({ expectedRoute, expectedEventId, priorKeys, reload }) => {
				const refusal = document.querySelector("#master")?.textContent?.trim();
				if (refusal?.startsWith("Reader refused:")) {
					return { state: "refused", detail: refusal };
				}
				const stamp = document.querySelector("#stat-hash")?.textContent?.trim();
				if (!stamp || stamp === "—" || !document.querySelector("#master h1"))
					return false;
				const [path] = expectedRoute.split("?", 2);
				const expectedLens = path.split("/", 1)[0] || "timeline";
				if (expectedLens === "timeline") {
					const timeline = document.querySelector("#timeline");
					const timelineKey =
						document.querySelector("#master")?.dataset.timelineKey;
					if (expectedEventId !== null) {
						const selected = Array.from(
							document.querySelectorAll("#timeline [data-event-id]"),
						).find((row) => row.dataset.eventId === expectedEventId);
						const detailIdentity = document.querySelector(
							"#detail-body [data-event-id]",
						);
						return timelineKey !== undefined &&
							selected?.getAttribute("aria-selected") === "true" &&
							detailIdentity?.dataset.eventId === expectedEventId &&
							timeline?.getAttribute("aria-activedescendant") === selected.id
							? { state: "ready" }
							: false;
					}
					return timeline &&
						timelineKey !== undefined &&
						!document.querySelector("#detail-body [data-event-id]")
						? { state: "ready" }
						: false;
				}
				const rawKey = document.querySelector("#master")?.dataset.changeListKey;
				if (!rawKey) return false;
				try {
					const key = JSON.parse(rawKey);
					if (path.includes("/")) {
						const readingKey =
							document.querySelector("#detail-body")?.dataset.changeReadingKey;
						return key.lens === expectedLens &&
							readingKey &&
							(reload || readingKey !== priorKeys.reading)
							? { state: "ready" }
							: false;
					}
					const retainedExactCompanion = priorKeys.route.startsWith(
						`#/${expectedLens}/`,
					);
					return key.lens === expectedLens &&
						(reload ||
							rawKey !== priorKeys.changeList ||
							retainedExactCompanion)
						? { state: "ready" }
						: false;
				} catch {
					return false;
				}
			},
			{ expectedRoute: route, expectedEventId, priorKeys, reload },
		);
		const readinessState = await readiness.jsonValue();
		await readiness.dispose();
		if (readinessState.state === "refused") {
			fail(label, readinessState.detail);
		}
		const semanticHash = await page.evaluate(() => location.hash);
		compare(
			!semanticHash.includes("token="),
			label,
			"capability leaked into the semantic route",
			false,
			semanticHash.includes("token="),
		);
		const metrics = await page.evaluate(() => ({
			width: document.documentElement.clientWidth,
			scrollWidth: document.documentElement.scrollWidth,
			liveCards: document.querySelectorAll(".unit-card[data-change-id]").length,
			liveEvents: document.querySelectorAll("#timeline [data-event-id]").length,
		}));
		compare(
			metrics.width === layout.width,
			label,
			"unexpected viewport width",
			layout.width,
			metrics.width,
		);
		compare(
			metrics.scrollWidth <= metrics.width,
			label,
			"horizontal overflow",
			`<= ${metrics.width}`,
			metrics.scrollWidth,
		);
		return metrics;
	};
	const hash = () => page.evaluate(() => location.hash);
	const routeParameter = (name) =>
		page.evaluate(
			(parameterName) =>
				new URLSearchParams(location.hash.split("?", 2)[1] ?? "").get(
					parameterName,
				),
			name,
		);
	const routeParameters = (names) =>
		page.evaluate((parameterNames) => {
			const params = new URLSearchParams(
				location.hash.split("?", 2)[1] ?? "",
			);
			return Object.fromEntries(
				parameterNames.map((name) => [name, params.get(name)]),
			);
		}, names);
	const shortRef = (value) => {
		let match = String(value).match(
			/^([a-z][a-z-]*):(?:git:|worktree:)?sha256:([0-9a-f]{6,})$/i,
		);
		if (match) return `${match[1]}:${match[2].slice(0, 8)}`;
		match = String(value).match(/^sha256:([0-9a-f]{8,})$/i);
		if (match) return `sha256:${match[1].slice(0, 8)}`;
		if (/^[0-9a-f]{40}$/i.test(String(value)))
			return String(value).slice(0, 10);
		return String(value);
	};
	const shortExact = (revisionId, artifactHash) =>
		`${shortRef(revisionId)} · ${shortRef(artifactHash)}`;
	const waitForTimelineRoute = (route) =>
		page.waitForFunction(
			(expectedRoute) =>
				location.hash === expectedRoute &&
				document.querySelector("#timeline")?.dataset.timelineRoute ===
					expectedRoute,
			route,
		);
	const expectedTimelineEventIdentity = (eventId) => ({
		eventId,
		title: eventId,
		name: `event ${eventId}`,
	});
	const readTimelineEventIdentity = async (label) => {
		const detailIdentity = page.locator("#detail-body [data-event-id]");
		const timelineDetailIdentityCount = await detailIdentity.count();
		requireCondition(
			timelineDetailIdentityCount === 1,
			label,
			"exact Timeline detail did not expose one event identity",
			1,
			timelineDetailIdentityCount,
		);
		return {
			eventId: await detailIdentity.getAttribute("data-event-id"),
			title: await detailIdentity.getAttribute("title"),
			name: await detailIdentity.getAttribute("aria-label"),
		};
	};
	const compareTimelineEventIdentity = (eventId, label, actual) => {
		const expected = expectedTimelineEventIdentity(eventId);
		compare(
			JSON.stringify(actual) === JSON.stringify(expected),
			label,
			`event detail did not retain exact identity ${eventId}`,
			expected,
			actual,
		);
		return expected;
	};
	const exactEventRouteFromTimelineRoute = (timelineRoute, eventId) => {
		const [timelinePath, timelineSearch = ""] = timelineRoute.split("?", 2);
		return `${timelinePath}/events/${encodeURIComponent(eventId)}${timelineSearch ? `?${timelineSearch}` : ""}`;
	};
	const waitForExactTimelineEvent = async (eventId) => {
		await page.waitForFunction((expectedEventId) => {
			const selected = document.querySelector(
				'#timeline [aria-selected="true"]',
			);
			const detailIdentity = document.querySelector(
				"#detail-body [data-event-id]",
			);
			return (
				location.hash.includes(
					`/timeline/events/${encodeURIComponent(expectedEventId)}`,
				) &&
				!document.querySelector("#detail")?.inert &&
				detailIdentity?.dataset.eventId === expectedEventId &&
				selected?.dataset.eventId === expectedEventId &&
				document
					.querySelector("#timeline")
					?.getAttribute("aria-activedescendant") === selected.id
			);
		}, eventId);
		return readTimelineEventIdentity("Timeline exact event identity");
	};
	const readExactDetailIdentitySources = () =>
		page
			.locator("#detail-body dd[title][aria-label]")
			.evaluateAll((nodes) => {
				const identityTokens = (value) =>
					value
						.split(/\s*(?:;|·)\s*/u)
						.map((token) => token.trim())
						.filter(Boolean);
				return nodes.map((node) => {
					const title = node.getAttribute("title") ?? "";
					const name = node.getAttribute("aria-label") ?? "";
					return {
						title,
						name,
						titleTokens: identityTokens(title),
						nameTokens: identityTokens(name),
					};
				});
			});
	const containsExactDetailIdentity = (sources, identity) =>
		sources.some(
			(source) =>
				source.titleTokens.includes(identity) &&
				source.nameTokens.includes(identity),
		);
	const waitForLens = (lens) =>
		page.waitForFunction((expectedLens) => {
			if (expectedLens === "timeline")
				return Boolean(document.querySelector("#timeline"));
			const rawKey = document.querySelector("#master")?.dataset.changeListKey;
			if (!rawKey) return false;
			try {
				return JSON.parse(rawKey).lens === expectedLens;
			} catch {
				return false;
			}
		}, lens);
	const selected = () =>
		page.locator(".unit-card.change-card-selected[data-change-id]");
	const cardNamesAreUseful = () =>
		page.evaluate(() =>
			Array.from(document.querySelectorAll(".unit-card[data-change-id]")).every(
				(card) => {
					const name = card.getAttribute("aria-label") || "";
					const exact = card.dataset.changeId || "";
					const peers = Array.from(
						card.querySelectorAll(".change-card-peer-open"),
					);
					const exactPeersAreNamed = peers.every((peer) => {
						const title = peer.getAttribute("title") || "";
						const identity = /^exact Revision (.+); artifact (.+)$/.exec(title);
						if (!identity) return false;
						const [, revisionId, artifactHash] = identity;
						const peerName = peer.getAttribute("aria-label") || "";
						return Boolean(
							revisionId &&
								artifactHash &&
								name.includes(revisionId) &&
								name.includes(artifactHash) &&
								peerName.includes(revisionId) &&
								peerName.includes(artifactHash),
						);
					});
					return (
						name.includes(exact) &&
						name.length > exact.length &&
						!/^Change\s+[^\s]+$/.test(name) &&
						exactPeersAreNamed
					);
				},
			),
		);
	const noHiddenTabStops = () =>
		page.evaluate(() => {
			const selector =
				"a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])";
			return Array.from(document.querySelectorAll(selector))
				.filter((node) =>
					node.closest(".hidden, [hidden], [aria-hidden='true'], [inert]"),
				)
				.every((node) => {
					if (node.closest("[inert]")) return true;
					const style = getComputedStyle(node);
					return (
						node.getClientRects().length === 0 ||
						style.display === "none" ||
						style.visibility === "hidden"
					);
				});
		});
	const expectLensHierarchy = async (expectedLens) => {
		const hierarchy = await page.evaluate(() => {
			const headings = Array.from(document.querySelectorAll("#master h1"));
			const current = document.querySelector(
				"#lens-switcher .lens-tab[aria-current='page']",
			);
			const metadata = document.querySelector("#master .lens-meta");
			return {
				headingCount: headings.length,
				heading: headings[0]?.textContent?.trim() || "",
				selectedTab: current?.textContent?.trim() || "",
				metadata: metadata?.textContent?.trim() || "",
			};
		});
		compare(
			hierarchy.headingCount === 1 &&
				hierarchy.heading === expectedLens &&
				hierarchy.selectedTab.startsWith(expectedLens) &&
				hierarchy.metadata.length > 0,
			`${expectedLens} lens hierarchy`,
			`lens heading, selected tab, or count/order metadata drifted: ${JSON.stringify(hierarchy)}`,
			{
				headingCount: 1,
				heading: expectedLens,
				selectedTabPrefix: expectedLens,
				metadata: "nonempty",
			},
			hierarchy,
		);
	};

	const isHistoryRequest = (requestUrl, server) => {
		const endpoint = `${server.baseUrl}/api/v2/history`;
		return requestUrl === endpoint || requestUrl.startsWith(`${endpoint}?`);
	};
	const exerciseReaderState = async (
		server,
		label,
		expectedText,
		expectHistory,
	) => {
		const historyRequests = [];
		const recordRequest = (request) => {
			if (isHistoryRequest(request.url(), server)) {
				historyRequests.push(request.url());
			}
		};
		page.on("request", recordRequest);
		try {
			await page.goto(bootstrapUrl(server), { waitUntil: "domcontentloaded" });
			await page.waitForFunction(
				(text) =>
					document.querySelector("#master")?.textContent?.includes(text),
				expectedText,
			);
			const semanticHash = await hash();
			compare(
				!semanticHash.includes("token="),
				label,
				"capability remained in the semantic route",
				false,
				semanticHash.includes("token="),
			);
			const historyRequestCount = historyRequests.length;
			compare(
				expectHistory ? historyRequestCount > 0 : historyRequestCount === 0,
				label,
				expectHistory
					? "ready empty L2 never requested authoritative history"
					: "unready profile leaked history request(s)",
				expectHistory ? "> 0" : 0,
				historyRequestCount,
			);
			await screenshot(`reader-${label}`);
		} finally {
			page.off("request", recordRequest);
		}
	};

	await diagnostics.section("Reader readiness", {
		setup: () => page.setViewportSize(layouts[0]),
		run: async () => {
			// Readiness sequencing is exercised against real tiny Inspector servers.
			// Unready profiles must stop before history; only the complete empty L2 root
			// is allowed to request and paint a zero-event Timeline.
			await exerciseReaderState(
				config.readerServers.emptyReadyL2,
				"empty-ready-l2",
				"0 recorded events",
				true,
			);
			await page.goto(
				`${config.readerServers.emptyReadyL2.baseUrl}/#/changes`,
				{ waitUntil: "domcontentloaded" },
			);
			await page.waitForFunction(() =>
				document.querySelector("#master")?.textContent?.includes("No Changes."),
			);
			const emptyChangesText = await page.locator("#master").innerText();
			compare(
				emptyChangesText.includes("No Changes."),
				"empty ready Changes",
				"an activated empty store did not expose the Change-aware empty state",
				true,
				emptyChangesText.includes("No Changes."),
			);
			await screenshot("reader-empty-ready-l2-changes");
			await exerciseReaderState(
				config.readerServers.l0,
				"l0",
				"Store migration required. No Change state was loaded.",
				false,
			);
			await exerciseReaderState(
				config.readerServers.m1,
				"m1",
				"Store migration in progress. Partial Change state is unavailable.",
				false,
			);

			// A structurally incompatible profile is a separate refusal from L0/M1.
			// Interception keeps the real authenticated request lifecycle while proving
			// that profile validation happens before any history fetch is dispatched.
			let mismatchHistoryRequests = 0;
			const recordMismatchRequest = (request) => {
				if (isHistoryRequest(request.url(), config.server))
					mismatchHistoryRequests += 1;
			};
			await page.route("**/api/v2/profile", async (route) => {
				const response = await route.fetch();
				const profile = await response.json();
				await route.fulfill({ response, json: { ...profile, version: 999 } });
			});
			page.on("request", recordMismatchRequest);
			try {
				await page.goto(url(""), { waitUntil: "domcontentloaded" });
				await page.waitForFunction(() =>
					document
						.querySelector("#master")
						?.textContent?.includes(
							"Reader refused: incompatible Inspector reader profile",
						),
				);
				compare(
					mismatchHistoryRequests === 0,
					"profile mismatch",
					"profile refusal leaked history request(s)",
					0,
					mismatchHistoryRequests,
				);
				await screenshot("reader-profile-mismatch");
			} finally {
				page.off("request", recordMismatchRequest);
				try {
					await page.unroute("**/api/v2/profile");
				} catch (error) {
					diagnostics.abort(
						`reader-profile interceptor cleanup failed: ${error instanceof Error ? error.message : String(error)}`,
					);
				}
			}
		},
		teardown: teardownSection,
	});

	await diagnostics.section("Timeline overview and chronology", {
		setup: () => open("", layouts[0], "default Timeline startup"),
		run: async (defaultTimeline) => {
			// The monitor is the default reader, including a capability-bearing startup
			// URL whose token must never survive in the semantic fragment.  Its entries
			// are virtualized; these checks deliberately assert a bounded live DOM, not
			// the total authoritative event count.
			await page.waitForFunction(() => {
				const repository = document
					.querySelector("#store-chip-repo")
					?.textContent?.trim();
				const labels = Array.from(
					document.querySelectorAll("#store-identity-rows dt"),
					(item) => item.textContent?.trim(),
				);
				return (
					repository &&
					repository !== "local server" &&
					labels.includes("repository") &&
					labels.includes("store")
				);
			});
			const identityChrome = await page.evaluate(() => {
				const rows = Array.from(
					document.querySelectorAll("#store-identity-rows dt"),
				).map((term) => ({
					label: term.textContent?.trim() || "",
					value: term.nextElementSibling?.textContent?.trim() || "",
				}));
				return {
					repository:
						document.querySelector("#store-chip-repo")?.textContent?.trim() ||
						"",
					chipName:
						document.querySelector("#store-chip")?.getAttribute("aria-label") ||
						"",
					rows,
					title: document.title,
					connection:
						document.querySelector("#connection-status")?.textContent?.trim() ||
						"",
					refresh:
						document.querySelector("#refresh-status")?.textContent?.trim() ||
						"",
					refreshState:
						document.querySelector("#refresh")?.getAttribute("data-state") ||
						"",
				};
			});
			expect(
				identityChrome.repository.length > 0 &&
					identityChrome.repository !== "local server" &&
					identityChrome.rows.some(
						(row) =>
							row.label === "repository" &&
							row.value === identityChrome.repository,
					) &&
					identityChrome.rows.some(
						(row) => row.label === "store" && row.value.endsWith(" store"),
					) &&
					identityChrome.rows.every(
						(row) =>
							row.value.length > 0 &&
							identityChrome.chipName.includes(row.value),
					) &&
					identityChrome.title ===
						`${identityChrome.repository} · Pointbreak Review` &&
					identityChrome.connection === "connected" &&
					identityChrome.refresh === "watching" &&
					identityChrome.refreshState === "watching",
				"repository identity chrome",
				`repository identity or accepted refresh state was incomplete: ${JSON.stringify(identityChrome)}`,
			);
			await expectLensHierarchy("Timeline");
			compare(
				defaultTimeline.liveEvents > 0 && defaultTimeline.liveEvents < 80,
				"default Timeline startup",
				"expected bounded live Timeline window",
				"1..79",
				defaultTimeline.liveEvents,
			);
			const initialTimelineText = await page.locator("#master").innerText();
			const recordedEvents = Number(
				(initialTimelineText.match(/([0-9]+) recorded events/) || [])[1],
			);
			compare(
				recordedEvents >= 300,
				"default Timeline startup",
				"expected recorded public events",
				">= 300",
				recordedEvents,
			);
			const defaultChronologyDeclared = initialTimelineText
				.toLowerCase()
				.includes("newest first");
			compare(
				defaultChronologyDeclared,
				"descending chronology",
				"default Timeline did not declare newest-first chronology",
				true,
				defaultChronologyDeclared,
			);
			expect(
				await page.locator("#timeline [data-event-id]").evaluateAll((rows) =>
					rows.every((row) => {
						const name = row.getAttribute("aria-label") || "";
						return name.includes("event ") && name.length > 24;
					}),
				),
				"readable Timeline rows",
				"a live Timeline row lacked an accessible event identity",
			);
			const semanticTimelineRows = await page
				.locator("#timeline [data-event-id]")
				.evaluateAll((rows) =>
					rows.map((row) => {
						const eventId = row.dataset.eventId || "";
						const title = row.querySelector(".title");
						const summary = row.querySelector(".event-summary");
						const eventLink = row.querySelector(
							'a.ref[data-timeline-context-kind="event"]',
						);
						const contextLinks = Array.from(
							row.querySelectorAll(
								'a.ref[data-timeline-context-kind="change"], a.ref[data-timeline-context-kind="revision"]',
							),
						);
						return {
							eventId,
							title: title?.textContent || "",
							titleSource: title?.getAttribute("title") || "",
							summary: summary?.textContent || "",
							eventVisible: eventLink?.textContent || "",
							eventTitle: eventLink?.getAttribute("title") || "",
							eventHref: eventLink?.getAttribute("href") || "",
							contextLinks: contextLinks.map((link) => ({
								visible: link.textContent || "",
								title: link.getAttribute("title") || "",
								href: link.getAttribute("href") || "",
							})),
						};
					}),
				);
			expect(
				semanticTimelineRows.every(
					(row) =>
						row.eventId.length > 0 &&
						row.title.length > 0 &&
						row.title.length <= 120 &&
						row.titleSource.length >= row.title.length &&
						row.eventVisible.length > 0 &&
						row.eventVisible !== row.eventTitle &&
						row.eventTitle === row.eventId &&
						row.eventHref.includes(
							`/timeline/events/${encodeURIComponent(row.eventId)}`,
						) &&
						row.summary.length <= 180,
				),
				"compact semantic Timeline rows",
				"Timeline did not retain bounded prose plus a shortened, exact event link",
			);
			const timelineContextLinks = semanticTimelineRows.flatMap(
				(row) => row.contextLinks,
			);
			expect(
				timelineContextLinks.length > 0 &&
					timelineContextLinks.every(
						(link) =>
							link.visible.length > 0 &&
							link.visible !== link.title &&
							(link.href.startsWith("#/changes/") ||
								link.href.startsWith("#/timeline?")),
					),
				"compact semantic Timeline rows",
				"Timeline Change or exact Revision references were not shortened native links",
			);
			const initialTimelineIds = await page
				.locator("#timeline [data-event-id]")
				.evaluateAll((rows) => rows.map((row) => row.dataset.eventId));
			await screenshot("wide-timeline-default");
			await screenshot("wide-timeline-semantic-rows");

			// Chronology is server-owned and continuations are opaque/signed.  Exercise
			// both directions through the UI rather than constructing a token here.
			const timelineKey = await page
				.locator("#master")
				.getAttribute("data-timeline-key");
			const timelineHash = await hash();
			const timelineNextPage = page.getByRole("button", {
				name: "Next page",
			});
			const timelineNextPageCount = await timelineNextPage.count();
			requireCondition(
				timelineNextPageCount === 1,
				"signed Timeline continuation",
				"initial Timeline page did not expose one next-page control",
				1,
				timelineNextPageCount,
			);
			await timelineNextPage.click();
			await page.waitForFunction(
				({ key, priorHash }) => {
					const master = document.querySelector("#master");
					const query = new URLSearchParams(
						location.hash.split("?", 2)[1] ?? "",
					);
					const nextKey = master?.dataset.timelineKey;
					return (
						location.hash !== priorHash &&
						query.has("after") &&
						nextKey !== undefined &&
						nextKey !== key &&
						Boolean(document.querySelector("#timeline")) &&
						document.querySelectorAll("#timeline [data-event-id]").length > 0 &&
						!master?.textContent?.includes("Loading Change generation")
					);
				},
				{ key: timelineKey, priorHash: timelineHash },
			);
			const nextTimelineHash = await hash();
			compare(
				nextTimelineHash.includes("after="),
				"signed Timeline continuation",
				"next Timeline page did not preserve a continuation in the route",
				true,
				nextTimelineHash.includes("after="),
			);
			const nextTimelineEventCount = await page
				.locator("#timeline [data-event-id]")
				.count();
			requireCondition(
				nextTimelineEventCount > 0,
				"signed Timeline continuation",
				"next Timeline page was empty",
				"> 0",
				nextTimelineEventCount,
			);
			const middlePageAnchor = await page
				.locator("#timeline [data-event-id]")
				.first()
				.getAttribute("data-event-id");
			requireCondition(
				Boolean(middlePageAnchor),
				"anchored Timeline paging",
				"middle Timeline page had no exact event anchor",
				"nonempty event ID",
				middlePageAnchor,
			);
			const timelinePreviousPage = page.getByRole("button", {
				name: "Previous page",
			});
			const timelinePreviousPageCount = await timelinePreviousPage.count();
			requireCondition(
				timelinePreviousPageCount === 1,
				"signed Timeline continuation",
				"continued Timeline page did not expose one previous-page control",
				1,
				timelinePreviousPageCount,
			);
			await timelinePreviousPage.click();
			await page.waitForFunction(
				(ids) =>
					JSON.stringify(
						Array.from(
							document.querySelectorAll("#timeline [data-event-id]"),
							(row) => row.dataset.eventId,
						),
					) === JSON.stringify(ids),
				initialTimelineIds,
			);
			const restoredTimelineIds = await page
				.locator("#timeline [data-event-id]")
				.evaluateAll((rows) => rows.map((row) => row.dataset.eventId));
			compare(
				JSON.stringify(restoredTimelineIds) ===
					JSON.stringify(initialTimelineIds),
				"opaque Timeline continuation",
				"Previous did not restore the original Timeline page identity",
				initialTimelineIds,
				restoredTimelineIds,
			);
			const anchoredTimelineRoute = `timeline?limit=100&order=desc&at=${encodeURIComponent(middlePageAnchor)}`;
			const followAnchoredNeighbor = async (name) => {
				await open(
					anchoredTimelineRoute,
					layouts[0],
					`anchored Timeline ${name.toLowerCase()}`,
				);
				const previousPageCount = await page
					.getByRole("button", { name: "Previous page" })
					.count();
				const nextPageCount = await page
					.getByRole("button", { name: "Next page" })
					.count();
				requireCondition(
					previousPageCount === 1 && nextPageCount === 1,
					"anchored Timeline paging",
					"middle anchored page did not expose both adjacent signed continuations",
					{ previousPageCount: 1, nextPageCount: 1 },
					{ previousPageCount, nextPageCount },
				);
				const before = await hash();
				const beforeKey = await page
					.locator("#master")
					.getAttribute("data-timeline-key");
				await page.getByRole("button", { name }).click();
				await page.waitForFunction(
					({ priorHash, priorKey }) => {
						const master = document.querySelector("#master");
						const query = new URLSearchParams(
							location.hash.split("?", 2)[1] ?? "",
						);
						const timelineKey = master?.dataset.timelineKey;
						return (
							location.hash !== priorHash &&
							query.has("after") &&
							!query.has("at") &&
							timelineKey !== undefined &&
							timelineKey !== priorKey &&
							Boolean(document.querySelector("#timeline")) &&
							document.querySelectorAll("#timeline [data-event-id]").length >
								0 &&
							!master?.textContent?.includes("Loading Change generation")
						);
					},
					{ priorHash: before, priorKey: beforeKey },
				);
				const anchoredTimelineText = await page.locator("#master").innerText();
				compare(
					!anchoredTimelineText.includes("Reader refused"),
					"anchored Timeline paging",
					`${name} retained the mutually exclusive at locator`,
					false,
					anchoredTimelineText.includes("Reader refused"),
				);
			};
			await followAnchoredNeighbor("Next page");
			await followAnchoredNeighbor("Previous page");
			await open(
				"timeline?limit=100&order=asc",
				layouts[0],
				"ascending Timeline",
			);
			const ascendingText = await page.locator("#master").innerText();
			const ascendingChronologyDeclared = ascendingText
				.toLowerCase()
				.includes("oldest first");
			compare(
				ascendingChronologyDeclared,
				"ascending chronology",
				"ascending Timeline did not declare oldest-first chronology",
				true,
				ascendingChronologyDeclared,
			);
			await screenshot("wide-timeline-ascending");
		},
		teardown: teardownSection,
	});

	await diagnostics.section("Timeline search and correlation", {
		setup: () =>
			open(
				"timeline?limit=100&order=desc",
				layouts[0],
				"Timeline search and correlation section setup",
			),
		run: async () => {
			// Search is a server-owned Timeline filter, not a client-side hide/show.
			// Exercise its shared clear-all control against a title that names one
			// exact fixture event. Individual chips belong to structured filters below.
			const timelineSearch = "Browser correction replacement";
			const applyTimelineSearch = async (label) => {
				const priorKey = await page
					.locator("#master")
					.getAttribute("data-timeline-key");
				const search = page.locator("#filter-text");
				await search.fill(timelineSearch);
				await search.press("Tab");
				await page.waitForFunction(
					({ queryText, timelineKey }) => {
						const master = document.querySelector("#master");
						const query = new URLSearchParams(
							location.hash.split("?", 2)[1] ?? "",
						);
						return (
							query.get("q") === queryText &&
							master?.dataset.timelineKey !== undefined &&
							master.dataset.timelineKey !== timelineKey &&
							document.querySelectorAll("#timeline [data-event-id]").length > 0
						);
					},
					{ queryText: timelineSearch, timelineKey: priorKey },
				);
				const correctionEventIds = await page
					.locator("#timeline [data-event-id]")
					.evaluateAll((rows) => rows.map((row) => row.dataset.eventId));
				const expectedCorrectionEventIds = [
					config.fixture.correction.eventId,
				];
				compare(
					JSON.stringify(correctionEventIds) ===
						JSON.stringify(expectedCorrectionEventIds),
					label,
					"server search did not isolate the exact correction event",
					expectedCorrectionEventIds,
					correctionEventIds,
				);
			};
			await open(
				"timeline?limit=100&order=desc",
				layouts[0],
				"Timeline search base",
			);
			await applyTimelineSearch("Timeline search filter");
			await page.locator("#filters-toggle").click();
			await page.getByRole("button", { name: "Clear all" }).click();
			await page.waitForFunction(() => {
				const query = new URLSearchParams(location.hash.split("?", 2)[1] ?? "");
				return !query.has("q") && Boolean(document.querySelector("#timeline"));
			});

			// Drive all typed filters through their reader controls. The browser does
			// not invent query values: Track, Change, and exact Revision options are
			// populated from the server's admitted completion facets.
			const applyTimelineFilter = async (
				id,
				value,
				key,
				label,
				expectedQuery,
			) => {
				await open(
					"timeline?limit=100&order=desc",
					layouts[0],
					`${label} base`,
				);
				const priorKey = await page
					.locator("#master")
					.getAttribute("data-timeline-key");
				const filtersToggle = page.locator("#filters-toggle");
				if ((await filtersToggle.getAttribute("aria-expanded")) !== "true") {
					await filtersToggle.click();
				}
				if (id === "timeline-filter-type") {
					await page.locator(`[data-event-type="${value}"]`).click();
				} else {
					await page.locator(`#${id}`).selectOption(value);
				}
				await page.waitForFunction(
					({ expectedKey, expected, timelineKey }) => {
						const master = document.querySelector("#master");
						const query = new URLSearchParams(
							location.hash.split("?", 2)[1] ?? "",
						);
						return (
							query.has(expectedKey) &&
							Object.entries(expected).every(
								([name, expectedValue]) => query.get(name) === expectedValue,
							) &&
							master?.dataset.timelineKey !== undefined &&
							master.dataset.timelineKey !== timelineKey &&
							document.querySelectorAll("#timeline [data-event-id]").length > 0
						);
					},
					{ expectedKey: key, expected: expectedQuery, timelineKey: priorKey },
				);
				const filteredEventCount = await page
					.locator("#timeline [data-event-id]")
					.count();
				compare(
					filteredEventCount > 0,
					label,
					"typed public fixture filter produced no Timeline entry",
					"> 0",
					filteredEventCount,
				);
				if ((await filtersToggle.getAttribute("aria-expanded")) !== "true") {
					await filtersToggle.click();
				}
				const remove = page.getByRole("button", {
					name: new RegExp(`^Remove ${key} filter:`),
				});
				const removeCount = await remove.count();
				requireCondition(
					removeCount === 1,
					label,
					"typed filter did not create one removable chip",
					1,
					removeCount,
				);
				const filteredKey = await page
					.locator("#master")
					.getAttribute("data-timeline-key");
				await remove.click();
				await page.waitForFunction(
					({ expectedKey, timelineKey }) => {
						const master = document.querySelector("#master");
						const query = new URLSearchParams(
							location.hash.split("?", 2)[1] ?? "",
						);
						return (
							!query.has(expectedKey) &&
							(expectedKey !== "revision" || !query.has("artifactHash")) &&
							master?.dataset.timelineKey !== undefined &&
							master.dataset.timelineKey !== timelineKey &&
							Boolean(document.querySelector("#timeline"))
						);
					},
					{ expectedKey: key, timelineKey: filteredKey },
				);
			};
			await applyTimelineFilter(
				"timeline-filter-type",
				"review_observation_recorded",
				"type",
				"Timeline type filter",
				{ type: "review_observation_recorded" },
			);
			await applyTimelineFilter(
				"timeline-filter-track",
				"agent:matrix-facts",
				"track",
				"Timeline track filter",
				{ track: "agent:matrix-facts" },
			);
			await applyTimelineFilter(
				"timeline-filter-change",
				config.fixture.rich.changeId,
				"change",
				"Timeline Change filter",
				{ change: config.fixture.rich.changeId },
			);
			await applyTimelineFilter(
				"timeline-filter-revision",
				JSON.stringify([
					config.fixture.rich.revisionId,
					config.fixture.rich.artifactHash,
				]),
				"revision",
				"Timeline exact Revision filter",
				{
					revision: config.fixture.rich.revisionId,
					artifactHash: config.fixture.rich.artifactHash,
				},
			);

			const inspectExactTimelineEvent = async (
				eventId,
				label,
				{ identities = [], prose = [] },
			) => {
				await open(
					`timeline/events/${encodeURIComponent(eventId)}?limit=100&order=asc`,
					layouts[0],
					label,
				);
				const row = page.locator(`#timeline [data-event-id="${eventId}"]`);
				const rowCount = await row.count();
				requireCondition(
					rowCount === 1,
					label,
					`exact event ${eventId} was not revealed`,
					1,
					rowCount,
				);
				const rowSelected = await row.getAttribute("aria-selected");
				compare(
					rowSelected === "true",
					label,
					`exact event ${eventId} was not selected`,
					"true",
					rowSelected,
				);
				const activeDescendant = await page
					.locator("#timeline")
					.getAttribute("aria-activedescendant");
				const rowId = await row.getAttribute("id");
				compare(
					activeDescendant === rowId,
					label,
					`exact event ${eventId} was not the active Timeline option`,
					rowId,
					activeDescendant,
				);
				const detailIdentity = await waitForExactTimelineEvent(eventId);
				compareTimelineEventIdentity(eventId, label, detailIdentity);
				const detail = await page.locator("#detail-body").innerText();
				for (const text of prose) {
					expect(detail.includes(text), label, `event detail omitted ${text}`);
				}
				const identitySources = await readExactDetailIdentitySources();
				for (const identity of identities) {
					compare(
						containsExactDetailIdentity(identitySources, identity),
						label,
						`event detail omitted exact identity ${identity}`,
						{ titleToken: identity, accessibleNameToken: identity },
						identitySources,
					);
				}
				return detail;
			};

			await inspectExactTimelineEvent(
				config.fixture.correction.eventId,
				"Timeline correction event",
				{
					identities: [config.fixture.correction.originObservationId],
					prose: ["Browser correction replacement"],
				},
			);
			await inspectExactTimelineEvent(
				config.fixture.factPort.eventId,
				"Timeline fact-port event",
				{
					identities: [config.fixture.factPort.portId],
					prose: ["context only observation"],
				},
			);
			await inspectExactTimelineEvent(
				config.fixture.historicalMembership.withdrawEventId,
				"Timeline membership-withdrawal event",
				{
					identities: [
						config.fixture.historicalMembership.claimId,
						config.fixture.historicalMembership.historicalChangeId,
						config.fixture.historicalMembership.revisionId,
					],
				},
			);

			// The original proposal remains correlated with both its direct Change and
			// the later-withdrawn historical membership. This is a server-derived
			// plural context, not an effective-membership inference from the card list.
			const historical = config.fixture.historicalMembership;
			await open(
				`timeline?limit=100&order=asc&type=work_object_proposed&change=${encodeURIComponent(historical.historicalChangeId)}&revision=${encodeURIComponent(historical.revisionId)}&artifactHash=${encodeURIComponent(historical.artifactHash)}`,
				layouts[0],
				"Timeline withdrawn historical membership",
			);
			const historicalProposals = page.locator("#timeline [data-event-id]");
			const historicalProposalCount = await historicalProposals.count();
			requireCondition(
				historicalProposalCount === 1,
				"Timeline withdrawn historical membership",
				"historical Change filter did not retain the Revision proposal",
				1,
				historicalProposalCount,
			);
			const historicalProposal = historicalProposals.first();
			const historicalProposalEventId =
				await historicalProposal.getAttribute("data-event-id");
			requireCondition(
				Boolean(historicalProposalEventId),
				"Timeline withdrawn historical membership",
				"historical proposal lacked an exact event ID",
				"nonempty event ID",
				historicalProposalEventId,
			);
			const historicalTimelineRoute = await hash();
			const expectedHistoricalEventRoute = exactEventRouteFromTimelineRoute(
				historicalTimelineRoute,
				historicalProposalEventId,
			);
			await historicalProposal.click();
			const historicalEventIdentity = await waitForExactTimelineEvent(
				historicalProposalEventId,
			);
			compareTimelineEventIdentity(
				historicalProposalEventId,
				"Timeline withdrawn historical membership",
				historicalEventIdentity,
			);
			const historicalEventRoute = await hash();
			const historicalEventRouteMatches = await currentRouteMatches(
				expectedHistoricalEventRoute,
			);
			compare(
				historicalEventRouteMatches,
				"Timeline withdrawn historical membership route",
				"historical proposal did not retain its bounded Timeline context",
				{ semanticRoute: expectedHistoricalEventRoute },
				{
					semanticRouteMatches: historicalEventRouteMatches,
					route: historicalEventRoute,
				},
			);
			const historicalProposalIdentitySources =
				await readExactDetailIdentitySources();
			for (const expectedChange of [
				historical.directChangeId,
				historical.historicalChangeId,
			]) {
				compare(
					containsExactDetailIdentity(
						historicalProposalIdentitySources,
						expectedChange,
					),
					"Timeline withdrawn historical membership",
					`proposal detail omitted exact correlated Change ${expectedChange}`,
					{
						titleToken: expectedChange,
						accessibleNameToken: expectedChange,
					},
					historicalProposalIdentitySources,
				);
			}

			await open(
				`timeline?limit=100&order=asc&change=${encodeURIComponent(config.fixture.equalTimestamp.changeId)}`,
				layouts[0],
				"Timeline equal occurredAt pair",
			);
			const equalTimestampOccurredAt = [];
			for (const eventId of config.fixture.equalTimestamp.eventIds) {
				const row = page.locator(`#timeline [data-event-id="${eventId}"]`);
				const rowCount = await row.count();
				requireCondition(
					rowCount === 1,
					"Timeline equal occurredAt pair",
					`equal-time event ${eventId} is absent`,
					1,
					rowCount,
				);
				const occurredAt = await row.locator("time").getAttribute("datetime");
				requireCondition(
					typeof occurredAt === "string" && occurredAt.length > 0,
					"Timeline equal occurredAt pair",
					`event ${eventId} did not expose an authored timestamp`,
					"one non-empty occurredAt",
					occurredAt,
				);
				equalTimestampOccurredAt.push(occurredAt);
			}
			compare(
				new Set(equalTimestampOccurredAt).size === 1,
				"Timeline equal occurredAt pair",
				`capture pair did not retain one operation timestamp: ${JSON.stringify(equalTimestampOccurredAt)}`,
				{ sharedTimestampCount: 1 },
				{
					sharedTimestampCount: new Set(equalTimestampOccurredAt).size,
					occurredAt: equalTimestampOccurredAt,
				},
			);
			const equalTimestampOrder = await page
				.locator("#timeline [data-event-id]")
				.evaluateAll(
					(rows, eventIds) =>
						rows
							.map((row) => row.dataset.eventId)
							.filter((eventId) => eventIds.includes(eventId)),
					config.fixture.equalTimestamp.eventIds,
				);
			const expectedEqualTimestampOrder =
				config.fixture.equalTimestamp.eventIds;
			compare(
				config.fixture.equalTimestamp.tieBreak === "event_id_asc" &&
					JSON.stringify(equalTimestampOrder) ===
						JSON.stringify(expectedEqualTimestampOrder),
				"Timeline equal occurredAt pair",
				`equal-time order ${JSON.stringify(equalTimestampOrder)} did not use event_id_asc`,
				{ tieBreak: "event_id_asc", eventIds: expectedEqualTimestampOrder },
				{
					tieBreak: config.fixture.equalTimestamp.tieBreak,
					eventIds: equalTimestampOrder,
				},
			);
		},
		teardown: teardownSection,
	});

	await diagnostics.section("Timeline preferences", {
		setup: () =>
			open(
				"timeline?limit=100&order=desc",
				layouts[0],
				"Timeline preferences section setup",
			),
		run: async () => {
			// Timeline gets its own preference evidence rather than inheriting the
			// Change-card captures below.
			await open(
				"timeline?limit=100&order=desc",
				layouts[0],
				"Timeline display preferences",
			);
			await page.locator("#view-toggle").click();
			await page.locator("#theme-dark").check();
			await page.locator("#density-comfortable").check();
			const comfortableRowPadding = await page
				.locator("#timeline .event")
				.first()
				.evaluate((row) => {
					const style = getComputedStyle(row);
					return (
						Number.parseFloat(style.paddingBlockStart) +
						Number.parseFloat(style.paddingBlockEnd)
					);
				});
			await page.locator("#density-compact").check();
			const compactRowPadding = await page
				.locator("#timeline .event")
				.first()
				.evaluate((row) => {
					const style = getComputedStyle(row);
					return (
						Number.parseFloat(style.paddingBlockStart) +
						Number.parseFloat(style.paddingBlockEnd)
					);
				});
			compare(
				Number.isFinite(comfortableRowPadding) &&
					Number.isFinite(compactRowPadding) &&
					compactRowPadding < comfortableRowPadding,
				"Timeline density geometry",
				`compact Timeline row padding ${compactRowPadding} did not tighten comfortable padding ${comfortableRowPadding}`,
				`< ${comfortableRowPadding}`,
				compactRowPadding,
			);
			await screenshot("wide-timeline-dark-compact");
			await page.locator("#theme-light").check();
			await page.locator("#density-comfortable").check();
			await screenshot("wide-timeline-light-comfortable");
			await page.locator("#view-toggle").click();
		},
		teardown: teardownSection,
	});

	await diagnostics.section("Timeline keyboard and exact detail", {
		setup: () =>
			open(
				"timeline?limit=100&order=desc",
				layouts[0],
				"Timeline keyboard and exact detail section setup",
			),
		run: async () => {
			// Local cursor movement must remain bounded and must never redirect from a
			// text input.  It is intentionally tested before clicking an event, which
			// parks monitoring as reader activity.
			await open(
				"timeline?limit=100&order=desc",
				layouts[0],
				"Timeline keyboard navigation",
			);
			const listbox = page.locator("#timeline");
			await listbox.focus();
			const activeEvent = () => listbox.getAttribute("aria-activedescendant");
			const waitForGlobalTimelineBoundary = async (
				boundary,
				priorHash,
				priorKey,
			) => {
				await page.waitForFunction(
					({ expectedBoundary, previousHash, previousKey }) => {
						const master = document.querySelector("#master");
						const list = document.querySelector("#timeline");
						const rows = Array.from(
							list?.querySelectorAll("[data-event-id]") ?? [],
						);
						const active = list?.getAttribute("aria-activedescendant");
						const timelineKey = master?.dataset.timelineKey;
						const expected =
							expectedBoundary === "first" ? rows[0]?.id : rows.at(-1)?.id;
						const query = new URLSearchParams(
							location.hash.split("?", 2)[1] ?? "",
						);
						const routeChanged = location.hash !== previousHash;
						const freshPage = !routeChanged || timelineKey !== previousKey;
						const summary =
							document.querySelector(".timeline-summary")?.textContent ?? "";
						const loaded = /loaded\s+\d+-(\d+)\s+of\s+(\d+)\s+matches/.exec(
							summary,
						);
						const terminalPage =
							document.querySelector('[data-timeline-page="next"]') === null &&
							loaded !== null &&
							loaded[1] === loaded[2];
						const reachedPage =
							expectedBoundary === "first"
								? !query.has("after") && !query.has("at")
								: terminalPage;
						return (
							reachedPage &&
							freshPage &&
							rows.length > 0 &&
							active === expected &&
							document.activeElement === list &&
							timelineKey !== undefined &&
							!master.textContent?.includes("Loading Change generation")
						);
					},
					{
						expectedBoundary: boundary,
						previousHash: priorHash,
						previousKey: priorKey,
					},
				);
			};
			const firstBoundaryHash = await hash();
			const firstBoundaryKey = await page
				.locator("#master")
				.getAttribute("data-timeline-key");
			await page.keyboard.press("g");
			await waitForGlobalTimelineBoundary(
				"first",
				firstBoundaryHash,
				firstBoundaryKey,
			);
			const firstActive = await activeEvent();
			requireCondition(
				Boolean(firstActive),
				"Timeline g",
				"g did not select the first readable event",
				"nonempty active event ID",
				firstActive,
			);
			await page.keyboard.press("j");
			const activeAfterJ = await activeEvent();
			expect(
				activeAfterJ !== firstActive,
				"Timeline j",
				"j did not advance the local event cursor",
			);
			await page.keyboard.press("k");
			const activeAfterK = await activeEvent();
			compare(
				activeAfterK === firstActive,
				"Timeline k",
				"k did not restore the preceding local event cursor",
				firstActive,
				activeAfterK,
			);
			const beforeLastBoundary = await hash();
			const beforeLastBoundaryKey = await page
				.locator("#master")
				.getAttribute("data-timeline-key");
			await page.keyboard.press("G");
			await waitForGlobalTimelineBoundary(
				"last",
				beforeLastBoundary,
				beforeLastBoundaryKey,
			);
			const lastActive = await activeEvent();
			expect(
				Boolean(lastActive) && lastActive !== firstActive,
				"Timeline G",
				"G did not select the last filtered event",
			);
			const beforeFirstBoundary = await hash();
			const beforeFirstBoundaryKey = await page
				.locator("#master")
				.getAttribute("data-timeline-key");
			await page.keyboard.press("g");
			await waitForGlobalTimelineBoundary(
				"first",
				beforeFirstBoundary,
				beforeFirstBoundaryKey,
			);
			const activeAfterGReturn = await activeEvent();
			compare(
				activeAfterGReturn === firstActive,
				"Timeline g return",
				"g did not return to the first filtered event",
				firstActive,
				activeAfterGReturn,
			);
			await page.keyboard.press("f");
			const fullForward = await activeEvent();
			expect(
				Boolean(fullForward) && fullForward !== firstActive,
				"Timeline f",
				"f did not advance by a bounded page",
			);
			await page.keyboard.press("b");
			const activeAfterB = await activeEvent();
			compare(
				activeAfterB === firstActive,
				"Timeline b",
				"b did not return across the bounded page movement",
				firstActive,
				activeAfterB,
			);
			await page.keyboard.press("d");
			const halfForward = await activeEvent();
			expect(
				Boolean(halfForward) && halfForward !== firstActive,
				"Timeline d",
				"d did not advance by a half page",
			);
			await page.keyboard.press("u");
			const activeAfterU = await activeEvent();
			compare(
				activeAfterU === firstActive,
				"Timeline u",
				"u did not return across the half-page movement",
				firstActive,
				activeAfterU,
			);
			await page.keyboard.press("/");
			const activeElementAfterSearch = await page.evaluate(
				() => document.activeElement?.id,
			);
			compare(
				activeElementAfterSearch === "filter-text",
				"Timeline search shortcut",
				"/ did not focus the shared filter field",
				"filter-text",
				activeElementAfterSearch,
			);
			const timelineHashBeforeTextGuard = await hash();
			await page.keyboard.press("j");
			await page.keyboard.press("?");
			const timelineHashAfterTextGuard = await hash();
			compare(
				timelineHashAfterTextGuard === timelineHashBeforeTextGuard,
				"Timeline text guard",
				"Timeline shortcut fired from the text filter",
				timelineHashBeforeTextGuard,
				timelineHashAfterTextGuard,
			);
			const visibleHelpCount = await page
				.locator("#key-help:not(.hidden)")
				.count();
			compare(
				visibleHelpCount === 0,
				"Timeline text guard",
				"help opened from the text filter",
				0,
				visibleHelpCount,
			);
			await page.keyboard.press("Escape");
			await listbox.focus();
			await page.keyboard.press("j");
			expect(
				await listbox.getAttribute("aria-activedescendant"),
				"Timeline roving focus",
				"j did not expose a selected active descendant",
			);

			// Open one exact event from the Timeline. Wide and narrow must retain the
			// same event route/detail, and browser history must restore the monitor.
			const timelineHashBeforeEvent = await hash();
			const selectedEventId = await page
				.locator("#timeline [data-event-id]")
				.first()
				.getAttribute("data-event-id");
			requireCondition(
				Boolean(selectedEventId),
				"Timeline exact event",
				"Timeline row lacked an event ID",
				"nonempty event ID",
				selectedEventId,
			);
			const expectedExactEventRoute = exactEventRouteFromTimelineRoute(
				timelineHashBeforeEvent,
				selectedEventId,
			);
			await page.locator("#timeline [data-event-id]").first().click();
			const selectedEventIdentity =
				await waitForExactTimelineEvent(selectedEventId);
			compareTimelineEventIdentity(
				selectedEventId,
				"Timeline exact event",
				selectedEventIdentity,
			);
			const settledExactEventRoute = await hash();
			const exactEventRouteMatches = await currentRouteMatches(
				expectedExactEventRoute,
			);
			compare(
				exactEventRouteMatches,
				"Timeline exact event route",
				"opening an exact event did not retain its bounded Timeline context",
				{ semanticRoute: expectedExactEventRoute },
				{
					semanticRouteMatches: exactEventRouteMatches,
					route: settledExactEventRoute,
				},
			);
			const exactEventList = page.locator("#timeline");
			const exactDetailClose = page.locator("#detail-close");
			await exactDetailClose.focus();
			const exactEventBeforeDetailArrow = await exactEventList.getAttribute(
				"aria-activedescendant",
			);
			await page.keyboard.press("ArrowDown");
			const exactDetailArrowState = {
				activeEvent: await exactEventList.getAttribute("aria-activedescendant"),
				activeElement: await page.evaluate(() => document.activeElement?.id),
				route: await hash(),
			};
			compare(
				exactDetailArrowState.activeEvent === exactEventBeforeDetailArrow &&
					exactDetailArrowState.activeElement === "detail-close" &&
					exactDetailArrowState.route === settledExactEventRoute,
				"Timeline exact event native arrow",
				"ArrowDown on exact detail chrome operated the background Timeline",
				{
					activeEvent: exactEventBeforeDetailArrow,
					activeElement: "detail-close",
					route: settledExactEventRoute,
				},
				exactDetailArrowState,
			);
			// The rendered Timeline is virtualized, so its rows are only the window
			// around the cursor. The follow cursor and g/G move across the whole
			// staged page, so read that page back from the same authenticated
			// endpoint the reader loaded rather than from what is on screen.
			const stagedPageQuery = timelineHashBeforeEvent.split("?", 2)[1] ?? "";
			const stagedPageResponse = await page.request.get(
				`${config.server.baseUrl}/api/v2/history?${stagedPageQuery}`,
				{ headers: { Authorization: `Bearer ${config.server.token}` } },
			);
			requireCondition(
				stagedPageResponse.ok(),
				"Timeline exact event staged page",
				"the loaded Timeline page could not be read back",
				200,
				stagedPageResponse.status(),
			);
			const fullPageIds = (await stagedPageResponse.json()).entries.map(
				(entry) => entry.eventId,
			);
			const startIndex = fullPageIds.indexOf(selectedEventId);
			requireCondition(
				startIndex >= 0 && fullPageIds.length - startIndex >= 3,
				"Timeline exact event staged page",
				"the fixture needs at least two staged events after the opened one",
				{ startIndex: ">= 0", stagedAfterStart: ">= 2" },
				{
					startIndex,
					stagedAfterStart: fullPageIds.length - 1 - Math.max(startIndex, 0),
				},
			);
			const renderedTimelineIds = () =>
				page
					.locator("#timeline [data-event-id]")
					.evaluateAll((rows) => rows.map((row) => row.dataset.eventId));
			const followTo = async (eventId, label, detail) => {
				try {
					await waitForExactTimelineEvent(eventId);
				} catch (error) {
					compare(
						false,
						label,
						detail,
						eventId,
						await page
							.locator("#detail-body [data-event-id]")
							.getAttribute("data-event-id")
							.catch(() => null),
					);
					throw error;
				}
			};

			// The reader keeps their place: j moves the open detail without taking
			// focus off the chrome the reader is holding.
			await exactDetailClose.focus();
			await page.keyboard.press("j");
			await followTo(
				fullPageIds[startIndex + 1],
				"Timeline exact event detail follow",
				"j did not move the open exact-event detail, or it stole focus from the detail",
			);
			const followedActiveElement = await page.evaluate(
				() => document.activeElement?.id,
			);
			compare(
				followedActiveElement === "detail-close",
				"Timeline exact event detail follow",
				"j did not move the open exact-event detail, or it stole focus from the detail",
				"detail-close",
				followedActiveElement,
			);

			await exactEventList.focus();
			await page.keyboard.press("j");
			const drivenEventId = fullPageIds[startIndex + 2];
			await followTo(
				drivenEventId,
				"Timeline exact event route drive",
				"j did not drive the exact-event detail route",
			);
			const drivenExactEventRoute = await hash();
			compare(
				drivenExactEventRoute !== settledExactEventRoute,
				"Timeline exact event route drive",
				"j did not drive the exact-event detail route",
				{ differsFrom: settledExactEventRoute },
				drivenExactEventRoute,
			);

			// The poll re-requests the page as loaded, so the window under a reader
			// cannot silently re-center on whatever the cursor reached.
			const renderedWindowIds = await renderedTimelineIds();
			await page.waitForTimeout(3500);
			const followedPollState = {
				cursorEventId: await page
					.locator('#timeline [aria-selected="true"]')
					.getAttribute("data-event-id"),
				detailEventId: await page
					.locator("#detail-body [data-event-id]")
					.getAttribute("data-event-id"),
				route: await hash(),
				renderedWindowIds: await renderedTimelineIds(),
			};
			compare(
				followedPollState.cursorEventId === drivenEventId &&
					followedPollState.detailEventId === drivenEventId &&
					followedPollState.route === drivenExactEventRoute &&
					JSON.stringify(followedPollState.renderedWindowIds) ===
						JSON.stringify(renderedWindowIds),
				"Timeline exact event poll anchor",
				"a poll re-centered the loaded Timeline window under a followed exact-event detail",
				{
					cursorEventId: drivenEventId,
					detailEventId: drivenEventId,
					route: drivenExactEventRoute,
					renderedWindowIds,
				},
				followedPollState,
			);

			await page.keyboard.press("k");
			await followTo(
				fullPageIds[startIndex + 1],
				"Timeline exact event keyboard",
				"k did not return the followed exact-event detail",
			);
			await page.keyboard.press("k");
			await followTo(
				selectedEventId,
				"Timeline exact event keyboard",
				"k did not return the followed exact-event detail",
			);

			const boundaryFollowDetail =
				"g/G did not move the followed exact-event detail across the loaded page";
			await page.keyboard.press("G");
			await followTo(
				fullPageIds[fullPageIds.length - 1],
				"Timeline exact event page boundaries",
				boundaryFollowDetail,
			);
			const lastStagedEventRoute = await hash();

			// A step past the loaded page's edge is refused out loud rather than
			// silently swallowed or smuggled into a page crossing.
			await page.keyboard.press("j");
			await page.waitForSelector("#command-feedback:not(.hidden)");
			const pageEdgeRefusalState = {
				feedback: (
					await page.locator("#command-feedback").textContent()
				)?.trim(),
				route: await hash(),
			};
			compare(
				Boolean(pageEdgeRefusalState.feedback) &&
					pageEdgeRefusalState.route === lastStagedEventRoute,
				"Timeline exact event page edge",
				"j past the last staged event did not refuse audibly on an unchanged route",
				{ feedback: "nonempty", route: lastStagedEventRoute },
				pageEdgeRefusalState,
			);

			await page.keyboard.press("g");
			await followTo(
				fullPageIds[0],
				"Timeline exact event page boundaries",
				boundaryFollowDetail,
			);

			await page.locator("#detail-read").click();
			expect(
				await page
					.locator(".split")
					.evaluate((node) => node.classList.contains("reading")),
				"Timeline event reading mode",
				"exact event did not enter reading mode",
			);
			// Reading mode hides the master entirely. The keys that move the detail
			// have to keep working with nothing of the list on screen.
			await page.keyboard.press("j");
			await followTo(
				fullPageIds[1],
				"Timeline event reading mode follow",
				"j did not move the detail while the master pane was hidden",
			);
			await page.keyboard.press("k");
			await followTo(
				fullPageIds[0],
				"Timeline event reading mode follow",
				"k did not move the detail while the master pane was hidden",
			);
			await screenshot("wide-timeline-event-detail");
			await page.locator("#master-rail").click();
			expect(
				!(await page
					.locator(".split")
					.evaluate((node) => node.classList.contains("reading"))),
				"Timeline event reading return",
				"master rail did not leave event reading mode",
			);

			// Back must still reach the Timeline in one step: every follow above
			// replaced the current entry rather than pushing a new one. Re-establish
			// the opened event first so the history assertions name a known route.
			for (let index = 1; index <= startIndex; index += 1) {
				await page.keyboard.press("j");
				await followTo(
					fullPageIds[index],
					"Timeline exact event history re-establish",
					"the followed cursor did not return to the opened exact event",
				);
			}
			await followTo(
				selectedEventId,
				"Timeline exact event history re-establish",
				"the followed cursor did not return to the opened exact event",
			);
			await page.goBack();
			await waitForTimelineRoute(timelineHashBeforeEvent);
			await page.goForward();
			const forwardedEventIdentity =
				await waitForExactTimelineEvent(selectedEventId);
			compareTimelineEventIdentity(
				selectedEventId,
				"Timeline event Forward identity",
				forwardedEventIdentity,
			);
			const forwardedEventRoute = await hash();
			const forwardedEventRouteMatches =
				await currentRouteMatches(expectedExactEventRoute);
			compare(
				forwardedEventRouteMatches,
				"Timeline event Forward route",
				"Forward did not restore the complete semantic exact-event route",
				{ semanticRoute: expectedExactEventRoute },
				{
					semanticRouteMatches: forwardedEventRouteMatches,
					route: forwardedEventRoute,
				},
			);
			await page.reload({ waitUntil: "domcontentloaded" });
			const reloadedEventIdentity =
				await waitForExactTimelineEvent(selectedEventId);
			compareTimelineEventIdentity(
				selectedEventId,
				"Timeline event reload",
				reloadedEventIdentity,
			);
			const reloadedEventRoute = await hash();
			const reloadedEventRouteMatches =
				await currentRouteMatches(expectedExactEventRoute);
			compare(
				reloadedEventRouteMatches,
				"Timeline event reload route",
				"reload did not retain the complete semantic exact-event route",
				{ semanticRoute: expectedExactEventRoute },
				{
					semanticRouteMatches: reloadedEventRouteMatches,
					route: reloadedEventRoute,
				},
			);
			await page.goBack();
			await waitForTimelineRoute(timelineHashBeforeEvent);

			await open(
				"timeline?limit=100&order=desc",
				layouts[1],
				"narrow Timeline",
			);
			const narrowTimelineHash = await hash();
			const narrowEventRows = page.locator("#timeline [data-event-id]");
			const narrowEventRowCount = await narrowEventRows.count();
			requireCondition(
				narrowEventRowCount > 0,
				"narrow Timeline event",
				"narrow Timeline had no exact event row",
				"> 0",
				narrowEventRowCount,
			);
			const narrowEventId = await narrowEventRows
				.first()
				.getAttribute("data-event-id");
			requireCondition(
				Boolean(narrowEventId),
				"narrow Timeline event",
				"Timeline row lacked an event ID",
				"nonempty event ID",
				narrowEventId,
			);
			const expectedNarrowEventRoute = exactEventRouteFromTimelineRoute(
				narrowTimelineHash,
				narrowEventId,
			);
			await narrowEventRows.first().click();
			const narrowEventIdentity = await waitForExactTimelineEvent(narrowEventId);
			compareTimelineEventIdentity(
				narrowEventId,
				"narrow Timeline event identity",
				narrowEventIdentity,
			);
			const narrowEventRoute = await hash();
			const narrowEventRouteMatches = await currentRouteMatches(
				expectedNarrowEventRoute,
			);
			compare(
				narrowEventRouteMatches,
				"narrow Timeline event route",
				"narrow exact event did not retain its bounded Timeline context",
				{ semanticRoute: expectedNarrowEventRoute },
				{
					semanticRouteMatches: narrowEventRouteMatches,
					route: narrowEventRoute,
				},
			);
			expect(
				await page
					.locator("#detail")
					.evaluate((node) => !node.inert && !node.hasAttribute("aria-hidden")),
				"narrow Timeline event",
				"narrow event detail remained inert",
			);
			expect(
				await page.evaluate(() =>
					["#topbar", "#toolbar", "#master-rail", "#master", ".divider"].every(
						(selector) => document.querySelector(selector)?.inert === true,
					),
				),
				"narrow Timeline event",
				"the fixed detail sheet left covered page controls keyboard-reachable",
			);
			await screenshot("narrow-timeline-event-detail");
			await page.setViewportSize({
				width: layouts[0].width,
				height: layouts[0].height,
			});
			await page.waitForFunction(
				() => {
					const exactAction = document.querySelector(
						"#detail-body [data-exact-diff-activation], #detail-body [data-event-diff-refusal]",
					);
					return (
						getComputedStyle(document.querySelector("#detail-back")).display ===
							"none" &&
						exactAction !== null &&
						document.activeElement === exactAction &&
						["#topbar", "#toolbar", "#master-rail", "#master", ".divider"].every(
							(selector) => document.querySelector(selector)?.inert === false,
						)
					);
				},
			);
			expect(
				await page.evaluate(() =>
					["#topbar", "#toolbar", "#master-rail", "#master", ".divider"].every(
						(selector) => document.querySelector(selector)?.inert === false,
					),
				),
				"Timeline event widen",
				"widening the detail sheet left the ordinary split-pane surface inert",
			);
			await page.setViewportSize({
				width: layouts[1].width,
				height: layouts[1].height,
			});
			await page.waitForFunction(
				() =>
					getComputedStyle(document.querySelector("#detail-back")).display !==
						"none" && document.querySelector("#master")?.inert === true,
			);
			await page.locator("#detail-back").click();
			await waitForTimelineRoute(narrowTimelineHash);
			expect(
				await page.evaluate(() =>
					["#topbar", "#toolbar", "#master-rail", "#master", ".divider"].every(
						(selector) => document.querySelector(selector)?.inert === false,
					),
				),
				"narrow Timeline event return",
				"closing the fixed detail sheet did not restore covered page controls",
			);
		},
		teardown: teardownSection,
	});

	await diagnostics.section("Timeline follow and stale continuation", {
		setup: async () => {
			await open(
				"timeline?limit=100&order=desc",
				layouts[0],
				"Timeline stale continuation setup",
			);
			const timelineKey = await page
				.locator("#master")
				.getAttribute("data-timeline-key");
			const timelineHash = await hash();
			const staleNextPage = page.getByRole("button", { name: "Next page" });
			const staleNextPageCount = await staleNextPage.count();
			requireCondition(
				staleNextPageCount === 1,
				"stale Timeline continuation",
				"Timeline did not expose one continuation before the append",
				1,
				staleNextPageCount,
			);
			await staleNextPage.click();
			await page.waitForFunction(
				({ key, priorHash }) => {
					const master = document.querySelector("#master");
					const query = new URLSearchParams(
						location.hash.split("?", 2)[1] ?? "",
					);
					return (
						location.hash !== priorHash &&
						query.has("after") &&
						master?.dataset.timelineKey !== undefined &&
						master.dataset.timelineKey !== key &&
						Boolean(document.querySelector("#timeline"))
					);
				},
				{ key: timelineKey, priorHash: timelineHash },
			);
			const staleTimelineRoute = (await hash()).replace(/^#\//, "");
			await open(
				"timeline?limit=100&order=desc",
				layouts[0],
				"Timeline follow setup",
			);
			return staleTimelineRoute;
		},
		run: async (staleTimelineRoute) => {
			// A parked Timeline must not repaint when the shell's disposable worker
			// appends an event.  The worker waits for this screenshot, then writes its
			// receipt; explicit catch-up is the only action that adopts the new head.
			await open(
				"timeline?limit=100&order=desc",
				layouts[0],
				"Timeline follow park",
			);
			const follow = page.locator("#follow-toggle");
			await follow.click();
			const followParkedText = await follow.innerText();
			compare(
				followParkedText.includes("Parked"),
				"Timeline park",
				"follow control did not expose parked state",
				true,
				followParkedText.includes("Parked"),
			);
			const parkedTimelineKey = await page
				.locator("#master")
				.getAttribute("data-timeline-key");
			const parkedSummary = await page.locator(".timeline-summary").innerText();
			await page.locator("#timeline").evaluate((node) => {
				node.dataset.browserRetention = "parked-window";
			});
			await page.evaluate(() => {
				const status = document.querySelector("#refresh-status");
				const states = [status?.textContent?.trim() || ""];
				window.__pointbreakRefreshStates = states;
				if (status) {
					new MutationObserver(() =>
						states.push(status.textContent?.trim() || ""),
					).observe(status, {
						childList: true,
						characterData: true,
						subtree: true,
					});
				}
			});
			insideAppendWindow = true;
			await screenshot("timeline-parked-before-append");
			// Wait for the normal poll/catch-up affordance. The shell worker's receipt
			// remains a completion-last evidence record and is checked after this
			// program returns; it is intentionally not exposed to the browser.
			await page.waitForFunction(() =>
				(document.querySelector("#follow-toggle")?.textContent || "").includes(
					"Show ",
				),
			);
			const parkedTimelineState = {
				key: await page.locator("#master").getAttribute("data-timeline-key"),
				retainedWindows: await page
					.locator('.timeline[data-browser-retention="parked-window"]')
					.count(),
				summary: await page.locator(".timeline-summary").innerText(),
			};
			compare(
				parkedTimelineState.key === parkedTimelineKey &&
					parkedTimelineState.retainedWindows === 1 &&
					parkedTimelineState.summary === parkedSummary,
				"Timeline parked stability",
				"parked Timeline replaced its retained logical window before explicit catch-up",
				{ key: parkedTimelineKey, retainedWindows: 1, summary: parkedSummary },
				parkedTimelineState,
			);
			await follow.click();
			await page.waitForFunction(
				() =>
					document.querySelector("#follow-toggle")?.textContent === "Following",
			);
			await page.waitForFunction(
				(key) => document.querySelector("#master")?.dataset.timelineKey !== key,
				parkedTimelineKey,
			);
			await page.waitForFunction(() =>
				window.__pointbreakRefreshStates?.includes("updated"),
			);
			await page.waitForFunction(
				() =>
					document.querySelector("#refresh-status")?.textContent === "watching",
			);
			const acceptedRefreshStates = await page.evaluate(
				() => window.__pointbreakRefreshStates ?? [],
			);
			const refreshLifecycle = {
				first: acceptedRefreshStates[0],
				sawUpdated: acceptedRefreshStates.includes("updated"),
				last: acceptedRefreshStates.at(-1),
			};
			compare(
				refreshLifecycle.first === "watching" &&
					refreshLifecycle.sawUpdated &&
					refreshLifecycle.last === "watching",
				"accepted refresh lifecycle",
				`refresh did not move watching -> updated -> watching after the accepted append: ${JSON.stringify(acceptedRefreshStates)}`,
				{ first: "watching", sawUpdated: true, last: "watching" },
				refreshLifecycle,
			);
			await screenshot("timeline-followed-after-append");
			insideAppendWindow = false;

			// The continuation captured above names the pre-append projection. Exercise
			// its real authenticated refusal, then prove the reader can recover by
			// returning to the unpositioned filtered head instead of reusing the token.
			const staleQuery = staleTimelineRoute.split("?", 2)[1] ?? "";
			const staleResponse = await page.request.get(
				`${config.server.baseUrl}/api/v2/history?${staleQuery}`,
				{ headers: { Authorization: `Bearer ${config.server.token}` } },
			);
			const staleStatus = staleResponse.status();
			compare(
				staleStatus === 409,
				"stale Timeline continuation",
				"old continuation did not return conflict",
				409,
				staleStatus,
			);
			const staleBody = await staleResponse.json();
			compare(
				staleBody.code === "stale_projection",
				"stale Timeline continuation",
				"old continuation returned an unexpected code",
				"stale_projection",
				staleBody.code,
			);
			await open(
				"timeline?limit=100&order=desc",
				layouts[0],
				"stale Timeline explicit head recovery",
			);
			const recoveredHeadHash = await hash();
			compare(
				!recoveredHeadHash.includes("after="),
				"stale Timeline explicit head recovery",
				"head recovery retained the stale continuation",
				false,
				recoveredHeadHash.includes("after="),
			);
		},
		teardown: async () => {
			insideAppendWindow = false;
			await teardownSection();
		},
	});

	await diagnostics.section("Changes and Attention paging", {
		setup: () =>
			open(
				"changes?limit=100&order=change_id_asc",
				layouts[0],
				"Changes and Attention paging section setup",
			),
		run: async () => {
			for (const layout of layouts) {
				const metrics = await open(
					"changes?limit=100&order=change_id_asc",
					layout,
					`${layout.name} changes`,
				);
				await expectLensHierarchy("Changes");
				compare(
					metrics.liveCards > 0 && metrics.liveCards <= 100,
					`${layout.name} changes`,
					"expected bounded live card count",
					"1..100",
					metrics.liveCards,
				);
				const nextPageControlCount = await page
					.getByRole("button", { name: /Next page/ })
					.count();
				requireCondition(
					nextPageControlCount > 0,
					`${layout.name} changes`,
					"363+ fixture did not offer pagination",
					"> 0",
					nextPageControlCount,
				);
				expect(
					await cardNamesAreUseful(),
					`${layout.name} card names`,
					"card accessible names must lead with human Revision presentation and retain exact identity",
				);
				expect(
					await noHiddenTabStops(),
					`${layout.name} hidden controls`,
					"a hidden control remains tabbable",
				);
				if (layout.name === "narrow") {
					const closedDetail = await page
						.locator("#detail")
						.evaluate((node) => ({
							inert: node.inert,
							hidden: node.getAttribute("aria-hidden"),
						}));
					compare(
						closedDetail.inert && closedDetail.hidden === "true",
						"narrow closed detail",
						"off-canvas detail was not removed from navigation and the accessibility tree",
						{ inert: true, hidden: "true" },
						closedDetail,
					);
				}
				const firstPageIds = await page
					.locator(".unit-card[data-change-id]")
					.evaluateAll((cards) => cards.map((card) => card.dataset.changeId));
				const sortedFirstPageIds = [...firstPageIds].sort();
				compare(
					JSON.stringify(firstPageIds) === JSON.stringify(sortedFirstPageIds),
					`${layout.name} stable order`,
					"first page is not change_id_asc",
					sortedFirstPageIds,
					firstPageIds,
				);
				await screenshot(`${layout.name}-changes`);
				const firstListKey = await page
					.locator("#master")
					.getAttribute("data-change-list-key");
				await page.getByRole("button", { name: /Next page/ }).click();
				await page.waitForFunction(() => location.hash.includes("after="));
				await page.waitForFunction(
					(key) =>
						document.querySelector("#master")?.dataset.changeListKey !== key,
					firstListKey,
				);
				await page.waitForFunction(
					() =>
						document.querySelectorAll(".unit-card[data-change-id]").length > 0,
				);
				const nextPageIds = await page
					.locator(".unit-card[data-change-id]")
					.evaluateAll((cards) => cards.map((card) => card.dataset.changeId));
				compare(
					nextPageIds.length > 0 && nextPageIds.length <= 100,
					`${layout.name} next page`,
					"next page has an unexpected card count",
					"1..100",
					nextPageIds.length,
				);
				const sortedNextPageIds = [...nextPageIds].sort();
				compare(
					JSON.stringify(nextPageIds) === JSON.stringify(sortedNextPageIds),
					`${layout.name} stable order`,
					"next page is not change_id_asc",
					sortedNextPageIds,
					nextPageIds,
				);
				const pageBoundary = {
					first: firstPageIds.at(-1),
					next: nextPageIds[0],
				};
				compare(
					pageBoundary.first < pageBoundary.next,
					`${layout.name} page boundary`,
					"next page does not follow the first page in change_id_asc order",
					"first < next",
					pageBoundary,
				);

				const attentionMetrics = await open(
					"attention?limit=50&order=change_id_asc",
					layout,
					`${layout.name} attention`,
				);
				await expectLensHierarchy("Attention");
				const attentionCountCopy = await page.evaluate(() => ({
					metadata:
						document.querySelector("#master .lens-meta")?.textContent?.trim() ||
						"",
					tabName:
						document
							.querySelector('[data-lens="attention"]')
							?.getAttribute("aria-label") || "",
					visibleCount:
						document
							.querySelector('[data-lens="attention"] .lens-count')
							?.textContent?.trim() || "",
					topbarStat:
						document.querySelector("#stat-attention")?.textContent?.trim() ||
						"",
					topbarStatTitle:
						document.querySelector("#stat-attention")?.getAttribute("title") ||
						"",
				}));
				compare(
					attentionCountCopy.metadata.includes("on this page") &&
						attentionCountCopy.tabName.includes("shown on this page") &&
						attentionCountCopy.visibleCount.endsWith(" shown") &&
						attentionCountCopy.topbarStat.endsWith(" shown on this page") &&
						attentionCountCopy.topbarStatTitle ===
							"Changes shown on this Attention page",
					`${layout.name} bounded Attention count`,
					`Attention count was presented as an unbounded total: ${JSON.stringify(attentionCountCopy)}`,
					{
						metadata: "includes on this page",
						tabName: "includes shown on this page",
						visibleCount: "ends with  shown",
						topbarStat: "ends with  shown on this page",
						topbarStatTitle: "Changes shown on this Attention page",
					},
					attentionCountCopy,
				);
				compare(
					attentionMetrics.liveCards <= 100,
					`${layout.name} attention`,
					"attention page exceeded live card bound",
					"<= 100",
					attentionMetrics.liveCards,
				);
				await screenshot(`${layout.name}-attention`);
			}
		},
		teardown: teardownSection,
	});

	await diagnostics.section("Attention guidance", {
		setup: () =>
			open(
				"attention?limit=100&order=change_id_asc",
				layouts[0],
				"Attention guidance section setup",
			),
		run: async () => {
			// Attention is a reading surface, not a redacted status badge. Every card
			// must say what is unresolved, what evidence is missing, and the next human
			// action. Diagnostics are deliberately optional in the protocol, but when a
			// server supplies them the reader must make each one readable.
			await open(
				"attention?limit=100&order=change_id_asc",
				layouts[0],
				"reason-bearing Attention",
			);
			const attentionPresentation = await page
				.locator(".unit-card[data-change-id]")
				.evaluateAll((cards) =>
					cards.map((card) => {
						const reason = card.querySelector(".change-card-attention-reason");
						const groupReason = card
							.closest(".attention-group")
							?.querySelector(".attention-group-heading");
						const ask = card.querySelector(".change-card-attention-ask");
						const evidence = card.querySelector(
							".change-card-attention-evidence",
						);
						const action = card.querySelector(".change-card-attention-action");
						const diagnostics = Array.from(
							card.querySelectorAll(".change-card-attention-diagnostics li"),
							(item) => item.textContent?.trim() || "",
						);
						return {
							changeId: card.dataset.changeId || "",
							reason: reason?.textContent?.trim() || "",
							reasonTitle: reason?.getAttribute("title") || "",
							groupReason: groupReason?.textContent?.trim() || "",
							ask: ask?.textContent?.trim() || "",
							evidence: evidence?.textContent?.trim() || "",
							action: action?.textContent?.trim() || "",
							diagnostics,
						};
					}),
				);
			compare(
				attentionPresentation.length > 0,
				"reason-bearing Attention",
				"the attention lens had no retained fixture Changes",
				"> 0",
				attentionPresentation.length,
			);
			// A per-card reason line appears only when it adds information beyond
			// the attention group heading, so the readable reason is the card's own
			// line when present and its group heading otherwise. A per-card line
			// still has to carry its explanatory title.
			expect(
				attentionPresentation.every(
					(card) =>
						card.changeId.length > 0 &&
						(card.reason.length > 0
							? card.reasonTitle.length > 0
							: card.groupReason.length > 0) &&
						card.ask.length > 0 &&
						card.evidence.length > 0 &&
						card.action.startsWith("Next: ") &&
						new Set([
							card.reason.length > 0 ? card.reason : card.groupReason,
							card.ask,
							card.evidence,
							card.action,
						]).size === 4 &&
						card.diagnostics.every((diagnostic) => diagnostic.length > 0),
				),
				"reason-bearing Attention",
				"an attention card omitted or collapsed its reason, ask, evidence, next action, or supplied diagnostic",
			);
			await screenshot("wide-attention-reasons");
		},
		teardown: teardownSection,
	});

	await diagnostics.section("Changes keyboard and filters", {
		setup: () =>
			open(
				"changes?limit=100&order=change_id_asc",
				layouts[0],
				"Changes keyboard and filters section setup",
			),
		run: async () => {
			await open(
				"changes?limit=100&order=change_id_asc",
				layouts[0],
				"keyboard changes",
			);
			await page.keyboard.press("j");
			const selectedCount = await selected().count();
			requireCondition(
				selectedCount === 1,
				"keyboard local selection",
				"j did not select exactly one local Change",
				1,
				selectedCount,
			);
			const selectedId = await selected().getAttribute("data-change-id");
			requireCondition(
				typeof selectedId === "string" && selectedId.length > 0,
				"keyboard local selection",
				"selected Change has no exact identity",
				"nonempty Change ID",
				selectedId,
			);
			const localSelectionHash = await hash();
			expect(
				!localSelectionHash.includes(selectedId),
				"keyboard local selection",
				"local selection changed the URL before Enter",
			);
			await page.keyboard.press("Enter");
			await page.waitForFunction(
				(id) => location.hash.includes(encodeURIComponent(id)),
				selectedId,
			);
			const enteredChangeHash = await hash();
			compare(
				enteredChangeHash.includes(
					`/changes/${encodeURIComponent(selectedId)}`,
				),
				"keyboard Enter",
				"Enter did not open the selected Change",
				`/changes/${encodeURIComponent(selectedId)}`,
				enteredChangeHash,
			);
			await page.waitForFunction(() =>
				Boolean(
					document.querySelector("#detail-body")?.dataset.changeReadingKey,
				),
			);
			await screenshot("wide-keyboard-change");
			await page.keyboard.press("Escape");
			await page.waitForFunction(() => location.hash.startsWith("#/changes?"));
			const returnedSelectionCount = await selected().count();
			compare(
				returnedSelectionCount === 1,
				"native control Enter",
				"returning from the selected Change lost the local cursor",
				1,
				returnedSelectionCount,
			);
			const beforeNativeEnter = await hash();
			const viewToggle = page.locator("#view-toggle");
			await viewToggle.focus();
			await page.keyboard.press("Enter");
			const expandedAfterEnter = await viewToggle.getAttribute("aria-expanded");
			compare(
				expandedAfterEnter === "true",
				"native control Enter",
				"Enter on the focused View control was intercepted",
				"true",
				expandedAfterEnter,
			);
			const hashAfterNativeEnter = await hash();
			compare(
				hashAfterNativeEnter === beforeNativeEnter,
				"native control Enter",
				"Enter on the focused View control opened the selected Change",
				beforeNativeEnter,
				hashAfterNativeEnter,
			);
			await page.keyboard.press("Enter");
			const expandedAfterSecondEnter =
				await viewToggle.getAttribute("aria-expanded");
			compare(
				expandedAfterSecondEnter === "false",
				"native control Enter",
				"second Enter did not close the focused View control",
				"false",
				expandedAfterSecondEnter,
			);
			await page.keyboard.press("G");
			const lastId = await selected().getAttribute("data-change-id");
			const lastLoadedId = await page
				.locator(".unit-card[data-change-id]")
				.last()
				.getAttribute("data-change-id");
			compare(
				lastId === lastLoadedId,
				"G boundary",
				"G did not select last loaded Change",
				lastLoadedId,
				lastId,
			);
			await page.keyboard.press("g");
			const firstSelectedId = await selected().getAttribute("data-change-id");
			const firstLoadedId = await page
				.locator(".unit-card[data-change-id]")
				.first()
				.getAttribute("data-change-id");
			compare(
				firstSelectedId === firstLoadedId,
				"g boundary",
				"g did not select first loaded Change",
				firstLoadedId,
				firstSelectedId,
			);
			await page.keyboard.press("3");
			await page.waitForFunction(() =>
				location.hash.startsWith("#/attention?"),
			);
			await waitForLens("attention");
			await page.keyboard.press("2");
			await page.waitForFunction(() => location.hash.startsWith("#/changes?"));
			await waitForLens("changes");
			await page.keyboard.press("1");
			await page.waitForFunction(() => location.hash.startsWith("#/timeline"));
			await waitForLens("timeline");
			await page.keyboard.press("2");
			await page.waitForFunction(() => location.hash === "#/changes");

			// Re-establish an explicitly bounded Changes route so the following filter
			// assertions prove that route edits retain caller-selected page shape.
			await open(
				"changes?limit=100&order=change_id_asc",
				layouts[0],
				"filter state setup",
			);
			const search = page.locator("#filter-text");
			await search.focus();
			const beforeSearch = await hash();
			await page.keyboard.press("2");
			await page.keyboard.press("?");
			await page.keyboard.press("j");
			const hashDuringSearch = await hash();
			compare(
				hashDuringSearch === beforeSearch,
				"shortcuts in search",
				"a reader shortcut fired while search had focus",
				beforeSearch,
				hashDuringSearch,
			);
			const openModalCount = await page.locator(".modal:not(.hidden)").count();
			compare(
				openModalCount === 0,
				"shortcuts in search",
				"help opened while search had focus",
				0,
				openModalCount,
			);
			await search.fill("uncommitted poll draft");
			await page.waitForTimeout(3500);
			const polledSearchValue = await search.inputValue();
			compare(
				polledSearchValue === "uncommitted poll draft",
				"poll search draft",
				"a background poll erased the focused uncommitted search draft",
				"uncommitted poll draft",
				polledSearchValue,
			);
			await search.fill("Browser scale Change 1");
			await search.press("Tab");
			await page.waitForFunction(() => {
				const query = location.hash.split("?", 2)[1] ?? "";
				return new URLSearchParams(query).get("q") === "Browser scale Change 1";
			});
			await page.locator("#filters-toggle").click();
			await page.locator("#change-filter-topology").selectOption("initial");
			const filteredPaging = await routeParameters(["limit", "order"]);
			const expectedPaging = { limit: "100", order: "change_id_asc" };
			compare(
				JSON.stringify(filteredPaging) === JSON.stringify(expectedPaging),
				"filter URL state",
				"filtering lost explicit paging or ordering state",
				expectedPaging,
				filteredPaging,
			);
			const filtersExpandedAfterRoute = await page
				.locator("#filters-toggle")
				.getAttribute("aria-expanded");
			compare(
				filtersExpandedAfterRoute === "false",
				"filter route dismissal",
				"route-changing facet left the Filters panel over the new result",
				"false",
				filtersExpandedAfterRoute,
			);
			await page.locator("#filters-toggle").click();
			await page.locator("#filter-clear").click();
			await page.waitForFunction(
				() =>
					!location.hash.includes("q=") && !location.hash.includes("after="),
			);
			await page.waitForFunction(() => {
				const rawKey = document.querySelector("#master")?.dataset.changeListKey;
				if (!rawKey) return false;
				try {
					const key = JSON.parse(rawKey);
					return key.lens === "changes" && !key.query?.q && !key.query?.after;
				} catch {
					return false;
				}
			});
			const clearedPaging = await routeParameters(["limit", "order"]);
			compare(
				JSON.stringify(clearedPaging) === JSON.stringify(expectedPaging),
				"clear reset",
				"clear reset did not preserve limit and order",
				expectedPaging,
				clearedPaging,
			);
			await screenshot("wide-filter-clear");
		},
		teardown: teardownSection,
	});

	await diagnostics.section("Change topology cards", {
		setup: () =>
			open(
				"changes?limit=100&order=change_id_asc",
				layouts[0],
				"Change topology cards section setup",
			),
		run: async () => {
			const topologyFixture = config.fixture.matrix.topology;
			const representativeCases = [
				[
					"initial",
					"initial topology",
					"topology=initial",
					topologyFixture.initial.change,
				],
				[
					"replacement",
					"replacement topology",
					"topology=replacement",
					topologyFixture.replacement.change,
				],
				[
					"parallel",
					"parallel topology",
					"topology=parallel_current",
					topologyFixture.parallel_current.change,
				],
				[
					"replacement-divergent",
					"replacement-divergent topology",
					"topology=replacement_divergent",
					topologyFixture.replacement_divergent.change,
				],
				[
					"consolidation",
					"consolidation topology",
					"topology=consolidation",
					topologyFixture.consolidation.change,
				],
				[
					"removed-resource-change",
					"removed resource Change availability",
					"availability=available",
					config.fixture.removed.changeId,
				],
			];
			for (const layout of layouts) {
				for (const [
					slug,
					name,
					filter,
					expectedChange,
				] of representativeCases) {
					const route = `changes?limit=100&order=change_id_asc&${filter}&q=${encodeURIComponent(expectedChange)}`;
					const topologyMetrics = await open(
						route,
						layout,
						`${layout.name} ${name}`,
					);
					compare(
						topologyMetrics.liveCards === 1,
						`${layout.name} ${name}`,
						`expected one exact representative card, saw ${topologyMetrics.liveCards}`,
						1,
						topologyMetrics.liveCards,
					);
					const representativeCard = page.locator(
						`.unit-card[data-change-id="${expectedChange}"]`,
					);
					const representativeCardCount = await representativeCard.count();
					requireCondition(
						representativeCardCount === 1,
						`${layout.name} ${name}`,
						`missing exact fixture Change ${expectedChange}`,
						1,
						representativeCardCount,
					);
					if (slug === "initial") {
						const controls = representativeCard.locator(
							"a[href]:not([tabindex='-1']), button:not([disabled]):not([tabindex='-1']), input:not([disabled]):not([tabindex='-1']), select:not([disabled]):not([tabindex='-1']), textarea:not([disabled]):not([tabindex='-1']), summary:not([tabindex='-1']), [tabindex]:not([tabindex='-1'])",
						);
						const controlCount = await controls.count();
						requireCondition(
							controlCount === 2,
							`${layout.name} ordinary card tab order`,
							"an ordinary Change card did not expose its primary action followed by its exact Revision link",
							2,
							controlCount,
						);
						const primaryControl = controls.nth(0);
						const exactRevisionControl = controls.nth(1);
						expect(
							await primaryControl
								.getAttribute("class")
								.then((value) => value?.includes("change-card-primary")),
							`${layout.name} ordinary card tab order`,
							"the ordinary Change card's first tab stop was not its primary action",
						);
						expect(
							await primaryControl
								.getAttribute("aria-label")
								.then((value) => value?.startsWith("Review Change. ")),
							`${layout.name} ordinary card action name`,
							"the ordinary Change card's primary action did not lead with its human review action",
						);
						const exactRevisionIdentity = await exactRevisionControl.evaluate(
							(control) => ({
								nativeAnchor: control instanceof HTMLAnchorElement,
								changeId: control.dataset.changeId ?? null,
								revisionId: control.dataset.revisionId ?? null,
								artifactHash: control.dataset.artifactHash ?? null,
								href: control.getAttribute("href"),
								accessibleName: control.getAttribute("aria-label"),
							}),
						);
						const expectedExactRevisionPath = `#/changes/${encodeURIComponent(topologyFixture.initial.change)}/revisions/${encodeURIComponent(topologyFixture.initial.current.revision)}`;
						const expectedArtifactParameter = `artifactHash=${encodeURIComponent(topologyFixture.initial.current.artifact)}`;
						const expectedAccessibleName = `Open exact Revision ${topologyFixture.initial.current.revision}; artifact ${topologyFixture.initial.current.artifact}; for Change ${topologyFixture.initial.change}`;
						const expectedExactRevisionIdentity = {
							nativeAnchor: true,
							changeId: topologyFixture.initial.change,
							revisionId: topologyFixture.initial.current.revision,
							artifactHash: topologyFixture.initial.current.artifact,
							href: `${expectedExactRevisionPath}?...&${expectedArtifactParameter}`,
							accessibleName: expectedAccessibleName,
						};
						compare(
							exactRevisionIdentity.nativeAnchor &&
								exactRevisionIdentity.changeId ===
									expectedExactRevisionIdentity.changeId &&
								exactRevisionIdentity.revisionId ===
									expectedExactRevisionIdentity.revisionId &&
								exactRevisionIdentity.artifactHash ===
									expectedExactRevisionIdentity.artifactHash &&
								exactRevisionIdentity.href?.startsWith(
									`${expectedExactRevisionPath}?`,
								) &&
								exactRevisionIdentity.href.includes(expectedArtifactParameter) &&
								exactRevisionIdentity.accessibleName === expectedAccessibleName,
							`${layout.name} ordinary card exact Revision link`,
							"the ordinary Change card's secondary action did not retain its exact Revision identity",
							expectedExactRevisionIdentity,
							exactRevisionIdentity,
						);
					}
					const sparseGeometry = await page.evaluate(() => {
						const units = document.querySelector("#master > .units");
						const card = units?.querySelector(".unit-card[data-change-id]");
						return {
							listHeight: units?.getBoundingClientRect().height ?? 0,
							cardHeight: card?.getBoundingClientRect().height ?? 0,
						};
					});
					compare(
						sparseGeometry.listHeight > 0 &&
							sparseGeometry.cardHeight > 0 &&
							sparseGeometry.cardHeight < sparseGeometry.listHeight * 0.75,
						`${layout.name} ${name}`,
						`single card stretched to ${sparseGeometry.cardHeight}/${sparseGeometry.listHeight}`,
						{ listHeight: "> 0", cardHeight: "> 0 and < 75% of list height" },
						sparseGeometry,
					);
					await screenshot(`${layout.name}-${slug}`);
				}
			}
		},
		teardown: teardownSection,
	});

	// The Change graph is server-laid (mmdflux geometry) rather than a client
	// inference. It must preserve both its SVG relationship map and an exact
	// textual equivalent. Available nodes are keyboard-operable; claim-only
	// context remains readable but deliberately does not choose a peer.
	const graphChange = config.fixture.graph.changeId;
	const graphRoute = `changes/${encodeURIComponent(graphChange)}?limit=100&order=change_id_asc`;
	await diagnostics.section("Change relationship graph", {
		setup: () =>
			open(graphRoute, layouts[0], "Change Revision relationship graph"),
		run: async () => {
			await page.waitForFunction(() =>
				Boolean(document.querySelector("#detail-body .change-revision-graph")),
			);
			const changeGraphMetrics = await page
				.locator("#detail-body .change-revision-graph")
				.evaluate((graph) => ({
					svg: graph.querySelectorAll("svg.change-revision-graph-svg").length,
					nodes: graph.querySelectorAll("g.change-revision-node").length,
					edges: graph.querySelectorAll("g.change-revision-edge").length,
					effective: graph.querySelectorAll(
						'g.change-revision-edge[data-edge-kind="effective-supersedes"]',
					).length,
					claims: graph.querySelectorAll(
						'g.change-revision-edge[data-edge-kind="pending-or-conflicting-claim"]',
					).length,
					edgePresentation: Array.from(
						graph.querySelectorAll("g.change-revision-edge"),
					).map((edge) => ({
						kind: edge.getAttribute("data-edge-kind") || "",
						successorRevision: edge.getAttribute("data-revision-id") || "",
						successorArtifact: edge.getAttribute("data-artifact-hash") || "",
						predecessorRevision:
							edge.getAttribute("data-predecessor-revision-id") || "",
						predecessorArtifact:
							edge.getAttribute("data-predecessor-artifact-hash") || "",
					})),
					textual: graph.querySelectorAll(
						"details[data-graph-textual-equivalent]",
					).length,
					nodePresentation: Array.from(
						graph.querySelectorAll("g.change-revision-node"),
					).map((node) => {
						const revision = node.getAttribute("data-revision-id") || "";
						const artifact = node.getAttribute("data-artifact-hash") || "";
						const label = node.getAttribute("aria-label") || "";
						return {
							revision,
							artifact,
							label,
							availability:
								node.getAttribute("data-context-availability") || "",
							role: node.getAttribute("role") || "",
							disabled: node.getAttribute("aria-disabled") || "",
						};
					}),
					nodeLabelGeometry: Array.from(
						graph.querySelectorAll("g.change-revision-node"),
						(node) => {
							const frame = node.querySelector("rect");
							const label = node.querySelector("text");
							const frameLeft = Number(frame?.getAttribute("x") || 0);
							const frameWidth = Number(frame?.getAttribute("width") || 0);
							const labelBounds = label?.getBBox();
							return {
								nodeId: node.getAttribute("data-graph-node-id") || "",
								label: label?.textContent || "",
								frameLeft,
								frameWidth,
								frameRight: frameLeft + frameWidth,
								labelLeft: labelBounds?.x ?? Number.NEGATIVE_INFINITY,
								labelWidth: labelBounds?.width ?? 0,
								labelRight:
									(labelBounds?.x ?? Number.POSITIVE_INFINITY) +
									(labelBounds?.width ?? 0),
							};
						},
					),
					textualNodes: Array.from(
						graph.querySelectorAll("[data-graph-text-nodes] > li"),
					).map((item) => ({
						text: item.textContent || "",
						actionTitle:
							item.querySelector("button")?.getAttribute("title") || "",
						actionName:
							item.querySelector("button")?.getAttribute("aria-label") || "",
					})),
				}));
			const changeGraphAvailableNodes =
				changeGraphMetrics.nodePresentation.filter(
					(node) => node.availability === "available",
				);
			const changeGraphContextNodes =
				changeGraphMetrics.nodePresentation.filter(
					(node) => node.availability === "relationship_context_only",
				);
			const clippedChangeGraphLabels =
				changeGraphMetrics.nodeLabelGeometry.filter(
					(node) =>
						!Number.isFinite(node.frameLeft) ||
						!Number.isFinite(node.frameWidth) ||
						!Number.isFinite(node.frameRight) ||
						!Number.isFinite(node.labelLeft) ||
						!Number.isFinite(node.labelWidth) ||
						!Number.isFinite(node.labelRight) ||
						node.label.length === 0 ||
						node.frameWidth <= 0 ||
						node.labelWidth <= 0 ||
						node.labelLeft < node.frameLeft ||
						node.labelRight > node.frameRight,
				);
			compare(
				clippedChangeGraphLabels.length === 0,
				"Change Revision graph label geometry",
				"one or more Change-graph labels escaped their server-sized node frame",
				[],
				clippedChangeGraphLabels,
			);
			const graphSuccessor = config.fixture.graph.successor;
			const graphContext = config.fixture.graph.context;
			const expectedNodeIdentities = [
				`${graphSuccessor.revisionId}@${graphSuccessor.artifactHash}`,
				`${graphContext.revisionId}@${graphContext.artifactHash}`,
			].sort();
			const actualNodeIdentities = changeGraphMetrics.nodePresentation
				.map((node) => `${node.revision}@${node.artifact}`)
				.sort();
			const graphSuccessorNode = changeGraphMetrics.nodePresentation.find(
				(node) =>
					node.revision === graphSuccessor.revisionId &&
					node.artifact === graphSuccessor.artifactHash,
			);
			const graphContextNode = changeGraphMetrics.nodePresentation.find(
				(node) =>
					node.revision === graphContext.revisionId &&
					node.artifact === graphContext.artifactHash,
			);
			const graphClaim = changeGraphMetrics.edgePresentation[0];
			const expectedChangeGraphCounts = {
				svg: 1,
				nodes: 2,
				edges: 1,
				effective: 0,
				claims: 1,
				textual: 1,
			};
			const actualChangeGraphCounts = {
				svg: changeGraphMetrics.svg,
				nodes: changeGraphMetrics.nodes,
				edges: changeGraphMetrics.edges,
				effective: changeGraphMetrics.effective,
				claims: changeGraphMetrics.claims,
				textual: changeGraphMetrics.textual,
			};
			compare(
				JSON.stringify(actualChangeGraphCounts) ===
					JSON.stringify(expectedChangeGraphCounts),
				"Change Revision relationship graph",
				"authoritative graph metrics did not match the fixture",
				expectedChangeGraphCounts,
				actualChangeGraphCounts,
			);
			compare(
				JSON.stringify(actualNodeIdentities) ===
					JSON.stringify(expectedNodeIdentities),
				"Change Revision relationship graph",
				"graph nodes did not retain the fixture's exact Revision identities",
				expectedNodeIdentities,
				actualNodeIdentities,
			);
			const expectedClaim = {
				kind: "pending-or-conflicting-claim",
				successorRevision: graphSuccessor.revisionId,
				successorArtifact: graphSuccessor.artifactHash,
				predecessorRevision: graphContext.revisionId,
				predecessorArtifact: graphContext.artifactHash,
			};
			compare(
				JSON.stringify(graphClaim) === JSON.stringify(expectedClaim),
				"Change Revision relationship graph",
				"graph edge did not retain the fixture's exact relationship tuple",
				expectedClaim,
				graphClaim,
			);
			const changeGraphEdgePartition =
				changeGraphMetrics.effective + changeGraphMetrics.claims;
			compare(
				changeGraphEdgePartition === changeGraphMetrics.edges,
				"Change Revision relationship graph",
				"graph edge kinds did not partition its rendered edges",
				changeGraphMetrics.edges,
				changeGraphEdgePartition,
			);
			const expectedChangeGraphPresentationCounts = {
				textualNodes: changeGraphMetrics.nodes,
				available: 1,
				contextOnly: 1,
			};
			const actualChangeGraphPresentationCounts = {
				textualNodes: changeGraphMetrics.textualNodes.length,
				available: changeGraphAvailableNodes.length,
				contextOnly: changeGraphContextNodes.length,
			};
			compare(
				JSON.stringify(actualChangeGraphPresentationCounts) ===
					JSON.stringify(expectedChangeGraphPresentationCounts),
				"Change Revision relationship graph",
				"graph node presentation counts did not match the fixture",
				expectedChangeGraphPresentationCounts,
				actualChangeGraphPresentationCounts,
			);
			expect(
				graphSuccessorNode?.availability === "available" &&
					graphContextNode?.availability === "relationship_context_only" &&
					changeGraphMetrics.nodePresentation.every(
						(node) =>
							node.revision.length > 0 &&
							node.artifact.length > 0 &&
							node.label.includes(node.revision) &&
							node.label.includes(node.artifact),
					) &&
					changeGraphAvailableNodes.every(
						(node) =>
							node.role === "link" &&
							node.disabled.length === 0 &&
							changeGraphMetrics.textualNodes.some(
								(item) =>
									item.actionTitle === `Open ${node.label}` &&
									item.actionName === `Open ${node.label}`,
							),
					) &&
					changeGraphContextNodes.every(
						(node) =>
							node.role === "group" &&
							node.disabled === "true" &&
							changeGraphMetrics.textualNodes.some(
								(item) =>
									item.text === node.label &&
									item.actionTitle.length === 0 &&
									item.actionName.length === 0,
							),
					),
				"Change Revision relationship graph",
				`invalid authoritative graph geometry: ${JSON.stringify(changeGraphMetrics)}`,
			);
			const changeGraphText = page.locator(
				"#detail-body .change-revision-graph details[data-graph-textual-equivalent]",
			);
			await changeGraphText.locator("summary").click();
			const changeGraphTextCounts = {
				nodes: await changeGraphText
					.locator("[data-graph-text-nodes] > li")
					.count(),
				actions: await changeGraphText
					.locator("[data-graph-text-nodes] button")
					.count(),
				edges: await changeGraphText
					.locator("[data-graph-text-edges] li")
					.count(),
			};
			const expectedChangeGraphTextCounts = {
				nodes: changeGraphMetrics.nodes,
				actions: changeGraphAvailableNodes.length,
				minimumEdges: changeGraphMetrics.edges,
			};
			compare(
				changeGraphTextCounts.nodes === expectedChangeGraphTextCounts.nodes &&
					changeGraphTextCounts.actions ===
						expectedChangeGraphTextCounts.actions &&
					changeGraphTextCounts.edges >=
						expectedChangeGraphTextCounts.minimumEdges,
				"Change Revision graph text alternative",
				"the textual graph equivalent omitted a readable node, available-node action, or relationship",
				expectedChangeGraphTextCounts,
				changeGraphTextCounts,
			);
			await screenshot("wide-change-revision-graph");
			const graphNodes = page.locator(
				"#detail-body .change-revision-graph g.change-revision-node[data-context-availability='available']",
			);
			const graphNodeCount = await graphNodes.count();
			requireCondition(
				graphNodeCount === 1,
				"Change Revision graph action",
				"graph did not expose exactly one available exact Revision node",
				1,
				graphNodeCount,
			);
			const graphNode = graphNodes.first();
			const graphIdentity = await graphNode.evaluate((node) => ({
				revisionId: node.getAttribute("data-revision-id"),
				artifactHash: node.getAttribute("data-artifact-hash"),
			}));
			const graphRevision = graphIdentity.revisionId;
			const graphArtifact = graphIdentity.artifactHash;
			requireCondition(
				Boolean(graphRevision && graphArtifact),
				"Change Revision graph action",
				"graph node omitted an exact Revision identity",
				{ revisionId: "nonempty", artifactHash: "nonempty" },
				graphIdentity,
			);
			await graphNode.focus();
			await page.keyboard.press("Enter");
			await page.waitForFunction(
				({ changeId, revision, artifact }) => {
					const query = new URLSearchParams(
						location.hash.split("?", 2)[1] ?? "",
					);
					return (
						location.hash.includes(
							`/changes/${encodeURIComponent(changeId)}/revisions/${encodeURIComponent(revision)}`,
						) &&
						query.get("artifactHash") === artifact &&
						Boolean(
							document.querySelector("#detail-body")?.dataset.changeReadingKey,
						)
					);
				},
				{
					changeId: graphChange,
					revision: graphRevision,
					artifact: graphArtifact,
				},
			);
			await page.goBack();
			await page.waitForFunction(
				(expectedRoute) =>
					location.hash === `#/${expectedRoute}` &&
					Boolean(
						document.querySelector("#detail-body .change-revision-graph"),
					),
				graphRoute,
			);

			await open(
				graphRoute,
				layouts[1],
				"narrow Change Revision relationship graph",
			);
			const narrowChangeGraphViewport = page.locator(
				"#detail-body .change-revision-graph [data-graph-viewport]",
			);
			const narrowChangeGraphGeometry =
				await narrowChangeGraphViewport.evaluate((viewport) => ({
					clientWidth: viewport.clientWidth,
					scrollWidth: viewport.scrollWidth,
					svgWidth:
						viewport.querySelector("svg")?.getBoundingClientRect().width || 0,
				}));
			compare(
				narrowChangeGraphGeometry.clientWidth > 0 &&
					narrowChangeGraphGeometry.scrollWidth >=
						narrowChangeGraphGeometry.clientWidth &&
					narrowChangeGraphGeometry.svgWidth > 0 &&
					narrowChangeGraphGeometry.scrollWidth >=
						narrowChangeGraphGeometry.svgWidth,
				"narrow intrinsic Change graph viewport",
				`Change graph viewport did not preserve its intrinsic canvas: ${JSON.stringify(narrowChangeGraphGeometry)}`,
				{
					clientWidth: "> 0",
					scrollWidth: ">= clientWidth and >= svgWidth",
					svgWidth: "> 0",
				},
				narrowChangeGraphGeometry,
			);
			const changeGraphMaxScroll = Math.max(
				0,
				narrowChangeGraphGeometry.scrollWidth -
					narrowChangeGraphGeometry.clientWidth,
			);
			await narrowChangeGraphViewport.focus();
			await page.keyboard.press("End");
			const changeGraphEnd = await narrowChangeGraphViewport.evaluate(
				(viewport) => viewport.scrollLeft,
			);
			await page.keyboard.press("Home");
			const changeGraphHome = await narrowChangeGraphViewport.evaluate(
				(viewport) => viewport.scrollLeft,
			);
			compare(
				changeGraphEnd === changeGraphMaxScroll && changeGraphHome === 0,
				"narrow Change graph keyboard panning",
				`Home/End panning produced ${changeGraphHome}/${changeGraphEnd} for ${JSON.stringify(narrowChangeGraphGeometry)}`,
				{
					home: 0,
					end: changeGraphMaxScroll,
				},
				{ home: changeGraphHome, end: changeGraphEnd },
			);
			await screenshot("narrow-change-revision-graph");
		},
		teardown: teardownSection,
	});

	await diagnostics.section("Exact Revision selection and history", {
		setup: () => {
			const parallel = config.fixture.matrix.topology.parallel_current;
			const route = `changes?limit=100&order=change_id_asc&topology=parallel_current&q=${encodeURIComponent(parallel.change)}`;
			return open(route, layouts[0], "Exact Revision selection section setup");
		},
		run: async () => {
			const topologyFixture = config.fixture.matrix.topology;

			const expectedParallelChange = topologyFixture.parallel_current.change;
			const parallelRoute = `changes?limit=100&order=change_id_asc&topology=parallel_current&q=${encodeURIComponent(expectedParallelChange)}`;
			await open(parallelRoute, layouts[0], "parallel explicit chooser");
			const parallelChange = await page
				.locator(".unit-card[data-change-id]")
				.evaluateAll((cards) => {
					const card = cards.find(
						(candidate) =>
							candidate.querySelectorAll(".change-card-peer-open").length > 1,
					);
					return card?.dataset.changeId ?? null;
				});
			requireCondition(
				typeof parallelChange === "string" &&
					parallelChange === expectedParallelChange,
				"parallel explicit chooser",
				"no exact fixture Change exposed its multiple current Revisions",
				expectedParallelChange,
				parallelChange,
			);
			const parallelCard = page.locator(
				`.unit-card[data-change-id="${parallelChange}"]`,
			);
			const peerButtons = parallelCard.locator(".change-card-peer-open");
			const expectedParallelPeers = topologyFixture.parallel_current.current
				.map((peer) => ({
					revisionId: peer.revision,
					artifactHash: peer.artifact,
					title: `exact Revision ${peer.revision}; artifact ${peer.artifact}`,
				}))
				.sort((left, right) => left.title.localeCompare(right.title));
			const renderedParallelPeers = await peerButtons.evaluateAll((peers) =>
				peers
					.map((peer) => ({
						title: peer.getAttribute("title") || "",
						name: peer.getAttribute("aria-label") || "",
					}))
					.sort((left, right) => left.title.localeCompare(right.title)),
			);
			const expectedParallelPeerTitles = expectedParallelPeers.map(
				(peer) => peer.title,
			);
			const actualParallelPeerTitles = renderedParallelPeers.map(
				(peer) => peer.title,
			);
			requireCondition(
				JSON.stringify(actualParallelPeerTitles) ===
					JSON.stringify(expectedParallelPeerTitles),
				"parallel explicit chooser",
				"card peer controls did not represent the fixture's exact current Revisions",
				expectedParallelPeerTitles,
				actualParallelPeerTitles,
			);
			const parallelCardName =
				(await parallelCard.getAttribute("aria-label")) || "";
			expect(
				expectedParallelPeers.every(
					(expected, index) =>
						parallelCardName.includes(expected.revisionId) &&
						parallelCardName.includes(expected.artifactHash) &&
						renderedParallelPeers[index].name.includes(expected.revisionId) &&
						renderedParallelPeers[index].name.includes(expected.artifactHash),
				),
				"parallel explicit chooser",
				"parallel card or peer names omitted an exact fixture identity",
			);
			const parallelControls = parallelCard.locator(
				"a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), summary, [tabindex]:not([tabindex='-1'])",
			);
			const parallelControlCount = await parallelControls.count();
			requireCondition(
				parallelControlCount === expectedParallelPeers.length + 1,
				"parallel card tab order",
				"parallel Change card exposed controls beyond its primary action and exact peers",
				expectedParallelPeers.length + 1,
				parallelControlCount,
			);
			const parallelPrimary = parallelCard.locator(".change-card-primary");
			const parallelPrimaryCount = await parallelPrimary.count();
			requireCondition(
				parallelPrimaryCount === 1,
				"parallel card tab order",
				"parallel Change card did not expose exactly one primary action",
				1,
				parallelPrimaryCount,
			);
			expect(
				await parallelControls
					.first()
					.getAttribute("class")
					.then((value) => value?.includes("change-card-primary")),
				"parallel card tab order",
				"parallel Change card did not lead with its primary action",
			);
			expect(
				await parallelPrimary
					.getAttribute("aria-label")
					.then(
						(value) =>
							value?.startsWith("Review current Revisions. ") &&
							value.includes(parallelCardName),
					),
				"parallel card action name",
				"parallel Change card's primary action did not lead with its human review action and retain the full card name",
			);
			await peerButtons.first().focus();
			await page.keyboard.press("Tab");
			const secondPeerFocused = await peerButtons
				.nth(1)
				.evaluate((node) => document.activeElement === node);
			compare(
				secondPeerFocused,
				"peer keyboard traversal",
				"Tab did not move directly between exact current-Revision peers",
				true,
				secondPeerFocused,
			);
			const peerFocus = await peerButtons.nth(1).evaluate((node) => {
				const style = getComputedStyle(node);
				return {
					outlineStyle: style.outlineStyle,
					outlineWidth: style.outlineWidth,
				};
			});
			compare(
				peerFocus.outlineStyle !== "none" && peerFocus.outlineWidth !== "0px",
				"visible peer focus",
				`peer focus indicator was ${peerFocus.outlineStyle}/${peerFocus.outlineWidth}`,
				{ outlineStyle: "not none", outlineWidth: "not 0px" },
				peerFocus,
			);
			await parallelPrimary.click();
			await page.waitForFunction(
				(changeId) =>
					location.hash.includes(encodeURIComponent(changeId)) &&
					!location.hash.includes("/revisions/"),
				parallelChange,
			);
			const detailPeerControlCount = await page
				.locator("#detail-body .change-card-peer-open")
				.count();
			compare(
				detailPeerControlCount === 0,
				"parallel explicit chooser",
				"card-only peer controls leaked into detail",
				0,
				detailPeerControlCount,
			);
			await page.waitForFunction(() =>
				Boolean(
					document.querySelector("#detail-body")?.dataset.changeReadingKey,
				),
			);
			const exactChoices = page.locator(
				"#detail-body button[aria-label^='Current Revision:']",
			);
			const exactChoiceCount = await exactChoices.count();
			requireCondition(
				exactChoiceCount > 1,
				"parallel explicit chooser",
				"Change detail did not require a human exact-Revision choice",
				"> 1",
				exactChoiceCount,
			);
			const renderedExactChoices = await exactChoices.evaluateAll((choices) =>
				choices
					.map((choice) => ({
						title: choice.getAttribute("title") || "",
						name: choice.getAttribute("aria-label") || "",
						visible: choice.textContent?.trim() || "",
					}))
					.sort((left, right) => left.title.localeCompare(right.title)),
			);
			const actualExactChoiceTitles = renderedExactChoices.map(
				(choice) => choice.title,
			);
			requireCondition(
				JSON.stringify(actualExactChoiceTitles) ===
					JSON.stringify(expectedParallelPeerTitles),
				"parallel explicit chooser",
				"Change detail choices did not represent the fixture's exact current Revisions",
				expectedParallelPeerTitles,
				actualExactChoiceTitles,
			);
			expect(
				renderedExactChoices.every(
					(choice, index) =>
						choice.name ===
							`Current Revision: open ${expectedParallelPeers[index].title}; for Change ${parallelChange}` &&
						choice.visible ===
							shortExact(
								expectedParallelPeers[index].revisionId,
								expectedParallelPeers[index].artifactHash,
							),
				),
				"parallel explicit chooser",
				"Change detail did not pair shortened visible identities with complete accessible identities",
			);
			const chosenExactTitle = await exactChoices.first().getAttribute("title");
			const chosenExactPeer = expectedParallelPeers.find(
				(peer) => peer.title === chosenExactTitle,
			);
			requireCondition(
				Boolean(chosenExactPeer),
				"parallel explicit chooser",
				"Change detail offered an exact Revision outside the fixture's current peers",
				expectedParallelPeerTitles,
				chosenExactTitle,
			);
			await exactChoices.first().click();
			await page.waitForFunction(() => location.hash.includes("/revisions/"));
			await page.waitForFunction(() =>
				Boolean(
					document.querySelector("#detail-body")?.dataset.changeReadingKey,
				),
			);
			const expectedExactRevisionRoute = {
				changeId: parallelChange,
				revisionId: chosenExactPeer.revisionId,
				artifactHash: chosenExactPeer.artifactHash,
			};
			const actualExactRevisionRoute = await page.evaluate(() => {
				const [path, rawQuery = ""] = location.hash.slice(2).split("?", 2);
				const [changes, encodedChangeId, revisions, encodedRevisionId] =
					path.split("/");
				const query = new URLSearchParams(rawQuery);
				return {
					changeId:
						changes === "changes" && encodedChangeId
							? decodeURIComponent(encodedChangeId)
							: null,
					revisionId:
						revisions === "revisions" && encodedRevisionId
							? decodeURIComponent(encodedRevisionId)
							: null,
					artifactHash: query.get("artifactHash"),
				};
			});
			requireCondition(
				JSON.stringify(actualExactRevisionRoute) ===
					JSON.stringify(expectedExactRevisionRoute),
				"parallel exact Revision route",
				"peer choice did not open its exact Change, Revision, and artifact identity",
				expectedExactRevisionRoute,
				actualExactRevisionRoute,
			);
			const copyLink = page
				.locator("#detail-body")
				.getByRole("button", { name: "Copy link" });
			const copyLinkCount = await copyLink.count();
			requireCondition(
				copyLinkCount === 1,
				"parallel exact Revision copy",
				"exact Revision detail omitted its relocated copy action",
				1,
				copyLinkCount,
			);
			await page.evaluate(() => {
				Object.defineProperty(navigator, "clipboard", {
					configurable: true,
					value: {
						writeText(value) {
							document.documentElement.dataset.pointbreakCopiedLink = value;
							return Promise.resolve();
						},
					},
				});
			});
			const exactRevisionUrl = page.url();
			await copyLink.click();
			await page.waitForFunction(
				(expected) =>
					document.documentElement.dataset.pointbreakCopiedLink === expected,
				exactRevisionUrl,
			);
			const copiedExactRevisionUrl = await page.evaluate(
				() => document.documentElement.dataset.pointbreakCopiedLink,
			);
			compare(
				copiedExactRevisionUrl === exactRevisionUrl,
				"parallel exact Revision copy",
				"Copy link did not copy the current exact Revision URL",
				exactRevisionUrl,
				copiedExactRevisionUrl,
			);
			await screenshot("wide-parallel-explicit-revision");
			const revisionReadingKey = await page
				.locator("#detail-body")
				.getAttribute("data-change-reading-key");
			await page
				.getByRole("button", { name: "Open authoritative captured diff" })
				.click();
			await page.waitForFunction(() => location.hash.includes("/resource?"));
			await page.waitForFunction((key) => {
				const next =
					document.querySelector("#detail-body")?.dataset.changeReadingKey;
				return Boolean(next && next !== key);
			}, revisionReadingKey);
			const detailCloseFocusedOnExactRoute = await page
				.locator("#detail-close")
				.evaluate((node) => document.activeElement === node);
			compare(
				detailCloseFocusedOnExactRoute,
				"exact route focus",
				"exact-to-exact detail replacement left focus on the document body",
				true,
				detailCloseFocusedOnExactRoute,
			);
			const resourceReadingKey = await page
				.locator("#detail-body")
				.getAttribute("data-change-reading-key");
			await page.goBack();
			await page.waitForFunction(
				() =>
					location.hash.includes("/revisions/") &&
					!location.hash.includes("/resource?"),
			);
			await page.waitForFunction((key) => {
				const next =
					document.querySelector("#detail-body")?.dataset.changeReadingKey;
				return Boolean(next && next !== key);
			}, resourceReadingKey);
			await page.keyboard.press("3");
			await page.waitForFunction(() =>
				location.hash.startsWith("#/attention?"),
			);
			await page.goBack();
			await page.waitForFunction(() => location.hash.includes("/revisions/"));
			await page.keyboard.press("Escape");
			await page.waitForFunction(() => location.hash.startsWith("#/changes?"));
			const historyOriginHash = await hash();
			compare(
				historyOriginHash.startsWith("#/changes?"),
				"exact history origin",
				"Back/Forward changed the exact route's originating lens",
				"#/changes?...",
				historyOriginHash,
			);
		},
		teardown: teardownSection,
	});

	await diagnostics.section("Shared Revision membership", {
		setup: () => {
			const changeId = config.fixture.matrix.shared_revision.changes[0];
			return open(
				`changes/${encodeURIComponent(changeId)}?limit=100&order=change_id_asc`,
				layouts[0],
				"Shared Revision membership section setup",
			);
		},
		run: async () => {
			for (const layout of layouts) {
				for (const changeId of config.fixture.matrix.shared_revision.changes) {
					const encodedMembershipChange = encodeURIComponent(changeId);
					await open(
						`changes/${encodedMembershipChange}?limit=100&order=change_id_asc`,
						layout,
						`${layout.name} shared Revision membership`,
					);
					const shared = config.fixture.matrix.shared_revision;
					const sharedNode = page.locator(
						`#detail-body .change-revision-graph [data-revision-id="${shared.revision}"][data-artifact-hash="${shared.artifact}"][data-member="true"]`,
					);
					const sharedNodeCount = await sharedNode.count();
					requireCondition(
						sharedNodeCount === 1,
						`${layout.name} shared Revision membership`,
						`Change ${changeId} omitted shared exact Revision`,
						1,
						sharedNodeCount,
					);
					const sharedNodeName =
						(await sharedNode.getAttribute("aria-label")) || "";
					const sharedNodeRole = await sharedNode.getAttribute("role");
					const sharedNodeAvailability = await sharedNode.getAttribute(
						"data-context-availability",
					);
					expect(
						sharedNodeName.includes(
							`exact Revision ${shared.revision}; artifact ${shared.artifact}`,
						) &&
							sharedNodeName.includes("Change member") &&
							sharedNodeName.includes("exact Change context available") &&
							sharedNodeRole === "link" &&
							sharedNodeAvailability === "available",
						`${layout.name} shared Revision membership`,
						`Change ${changeId} did not expose the shared Revision as an available exact member`,
					);
					expect(
						(await sharedNode.locator("text").textContent())?.endsWith(
							shortRef(shared.revision),
						),
						`${layout.name} shared Revision membership`,
						`Change ${changeId} did not render the shared Revision's shortened visible identity`,
					);
				}
				await screenshot(`${layout.name}-shared-revision-membership`);
			}
		},
		teardown: teardownSection,
	});

	await diagnostics.section("Split, preferences, and dialogs", {
		setup: () =>
			open(
				"changes?limit=100&order=change_id_asc",
				layouts[0],
				"Split, preferences, and dialogs section setup",
			),
		run: async () => {
			await open(
				"changes?limit=100&order=change_id_asc",
				layouts[0],
				"split bounds",
			);
			await page.locator(".change-card-primary").first().click();
			await page.waitForFunction(
				() => !document.querySelector("#detail")?.inert,
			);
			const divider = page.locator(".divider");
			const splitBox = await page.locator(".split").boundingBox();
			const dividerBox = await divider.boundingBox();
			requireCondition(
				Boolean(splitBox && dividerBox),
				"split pointer drag",
				"visible split geometry was unavailable",
				{ split: "bounding box", divider: "bounding box" },
				{ split: splitBox, divider: dividerBox },
			);
			await page.mouse.move(
				dividerBox.x + dividerBox.width / 2,
				dividerBox.y + dividerBox.height / 2,
			);
			await page.mouse.down();
			await page.mouse.move(
				splitBox.x + splitBox.width * 0.62,
				dividerBox.y + dividerBox.height / 2,
			);
			await page.mouse.up();
			const draggedSplit = Number(await divider.getAttribute("aria-valuenow"));
			compare(
				draggedSplit >= 61 && draggedSplit <= 63,
				"split pointer drag",
				`pointer drag produced ${draggedSplit} instead of approximately 62`,
				"61..63",
				draggedSplit,
			);
			await divider.dblclick();
			const splitAfterPointerReset =
				await divider.getAttribute("aria-valuenow");
			compare(
				splitAfterPointerReset === "50",
				"split pointer reset",
				"double-click did not restore the balanced split",
				"50",
				splitAfterPointerReset,
			);
			await divider.focus();
			for (let step = 0; step < 40; step += 1)
				await page.keyboard.press("ArrowLeft");
			const splitAtLowerBound = await divider.getAttribute("aria-valuenow");
			compare(
				splitAtLowerBound === "25",
				"split lower bound",
				"divider moved below its declared lower bound",
				"25",
				splitAtLowerBound,
			);
			for (let step = 0; step < 80; step += 1)
				await page.keyboard.press("ArrowRight");
			const splitAtUpperBound = await divider.getAttribute("aria-valuenow");
			compare(
				splitAtUpperBound === "75",
				"split upper bound",
				"divider moved above its declared upper bound",
				"75",
				splitAtUpperBound,
			);
			await page.keyboard.press("Enter");
			const splitAfterKeyboardReset =
				await divider.getAttribute("aria-valuenow");
			compare(
				splitAfterKeyboardReset === "50",
				"split reset",
				"Enter did not restore the balanced split",
				"50",
				splitAfterKeyboardReset,
			);

			await page.locator("#view-toggle").click();
			await page.locator("#theme-dark").check();
			await page.locator("#density-compact").check();
			const darkCompactPreference = await page.evaluate(() => ({
				theme: document.documentElement.dataset.theme,
				compact: document.documentElement.classList.contains("compact"),
			}));
			const expectedDarkCompactPreference = { theme: "dark", compact: true };
			compare(
				JSON.stringify(darkCompactPreference) ===
					JSON.stringify(expectedDarkCompactPreference),
				"dark compact preference",
				"dark/compact preference was not applied",
				expectedDarkCompactPreference,
				darkCompactPreference,
			);
			await screenshot("wide-dark-compact");
			await page.locator("#theme-light").check();
			await page.locator("#density-comfortable").check();
			const lightComfortablePreference = await page.evaluate(() => ({
				theme: document.documentElement.dataset.theme,
				compact: document.documentElement.classList.contains("compact"),
			}));
			const expectedLightComfortablePreference = {
				theme: "light",
				compact: false,
			};
			compare(
				JSON.stringify(lightComfortablePreference) ===
					JSON.stringify(expectedLightComfortablePreference),
				"light comfortable preference",
				"light/comfortable preference was not applied",
				expectedLightComfortablePreference,
				lightComfortablePreference,
			);
			await screenshot("wide-light-comfortable");

			await page.locator("#view-toggle").focus();
			await page.keyboard.press("?");
			const help = page.locator("#key-help");
			expect(
				await help.evaluate((node) => !node.classList.contains("hidden")),
				"help overlay",
				"? did not open help",
			);
			expect(
				await page.evaluate(
					() => document.activeElement?.id === "key-help-close",
				),
				"help focus",
				"help did not move focus into its dialog",
			);
			const helpText = await help.innerText();
			expect(
				helpText.includes("Timeline / Changes / Attention") &&
					helpText.includes("one exact Revision in a stable Change") &&
					helpText.includes("change attention"),
				"Change-aware help",
				"help did not describe the served Change lenses and exact-Revision workflow",
			);
			await page.keyboard.press("Shift+Tab");
			expect(
				await page.evaluate(() =>
					document.querySelector("#key-help")?.contains(document.activeElement),
				),
				"help modal trap",
				"Shift+Tab escaped the help dialog",
			);
			await page.keyboard.press("Escape");
			expect(
				await page.evaluate(() => document.activeElement?.id === "view-toggle"),
				"help focus restoration",
				"help did not restore focus to its opener",
			);
			await page.keyboard.press("Control+k");
			const palette = page.locator("#cmd-palette");
			expect(
				await palette.evaluate((node) => !node.classList.contains("hidden")),
				"palette overlay",
				"Cmd/Ctrl+K did not open palette",
			);
			expect(
				await page.evaluate(() => document.activeElement?.id === "cmd-input"),
				"palette focus",
				"palette did not focus its input",
			);
			await page.keyboard.press("Escape");
			expect(
				await page.evaluate(() => document.activeElement?.id === "view-toggle"),
				"palette focus restoration",
				"palette did not restore focus to its opener",
			);

			await page.keyboard.press("Control+Shift+p");
			expect(
				await palette.evaluate((node) => !node.classList.contains("hidden")),
				"alternate palette chord",
				"Ctrl+Shift+P did not open palette",
			);
			await page.keyboard.press("Escape");
		},
		teardown: teardownSection,
	});

	const encodedChange = encodeURIComponent(config.fixture.rich.changeId);
	const encodedRevision = encodeURIComponent(config.fixture.rich.revisionId);
	const encodedArtifact = encodeURIComponent(config.fixture.rich.artifactHash);
	const exact = `changes/${encodedChange}/revisions/${encodedRevision}?artifactHash=${encodedArtifact}&limit=100&order=change_id_asc`;

	await diagnostics.section("Fact relationship graph", {
		setup: () => open(exact, layouts[0], "exact fact relationship graph"),
		run: async () => {
			// The rich matrix carries fact supersession, assessment replacement, and a
			// context-only fact port. Its exact reader therefore proves that the fact
			// graph retains those distinct relationships. Available exact nodes expose
			// route actions; context-only nodes remain readable but noninteractive.
			await page.waitForFunction(() =>
				Boolean(
					document.querySelector("#detail-body .fact-relationship-graph"),
				),
			);
			const factGraphMetrics = await page
				.locator("#detail-body .fact-relationship-graph")
				.evaluate(
					(graph, factPortId) => ({
						viewports: graph.querySelectorAll("[data-graph-viewport]").length,
						viewportTabIndex:
							graph
								.querySelector("[data-graph-viewport]")
								?.getAttribute("tabindex") || "",
						viewportInstructions:
							graph
								.querySelector("[data-graph-viewport]")
								?.getAttribute("aria-label") || "",
						svg: graph.querySelectorAll("svg.fact-relationship-graph-svg")
							.length,
						svgWidth: Number(
							graph
								.querySelector("svg.fact-relationship-graph-svg")
								?.getAttribute("width") || 0,
						),
						svgMaxWidth: graph.querySelector("svg.fact-relationship-graph-svg")
							? getComputedStyle(
									graph.querySelector("svg.fact-relationship-graph-svg"),
								).maxWidth
							: "",
						nodes: graph.querySelectorAll("g.fact-relationship-node").length,
						edges: graph.querySelectorAll("g.fact-relationship-edge").length,
						observationSupersedes: graph.querySelectorAll(
							'g.fact-relationship-edge[data-edge-kind="observation-supersedes"]',
						).length,
						assessmentReplaces: graph.querySelectorAll(
							'g.fact-relationship-edge[data-edge-kind="assessment-replaces"]',
						).length,
						factPorts: graph.querySelectorAll(
							'g.fact-relationship-edge[data-edge-kind="fact-port"]',
						).length,
						expectedFactPort: Array.from(
							graph.querySelectorAll("g.fact-relationship-edge"),
						).some((edge) => edge.getAttribute("data-port-id") === factPortId),
						textual: graph.querySelectorAll(
							"details[data-graph-textual-equivalent]",
						).length,
						textualOutsideViewport:
							graph.querySelector(
								"[data-graph-viewport] details[data-graph-textual-equivalent]",
							) === null,
						nodePresentation: Array.from(
							graph.querySelectorAll("g.fact-relationship-node"),
						).map((node) => {
							const revision = node.getAttribute("data-revision-id") || "";
							const artifact = node.getAttribute("data-artifact-hash") || "";
							const label = node.getAttribute("aria-label") || "";
							return {
								revision,
								artifact,
								label,
								availability:
									node.getAttribute("data-context-availability") || "",
								role: node.getAttribute("role") || "",
								disabled: node.getAttribute("aria-disabled") || "",
								};
							}),
						nodeLabelGeometry: Array.from(
							graph.querySelectorAll("g.fact-relationship-node"),
							(node) => {
								const frame = node.querySelector("rect");
								const label = node.querySelector("text");
								const frameLeft = Number(frame?.getAttribute("x") || 0);
								const frameWidth = Number(frame?.getAttribute("width") || 0);
								const labelBounds = label?.getBBox();
								return {
									nodeId: node.getAttribute("data-graph-node-id") || "",
									label: label?.textContent || "",
									frameLeft,
									frameWidth,
									frameRight: frameLeft + frameWidth,
									labelLeft: labelBounds?.x ?? Number.NEGATIVE_INFINITY,
									labelWidth: labelBounds?.width ?? 0,
									labelRight:
										(labelBounds?.x ?? Number.POSITIVE_INFINITY) +
										(labelBounds?.width ?? 0),
								};
							},
						),
						textualNodes: Array.from(
							graph.querySelectorAll("[data-graph-text-nodes] > li"),
						).map((item) => ({
							text: item.textContent || "",
							actionTitle:
								item.querySelector("button")?.getAttribute("title") || "",
							actionName:
								item.querySelector("button")?.getAttribute("aria-label") || "",
						})),
					}),
					config.fixture.factPort.portId,
				);
			const factGraphAvailableNodes = factGraphMetrics.nodePresentation.filter(
				(node) => node.availability === "available",
			);
			const factGraphContextNodes = factGraphMetrics.nodePresentation.filter(
				(node) => node.availability === "relationship_context_only",
			);
			const clippedFactGraphLabels =
				factGraphMetrics.nodeLabelGeometry.filter(
					(node) =>
						!Number.isFinite(node.frameLeft) ||
						!Number.isFinite(node.frameWidth) ||
						!Number.isFinite(node.frameRight) ||
						!Number.isFinite(node.labelLeft) ||
						!Number.isFinite(node.labelWidth) ||
						!Number.isFinite(node.labelRight) ||
						node.label.length === 0 ||
						node.frameWidth <= 0 ||
						node.labelWidth <= 0 ||
						node.labelLeft < node.frameLeft ||
						node.labelRight > node.frameRight,
				);
			compare(
				clippedFactGraphLabels.length === 0,
				"exact fact graph label geometry",
				"one or more fact-graph labels escaped their server-sized node frame",
				[],
				clippedFactGraphLabels,
			);
			const expectedFactGraphStructure = {
				viewports: 1,
				viewportTabIndex: "0",
				svg: 1,
				svgMaxWidth: "none",
				textual: 1,
				textualOutsideViewport: true,
			};
			const actualFactGraphStructure = {
				viewports: factGraphMetrics.viewports,
				viewportTabIndex: factGraphMetrics.viewportTabIndex,
				svg: factGraphMetrics.svg,
				svgMaxWidth: factGraphMetrics.svgMaxWidth,
				textual: factGraphMetrics.textual,
				textualOutsideViewport: factGraphMetrics.textualOutsideViewport,
			};
			compare(
				JSON.stringify(actualFactGraphStructure) ===
					JSON.stringify(expectedFactGraphStructure),
				"exact fact relationship graph",
				"fact graph structure did not preserve its expected viewport and textual equivalent",
				expectedFactGraphStructure,
				actualFactGraphStructure,
			);
			const factGraphMinimums = {
				svgWidth: "> 0",
				nodes: ">= 2",
				edges: ">= 2",
				observationSupersedes: ">= 1",
				factPorts: ">= 1",
			};
			const factGraphActualMetrics = {
				svgWidth: factGraphMetrics.svgWidth,
				nodes: factGraphMetrics.nodes,
				edges: factGraphMetrics.edges,
				observationSupersedes: factGraphMetrics.observationSupersedes,
				factPorts: factGraphMetrics.factPorts,
			};
			compare(
				factGraphActualMetrics.svgWidth > 0 &&
					factGraphActualMetrics.nodes >= 2 &&
					factGraphActualMetrics.edges >= 2 &&
					factGraphActualMetrics.observationSupersedes >= 1 &&
					factGraphActualMetrics.factPorts >= 1,
				"exact fact relationship graph",
				"fact graph metrics did not retain the expected relationship density",
				factGraphMinimums,
				factGraphActualMetrics,
			);
			const factGraphPresentationCounts = {
				textualNodes: factGraphMetrics.textualNodes.length,
				available: factGraphAvailableNodes.length,
				contextOnly: factGraphContextNodes.length,
			};
			compare(
				factGraphPresentationCounts.textualNodes === factGraphMetrics.nodes &&
					factGraphPresentationCounts.available > 0 &&
					factGraphPresentationCounts.contextOnly > 0,
				"exact fact relationship graph",
				"fact graph presentation counts did not retain readable nodes and relationship context",
				{
					textualNodes: factGraphMetrics.nodes,
					available: "> 0",
					contextOnly: "> 0",
				},
				factGraphPresentationCounts,
			);
			expect(
				factGraphMetrics.viewportInstructions.includes("Left") &&
					factGraphMetrics.viewportInstructions.includes("End") &&
					factGraphMetrics.expectedFactPort &&
					factGraphMetrics.nodePresentation.every(
						(node) =>
							node.revision.length > 0 &&
							node.artifact.length > 0 &&
							node.label.includes(node.revision) &&
							node.label.includes(node.artifact),
					) &&
					factGraphAvailableNodes.every(
						(node) =>
							node.role === "link" &&
							node.disabled.length === 0 &&
							factGraphMetrics.textualNodes.some(
								(item) =>
									item.actionTitle === node.label &&
									item.actionName === node.label,
							),
					) &&
					factGraphContextNodes.every(
						(node) =>
							node.role === "group" &&
							node.disabled === "true" &&
							factGraphMetrics.textualNodes.some(
								(item) =>
									item.text === node.label &&
									item.actionTitle.length === 0 &&
									item.actionName.length === 0,
							),
					),
				"exact fact relationship graph",
				`rich exact Revision did not retain its relationship evidence: ${JSON.stringify(factGraphMetrics)}`,
			);
			const factGraphText = page.locator(
				"#detail-body .fact-relationship-graph details[data-graph-textual-equivalent]",
			);
			await factGraphText.locator("summary").click();
			const factGraphTextCounts = {
				nodes: await factGraphText
					.locator("[data-graph-text-nodes] > li")
					.count(),
				actions: await factGraphText
					.locator("[data-graph-text-nodes] button")
					.count(),
				edges: await factGraphText
					.locator("[data-graph-text-edges] li")
					.count(),
			};
			const expectedFactGraphTextCounts = {
				nodes: factGraphMetrics.nodes,
				actions: factGraphAvailableNodes.length,
				minimumEdges: factGraphMetrics.edges,
			};
			compare(
				factGraphTextCounts.nodes === expectedFactGraphTextCounts.nodes &&
					factGraphTextCounts.actions === expectedFactGraphTextCounts.actions &&
					factGraphTextCounts.edges >= expectedFactGraphTextCounts.minimumEdges,
				"exact fact graph text alternative",
				"the textual fact graph equivalent omitted a readable node, available-node action, or relationship",
				expectedFactGraphTextCounts,
				factGraphTextCounts,
			);
			await screenshot("wide-exact-fact-relationship-graph");
			const factGraphNodes = page.locator(
				"#detail-body .fact-relationship-node[data-node-kind='fact'][data-context-availability='available']",
			);
			const factGraphNodeCount = await factGraphNodes.count();
			requireCondition(
				factGraphNodeCount > 0,
				"exact fact graph action",
				"the graph did not expose an available exact fact node",
				"> 0",
				factGraphNodeCount,
			);
			const factGraphNode = factGraphNodes.first();
			const graphFactId =
				await factGraphNode.getAttribute("data-graph-fact-id");
			requireCondition(
				Boolean(graphFactId),
				"exact fact graph action",
				"the graph did not expose a focusable exact fact node",
				"nonempty fact ID",
				graphFactId,
			);
			await factGraphNode.focus();
			await page.keyboard.press("Enter");
			await page.waitForFunction((factId) => {
				const query = new URLSearchParams(location.hash.split("?", 2)[1] ?? "");
				const target = Array.from(
					document.querySelectorAll("#detail-body [data-fact-id]"),
				).find((element) => element.dataset.factId === factId);
				return (
					query.get("fact") === factId &&
					target?.dataset.exactFocus === "true" &&
					document.activeElement === target
				);
			}, graphFactId);
			await page.goBack();
			await page.waitForFunction(
				(expectedRoute) =>
					location.hash === `#/${expectedRoute}` &&
					Boolean(
						document.querySelector("#detail-body .fact-relationship-graph"),
					),
				exact,
			);
		},
		teardown: teardownSection,
	});

	await diagnostics.section("Annotated diff", {
		setup: () => open(exact, layouts[0], "Annotated diff section setup"),
		run: async () => {
			// An annotated diff is a first-class full-frame exact route, not an
			// embedded side panel. It keeps the captured byte view and inlined facts
			// together, advertises its keys, restores focus, and round-trips through
			// reload and browser history without losing the exact Revision selector.
			const annotatedDiffOpener = page.getByRole("button", {
				name: "Open annotated diff",
			});
			const annotatedDiffOpenerCount = await annotatedDiffOpener.count();
			requireCondition(
				annotatedDiffOpenerCount === 1,
				"annotated diff entry",
				"exact Revision did not offer one full-frame annotated diff action",
				1,
				annotatedDiffOpenerCount,
			);
			const canonicalRevisionRoute = await hash();
			const [canonicalRevisionPath, canonicalRevisionSearch = ""] =
				canonicalRevisionRoute.split("?", 2);
			const expectedAnnotatedDiffRoute = `${canonicalRevisionPath}/diff${canonicalRevisionSearch ? `?${canonicalRevisionSearch}` : ""}`;
			const canonicalReadingIdentityLocator = page.locator(
				"#detail-body .detail-identity [data-revision-id]",
			);
			const canonicalReadingIdentityCount =
				await canonicalReadingIdentityLocator.count();
			requireCondition(
				canonicalReadingIdentityCount === 1,
				"annotated diff return binding",
				"the accepted exact Revision did not expose one reading identity",
				1,
				canonicalReadingIdentityCount,
			);
			const canonicalReadingIdentity =
				await canonicalReadingIdentityLocator
				.evaluate((identity) => ({
					revisionId: identity.dataset.revisionId ?? null,
					artifactHash: identity.dataset.artifactHash ?? null,
				}));
			requireCondition(
				canonicalReadingIdentity.revisionId ===
					config.fixture.rich.revisionId &&
					canonicalReadingIdentity.artifactHash ===
						config.fixture.rich.artifactHash,
				"annotated diff return binding",
				"the accepted exact Revision had the wrong reading identity",
				{
					revisionId: config.fixture.rich.revisionId,
					artifactHash: config.fixture.rich.artifactHash,
				},
				canonicalReadingIdentity,
			);
			const waitForCanonicalRevisionSurface = async (phase) => {
				let transitionError = null;
				try {
					await waitForCurrentRoute(canonicalRevisionRoute);
					await page.waitForFunction(
						(expectedIdentity) => {
						const diff = document.querySelector("#diff-page");
						const split = document.querySelector(".split");
						const detail = document.querySelector("#detail-body");
						const identity = detail?.querySelector(
							".detail-identity [data-revision-id][data-artifact-hash]",
						);
						if (
							!(diff instanceof HTMLElement) ||
							!(split instanceof HTMLElement) ||
							!(detail instanceof HTMLElement) ||
							!(identity instanceof HTMLElement)
						)
							return false;
						return (
							diff.classList.contains("hidden") &&
							!split.classList.contains("hidden") &&
							Boolean(detail.dataset.changeReadingKey) &&
							identity.dataset.revisionId === expectedIdentity.revisionId &&
							identity.dataset.artifactHash === expectedIdentity.artifactHash
						);
					},
						canonicalReadingIdentity,
					);
				} catch (error) {
					transitionError = error instanceof Error ? error.message : String(error);
				}
				const actual = await page.evaluate(() => {
					const diff = document.querySelector("#diff-page");
					const split = document.querySelector(".split");
					const detail = document.querySelector("#detail-body");
					const identity = detail?.querySelector(
						".detail-identity [data-revision-id][data-artifact-hash]",
					);
					return {
						route: location.hash,
						diffHidden: diff?.classList.contains("hidden") ?? null,
						splitVisible: split
							? !split.classList.contains("hidden")
							: null,
						hasReadingKey: Boolean(detail?.dataset.changeReadingKey),
						revisionId: identity?.dataset.revisionId ?? null,
						artifactHash: identity?.dataset.artifactHash ?? null,
					};
				});
				const routeMatches = await currentRouteMatches(canonicalRevisionRoute);
				requireCondition(
					transitionError === null &&
						routeMatches &&
						actual.diffHidden === true &&
						actual.splitVisible === true &&
						actual.hasReadingKey &&
						actual.revisionId === canonicalReadingIdentity.revisionId &&
						actual.artifactHash === canonicalReadingIdentity.artifactHash,
					`annotated diff ${phase} return`,
					`${phase} did not restore the canonical exact Revision surface${transitionError ? `: ${transitionError}` : ""}`,
					{
						semanticRoute: canonicalRevisionRoute,
						diffHidden: true,
						splitVisible: true,
						hasReadingKey: true,
						...canonicalReadingIdentity,
					},
					{ semanticRouteMatches: routeMatches, ...actual },
				);
			};
			await annotatedDiffOpener.click();
			await page.waitForFunction(
				() =>
					!document.querySelector("#diff-page")?.classList.contains("hidden") &&
					document.querySelector(".split")?.classList.contains("hidden"),
			);
			const annotatedDiffEntryRoute = await hash();
			const annotatedDiffEntryRouteMatches = await currentRouteMatches(
				expectedAnnotatedDiffRoute,
			);
			compare(
				annotatedDiffEntryRouteMatches,
				"annotated diff entry route",
				"annotated diff entry did not retain the complete exact Revision context",
				{ semanticRoute: expectedAnnotatedDiffRoute },
				{
					semanticRouteMatches: annotatedDiffEntryRouteMatches,
					route: annotatedDiffEntryRoute,
				},
			);
			const diffCloseFocusedOnEntry = await page
				.locator("#diff-page-close")
				.evaluate((node) => document.activeElement === node);
			compare(
				diffCloseFocusedOnEntry,
				"annotated diff entry focus",
				"entering the full-frame diff did not move focus to its explicit return action",
				true,
				diffCloseFocusedOnEntry,
			);
			const diffMetrics = await page.locator("#diff-page").evaluate((diff) => ({
				headingCount: diff.querySelectorAll("h1").length,
				title: diff.querySelector("#diff-page-title")?.textContent || "",
				titleSource:
					diff.querySelector("#diff-page-title")?.getAttribute("title") || "",
				keyHints:
					diff
						.querySelector(".diff-page-keys")
						?.textContent?.replace(/\s+/g, " ")
						.trim() || "",
				files: diff.querySelectorAll("#diff-page-body .dfile").length,
				facts: Array.from(
					diff.querySelectorAll("#diff-page-body [data-anno]"),
					(item) => item.dataset.anno || "",
				).filter(
					(id, index, values) => id.length > 0 && values.indexOf(id) === index,
				),
				splitVisible: !document
					.querySelector(".split")
					?.classList.contains("hidden"),
			}));
			compare(
				diffMetrics.headingCount === 1 && !diffMetrics.splitVisible,
				"first-class annotated diff",
				"full-frame diff did not preserve its sole heading and full-frame state",
				{ headingCount: 1, splitVisible: false },
				{
					headingCount: diffMetrics.headingCount,
					splitVisible: diffMetrics.splitVisible,
				},
			);
			requireCondition(
				diffMetrics.title.length > 0 &&
					diffMetrics.titleSource.includes(config.fixture.rich.revisionId) &&
					diffMetrics.titleSource.includes(config.fixture.rich.artifactHash) &&
					diffMetrics.keyHints.includes("[") &&
					diffMetrics.keyHints.includes("]") &&
					diffMetrics.keyHints.includes("p") &&
					diffMetrics.keyHints.includes("n") &&
					diffMetrics.files >= 1 &&
					diffMetrics.facts.length >= 1,
				"first-class annotated diff",
				`full-frame diff omitted captured files, inline facts, exact identity, or key hints: ${JSON.stringify(diffMetrics)}`,
				{
					title: "nonempty",
					titleSourceIncludes: [
						config.fixture.rich.revisionId,
						config.fixture.rich.artifactHash,
					],
					keyHints: ["[", "]", "p", "n"],
					minimumFiles: 1,
					minimumFacts: 1,
				},
				diffMetrics,
			);
			const diffFilePaths = await page
				.locator("#diff-page-body .dfile")
				.evaluateAll((files) =>
					files.map(
						(file) =>
							file.dataset.filePath ||
							file.dataset.newFilePath ||
							file.dataset.oldFilePath ||
							"",
					),
				);
			requireCondition(
				diffFilePaths.every((path) => path.length > 0),
				"annotated diff file navigation",
				"a full-frame diff file had no routeable path",
				"every file path is nonempty",
				diffFilePaths,
			);
			const diffBody = page.locator("#diff-page-body");
			await diffBody.locator(".dfile").first().focus();
			await page.keyboard.press("]");
			await page.waitForFunction(
				(filePath) =>
					new URLSearchParams(location.hash.split("?", 2)[1] ?? "").get(
						"file",
					) === filePath,
				diffFilePaths[0],
			);
			await waitForRoutedDiffFocus("file", diffFilePaths[0]);
			const focusedDiffFile = await page
				.locator("#diff-page-body .dfile")
				.first()
				.getAttribute("data-exact-focus");
			compare(
				focusedDiffFile === "true",
				"annotated diff ] file key",
				"the first file key did not focus the routed file section",
				"true",
				focusedDiffFile,
			);
			await page.keyboard.press("[");
			const routedDiffFile = await routeParameter("file");
			compare(
				routedDiffFile === diffFilePaths[0],
				"annotated diff [ file key",
				"the backward file key did not retain the first-file boundary",
				diffFilePaths[0],
				routedDiffFile,
			);
			const firstDiffFact = diffMetrics.facts[0];
			await page.keyboard.press("n");
			await page.waitForFunction(
				(factId) =>
					new URLSearchParams(location.hash.split("?", 2)[1] ?? "").get(
						"fact",
					) === factId,
				firstDiffFact,
			);
			await waitForRoutedDiffFocus("fact", firstDiffFact);
			const focusedDiffFact = await page
				.locator(`#diff-page-body [data-anno]`)
				.evaluateAll(
					(nodes, factId) =>
						nodes.some(
							(node) =>
								node.dataset.anno === factId &&
								node.dataset.exactFocus === "true",
						),
					firstDiffFact,
				);
			compare(
				focusedDiffFact,
				"annotated diff n fact key",
				"the first fact key did not focus an inlined fact",
				true,
				focusedDiffFact,
			);
			await page.keyboard.press("p");
			const routedDiffFact = await routeParameter("fact");
			compare(
				routedDiffFact === firstDiffFact,
				"annotated diff p fact key",
				"the backward fact key did not retain the first-fact boundary",
				firstDiffFact,
				routedDiffFact,
			);
			const focusedDiffRoute = await hash();
			await screenshot("wide-annotated-diff-full-frame");
			await page.reload({ waitUntil: "domcontentloaded" });
			await waitForCurrentRoute(focusedDiffRoute);
			await page.waitForFunction(
				() =>
					!document.querySelector("#diff-page")?.classList.contains("hidden") &&
					document.querySelectorAll("#diff-page-body .dfile").length > 0,
			);
			const diffCloseFocusedOnReload = await page
				.locator("#diff-page-close")
				.evaluate((node) => document.activeElement === node);
			compare(
				diffCloseFocusedOnReload,
				"annotated diff reload focus",
				"reload did not restore focus to the diff return action",
				true,
				diffCloseFocusedOnReload,
			);
			await page.locator("#diff-page-close").click();
			await waitForCanonicalRevisionSurface("Close");
			const detailCloseFocusedOnDiffReturn = await page
				.locator("#detail-close")
				.evaluate((node) => document.activeElement === node);
			compare(
				detailCloseFocusedOnDiffReturn,
				"annotated diff return focus",
				"returning from the full-frame diff did not restore detail focus",
				true,
				detailCloseFocusedOnDiffReturn,
			);
			await page.goBack();
			await waitForCurrentRoute(focusedDiffRoute);
			await page.waitForFunction(
				() =>
					!document.querySelector("#diff-page")?.classList.contains("hidden"),
			);
			await page.goForward();
			await waitForCanonicalRevisionSurface("Forward");
		},
		teardown: teardownSection,
	});

	await diagnostics.section("Exact detail and reading", {
		setup: () =>
			open(exact, layouts[1], "Exact detail and reading section setup"),
		run: async () => {
			await open(exact, layouts[1], "narrow exact revision");
			await page.waitForFunction(() =>
				Boolean(
					document.querySelector("#detail-body")?.dataset.changeReadingKey,
				),
			);
			expect(
				await page
					.locator("#detail")
					.evaluate((node) => !node.inert && !node.hasAttribute("aria-hidden")),
				"narrow exact revision",
				"open detail remained inert or hidden",
			);
			const narrowExactActivation = page.locator(
				"#detail-body [data-exact-diff-activation]",
			);
			const narrowExactActivationCount = await narrowExactActivation.count();
			requireCondition(
				narrowExactActivationCount === 1,
				"narrow detail focus",
				"narrow exact detail did not expose one primary annotated diff action",
				1,
				narrowExactActivationCount,
			);
			compare(
				await narrowExactActivation.evaluate(
					(node) => document.activeElement === node,
				),
				"narrow detail focus",
				"opening the narrow exact detail did not focus its primary annotated diff action",
				"#detail-body [data-exact-diff-activation]",
				await page.evaluate(() => {
					const active = document.activeElement;
					return active instanceof HTMLElement
						? active.id || active.dataset.exactDiffActivation || active.tagName
						: null;
				}),
			);
			const narrowExactIdentity = page.locator(
				"#detail-body .detail-identity code",
			);
			const narrowExactIdentityCount = await narrowExactIdentity.count();
			requireCondition(
				narrowExactIdentityCount === 1,
				"narrow exact revision",
				"exact Revision identity was not rendered once",
				1,
				narrowExactIdentityCount,
			);
			const narrowExactPresentation = {
				visible: await narrowExactIdentity.textContent(),
				title: await narrowExactIdentity.getAttribute("title"),
				name: await narrowExactIdentity.getAttribute("aria-label"),
			};
			const narrowExactFullIdentity = `exact Revision ${config.fixture.rich.revisionId}; artifact ${config.fixture.rich.artifactHash}`;
			const expectedNarrowExactPresentation = {
				visible: shortExact(
					config.fixture.rich.revisionId,
					config.fixture.rich.artifactHash,
				),
				title: narrowExactFullIdentity,
				name: narrowExactFullIdentity,
			};
			compare(
				JSON.stringify(narrowExactPresentation) ===
					JSON.stringify(expectedNarrowExactPresentation),
				"narrow exact revision",
				"exact Revision did not pair its shortened visible identity with complete title and accessible identity",
				expectedNarrowExactPresentation,
				narrowExactPresentation,
			);
			const narrowExactRoute = await hash();
			const narrowExactRouteMatches = await currentRouteMatches(`#/${exact}`);
			compare(
				narrowExactRouteMatches,
				"narrow exact revision",
				"exact Change, Revision, or artifact identity was lost from the route",
				{ semanticRoute: `#/${exact}` },
				{ semanticRouteMatches: narrowExactRouteMatches, route: narrowExactRoute },
			);
			const exactText = await page.locator("body").innerText();
			for (const expected of [
				"Matrix fact",
				"Open decision",
				"passed current",
				"Association comparisons",
			]) {
				expect(
					exactText.includes(expected),
					"narrow rich revision",
					`missing representative detail: ${expected}`,
				);
			}
			const narrowFactGraphViewport = page.locator(
				"#detail-body .fact-relationship-graph [data-graph-viewport]",
			);
			const narrowGraphGeometry = await narrowFactGraphViewport.evaluate(
				(viewport) => ({
					clientWidth: viewport.clientWidth,
					scrollWidth: viewport.scrollWidth,
					svgWidth:
						viewport.querySelector("svg")?.getBoundingClientRect().width || 0,
				}),
			);
			compare(
				narrowGraphGeometry.clientWidth > 0 &&
					narrowGraphGeometry.scrollWidth > narrowGraphGeometry.clientWidth &&
					narrowGraphGeometry.svgWidth > narrowGraphGeometry.clientWidth,
				"narrow intrinsic graph viewport",
				`fact graph was compressed instead of pannable: ${JSON.stringify(narrowGraphGeometry)}`,
				{
					clientWidth: "> 0",
					scrollWidth: "> clientWidth",
					svgWidth: "> clientWidth",
				},
				narrowGraphGeometry,
			);
			await narrowFactGraphViewport.focus();
			await page.keyboard.press("End");
			const graphEnd = await narrowFactGraphViewport.evaluate(
				(viewport) => viewport.scrollLeft,
			);
			await page.keyboard.press("Home");
			const graphHome = await narrowFactGraphViewport.evaluate(
				(viewport) => viewport.scrollLeft,
			);
			compare(
				graphEnd ===
					narrowGraphGeometry.scrollWidth - narrowGraphGeometry.clientWidth &&
					graphHome === 0,
				"narrow graph keyboard panning",
				`Home/End panning produced ${graphHome}/${graphEnd} for ${JSON.stringify(narrowGraphGeometry)}`,
				{
					home: 0,
					end:
						narrowGraphGeometry.scrollWidth - narrowGraphGeometry.clientWidth,
				},
				{ home: graphHome, end: graphEnd },
			);
			await screenshot("narrow-exact-detail");
			await page.locator("#detail-body").evaluate((node) => {
				node.scrollTop = node.scrollHeight;
			});
			const narrowBackBounds = await page.evaluate(() => {
				const detail = document
					.querySelector("#detail")
					?.getBoundingClientRect();
				const back = document
					.querySelector("#detail-back")
					?.getBoundingClientRect();
				return detail && back
					? {
							detailTop: detail.top,
							detailBottom: detail.bottom,
							backTop: back.top,
							backBottom: back.bottom,
						}
					: null;
			});
			compare(
				narrowBackBounds !== null &&
					narrowBackBounds.backTop >= narrowBackBounds.detailTop &&
					narrowBackBounds.backBottom <= narrowBackBounds.detailBottom,
				"narrow persistent return",
				`Back control left the detail viewport: ${JSON.stringify(narrowBackBounds)}`,
				{ backTop: ">= detailTop", backBottom: "<= detailBottom" },
				narrowBackBounds,
			);
			await page.locator("#detail-back").click();
			await page.waitForFunction(() => location.hash.startsWith("#/changes?"));
			const narrowDetailClosed = await page
				.locator(".split")
				.evaluate((node) => node.classList.contains("split-closed"));
			compare(
				narrowDetailClosed,
				"narrow detail return",
				"Back did not close the narrow detail sheet",
				true,
				narrowDetailClosed,
			);
			expect(
				await page
					.locator("#detail")
					.evaluate(
						(node) => node.inert && node.getAttribute("aria-hidden") === "true",
					),
				"narrow detail return",
				"closed detail remained exposed to keyboard or assistive navigation",
			);
			expect(
				await page.evaluate(() => {
					const master = document.querySelector("#master");
					return (
						document.activeElement === master ||
						Boolean(master?.contains(document.activeElement))
					);
				}),
				"narrow detail focus restoration",
				"closing the narrow detail did not restore focus to the retained list surface",
			);

			await open(exact, layouts[0], "wide exact revision");
			await page.waitForFunction(() =>
				Boolean(
					document.querySelector("#detail-body")?.dataset.changeReadingKey,
				),
			);
			const detailViewport = page.locator("#detail-body");
			const detailScrollMetrics = await detailViewport.evaluate((node) => ({
				clientHeight: node.clientHeight,
				scrollHeight: node.scrollHeight,
			}));
			compare(
				detailScrollMetrics.scrollHeight > detailScrollMetrics.clientHeight,
				"reading scroll",
				"rich exact detail did not produce a real scroll range",
				{ scrollHeight: "> clientHeight" },
				detailScrollMetrics,
			);
			const readingToggle = page.locator("#detail-read");
			await readingToggle.focus();
			await detailViewport.evaluate((node) => {
				node.scrollTop = Math.min(80, node.scrollHeight - node.clientHeight);
			});
			const beforeReadingScroll = await detailViewport.evaluate(
				(node) => node.scrollTop,
			);
			compare(
				beforeReadingScroll > 0,
				"reading scroll",
				"failed to establish a non-zero detail scroll position",
				"> 0",
				beforeReadingScroll,
			);
			await page.keyboard.press("Enter");
			const readingModeActive = await page
				.locator(".split")
				.evaluate((node) => node.classList.contains("reading"));
			compare(
				readingModeActive,
				"reading mode",
				"reading mode was not entered",
				true,
				readingModeActive,
			);
			const scrollAfterReadingMode = await detailViewport.evaluate(
				(node) => node.scrollTop,
			);
			compare(
				scrollAfterReadingMode === beforeReadingScroll,
				"reading scroll",
				"reading mode lost detail scroll position",
				beforeReadingScroll,
				scrollAfterReadingMode,
			);
			const wideHeaderBounds = await page.evaluate(() => {
				const detail = document
					.querySelector("#detail")
					?.getBoundingClientRect();
				const close = document
					.querySelector("#detail-close")
					?.getBoundingClientRect();
				return detail && close
					? {
							detailTop: detail.top,
							detailBottom: detail.bottom,
							closeTop: close.top,
							closeBottom: close.bottom,
						}
					: null;
			});
			compare(
				wideHeaderBounds !== null &&
					wideHeaderBounds.closeTop >= wideHeaderBounds.detailTop &&
					wideHeaderBounds.closeBottom <= wideHeaderBounds.detailBottom,
				"reading persistent controls",
				`detail controls left the reading viewport: ${JSON.stringify(wideHeaderBounds)}`,
				{ closeTop: ">= detailTop", closeBottom: "<= detailBottom" },
				wideHeaderBounds,
			);
			await page.locator("#master-rail").click();
			const readingModeAfterReturn = await page
				.locator(".split")
				.evaluate((node) => node.classList.contains("reading"));
			compare(
				!readingModeAfterReturn,
				"reading return path",
				"master rail did not restore split mode",
				false,
				readingModeAfterReturn,
			);
			await screenshot("wide-exact-reading");
		},
		teardown: teardownSection,
	});

	const resourceCases = [
		["removed", config.fixture.removed, "captured_resource_removed"],
		["missing", config.fixture.missing, "captured_resource_missing"],
	];
	await diagnostics.section("Exact resource availability", {
		setup: () => {
			const fixture = config.fixture.removed;
			const route = `changes/${encodeURIComponent(fixture.changeId)}/revisions/${encodeURIComponent(fixture.revisionId)}/resource?artifactHash=${encodeURIComponent(fixture.artifactHash)}`;
			return open(route, layouts[0], "exact resource section setup");
		},
		run: async () => {
			for (const layout of layouts) {
				for (const [availability, fixture, diagnostic] of resourceCases) {
					const resourceRoute = `changes/${encodeURIComponent(fixture.changeId)}/revisions/${encodeURIComponent(fixture.revisionId)}/resource?artifactHash=${encodeURIComponent(fixture.artifactHash)}`;
					await open(
						resourceRoute,
						layout,
						`${layout.name} ${availability} resource`,
					);
					await page.waitForFunction(
						({ expectedAvailability, expectedDiagnostic }) => {
							const text =
								document.querySelector("#detail-body")?.textContent ?? "";
							return (
								text.includes(`availability: ${expectedAvailability}`) &&
								text.includes(expectedDiagnostic)
							);
						},
						{
							expectedAvailability: availability,
							expectedDiagnostic: diagnostic,
						},
					);
					const resourceText = await page.locator("#detail-body").innerText();
					expect(
						resourceText.includes(`availability: ${availability}`),
						`${layout.name} ${availability} resource`,
						`exact availability was not ${availability}`,
					);
					expect(
						resourceText.includes(diagnostic),
						`${layout.name} ${availability} resource`,
						`missing exact diagnostic ${diagnostic}`,
					);
					expect(
						resourceText.includes(
							"Captured bytes are unavailable. No live or associated-commit bytes were substituted.",
						),
						`${layout.name} ${availability} resource`,
						"bodyless exact resource did not state its non-substitution guarantee",
					);
					expect(
						!resourceText.includes("captured document:"),
						`${layout.name} ${availability} resource`,
						"bodyless exact resource exposed a captured-document hash",
					);
					const capturedDiffCount = await page
						.locator("#detail-body .captured-diff")
						.count();
					compare(
						capturedDiffCount === 0,
						`${layout.name} ${availability} resource`,
						"bodyless exact resource rendered a captured or substituted diff",
						0,
						capturedDiffCount,
					);
					const detailOverflow = await page
						.locator("#detail-body")
						.evaluate((node) => ({
							clientWidth: node.clientWidth,
							scrollWidth: node.scrollWidth,
						}));
					compare(
						detailOverflow.scrollWidth <= detailOverflow.clientWidth,
						`${layout.name} ${availability} resource`,
						`exact identity overflowed detail width ${detailOverflow.scrollWidth}/${detailOverflow.clientWidth}`,
						{ scrollWidth: "<= clientWidth" },
						detailOverflow,
					);
					await screenshot(`${layout.name}-${availability}-resource`);
				}
			}
		},
		teardown: teardownSection,
	});

	await diagnostics.section("Polling retention and reduced motion", {
		setup: async () => {
			await page.emulateMedia({ reducedMotion: "reduce" });
			return open(
				"changes?limit=100&order=change_id_asc",
				layouts[0],
				"wide reduced motion",
			);
		},
		run: async () => {
			await page
				.locator(".unit-card[data-change-id]")
				.first()
				.evaluate((node) => {
					node.dataset.browserRetention = "same-generation";
				});
			await page.waitForTimeout(3500);
			const retainedCardCount = await page
				.locator('.unit-card[data-browser-retention="same-generation"]')
				.count();
			compare(
				retainedCardCount === 1,
				"same-generation DOM retention",
				"polling repainted an unchanged Change generation",
				1,
				retainedCardCount,
			);
			const semanticChangeSurface = await page.waitForFunction(() => {
				const master = document.querySelector(
					"#master[data-change-list-key]",
				);
				if (
					!master ||
					master.textContent?.includes("Loading Change generation")
				)
					return false;
				const cards = Array.from(
					master.querySelectorAll(".unit-card[data-change-id]"),
				);
				return (
					cards.length > 0 &&
					cards.every((card) => {
						const primary = card.querySelector(".change-card-primary");
						if (!(primary instanceof HTMLElement)) return false;
						const style = getComputedStyle(primary);
						const bounds = primary.getBoundingClientRect();
						return (
							(card.getAttribute("data-change-id") || "").trim().length > 0 &&
							primary.innerText.trim().length > 0 &&
							style.display !== "none" &&
							style.visibility === "visible" &&
							style.opacity !== "0" &&
							bounds.width > 0 &&
							bounds.height > 0
						);
					})
				);
			});
			const semanticChangeSurfaceReady =
				await semanticChangeSurface.jsonValue();
			compare(
				semanticChangeSurfaceReady === true,
				"reduced-motion semantic Change paint",
				"retained Changes were not fully painted before visual capture",
				true,
				semanticChangeSurfaceReady,
			);
			await page.evaluate(
				() =>
					new Promise((resolve) => {
						requestAnimationFrame(() => requestAnimationFrame(resolve));
					}),
			);
			const reducedMotion = await page.evaluate(() => {
				const detail = document.querySelector("#detail");
				const live = document.querySelector("#refresh");
				if (live) live.dataset.state = "degraded";
				return {
					mediaMatches: matchMedia("(prefers-reduced-motion: reduce)").matches,
					detailTransitionDuration: detail
						? getComputedStyle(detail).transitionDuration
						: null,
					liveAnimationName: live ? getComputedStyle(live).animationName : null,
				};
			});
			compare(
				reducedMotion.mediaMatches,
				"reduced motion",
				"media emulation did not apply",
				true,
				reducedMotion.mediaMatches,
			);
			compare(
				reducedMotion.detailTransitionDuration === "0s",
				"reduced motion",
				`detail transition remained ${reducedMotion.detailTransitionDuration}`,
				"0s",
				reducedMotion.detailTransitionDuration,
			);
			compare(
				reducedMotion.liveAnimationName === "none",
				"reduced motion",
				`status animation remained ${reducedMotion.liveAnimationName}`,
				"none",
				reducedMotion.liveAnimationName,
			);
			await screenshot("wide-reduced-motion");

			await open(
				"timeline?limit=100&order=desc",
				layouts[1],
				"narrow reduced motion",
			);
			const narrowReducedMotionDuration = await page
				.locator("#detail")
				.evaluate((detail) => getComputedStyle(detail).transitionDuration);
			compare(
				narrowReducedMotionDuration === "0s",
				"narrow reduced motion",
				`narrow detail sheet transition remained ${narrowReducedMotionDuration}`,
				"0s",
				narrowReducedMotionDuration,
			);
			await screenshot("narrow-reduced-motion");
		},
		teardown: async () => {
			await page.emulateMedia({ reducedMotion: "no-preference" });
			await teardownSection();
		},
	});

	await settleResponseInspections();
	const deliberateTransitionResponses = serviceUnavailableResponses.filter(
		(response) =>
			isDeliberateChangeProjectionTransition(response, config.server.baseUrl),
	);
	const transitionResponsesByUrl = new Map();
	for (const response of deliberateTransitionResponses) {
		transitionResponsesByUrl.set(
			response.url,
			(transitionResponsesByUrl.get(response.url) ?? 0) + 1,
		);
	}
	const genericServiceUnavailable =
		"Failed to load resource: the server responded with a status of 503 (Service Unavailable)";
	const unexpectedConsoleErrors = consoleErrors.filter((error) => {
		if (
			error.text !== genericServiceUnavailable ||
			!error.insideAppendWindow ||
			error.url === null
		)
			return true;
		const remaining = transitionResponsesByUrl.get(error.url) ?? 0;
		if (remaining === 0) return true;
		transitionResponsesByUrl.set(error.url, remaining - 1);
		return false;
	});
	const unexpectedServiceUnavailableResponses =
		serviceUnavailableResponses.filter(
			(response) =>
				!isDeliberateChangeProjectionTransition(
					response,
					config.server.baseUrl,
				),
		);

	await diagnostics.section("Browser runtime", async () => {
		expect(
			unexpectedConsoleErrors.length === 0 &&
				unexpectedServiceUnavailableResponses.length === 0,
			"browser console",
			[
				...unexpectedConsoleErrors.map(
					(error) => `${error.text}${error.url ? ` @ ${error.url}` : ""}`,
				),
				...unexpectedServiceUnavailableResponses.map(
					(response) =>
						`${response.status} ${response.url}: ${JSON.stringify(response.body)}`,
				),
			].join("\n"),
			{
				expected: [],
				actual: {
					console: unexpectedConsoleErrors,
					responses: unexpectedServiceUnavailableResponses,
				},
			},
		);
		expect(pageErrors.length === 0, "browser page", pageErrors.join("\n"), {
			expected: [],
			actual: pageErrors,
		});
		expect(
			requestFailures.length === 0,
			"browser requests",
			requestFailures
				.map(
					(failure) =>
						`${failure.method} ${failure.url}: ${failure.error}`,
				)
				.join("\n"),
			{
				expected: [],
				actual: requestFailures,
			},
		);
	});

	const completion = diagnostics.result({ screenshotCount: screenshots });
	console.log(`POINTBREAK_BROWSER_RESULT=${JSON.stringify(completion)}`);
	return completion;
})(__POINTBREAK_CHANGE_BROWSER_CONFIG__)
