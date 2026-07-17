((config) => async (page) => {
  const consoleErrors = [];
  const pageErrors = [];
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text());
  });
  page.on("pageerror", (error) => pageErrors.push(error.message));

  const layouts = [
    { name: "wide", width: 1440, height: 1000, density: "comfortable" },
    { name: "compact", width: 900, height: 506, density: "compact" },
    { name: "narrow", width: 390, height: 844, density: "comfortable" },
  ];

  const normalized = (value) => String(value ?? "").replace(/\s+/g, " ").trim();
  const short = (value, size = 8) => {
    const colon = value.lastIndexOf(":");
    return value.slice(colon + 1, colon + 1 + size);
  };
  const fail = (label, message) => {
    throw new Error(`${label}: ${message}`);
  };
  const expectText = async (label, terms) => {
    const text = normalized(await page.locator("body").innerText());
    for (const term of terms) {
      if (!text.toLowerCase().includes(term.toLowerCase())) {
        fail(
          label,
          `missing visible text ${JSON.stringify(term)}; visible text: ${text.slice(0, 4000)}`,
        );
      }
    }
    return text;
  };
  const expectRef = async (label, id) => {
    const count = await page
      .locator(`[data-ref-id="${id}"], [title="${id}"]`)
      .count();
    if (!count) fail(label, `immutable id is not visible: ${id}`);
  };
  const expectNoOverflow = async (label) => {
    const metrics = await page.evaluate(() => ({
      width: document.documentElement.clientWidth,
      scrollWidth: document.documentElement.scrollWidth,
      viewportWidth: innerWidth,
      viewportHeight: innerHeight,
    }));
    if (metrics.scrollWidth > metrics.width) {
      fail(label, `horizontal overflow ${metrics.scrollWidth}px in ${metrics.width}px`);
    }
    return metrics;
  };
  const setDensity = async (density) => {
    const choice = page.locator(`#density-${density}`);
    if (await choice.isChecked()) return;
    await choice.evaluate((input) => input.click());
    await page.waitForFunction(
      (compact) =>
        document.documentElement.classList.contains("compact") === compact,
      density === "compact",
    );
  };
  const routeUrl = (server, route) => {
    const separator = route.includes("?") ? "&" : "?";
    return `${server.baseUrl}/#/${route}${separator}token=${encodeURIComponent(server.token)}`;
  };
  const openRoute = async (label, server, route, layout) => {
    await page.setViewportSize({ width: layout.width, height: layout.height });
    await page.goto(routeUrl(server, route), { waitUntil: "domcontentloaded" });
    await page.waitForFunction(
      () => document.querySelector("#connection-status")?.textContent === "connected",
    );
    await setDensity(layout.density);
    const metrics = await expectNoOverflow(label);
    if (
      metrics.viewportWidth !== layout.width ||
      metrics.viewportHeight !== layout.height
    ) {
      fail(
        label,
        `unexpected viewport ${metrics.viewportWidth}x${metrics.viewportHeight}`,
      );
    }
  };
  const revisionRoute = (id) => `revision/${encodeURIComponent(id)}?lens=list`;
  const diffRoute = (id) => `revision/${encodeURIComponent(id)}/diff`;
  const screenshot = async (name) => {
    await page.screenshot({
      path: `${config.artifactDir}/${name}.png`,
      type: "png",
      fullPage: false,
    });
  };

  const canonicalTerms = [
    "refactor checkout identity handling",
    "summary —",
    "work refactor checkout identity handling",
    "landing merged",
    "actor:agent:pointbreak-example-author",
    "actor:agent:pointbreak-example-reviewer",
    "responded",
    "approved",
    "Checkout now preserves an absent user as a null identifier",
    "needs-changes",
    "replaced",
    "accepted",
    "current",
    "null-user branch",
    "outstanding",
    "null-user regression test",
    "current result",
  ];

  for (const layout of layouts) {
    const label = `canonical ${layout.name} detail`;
    await openRoute(
      label,
      config.canonical,
      revisionRoute(config.canonical.revisionId),
      layout,
    );
    await page.getByRole("heading", { name: "Revision", exact: true }).waitFor();
    await expectText(label, canonicalTerms);
    await expectRef(label, config.canonical.revisionId);
    await expectRef(label, config.canonical.objectId);
    await screenshot(`canonical-${layout.name}-detail`);

    const diffLabel = `canonical ${layout.name} diff`;
    await openRoute(
      diffLabel,
      config.canonical,
      diffRoute(config.canonical.revisionId),
      layout,
    );
    await page.getByRole("region", { name: "annotated diff" }).waitFor();
    await page
      .getByRole("heading", { name: /Decision context/ })
      .first()
      .waitFor();
    await expectNoOverflow(diffLabel);
    await expectText(diffLabel, [
      "Decision context (9)",
      "actor:agent:pointbreak-example-author",
      "actor:agent:pointbreak-example-reviewer",
      "answered by actor:agent:pointbreak-example-author",
      "needs-changes",
      "replaced",
      "accepted",
      "current",
      "null-user branch",
      "outstanding",
    ]);
    await expectRef(diffLabel, config.canonical.revisionId);
    await expectRef(diffLabel, config.canonical.objectId);
    await screenshot(`canonical-${layout.name}-diff`);
  }

  const syntheticTerms = [
    "Decision continuity matrix",
    "summary Decision continuity matrix",
    "work working-tree changes",
    "landing merged",
    "current commits",
    "withdrawn commits",
    "actor:agent:pointbreak-matrix-fact-writer",
    "actor:agent:pointbreak-matrix-participant-opener",
    "actor:agent:pointbreak-matrix-participant-responder",
    "actor:agent:pointbreak-matrix-request-opener",
    "actor:agent:pointbreak-matrix-response-one",
    "actor:agent:pointbreak-matrix-response-two",
    "Open decision",
    "open",
    "insufficient_evidence",
    "Responded decision",
    "responded",
    "approved",
    "the evidence is sufficient",
    "Ambiguous decision",
    "ambiguous",
    "first response approves",
    "second response rejects",
    "needs-changes",
    "replaced",
    "accepted-with-follow-up",
    "current",
    "passed current",
    "failed current",
    "errored current",
    "skipped only",
    "failed then passed",
    "resolved by strictly later pass",
    "equal time",
    "outstanding",
    "historical",
    "current result",
  ];

  for (const layout of layouts) {
    const label = `synthetic ${layout.name} detail`;
    await openRoute(
      label,
      config.synthetic,
      revisionRoute(config.synthetic.ids.primary_revision),
      layout,
    );
    await page.getByRole("heading", { name: "Revision", exact: true }).waitFor();
    await expectText(label, syntheticTerms);
    await expectRef(label, config.synthetic.ids.primary_revision);
    await expectRef(label, config.synthetic.primaryObjectId);
    await screenshot(`synthetic-${layout.name}-detail`);

    const diffLabel = `synthetic ${layout.name} diff`;
    await openRoute(
      diffLabel,
      config.synthetic,
      diffRoute(config.synthetic.ids.primary_revision),
      layout,
    );
    await page.getByRole("region", { name: "annotated diff" }).waitFor();
    await page
      .getByRole("heading", { name: /Decision context/ })
      .first()
      .waitFor();
    await expectNoOverflow(diffLabel);
    await expectText(diffLabel, [
      "Decision context (20)",
      "20 facts",
      "0 unanchored",
      "actor:agent:pointbreak-matrix-fact-writer",
      "actor:agent:pointbreak-matrix-participant-opener",
      "answered by actor:agent:pointbreak-matrix-participant-responder",
      "actor:agent:pointbreak-matrix-request-opener",
      "actor:agent:pointbreak-matrix-response-one",
      "actor:agent:pointbreak-matrix-response-two",
      "needs-changes",
      "replaced",
      "accepted-with-follow-up",
      "current",
      "failed then passed",
      "resolved by strictly later pass",
      "equal time",
      "outstanding",
    ]);
    await expectRef(diffLabel, config.synthetic.ids.primary_revision);
    await expectRef(diffLabel, config.synthetic.primaryObjectId);
    await screenshot(`synthetic-${layout.name}-diff`);

    const navigator = page
      .getByRole("navigation", { name: "diff files" })
      .getByRole("region", { name: "Decision context" });
    const firstFact = navigator.getByRole("button").first();
    await firstFact.click();
    await page
      .waitForFunction(() => location.hash.includes("focus="), undefined, {
        timeout: 5000,
      })
      .catch(() => fail(diffLabel, `navigator did not set ?focus=: ${page.url()}`));
    const firstFocus = page.url();
    if (layout.name === "wide") {
      await page.evaluate(() => {
        if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
      });
      await page.keyboard.press("p");
      await page
        .waitForFunction((previous) => location.href !== previous, firstFocus, {
          timeout: 5000,
        })
        .catch(() => fail(diffLabel, `p did not change focus: ${page.url()}`));
      const previousFocus = page.url();
      await page.keyboard.press("n");
      await page
        .waitForFunction((previous) => location.href !== previous, previousFocus, {
          timeout: 5000,
        })
        .catch(() => fail(diffLabel, `n did not change focus: ${page.url()}`));
    }
    await page.keyboard.press("Escape");
    await page
      .waitForFunction(() => !location.hash.includes("/diff"), undefined, {
        timeout: 5000,
      })
      .catch(() => fail(diffLabel, `Escape did not leave the diff: ${page.url()}`));
  }

  const wide = layouts[0];
  await openRoute("synthetic wide list", config.synthetic, "list", wide);
  await expectText("synthetic wide list", [
    "Decision continuity matrix",
    "Live landing matrix",
    "Unassessed matrix",
    "Supersession root",
    "Competing head A",
    "Competing head B",
    "Staged matrix",
    "Unstaged matrix",
    "Detached worktree matrix",
    "Missing object matrix",
    "superseded by",
    "ambiguous current assessment",
    "1 stale fact",
    "landing merged",
    "landing open",
    "landing unknown",
    "landing unreachable",
  ]);

  const detailCases = [
    {
      id: config.synthetic.ids.live_revision,
      terms: [
        "Live landing matrix",
        "work working-tree changes on feat/live-matrix",
        "landing live",
      ],
    },
    {
      id: config.synthetic.ids.unassessed_revision,
      terms: [
        "Unassessed matrix",
        "floating revision — no landing commit association recorded",
      ],
    },
    {
      id: config.synthetic.ids.range_revision,
      terms: [
        "Range matrix",
        "work range matrix target",
        "anchored capture target live",
        "no landing commit association recorded",
      ],
    },
    {
      id: config.synthetic.ids.root_revision,
      terms: [
        "Root matrix",
        "work range matrix target",
        "git_tree",
        "anchored capture target live",
      ],
    },
    {
      id: config.synthetic.ids.staged_revision,
      terms: ["Staged matrix", "work staged changes"],
    },
    {
      id: config.synthetic.ids.unstaged_revision,
      terms: [
        "Unstaged matrix",
        "work unstaged changes on feat/source-matrix",
      ],
    },
    {
      id: config.synthetic.ids.detached_revision,
      terms: [
        "Detached worktree matrix",
        "work working-tree changes",
        "floating revision — no landing commit association recorded",
      ],
    },
    {
      id: config.synthetic.ids.missing_revision,
      terms: [
        "Missing object matrix",
        "work commit range",
        "anchored capture target missing",
        "no landing commit association recorded",
      ],
    },
    {
      id: config.synthetic.ids.superseded_revision,
      terms: ["Supersession root", "competing heads", "Stale predecessor fact"],
      refs: [
        config.synthetic.ids.ambiguous_assessment_revision,
        config.synthetic.ids.competing_revision,
      ],
    },
    {
      id: config.synthetic.ids.ambiguous_assessment_revision,
      terms: [
        "Competing head A",
        "ambiguous current assessment",
        "accepted",
        "needs-changes",
        "Candidate A accepts.",
        "Candidate B requests changes.",
      ],
    },
  ];

  for (const entry of detailCases) {
    const label = `synthetic detail ${short(entry.id)}`;
    await openRoute(
      label,
      config.synthetic,
      revisionRoute(entry.id),
      wide,
    );
    await page.getByRole("heading", { name: "Revision", exact: true }).waitFor();
    await expectText(label, entry.terms);
    await expectRef(label, entry.id);
    for (const id of entry.refs ?? []) await expectRef(label, id);
  }

  await page.waitForTimeout(100);
  if (consoleErrors.length) {
    fail("browser console", consoleErrors.join("\n"));
  }
  if (pageErrors.length) fail("browser page", pageErrors.join("\n"));

  return {
    status: "passed",
    layouts: layouts.map(({ name, width, height, density }) => ({
      name,
      width,
      height,
      density,
    })),
    canonicalRevision: config.canonical.revisionId,
    syntheticRevision: config.synthetic.ids.primary_revision,
  };
})(__POINTBREAK_BROWSER_GATE_CONFIG__)
