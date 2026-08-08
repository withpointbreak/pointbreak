// Internal browser program injected by change-inspector-browser-verify.sh.
// It only reads the disposable Inspector page and writes screenshots under its configured root.
((config) => async (page) => {
  let assertions = 0;
  let screenshots = 0;
  const consoleErrors = [];
  const pageErrors = [];
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text());
  });
  page.on("pageerror", (error) => pageErrors.push(error.message));

  const layouts = [
    { name: "wide", width: 1440, height: 1000 },
    { name: "narrow", width: 390, height: 844 },
  ];
  const fail = (label, detail) => { throw new Error(`${label}: ${detail}`); };
  const expect = (condition, label, detail) => {
    assertions += 1;
    if (!condition) fail(label, detail);
  };
  // The launcher has already moved the one-time fragment capability into
  // origin-scoped sessionStorage. Route changes are same-document navigation
  // and therefore use only the strict, shareable Change route grammar.
  const url = (route) => `${config.server.baseUrl}/#/${route}`;
  const screenshot = async (name) => {
    screenshots += 1;
    await page.screenshot({ path: `${config.artifactDir}/${name}.png`, type: "png", fullPage: false });
  };
  const open = async (route, layout, label) => {
    await page.setViewportSize({ width: layout.width, height: layout.height });
    await page.goto(url(route), { waitUntil: "domcontentloaded" });
    await page.waitForFunction(() => document.querySelector("#connection-status")?.textContent === "connected");
    await page.waitForFunction((expectedRoute) => {
      const stamp = document.querySelector("#stat-hash")?.textContent?.trim();
      if (!stamp || stamp === "—" || !document.querySelector("#master h1")) return false;
      const [path, query = ""] = expectedRoute.split("?", 2);
      const expectedLens = path.split("/", 1)[0] || "timeline";
      if (expectedLens === "timeline") {
        const expectedAfter = new URLSearchParams(query).get("after");
        const current = new URLSearchParams(location.hash.split("?", 2)[1] ?? "");
        return Boolean(document.querySelector("#timeline"))
          && current.get("after") === expectedAfter;
      }
      const rawKey = document.querySelector("#master")?.dataset.changeListKey;
      if (!rawKey) return false;
      try {
        const key = JSON.parse(rawKey);
        const expectedAfter = new URLSearchParams(query).get("after");
        return key.lens === expectedLens && (key.query?.after ?? null) === expectedAfter;
      } catch {
        return false;
      }
    }, route);
    expect(
      !(await page.evaluate(() => location.hash)).includes("token="),
      label,
      "capability leaked into the semantic route",
    );
    const metrics = await page.evaluate(() => ({
      width: document.documentElement.clientWidth,
      scrollWidth: document.documentElement.scrollWidth,
      liveCards: document.querySelectorAll(".unit-card[data-change-id]").length,
      liveEvents: document.querySelectorAll("#timeline [data-event-id]").length,
    }));
    expect(metrics.width === layout.width, label, `unexpected viewport width ${metrics.width}`);
    expect(metrics.scrollWidth <= metrics.width, label, `horizontal overflow ${metrics.scrollWidth}/${metrics.width}`);
    return metrics;
  };
  const hash = () => page.evaluate(() => location.hash);
  const waitForLens = (lens) => page.waitForFunction((expectedLens) => {
    if (expectedLens === "timeline") return Boolean(document.querySelector("#timeline"));
    const rawKey = document.querySelector("#master")?.dataset.changeListKey;
    if (!rawKey) return false;
    try {
      return JSON.parse(rawKey).lens === expectedLens;
    } catch {
      return false;
    }
  }, lens);
  const selected = () => page.locator(".unit-card.change-card-selected[data-change-id]");
  const cardNamesAreUseful = () => page.evaluate(() =>
    Array.from(document.querySelectorAll(".unit-card[data-change-id]")).every((card) => {
      const name = card.getAttribute("aria-label") || "";
      const exact = card.dataset.changeId || "";
      const peers = Array.from(card.querySelectorAll(".change-card-peer-open"));
      const exactPeersAreNamed = peers.every((peer) => {
        const [revisionId, artifactHash] = (peer.getAttribute("title") || "").split(" ");
        const peerName = peer.getAttribute("aria-label") || "";
        const copyName = peer.parentElement?.querySelector("button:last-child")?.getAttribute("aria-label") || "";
        return Boolean(
          revisionId && artifactHash
          && name.includes(revisionId) && name.includes(artifactHash)
          && peerName.includes(revisionId) && peerName.includes(artifactHash)
          && copyName.includes(revisionId) && copyName.includes(artifactHash)
        );
      });
      return name.includes(exact)
        && name.length > exact.length
        && !/^Change\s+[^\s]+$/.test(name)
        && exactPeersAreNamed;
    }),
  );
  const noHiddenTabStops = () => page.evaluate(() => {
    const selector = "a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])";
    return Array.from(document.querySelectorAll(selector))
      .filter((node) => node.closest(".hidden, [hidden], [aria-hidden='true'], [inert]"))
      .every((node) => {
        if (node.closest("[inert]")) return true;
        const style = getComputedStyle(node);
        return node.getClientRects().length === 0 || style.display === "none" || style.visibility === "hidden";
      });
  });

  const bootstrapUrl = (server) =>
    `${server.baseUrl}/#/?token=${encodeURIComponent(server.token)}`;
  const exerciseReaderState = async (server, label, expectedText, expectHistory) => {
    const origin = new URL(server.baseUrl).origin;
    const historyRequests = [];
    const recordRequest = (request) => {
      const requested = new URL(request.url());
      if (requested.origin === origin && requested.pathname === "/api/v2/history") {
        historyRequests.push(request.url());
      }
    };
    page.on("request", recordRequest);
    try {
      await page.goto(bootstrapUrl(server), { waitUntil: "domcontentloaded" });
      await page.waitForFunction(
        (text) => document.querySelector("#master")?.textContent?.includes(text),
        expectedText,
      );
      expect(!(await hash()).includes("token="), label, "capability remained in the semantic route");
      expect(
        expectHistory ? historyRequests.length > 0 : historyRequests.length === 0,
        label,
        expectHistory
          ? "ready empty L2 never requested authoritative history"
          : `unready profile leaked ${historyRequests.length} history request(s)`,
      );
      await screenshot(`reader-${label}`);
    } finally {
      page.off("request", recordRequest);
    }
  };

  // Readiness sequencing is exercised against real tiny Inspector servers.
  // Unready profiles must stop before history; only the complete empty L2 root
  // is allowed to request and paint a zero-event Timeline.
  await exerciseReaderState(
    config.readerServers.emptyReadyL2,
    "empty-ready-l2",
    "0 recorded events",
    true,
  );
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
    const requested = new URL(request.url());
    if (
      requested.origin === new URL(config.server.baseUrl).origin
      && requested.pathname === "/api/v2/history"
    ) mismatchHistoryRequests += 1;
  };
  await page.route("**/api/v2/profile", (route) => route.fulfill({
    status: 200,
    contentType: "application/json",
    body: JSON.stringify({ schema: "pointbreak.inspect-reader-profile", version: 999 }),
  }));
  page.on("request", recordMismatchRequest);
  try {
    await page.goto(url(""), { waitUntil: "domcontentloaded" });
    await page.waitForFunction(() =>
      document.querySelector("#master")?.textContent?.includes(
        "Reader refused: incompatible Inspector reader profile",
      ));
    expect(mismatchHistoryRequests === 0, "profile mismatch", `profile refusal leaked ${mismatchHistoryRequests} history request(s)`);
    await screenshot("reader-profile-mismatch");
  } finally {
    page.off("request", recordMismatchRequest);
    await page.unroute("**/api/v2/profile");
  }

  // The monitor is the default reader, including a capability-bearing startup
  // URL whose token must never survive in the semantic fragment.  Its entries
  // are virtualized; these checks deliberately assert a bounded live DOM, not
  // the total authoritative event count.
  const defaultTimeline = await open("", layouts[0], "default Timeline startup");
  expect(defaultTimeline.liveEvents > 0 && defaultTimeline.liveEvents < 80, "default Timeline startup", `expected a bounded live Timeline window, saw ${defaultTimeline.liveEvents}`);
  const initialTimelineText = await page.locator("#master").innerText();
  const recordedEvents = Number((initialTimelineText.match(/([0-9]+) recorded events/) || [])[1]);
  expect(recordedEvents >= 300, "default Timeline startup", `expected 300+ recorded public events, saw ${recordedEvents}`);
  expect(initialTimelineText.includes("Newest first"), "descending chronology", "default Timeline did not declare newest-first chronology");
  expect(await page.locator("#timeline [data-event-id]").evaluateAll((rows) => rows.every((row) => {
    const name = row.getAttribute("aria-label") || "";
    return name.includes("event ") && name.length > 24;
  })), "readable Timeline rows", "a live Timeline row lacked an accessible event identity");
  const initialTimelineIds = await page.locator("#timeline [data-event-id]").evaluateAll((rows) => rows.map((row) => row.dataset.eventId));
  await screenshot("wide-timeline-default");

  // Chronology is server-owned and continuations are opaque/signed.  Exercise
  // both directions through the UI rather than constructing a token here.
  const timelineKey = await page.locator("#master").getAttribute("data-timeline-key");
  await page.getByRole("button", { name: "Next page" }).click();
  await page.waitForFunction((key) => document.querySelector("#master")?.dataset.timelineKey !== key, timelineKey);
  expect((await hash()).includes("after="), "signed Timeline continuation", "next Timeline page did not preserve a continuation in the route");
  const staleTimelineRoute = (await hash()).replace(/^#\//, "");
  expect(await page.locator("#timeline [data-event-id]").count() > 0, "signed Timeline continuation", "next Timeline page was empty");
  const middlePageAnchor = await page.locator("#timeline [data-event-id]").first().getAttribute("data-event-id");
  expect(Boolean(middlePageAnchor), "anchored Timeline paging", "middle Timeline page had no exact event anchor");
  await page.getByRole("button", { name: "Previous page" }).click();
  await page.waitForFunction((ids) => JSON.stringify(Array.from(document.querySelectorAll("#timeline [data-event-id]"), (row) => row.dataset.eventId)) === JSON.stringify(ids), initialTimelineIds);
  expect(JSON.stringify(await page.locator("#timeline [data-event-id]").evaluateAll((rows) => rows.map((row) => row.dataset.eventId))) === JSON.stringify(initialTimelineIds), "opaque Timeline continuation", "Previous did not restore the original Timeline page identity");
  const anchoredTimelineRoute = `timeline?limit=100&order=desc&at=${encodeURIComponent(middlePageAnchor)}`;
  const followAnchoredNeighbor = async (name) => {
    await open(anchoredTimelineRoute, layouts[0], `anchored Timeline ${name.toLowerCase()}`);
    expect(
      await page.getByRole("button", { name: "Previous page" }).count() === 1
        && await page.getByRole("button", { name: "Next page" }).count() === 1,
      "anchored Timeline paging",
      "middle anchored page did not expose both adjacent signed continuations",
    );
    const before = await hash();
    await page.getByRole("button", { name }).click();
    await page.waitForFunction((prior) => {
      const query = new URLSearchParams(location.hash.split("?", 2)[1] ?? "");
      return location.hash !== prior && query.has("after") && !query.has("at");
    }, before);
    expect(
      !((await page.locator("#master").innerText()).includes("Reader refused")),
      "anchored Timeline paging",
      `${name} retained the mutually exclusive at locator`,
    );
  };
  await followAnchoredNeighbor("Next page");
  await followAnchoredNeighbor("Previous page");
  await open("timeline?limit=100&order=asc", layouts[0], "ascending Timeline");
  const ascendingText = await page.locator("#master").innerText();
  expect(ascendingText.includes("Oldest first"), "ascending chronology", "ascending Timeline did not declare oldest-first chronology");
  await screenshot("wide-timeline-ascending");

  // Drive all typed filters through their reader controls. The browser does
  // not invent query values: Track, Change, and exact Revision options are
  // populated from the server's admitted completion facets.
  const applyTimelineFilter = async (id, value, key, label, expectedQuery) => {
    await open("timeline?limit=100&order=desc", layouts[0], `${label} base`);
    const filtersToggle = page.locator("#filters-toggle");
    if (await filtersToggle.getAttribute("aria-expanded") !== "true") {
      await filtersToggle.click();
    }
    const select = page.locator(`#${id}`);
    await select.selectOption(value);
    await page.waitForFunction(({ expectedKey, expected }) => {
      const query = new URLSearchParams(location.hash.split("?", 2)[1] ?? "");
      return query.has(expectedKey)
        && Object.entries(expected).every(([name, expectedValue]) =>
          query.get(name) === expectedValue);
    }, { expectedKey: key, expected: expectedQuery });
    expect(await page.locator("#timeline [data-event-id]").count() > 0, label, "typed public fixture filter produced no Timeline entry");
    const remove = page.getByRole("button", { name: new RegExp(`^Remove ${key} filter:`) });
    expect(await remove.count() === 1, label, "typed filter did not create one removable chip");
    await remove.click();
    await page.waitForFunction((expectedKey) => {
      const query = new URLSearchParams(location.hash.split("?", 2)[1] ?? "");
      return !query.has(expectedKey)
        && (expectedKey !== "revision" || !query.has("artifactHash"));
    }, key);
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
    JSON.stringify([config.fixture.rich.revisionId, config.fixture.rich.artifactHash]),
    "revision",
    "Timeline exact Revision filter",
    {
      revision: config.fixture.rich.revisionId,
      artifactHash: config.fixture.rich.artifactHash,
    },
  );

  const inspectExactTimelineEvent = async (eventId, label, expectedText) => {
    await open(
      `timeline?limit=100&order=asc&at=${encodeURIComponent(eventId)}`,
      layouts[0],
      label,
    );
    const row = page.locator(`#timeline [data-event-id="${eventId}"]`);
    expect(await row.count() === 1, label, `exact event ${eventId} was not revealed`);
    await row.click();
    await page.waitForFunction(
      (id) => location.hash.includes("/events/")
        && document.querySelector("#detail-body")?.textContent?.includes(id),
      eventId,
    );
    const detail = await page.locator("#detail-body").innerText();
    for (const text of expectedText) {
      expect(detail.includes(text), label, `event detail omitted ${text}`);
    }
    return detail;
  };

  await inspectExactTimelineEvent(
    config.fixture.correction.eventId,
    "Timeline correction event",
    [config.fixture.correction.originObservationId, "Browser correction replacement"],
  );
  await inspectExactTimelineEvent(
    config.fixture.factPort.eventId,
    "Timeline fact-port event",
    [config.fixture.factPort.portId, "context_only"],
  );
  await inspectExactTimelineEvent(
    config.fixture.historicalMembership.withdrawEventId,
    "Timeline membership-withdrawal event",
    [
      config.fixture.historicalMembership.claimId,
      config.fixture.historicalMembership.historicalChangeId,
      config.fixture.historicalMembership.revisionId,
    ],
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
  const historicalProposal = page.locator("#timeline [data-event-id]").first();
  expect(await historicalProposal.count() === 1, "Timeline withdrawn historical membership", "historical Change filter did not retain the Revision proposal");
  await historicalProposal.click();
  await page.waitForFunction(() => location.hash.includes("/events/") && !document.querySelector("#detail")?.inert);
  const historicalProposalDetail = await page.locator("#detail-body").innerText();
  for (const expectedChange of [historical.directChangeId, historical.historicalChangeId]) {
    expect(
      historicalProposalDetail.includes(expectedChange),
      "Timeline withdrawn historical membership",
      `proposal detail omitted correlated Change ${expectedChange}`,
    );
  }

  await open(
    "timeline?limit=100&order=asc&track=agent%3Abrowser-equal-time",
    layouts[0],
    "Timeline equal occurredAt pair",
  );
  for (const eventId of config.fixture.equalTimestamp.eventIds) {
    const row = page.locator(`#timeline [data-event-id="${eventId}"]`);
    expect(await row.count() === 1, "Timeline equal occurredAt pair", `equal-time event ${eventId} is absent`);
    expect(
      await row.locator("time").getAttribute("datetime") === config.fixture.equalTimestamp.occurredAt,
      "Timeline equal occurredAt pair",
      `event ${eventId} did not retain ${config.fixture.equalTimestamp.occurredAt}`,
    );
  }
  const equalTimestampOrder = await page.locator("#timeline [data-event-id]").evaluateAll(
    (rows, eventIds) => rows
      .map((row) => row.dataset.eventId)
      .filter((eventId) => eventIds.includes(eventId)),
    config.fixture.equalTimestamp.eventIds,
  );
  expect(
    config.fixture.equalTimestamp.tieBreak === "event_id_asc"
      && JSON.stringify(equalTimestampOrder) === JSON.stringify(config.fixture.equalTimestamp.eventIds),
    "Timeline equal occurredAt pair",
    `equal-time order ${JSON.stringify(equalTimestampOrder)} did not use event_id_asc`,
  );

  // Timeline gets its own preference evidence rather than inheriting the
  // Change-card captures below.
  await open("timeline?limit=100&order=desc", layouts[0], "Timeline display preferences");
  await page.locator("#view-toggle").click();
  await page.locator("#theme-dark").check();
  await page.locator("#density-compact").check();
  await screenshot("wide-timeline-dark-compact");
  await page.locator("#theme-light").check();
  await page.locator("#density-comfortable").check();
  await screenshot("wide-timeline-light-comfortable");
  await page.locator("#view-toggle").click();

  // Local cursor movement must remain bounded and must never redirect from a
  // text input.  It is intentionally tested before clicking an event, which
  // parks monitoring as reader activity.
  await open("timeline?limit=100&order=desc", layouts[0], "Timeline keyboard navigation");
  const listbox = page.locator("#timeline");
  await listbox.focus();
  const activeEvent = () => listbox.getAttribute("aria-activedescendant");
  await page.keyboard.press("g");
  const firstActive = await activeEvent();
  expect(Boolean(firstActive), "Timeline g", "g did not select the first readable event");
  await page.keyboard.press("j");
  expect((await activeEvent()) !== firstActive, "Timeline j", "j did not advance the local event cursor");
  await page.keyboard.press("k");
  expect(await activeEvent() === firstActive, "Timeline k", "k did not restore the preceding local event cursor");
  await page.keyboard.press("G");
  const lastActive = await activeEvent();
  expect(Boolean(lastActive) && lastActive !== firstActive, "Timeline G", "G did not select the last rendered event");
  await page.keyboard.press("g");
  await page.keyboard.press("f");
  const fullForward = await activeEvent();
  expect(Boolean(fullForward) && fullForward !== firstActive, "Timeline f", "f did not advance by a bounded page");
  await page.keyboard.press("b");
  expect(await activeEvent() === firstActive, "Timeline b", "b did not return across the bounded page movement");
  await page.keyboard.press("d");
  const halfForward = await activeEvent();
  expect(Boolean(halfForward) && halfForward !== firstActive, "Timeline d", "d did not advance by a half page");
  await page.keyboard.press("u");
  expect(await activeEvent() === firstActive, "Timeline u", "u did not return across the half-page movement");
  await page.keyboard.press("/");
  expect(await page.evaluate(() => document.activeElement?.id === "filter-text"), "Timeline search shortcut", "/ did not focus the shared filter field");
  const timelineHashBeforeTextGuard = await hash();
  await page.keyboard.press("j");
  await page.keyboard.press("?");
  expect(await hash() === timelineHashBeforeTextGuard, "Timeline text guard", "Timeline shortcut fired from the text filter");
  expect(await page.locator("#key-help:not(.hidden)").count() === 0, "Timeline text guard", "help opened from the text filter");
  await page.keyboard.press("Escape");
  await listbox.focus();
  await page.keyboard.press("j");
  expect(await listbox.getAttribute("aria-activedescendant"), "Timeline roving focus", "j did not expose a selected active descendant");

  // Open one exact event from the Timeline. Wide and narrow must retain the
  // same event route/detail, and browser history must restore the monitor.
  const selectedEventId = await page.locator("#timeline [data-event-id]").first().getAttribute("data-event-id");
  expect(Boolean(selectedEventId), "Timeline exact event", "Timeline row lacked an event ID");
  await page.locator("#timeline [data-event-id]").first().click();
  await page.waitForFunction((eventId) => location.hash.includes(`/timeline/events/${encodeURIComponent(eventId)}`), selectedEventId);
  expect((await page.locator("#detail-body").innerText()).includes(selectedEventId), "Timeline exact event", "event detail did not retain its exact event identity");
  await page.locator("#detail-read").click();
  expect(await page.locator(".split").evaluate((node) => node.classList.contains("reading")), "Timeline event reading mode", "exact event did not enter reading mode");
  await screenshot("wide-timeline-event-detail");
  await page.locator("#master-rail").click();
  expect(!(await page.locator(".split").evaluate((node) => node.classList.contains("reading"))), "Timeline event reading return", "master rail did not leave event reading mode");
  await page.goBack();
  await page.waitForFunction(() => location.hash.startsWith("#/timeline"));
  await page.goForward();
  await page.waitForFunction((eventId) => location.hash.includes(`/timeline/events/${encodeURIComponent(eventId)}`), selectedEventId);
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForFunction((eventId) => location.hash.includes(`/timeline/events/${encodeURIComponent(eventId)}`) && Boolean(document.querySelector("#timeline")), selectedEventId);
  expect((await page.locator("#detail-body").innerText()).includes(selectedEventId), "Timeline event reload", "reload lost the exact event detail");
  await page.goBack();
  await page.waitForFunction(() => location.hash.startsWith("#/timeline"));

  await open("timeline?limit=100&order=desc", layouts[1], "narrow Timeline");
  await page.locator("#timeline [data-event-id]").first().click();
  await page.waitForFunction(() => location.hash.includes("/timeline/events/"));
  expect(await page.locator("#detail").evaluate((node) => !node.inert && !node.hasAttribute("aria-hidden")), "narrow Timeline event", "narrow event detail remained inert");
  await screenshot("narrow-timeline-event-detail");
  await page.locator("#detail-back").click();
  await page.waitForFunction(() => location.hash.startsWith("#/timeline"));

  // A parked Timeline must not repaint when the shell's disposable worker
  // appends an event.  The worker waits for this screenshot, then writes its
  // receipt; explicit catch-up is the only action that adopts the new head.
  await open("timeline?limit=100&order=desc", layouts[0], "Timeline follow park");
  const follow = page.locator("#follow-toggle");
  await follow.click();
  expect((await follow.innerText()).includes("Parked"), "Timeline park", "follow control did not expose parked state");
  const parkedRows = await page.locator("#timeline [data-event-id]").evaluateAll((rows) => rows.map((row) => row.dataset.eventId));
  await screenshot("timeline-parked-before-append");
  // Wait for the normal poll/catch-up affordance. The shell worker's receipt
  // remains a completion-last evidence record and is checked after this
  // program returns; it is intentionally not exposed to the browser.
  await page.waitForFunction(() => (document.querySelector("#follow-toggle")?.textContent || "").includes("Show "));
  expect(JSON.stringify(await page.locator("#timeline [data-event-id]").evaluateAll((rows) => rows.map((row) => row.dataset.eventId))) === JSON.stringify(parkedRows), "Timeline parked stability", "parked Timeline adopted the appended head before explicit catch-up");
  await follow.click();
  await page.waitForFunction(() => document.querySelector("#follow-toggle")?.textContent === "Following");
  await screenshot("timeline-followed-after-append");

  // The continuation captured above names the pre-append projection. Exercise
  // its real authenticated refusal, then prove the reader can recover by
  // returning to the unpositioned filtered head instead of reusing the token.
  const staleQuery = staleTimelineRoute.split("?", 2)[1] ?? "";
  const staleResponse = await page.request.get(
    `${config.server.baseUrl}/api/v2/history?${staleQuery}`,
    { headers: { Authorization: `Bearer ${config.server.token}` } },
  );
  expect(staleResponse.status() === 409, "stale Timeline continuation", `old continuation returned HTTP ${staleResponse.status()}`);
  const staleBody = await staleResponse.json();
  expect(staleBody.code === "stale_projection", "stale Timeline continuation", `old continuation returned ${staleBody.code}`);
  await open("timeline?limit=100&order=desc", layouts[0], "stale Timeline explicit head recovery");
  expect(!(await hash()).includes("after="), "stale Timeline explicit head recovery", "head recovery retained the stale continuation");

  for (const layout of layouts) {
    const metrics = await open("changes?limit=100&order=change_id_asc", layout, `${layout.name} changes`);
    expect(metrics.liveCards > 0 && metrics.liveCards <= 100, `${layout.name} changes`, `expected bounded live card count, saw ${metrics.liveCards}`);
    expect(await page.getByRole("button", { name: /Next page/ }).count() > 0, `${layout.name} changes`, "363+ fixture did not offer pagination");
    expect(await cardNamesAreUseful(), `${layout.name} card names`, "card accessible names must lead with human Revision presentation and retain exact identity");
    expect(await noHiddenTabStops(), `${layout.name} hidden controls`, "a hidden control remains tabbable");
    if (layout.name === "narrow") {
      const closedDetail = await page.locator("#detail").evaluate((node) => ({ inert: node.inert, hidden: node.getAttribute("aria-hidden") }));
      expect(closedDetail.inert && closedDetail.hidden === "true", "narrow closed detail", "off-canvas detail was not removed from navigation and the accessibility tree");
    }
    const firstPageIds = await page.locator(".unit-card[data-change-id]").evaluateAll((cards) => cards.map((card) => card.dataset.changeId));
    expect(JSON.stringify(firstPageIds) === JSON.stringify([...firstPageIds].sort()), `${layout.name} stable order`, "first page is not change_id_asc");
    await screenshot(`${layout.name}-changes`);
    const firstListKey = await page.locator("#master").getAttribute("data-change-list-key");
    await page.getByRole("button", { name: /Next page/ }).click();
    await page.waitForFunction(() => location.hash.includes("after="));
    await page.waitForFunction((key) => document.querySelector("#master")?.dataset.changeListKey !== key, firstListKey);
    await page.waitForFunction(() => document.querySelectorAll(".unit-card[data-change-id]").length > 0);
    const nextPageIds = await page.locator(".unit-card[data-change-id]").evaluateAll((cards) => cards.map((card) => card.dataset.changeId));
    expect(nextPageIds.length > 0 && nextPageIds.length <= 100, `${layout.name} next page`, `next page has ${nextPageIds.length} cards`);
    expect(JSON.stringify(nextPageIds) === JSON.stringify([...nextPageIds].sort()), `${layout.name} stable order`, "next page is not change_id_asc");
    expect(firstPageIds.at(-1) < nextPageIds[0], `${layout.name} page boundary`, "next page does not follow the first page in change_id_asc order");

    const attentionMetrics = await open("attention?limit=50&order=change_id_asc", layout, `${layout.name} attention`);
    expect(attentionMetrics.liveCards <= 100, `${layout.name} attention`, `attention page exceeded live card bound: ${attentionMetrics.liveCards}`);
    await screenshot(`${layout.name}-attention`);
  }

  await open("changes?limit=100&order=change_id_asc", layouts[0], "keyboard changes");
  await page.keyboard.press("j");
  expect(await selected().count() === 1, "keyboard local selection", "j did not select exactly one local Change");
  const selectedId = await selected().getAttribute("data-change-id");
  expect(typeof selectedId === "string" && selectedId.length > 0, "keyboard local selection", "selected Change has no exact identity");
  expect(!(await hash()).includes(selectedId), "keyboard local selection", "local selection changed the URL before Enter");
  await page.keyboard.press("Enter");
  await page.waitForFunction((id) => location.hash.includes(encodeURIComponent(id)), selectedId);
  expect((await hash()).includes(`/changes/${encodeURIComponent(selectedId)}`), "keyboard Enter", "Enter did not open the selected Change");
  await page.waitForFunction(() => Boolean(document.querySelector("#detail-body")?.dataset.changeReadingKey));
  await screenshot("wide-keyboard-change");
  await page.keyboard.press("Escape");
  await page.waitForFunction(() => location.hash.startsWith("#/changes?"));
  expect(await selected().count() === 1, "native control Enter", "returning from the selected Change lost the local cursor");
  const beforeNativeEnter = await hash();
  const viewToggle = page.locator("#view-toggle");
  await viewToggle.focus();
  await page.keyboard.press("Enter");
  expect(await viewToggle.getAttribute("aria-expanded") === "true", "native control Enter", "Enter on the focused View control was intercepted");
  expect(await hash() === beforeNativeEnter, "native control Enter", "Enter on the focused View control opened the selected Change");
  await page.keyboard.press("Enter");
  expect(await viewToggle.getAttribute("aria-expanded") === "false", "native control Enter", "second Enter did not close the focused View control");
  await page.keyboard.press("G");
  const lastId = await selected().getAttribute("data-change-id");
  expect(lastId === await page.locator(".unit-card[data-change-id]").last().getAttribute("data-change-id"), "G boundary", "G did not select last loaded Change");
  await page.keyboard.press("g");
  expect(await selected().getAttribute("data-change-id") === await page.locator(".unit-card[data-change-id]").first().getAttribute("data-change-id"), "g boundary", "g did not select first loaded Change");
  await page.keyboard.press("3");
  await page.waitForFunction(() => location.hash.startsWith("#/attention?"));
  await waitForLens("attention");
  await page.keyboard.press("2");
  await page.waitForFunction(() => location.hash.startsWith("#/changes?"));
  await waitForLens("changes");
  await page.keyboard.press("1");
  await page.waitForFunction(() => location.hash.startsWith("#/timeline"));
  await waitForLens("timeline");
  await page.keyboard.press("2");
  await page.waitForFunction(() => location.hash.startsWith("#/changes?"));

  const search = page.locator("#filter-text");
  await search.focus();
  const beforeSearch = await hash();
  await page.keyboard.press("2");
  await page.keyboard.press("?");
  await page.keyboard.press("j");
  expect(await hash() === beforeSearch, "shortcuts in search", "a reader shortcut fired while search had focus");
  expect(await page.locator(".modal:not(.hidden)").count() === 0, "shortcuts in search", "help opened while search had focus");
  await search.fill("uncommitted poll draft");
  await page.waitForTimeout(3500);
  expect(await search.inputValue() === "uncommitted poll draft", "poll search draft", "a background poll erased the focused uncommitted search draft");
  await search.fill("Browser scale Change 1");
  await search.press("Tab");
  await page.waitForFunction(() => {
    const query = location.hash.split("?", 2)[1] ?? "";
    return new URLSearchParams(query).get("q") === "Browser scale Change 1";
  });
  await page.locator("#filters-toggle").click();
  await page.locator("#change-filter-topology").selectOption("initial");
  const filteredHash = await hash();
  expect(filteredHash.includes("limit=100") && filteredHash.includes("order=change_id_asc"), "filter URL state", "filtering lost explicit paging or ordering state");
  expect(
    await page.locator("#filters-toggle").getAttribute("aria-expanded") === "false",
    "filter route dismissal",
    "route-changing facet left the Filters panel over the new result",
  );
  await page.locator("#filters-toggle").click();
  await page.locator("#filter-clear").click();
  await page.waitForFunction(() => !location.hash.includes("q=") && !location.hash.includes("after="));
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
  const clearedHash = await hash();
  expect(clearedHash.includes("limit=100") && clearedHash.includes("order=change_id_asc"), "clear reset", "clear reset did not preserve limit and order");
  await screenshot("wide-filter-clear");

  const topologyFixture = config.fixture.matrix.topology;
  const representativeCases = [
    ["initial", "initial topology", "topology=initial", topologyFixture.initial.change],
    ["replacement", "replacement topology", "topology=replacement", topologyFixture.replacement.change],
    ["parallel", "parallel topology", "topology=parallel_current", topologyFixture.parallel_current.change],
    ["replacement-divergent", "replacement-divergent topology", "topology=replacement_divergent", topologyFixture.replacement_divergent.change],
    ["consolidation", "consolidation topology", "topology=consolidation", topologyFixture.consolidation.change],
    ["removed-resource-change", "removed resource Change availability", "availability=available", config.fixture.removed.changeId],
  ];
  for (const layout of layouts) {
    for (const [slug, name, filter, expectedChange] of representativeCases) {
      const route = `changes?limit=100&order=change_id_asc&${filter}&q=${encodeURIComponent(expectedChange)}`;
      const topologyMetrics = await open(route, layout, `${layout.name} ${name}`);
      expect(topologyMetrics.liveCards === 1, `${layout.name} ${name}`, `expected one exact representative card, saw ${topologyMetrics.liveCards}`);
      expect(await page.locator(`.unit-card[data-change-id="${expectedChange}"]`).count() === 1, `${layout.name} ${name}`, `missing exact fixture Change ${expectedChange}`);
      const sparseGeometry = await page.evaluate(() => {
        const units = document.querySelector("#master > .units");
        const card = units?.querySelector(".unit-card[data-change-id]");
        return {
          listHeight: units?.getBoundingClientRect().height ?? 0,
          cardHeight: card?.getBoundingClientRect().height ?? 0,
        };
      });
      expect(
        sparseGeometry.listHeight > 0
          && sparseGeometry.cardHeight > 0
          && sparseGeometry.cardHeight < sparseGeometry.listHeight * 0.75,
        `${layout.name} ${name}`,
        `single card stretched to ${sparseGeometry.cardHeight}/${sparseGeometry.listHeight}`,
      );
      await screenshot(`${layout.name}-${slug}`);
    }
  }

  const expectedParallelChange = topologyFixture.parallel_current.change;
  const parallelRoute = `changes?limit=100&order=change_id_asc&topology=parallel_current&q=${encodeURIComponent(expectedParallelChange)}`;
  await open(parallelRoute, layouts[0], "parallel explicit chooser");
  const parallelChange = await page.locator(".unit-card[data-change-id]").evaluateAll((cards) => {
    const card = cards.find((candidate) => candidate.querySelectorAll(".change-card-peer-open").length > 1);
    return card?.dataset.changeId ?? null;
  });
  expect(typeof parallelChange === "string" && parallelChange.length > 0, "parallel explicit chooser", "no Change exposed multiple current Revisions");
  expect(parallelChange === expectedParallelChange, "parallel explicit chooser", `opened ${parallelChange} instead of exact fixture ${expectedParallelChange}`);
  const parallelCard = page.locator(`.unit-card[data-change-id="${parallelChange}"]`);
  const peerButtons = parallelCard.locator(".change-card-peer-open");
  await peerButtons.first().focus();
  await page.keyboard.press("Tab");
  const firstPeerCopy = parallelCard.getByRole("button", { name: /^Copy exact Revision / }).first();
  expect(await firstPeerCopy.evaluate((node) => document.activeElement === node), "peer keyboard traversal", "Tab skipped the first exact Revision copy action");
  await page.keyboard.press("Tab");
  expect(await peerButtons.nth(1).evaluate((node) => document.activeElement === node), "peer keyboard traversal", "Tab did not move between exact current-Revision peers");
  const peerFocus = await peerButtons.nth(1).evaluate((node) => {
    const style = getComputedStyle(node);
    return { outlineStyle: style.outlineStyle, outlineWidth: style.outlineWidth };
  });
  expect(peerFocus.outlineStyle !== "none" && peerFocus.outlineWidth !== "0px", "visible peer focus", `peer focus indicator was ${peerFocus.outlineStyle}/${peerFocus.outlineWidth}`);
  await parallelCard.getByRole("button", { name: /^Open Change / }).click();
  await page.waitForFunction((changeId) => location.hash.includes(encodeURIComponent(changeId)) && !location.hash.includes("/revisions/"), parallelChange);
  expect(await page.locator("#detail-body .change-card-peer-open").count() === 0, "parallel explicit chooser", "card-only peer controls leaked into detail");
  await page.waitForFunction(() => Boolean(document.querySelector("#detail-body")?.dataset.changeReadingKey));
  const exactChoices = page.locator("#detail-body button[aria-label^='Current Revision:']");
  expect(await exactChoices.count() > 1, "parallel explicit chooser", "Change detail did not require a human exact-Revision choice");
  expect(await exactChoices.evaluateAll((choices) => choices.every((choice) => {
    const revisionId = choice.textContent?.trim() || "";
    const name = choice.getAttribute("aria-label") || "";
    return revisionId.length > 0 && name.includes(revisionId) && name.includes("; artifact sha256:");
  })), "parallel explicit chooser", "Change detail exact-Revision names omitted the artifact identity");
  await exactChoices.first().click();
  await page.waitForFunction(() => location.hash.includes("/revisions/"));
  await page.waitForFunction(() => Boolean(document.querySelector("#detail-body")?.dataset.changeReadingKey));
  await screenshot("wide-parallel-explicit-revision");
  const revisionReadingKey = await page.locator("#detail-body").getAttribute("data-change-reading-key");
  await page.getByRole("button", { name: "Open authoritative captured diff" }).click();
  await page.waitForFunction(() => location.hash.includes("/resource?"));
  await page.waitForFunction((key) => {
    const next = document.querySelector("#detail-body")?.dataset.changeReadingKey;
    return Boolean(next && next !== key);
  }, revisionReadingKey);
  expect(await page.locator("#detail-close").evaluate((node) => document.activeElement === node), "exact route focus", "exact-to-exact detail replacement left focus on the document body");
  const resourceReadingKey = await page.locator("#detail-body").getAttribute("data-change-reading-key");
  await page.goBack();
  await page.waitForFunction(() => location.hash.includes("/revisions/") && !location.hash.includes("/resource?"));
  await page.waitForFunction((key) => {
    const next = document.querySelector("#detail-body")?.dataset.changeReadingKey;
    return Boolean(next && next !== key);
  }, resourceReadingKey);
  await page.keyboard.press("3");
  await page.waitForFunction(() => location.hash.startsWith("#/attention?"));
  await page.goBack();
  await page.waitForFunction(() => location.hash.includes("/revisions/"));
  await page.keyboard.press("Escape");
  await page.waitForFunction(() => location.hash.startsWith("#/changes?"));
  expect((await hash()).startsWith("#/changes?"), "exact history origin", "Back/Forward changed the exact route's originating lens");

  for (const layout of layouts) {
    for (const changeId of config.fixture.matrix.shared_revision.changes) {
      const encodedMembershipChange = encodeURIComponent(changeId);
      await open(`changes/${encodedMembershipChange}?limit=100&order=change_id_asc`, layout, `${layout.name} shared Revision membership`);
      await page.waitForFunction((revisionId) => document.querySelector("#detail-body")?.textContent?.includes(revisionId), config.fixture.matrix.shared_revision.revision);
      expect((await page.locator("#detail-body").innerText()).includes(config.fixture.matrix.shared_revision.revision), `${layout.name} shared Revision membership`, `Change ${changeId} omitted shared exact Revision`);
    }
    await screenshot(`${layout.name}-shared-revision-membership`);
  }

  await open("changes?limit=100&order=change_id_asc", layouts[0], "split bounds");
  await page.getByRole("button", { name: /^Open Change / }).first().click();
  await page.waitForFunction(() => !document.querySelector("#detail")?.inert);
  const divider = page.locator(".divider");
  const splitBox = await page.locator(".split").boundingBox();
  const dividerBox = await divider.boundingBox();
  expect(Boolean(splitBox && dividerBox), "split pointer drag", "visible split geometry was unavailable");
  if (!splitBox || !dividerBox) throw new Error("visible split geometry was unavailable");
  await page.mouse.move(dividerBox.x + dividerBox.width / 2, dividerBox.y + dividerBox.height / 2);
  await page.mouse.down();
  await page.mouse.move(splitBox.x + splitBox.width * 0.62, dividerBox.y + dividerBox.height / 2);
  await page.mouse.up();
  const draggedSplit = Number(await divider.getAttribute("aria-valuenow"));
  expect(draggedSplit >= 61 && draggedSplit <= 63, "split pointer drag", `pointer drag produced ${draggedSplit} instead of approximately 62`);
  await divider.dblclick();
  expect(await divider.getAttribute("aria-valuenow") === "50", "split pointer reset", "double-click did not restore the balanced split");
  await divider.focus();
  for (let step = 0; step < 40; step += 1) await page.keyboard.press("ArrowLeft");
  expect(await divider.getAttribute("aria-valuenow") === "25", "split lower bound", "divider moved below its declared lower bound");
  for (let step = 0; step < 80; step += 1) await page.keyboard.press("ArrowRight");
  expect(await divider.getAttribute("aria-valuenow") === "75", "split upper bound", "divider moved above its declared upper bound");
  await page.keyboard.press("Enter");
  expect(await divider.getAttribute("aria-valuenow") === "50", "split reset", "Enter did not restore the balanced split");

  await page.locator("#view-toggle").click();
  await page.locator("#theme-dark").check();
  await page.locator("#density-compact").check();
  expect(await page.evaluate(() => document.documentElement.dataset.theme === "dark" && document.documentElement.classList.contains("compact")), "dark compact preference", "dark/compact preference was not applied");
  await screenshot("wide-dark-compact");
  await page.locator("#theme-light").check();
  await page.locator("#density-comfortable").check();
  expect(await page.evaluate(() => document.documentElement.dataset.theme === "light" && !document.documentElement.classList.contains("compact")), "light comfortable preference", "light/comfortable preference was not applied");
  await screenshot("wide-light-comfortable");

  await page.locator("#view-toggle").focus();
  await page.keyboard.press("?");
  const help = page.locator("#key-help");
  expect(await help.evaluate((node) => !node.classList.contains("hidden")), "help overlay", "? did not open help");
  expect(await page.evaluate(() => document.activeElement?.id === "key-help-close"), "help focus", "help did not move focus into its dialog");
  await page.keyboard.press("Shift+Tab");
  expect(await page.evaluate(() => document.querySelector("#key-help")?.contains(document.activeElement)), "help modal trap", "Shift+Tab escaped the help dialog");
  await page.keyboard.press("Escape");
  expect(await page.evaluate(() => document.activeElement?.id === "view-toggle"), "help focus restoration", "help did not restore focus to its opener");
  await page.keyboard.press("Control+k");
  const palette = page.locator("#cmd-palette");
  expect(await palette.evaluate((node) => !node.classList.contains("hidden")), "palette overlay", "Cmd/Ctrl+K did not open palette");
  expect(await page.evaluate(() => document.activeElement?.id === "cmd-input"), "palette focus", "palette did not focus its input");
  await page.keyboard.press("Escape");
  expect(await page.evaluate(() => document.activeElement?.id === "view-toggle"), "palette focus restoration", "palette did not restore focus to its opener");

  await page.keyboard.press("Control+Shift+p");
  expect(await palette.evaluate((node) => !node.classList.contains("hidden")), "alternate palette chord", "Ctrl+Shift+P did not open palette");
  await page.keyboard.press("Escape");

  const encodedChange = encodeURIComponent(config.fixture.rich.changeId);
  const encodedRevision = encodeURIComponent(config.fixture.rich.revisionId);
  const encodedArtifact = encodeURIComponent(config.fixture.rich.artifactHash);
  const exact = `changes/${encodedChange}/revisions/${encodedRevision}?artifactHash=${encodedArtifact}&limit=100&order=change_id_asc`;
  await open(exact, layouts[1], "narrow exact revision");
  await page.waitForFunction(() => Boolean(document.querySelector("#detail-body")?.dataset.changeReadingKey));
  expect(await page.locator("#detail").evaluate((node) => !node.inert && !node.hasAttribute("aria-hidden")), "narrow exact revision", "open detail remained inert or hidden");
  expect(await page.evaluate(() => document.activeElement?.id === "detail-back"), "narrow detail focus", "opening the narrow detail did not move focus into the sheet");
  const exactText = await page.locator("body").innerText();
  expect(exactText.includes(config.fixture.rich.changeId) && exactText.includes(config.fixture.rich.revisionId), "narrow exact revision", "exact identities are missing");
  for (const expected of ["Matrix fact", "Open decision", "passed current", "Association comparisons"]) {
    expect(exactText.includes(expected), "narrow rich revision", `missing representative detail: ${expected}`);
  }
  await screenshot("narrow-exact-detail");
  await page.locator("#detail-body").evaluate((node) => { node.scrollTop = node.scrollHeight; });
  const narrowBackBounds = await page.evaluate(() => {
    const detail = document.querySelector("#detail")?.getBoundingClientRect();
    const back = document.querySelector("#detail-back")?.getBoundingClientRect();
    return detail && back
      ? { detailTop: detail.top, detailBottom: detail.bottom, backTop: back.top, backBottom: back.bottom }
      : null;
  });
  expect(
    narrowBackBounds !== null
      && narrowBackBounds.backTop >= narrowBackBounds.detailTop
      && narrowBackBounds.backBottom <= narrowBackBounds.detailBottom,
    "narrow persistent return",
    `Back control left the detail viewport: ${JSON.stringify(narrowBackBounds)}`,
  );
  await page.locator("#detail-back").click();
  await page.waitForFunction(() => location.hash.startsWith("#/changes?"));
  expect(await page.locator(".split").evaluate((node) => node.classList.contains("split-closed")), "narrow detail return", "Back did not close the narrow detail sheet");
  expect(await page.locator("#detail").evaluate((node) => node.inert && node.getAttribute("aria-hidden") === "true"), "narrow detail return", "closed detail remained exposed to keyboard or assistive navigation");
  expect(await page.evaluate(() => {
    const master = document.querySelector("#master");
    return document.activeElement === master || Boolean(master?.contains(document.activeElement));
  }), "narrow detail focus restoration", "closing the narrow detail did not restore focus to the retained list surface");

  await open(exact, layouts[0], "wide exact revision");
  await page.waitForFunction(() => Boolean(document.querySelector("#detail-body")?.dataset.changeReadingKey));
  const detail = page.locator("#detail");
  const detailViewport = page.locator("#detail-body");
  expect(await detailViewport.evaluate((node) => node.scrollHeight > node.clientHeight), "reading scroll", "rich exact detail did not produce a real scroll range");
  const readingToggle = page.locator("#detail-read");
  await readingToggle.focus();
  await detailViewport.evaluate((node) => { node.scrollTop = Math.min(80, node.scrollHeight - node.clientHeight); });
  const beforeReadingScroll = await detailViewport.evaluate((node) => node.scrollTop);
  expect(beforeReadingScroll > 0, "reading scroll", "failed to establish a non-zero detail scroll position");
  await page.keyboard.press("Enter");
  expect(await page.locator(".split").evaluate((node) => node.classList.contains("reading")), "reading mode", "reading mode was not entered");
  expect(await detailViewport.evaluate((node) => node.scrollTop) === beforeReadingScroll, "reading scroll", "reading mode lost detail scroll position");
  const wideHeaderBounds = await page.evaluate(() => {
    const detail = document.querySelector("#detail")?.getBoundingClientRect();
    const close = document.querySelector("#detail-close")?.getBoundingClientRect();
    return detail && close
      ? { detailTop: detail.top, detailBottom: detail.bottom, closeTop: close.top, closeBottom: close.bottom }
      : null;
  });
  expect(
    wideHeaderBounds !== null
      && wideHeaderBounds.closeTop >= wideHeaderBounds.detailTop
      && wideHeaderBounds.closeBottom <= wideHeaderBounds.detailBottom,
    "reading persistent controls",
    `detail controls left the reading viewport: ${JSON.stringify(wideHeaderBounds)}`,
  );
  await page.locator("#master-rail").click();
  expect(!(await page.locator(".split").evaluate((node) => node.classList.contains("reading"))), "reading return path", "master rail did not restore split mode");
  await screenshot("wide-exact-reading");
  const resourceCases = [
    ["removed", config.fixture.removed, "captured_resource_removed"],
    ["missing", config.fixture.missing, "captured_resource_missing"],
  ];
  for (const layout of layouts) {
    for (const [availability, fixture, diagnostic] of resourceCases) {
      const resourceRoute = `changes/${encodeURIComponent(fixture.changeId)}/revisions/${encodeURIComponent(fixture.revisionId)}/resource?artifactHash=${encodeURIComponent(fixture.artifactHash)}`;
      await open(resourceRoute, layout, `${layout.name} ${availability} resource`);
      await page.waitForFunction(
        ({ expectedAvailability, expectedDiagnostic }) => {
          const text = document.querySelector("#detail-body")?.textContent ?? "";
          return text.includes(`availability: ${expectedAvailability}`) && text.includes(expectedDiagnostic);
        },
        { expectedAvailability: availability, expectedDiagnostic: diagnostic },
      );
      const resourceText = await page.locator("#detail-body").innerText();
      expect(resourceText.includes(`availability: ${availability}`), `${layout.name} ${availability} resource`, `exact availability was not ${availability}`);
      expect(resourceText.includes(diagnostic), `${layout.name} ${availability} resource`, `missing exact diagnostic ${diagnostic}`);
      expect(resourceText.includes("Captured bytes are unavailable. No live or associated-commit bytes were substituted."), `${layout.name} ${availability} resource`, "bodyless exact resource did not state its non-substitution guarantee");
      expect(!resourceText.includes("captured document:"), `${layout.name} ${availability} resource`, "bodyless exact resource exposed a captured-document hash");
      expect(await page.locator("#detail-body .captured-diff").count() === 0, `${layout.name} ${availability} resource`, "bodyless exact resource rendered a captured or substituted diff");
      const detailOverflow = await page.locator("#detail-body").evaluate((node) => ({
        clientWidth: node.clientWidth,
        scrollWidth: node.scrollWidth,
      }));
      expect(
        detailOverflow.scrollWidth <= detailOverflow.clientWidth,
        `${layout.name} ${availability} resource`,
        `exact identity overflowed detail width ${detailOverflow.scrollWidth}/${detailOverflow.clientWidth}`,
      );
      await screenshot(`${layout.name}-${availability}-resource`);
    }
  }

  await page.emulateMedia({ reducedMotion: "reduce" });
  await open("changes?limit=100&order=change_id_asc", layouts[0], "wide reduced motion");
  await page.locator(".unit-card[data-change-id]").first().evaluate((node) => { node.dataset.browserRetention = "same-generation"; });
  await page.waitForTimeout(3500);
  expect(await page.locator('.unit-card[data-browser-retention="same-generation"]').count() === 1, "same-generation DOM retention", "polling repainted an unchanged Change generation");
  const reducedMotion = await page.evaluate(() => {
    const detail = document.querySelector("#detail");
    const live = document.querySelector("#refresh");
    if (live) live.dataset.state = "degraded";
    return { mediaMatches: matchMedia("(prefers-reduced-motion: reduce)").matches, detailTransitionDuration: detail ? getComputedStyle(detail).transitionDuration : null, liveAnimationName: live ? getComputedStyle(live).animationName : null };
  });
  expect(reducedMotion.mediaMatches, "reduced motion", "media emulation did not apply");
  expect(reducedMotion.detailTransitionDuration === "0s", "reduced motion", `detail transition remained ${reducedMotion.detailTransitionDuration}`);
  expect(reducedMotion.liveAnimationName === "none", "reduced motion", `status animation remained ${reducedMotion.liveAnimationName}`);
  await screenshot("wide-reduced-motion");

  expect(consoleErrors.length === 0, "browser console", consoleErrors.join("\n"));
  expect(pageErrors.length === 0, "browser page", pageErrors.join("\n"));
  return { assertionCount: assertions, screenshotCount: screenshots };
})(__POINTBREAK_CHANGE_BROWSER_CONFIG__)
