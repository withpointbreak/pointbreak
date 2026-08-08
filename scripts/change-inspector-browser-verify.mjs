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
  const url = (route) => {
    const separator = route.includes("?") ? "&" : "?";
    return `${config.server.baseUrl}/#/${route}${separator}token=${encodeURIComponent(config.server.token)}`;
  };
  const screenshot = async (name) => {
    screenshots += 1;
    await page.screenshot({ path: `${config.artifactDir}/${name}.png`, type: "png", fullPage: false });
  };
  const open = async (route, layout, label) => {
    await page.setViewportSize({ width: layout.width, height: layout.height });
    await page.goto(url(route), { waitUntil: "domcontentloaded" });
    await page.waitForFunction(() => document.querySelector("#connection-status")?.textContent === "connected");
    await page.waitForFunction(() => {
      const stamp = document.querySelector("#stat-hash")?.textContent?.trim();
      return Boolean(stamp && stamp !== "—" && document.querySelector("#master h1"));
    });
    const metrics = await page.evaluate(() => ({
      width: document.documentElement.clientWidth,
      scrollWidth: document.documentElement.scrollWidth,
      liveCards: document.querySelectorAll(".unit-card[data-change-id]").length,
    }));
    expect(metrics.width === layout.width, label, `unexpected viewport width ${metrics.width}`);
    expect(metrics.scrollWidth <= metrics.width, label, `horizontal overflow ${metrics.scrollWidth}/${metrics.width}`);
    return metrics;
  };
  const hash = () => page.evaluate(() => location.hash);
  const selected = () => page.locator(".unit-card.change-card-selected[data-change-id]");
  const cardNamesAreUseful = () => page.evaluate(() =>
    Array.from(document.querySelectorAll(".unit-card[data-change-id]")).every((card) => {
      const name = card.getAttribute("aria-label") || "";
      const exact = card.dataset.changeId || "";
      return name.includes(exact) && name.length > exact.length && !/^Change\s+[^\s]+$/.test(name);
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
  await screenshot("wide-keyboard-change");
  await page.keyboard.press("Escape");
  await page.waitForFunction(() => location.hash.startsWith("#/changes?"));
  await page.keyboard.press("G");
  const lastId = await selected().getAttribute("data-change-id");
  expect(lastId === await page.locator(".unit-card[data-change-id]").last().getAttribute("data-change-id"), "G boundary", "G did not select last loaded Change");
  await page.keyboard.press("g");
  expect(await selected().getAttribute("data-change-id") === await page.locator(".unit-card[data-change-id]").first().getAttribute("data-change-id"), "g boundary", "g did not select first loaded Change");
  await page.keyboard.press("2");
  await page.waitForFunction(() => location.hash.startsWith("#/attention?"));
  await page.keyboard.press("1");
  await page.waitForFunction(() => location.hash.startsWith("#/changes?"));
  const beforeThree = await hash();
  await page.keyboard.press("3");
  expect(await hash() === beforeThree, "inert 3", "3 unexpectedly changed route");

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
  await page.locator("#filter-clear").click();
  await page.waitForFunction(() => !location.hash.includes("q=") && !location.hash.includes("after="));
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
    ["incomplete-resource", "incomplete resource topology", "availability=incomplete", config.fixture.removed.changeId],
  ];
  for (const layout of layouts) {
    for (const [slug, name, filter, expectedChange] of representativeCases) {
      const route = `changes?limit=100&order=change_id_asc&${filter}&q=${encodeURIComponent(expectedChange)}`;
      const topologyMetrics = await open(route, layout, `${layout.name} ${name}`);
      expect(topologyMetrics.liveCards === 1, `${layout.name} ${name}`, `expected one exact representative card, saw ${topologyMetrics.liveCards}`);
      expect(await page.locator(`.unit-card[data-change-id="${expectedChange}"]`).count() === 1, `${layout.name} ${name}`, `missing exact fixture Change ${expectedChange}`);
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
  await page.keyboard.press("2");
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
  const divider = page.locator(".divider");
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
  await page.locator("#detail-back").click();
  await page.waitForFunction(() => location.hash.startsWith("#/changes?"));
  expect(await page.locator(".split").evaluate((node) => node.classList.contains("split-closed")), "narrow detail return", "Back did not close the narrow detail sheet");
  expect(await page.locator("#detail").evaluate((node) => node.inert && node.getAttribute("aria-hidden") === "true"), "narrow detail return", "closed detail remained exposed to keyboard or assistive navigation");
  expect(await page.evaluate(() => document.activeElement?.id === "master"), "narrow detail focus restoration", "closing a direct-linked detail did not restore a stable list focus target");

  await open(exact, layouts[0], "wide exact revision");
  await page.waitForFunction(() => Boolean(document.querySelector("#detail-body")?.dataset.changeReadingKey));
  const detail = page.locator("#detail");
  expect(await detail.evaluate((node) => node.scrollHeight > node.clientHeight), "reading scroll", "rich exact detail did not produce a real scroll range");
  await detail.evaluate((node) => { node.scrollTop = Math.min(80, node.scrollHeight - node.clientHeight); });
  const beforeReadingScroll = await detail.evaluate((node) => node.scrollTop);
  expect(beforeReadingScroll > 0, "reading scroll", "failed to establish a non-zero detail scroll position");
  await page.locator("#detail-read").click();
  expect(await page.locator(".split").evaluate((node) => node.classList.contains("reading")), "reading mode", "reading mode was not entered");
  expect(await detail.evaluate((node) => node.scrollTop) === beforeReadingScroll, "reading scroll", "reading mode lost detail scroll position");
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
  console.log(JSON.stringify({ assertionCount: assertions, screenshotCount: screenshots }));
})(__POINTBREAK_CHANGE_BROWSER_CONFIG__)
