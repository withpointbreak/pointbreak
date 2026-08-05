"use strict";
(() => {
  var __defProp = Object.defineProperty;
  var __name = (target, value) => __defProp(target, "name", { value, configurable: true });

  // src/auth.ts
  var SESSION_TOKEN_PREFIX = "pointbreak.inspect-token.v1:";
  var credentialVersion = 0;
  function sessionTokenKey(origin = location.origin) {
    return `${SESSION_TOKEN_PREFIX}${origin}`;
  }
  __name(sessionTokenKey, "sessionTokenKey");
  function getSessionToken() {
    return sessionStorage.getItem(sessionTokenKey());
  }
  __name(getSessionToken, "getSessionToken");
  function setSessionToken(token) {
    if (!token) throw new Error("invalid capability");
    sessionStorage.setItem(sessionTokenKey(), token);
    credentialVersion += 1;
  }
  __name(setSessionToken, "setSessionToken");
  function sessionCredentialVersion() {
    return credentialVersion;
  }
  __name(sessionCredentialVersion, "sessionCredentialVersion");
  function decoded(value) {
    try {
      return decodeURIComponent(value.replace(/\+/g, "%20"));
    } catch {
      throw new Error("invalid capability");
    }
  }
  __name(decoded, "decoded");
  function extractCapability(hash) {
    const prefixed = hash.startsWith("#") ? hash : `#${hash}`;
    const queryAt = prefixed.indexOf("?");
    if (queryAt < 0) return { token: null, cleanedHash: prefixed };
    const route = prefixed.slice(0, queryAt);
    const kept = [];
    const tokens = [];
    for (const pair of prefixed.slice(queryAt + 1).split("&")) {
      if (!pair) continue;
      const separator = pair.indexOf("=");
      const rawKey = separator < 0 ? pair : pair.slice(0, separator);
      const rawValue = separator < 0 ? "" : pair.slice(separator + 1);
      if (decoded(rawKey) === "token") tokens.push(decoded(rawValue));
      else kept.push(pair);
    }
    if (tokens.length > 1 || tokens.length === 1 && !tokens[0]) {
      throw new Error("invalid capability");
    }
    return {
      token: tokens[0] ?? null,
      cleanedHash: kept.length ? `${route}?${kept.join("&")}` : route
    };
  }
  __name(extractCapability, "extractCapability");
  function bootstrapCapability() {
    const result = extractCapability(location.hash);
    if (result.token !== null) {
      setSessionToken(result.token);
      history.replaceState(
        history.state,
        "",
        `${location.pathname}${location.search}${result.cleanedHash}`
      );
    }
    return result;
  }
  __name(bootstrapCapability, "bootstrapCapability");
  function isLoopbackLiteral(hostname) {
    const unbracketed = hostname.replace(/^\[|\]$/g, "").toLowerCase();
    if (unbracketed === "::1") return true;
    const octets = unbracketed.split(".");
    return octets.length === 4 && octets.every((octet) => /^\d+$/.test(octet) && Number(octet) <= 255) && Number(octets[0]) === 127;
  }
  __name(isLoopbackLiteral, "isLoopbackLiteral");
  function routeWithToken(route, token) {
    const cleaned = extractCapability(route).cleanedHash;
    const separator = cleaned.includes("?") ? "&" : "?";
    return `${cleaned}${separator}token=${encodeURIComponent(token)}`;
  }
  __name(routeWithToken, "routeWithToken");
  function resolveReconnectInput(input, currentOrigin, currentRoute) {
    const value = input.trim();
    if (!value) throw new Error("invalid capability URL");
    if (!/^[a-z][a-z0-9+.-]*:/i.test(value)) {
      return { kind: "retry", token: value };
    }
    let url;
    try {
      url = new URL(value);
    } catch {
      throw new Error("invalid capability URL");
    }
    if (url.protocol !== "http:" || !isLoopbackLiteral(url.hostname) || url.username || url.password) {
      throw new Error("invalid capability URL");
    }
    let extraction;
    try {
      extraction = extractCapability(url.hash);
    } catch {
      throw new Error("invalid capability URL");
    }
    if (!extraction.token) throw new Error("invalid capability URL");
    if (url.origin === currentOrigin) {
      return { kind: "retry", token: extraction.token };
    }
    return {
      kind: "navigate",
      url: `${url.origin}/${routeWithToken(currentRoute, extraction.token)}`
    };
  }
  __name(resolveReconnectInput, "resolveReconnectInput");
  var AuthCoordinator = class {
    constructor(options) {
      this.options = options;
    }
    options;
    static {
      __name(this, "AuthCoordinator");
    }
    recovery = null;
    recoverUnauthorized() {
      if (this.recovery) return this.recovery;
      this.recovery = this.promptAndApply().finally(() => {
        this.recovery = null;
      });
      return this.recovery;
    }
    reconnect() {
      return this.recoverUnauthorized();
    }
    async promptAndApply() {
      while (true) {
        const input = await this.options.prompt();
        if (input === null) {
          clearReconnectError();
          return false;
        }
        let target;
        try {
          target = resolveReconnectInput(
            input,
            this.options.currentOrigin(),
            this.options.currentRoute()
          );
        } catch {
          showReconnectError("Enter a token or an HTTP loopback capability URL.");
          continue;
        }
        clearReconnectError();
        if (target.kind === "retry") {
          setSessionToken(target.token);
          return true;
        }
        this.options.navigate(target.url);
        return false;
      }
    }
  };
  var installedCoordinator = null;
  function installAuthCoordinator(coordinator) {
    installedCoordinator = coordinator;
  }
  __name(installAuthCoordinator, "installAuthCoordinator");
  function installDefaultAuthCoordinator() {
    const coordinator = new AuthCoordinator({
      prompt: promptForCredential,
      navigate: /* @__PURE__ */ __name((url) => location.replace(url), "navigate"),
      currentOrigin: /* @__PURE__ */ __name(() => location.origin, "currentOrigin"),
      currentRoute: /* @__PURE__ */ __name(() => location.hash, "currentRoute")
    });
    installAuthCoordinator(coordinator);
    return coordinator;
  }
  __name(installDefaultAuthCoordinator, "installDefaultAuthCoordinator");
  function recoverUnauthorized() {
    return installedCoordinator?.recoverUnauthorized() ?? Promise.resolve(false);
  }
  __name(recoverUnauthorized, "recoverUnauthorized");
  function requestReconnect() {
    return installedCoordinator?.reconnect() ?? Promise.resolve(false);
  }
  __name(requestReconnect, "requestReconnect");
  function showReconnectError(message2) {
    const error = document.querySelector("#reconnect-error");
    if (!error) return;
    error.textContent = message2;
    error.classList.remove("hidden");
  }
  __name(showReconnectError, "showReconnectError");
  function clearReconnectError() {
    const error = document.querySelector("#reconnect-error");
    if (!error) return;
    error.textContent = "";
    error.classList.add("hidden");
  }
  __name(clearReconnectError, "clearReconnectError");
  function promptForCredential() {
    const dialog = document.querySelector("#reconnect-dialog");
    const form = dialog?.querySelector("form");
    const input = document.querySelector("#reconnect-input");
    const cancel = document.querySelector("#reconnect-cancel");
    if (!dialog || !form || !input || !cancel) return Promise.resolve(null);
    dialog.classList.remove("hidden");
    input.value = "";
    input.focus();
    return new Promise((resolve) => {
      let settled = false;
      const finish = /* @__PURE__ */ __name((value) => {
        if (settled) return;
        settled = true;
        form.removeEventListener("submit", onSubmit);
        cancel.removeEventListener("click", onCancel);
        input.value = "";
        dialog.classList.add("hidden");
        resolve(value);
      }, "finish");
      const onSubmit = /* @__PURE__ */ __name((event) => {
        event.preventDefault();
        finish(input.value);
      }, "onSubmit");
      const onCancel = /* @__PURE__ */ __name(() => finish(null), "onCancel");
      form.addEventListener("submit", onSubmit);
      cancel.addEventListener("click", onCancel);
    });
  }
  __name(promptForCredential, "promptForCredential");

  // src/connection.ts
  var snapshot = {
    connection: "connecting",
    refresh: "idle"
  };
  function connectionPresentation(state) {
    const refreshLabel = state.refresh === "degraded" ? "response error" : state.refresh;
    switch (state.connection) {
      case "unauthorized":
        return {
          serverLabel: "local server",
          connectionLabel: "authentication required",
          refreshLabel,
          action: "Reconnect",
          canConnectAnother: false
        };
      case "unreachable":
        return {
          serverLabel: "local server",
          connectionLabel: "server unavailable",
          refreshLabel,
          action: "Retry",
          canConnectAnother: true
        };
      case "connected":
        return {
          serverLabel: "local server",
          connectionLabel: "connected",
          refreshLabel,
          action: state.refresh === "degraded" ? "Retry" : null,
          canConnectAnother: false
        };
      case "connecting":
        return {
          serverLabel: "local server",
          connectionLabel: "connecting",
          refreshLabel,
          action: null,
          canConnectAnother: false
        };
    }
  }
  __name(connectionPresentation, "connectionPresentation");
  function markRequestSuccess() {
    snapshot = {
      connection: "connected",
      refresh: snapshot.refresh
    };
    renderConnectionChrome();
  }
  __name(markRequestSuccess, "markRequestSuccess");
  function markRequestFailure(kind) {
    snapshot = kind === "protocol" ? { connection: "connected", refresh: "degraded" } : { ...snapshot, connection: kind };
    renderConnectionChrome();
  }
  __name(markRequestFailure, "markRequestFailure");
  var actions = null;
  function configureConnectionActions(next) {
    actions = next;
  }
  __name(configureConnectionActions, "configureConnectionActions");
  function initConnectionControls() {
    document.querySelector("#connection-action")?.addEventListener("click", () => {
      if (!actions) return;
      if (snapshot.connection === "unauthorized") void actions.reconnect();
      else void actions.retry();
    });
    document.querySelector("#connect-another")?.addEventListener("click", () => {
      if (actions) void actions.reconnect();
    });
    renderConnectionChrome();
  }
  __name(initConnectionControls, "initConnectionControls");
  function renderConnectionChrome() {
    const presentation = connectionPresentation(snapshot);
    const root = document.querySelector("#store-identity");
    root?.classList.remove("hidden");
    const connection = document.querySelector("#connection-status");
    if (connection) connection.textContent = presentation.connectionLabel;
    const refresh2 = document.querySelector("#refresh-status");
    if (refresh2) refresh2.textContent = presentation.refreshLabel;
    const legacyRefresh = document.querySelector("#stat-live");
    if (legacyRefresh) {
      legacyRefresh.textContent = presentation.refreshLabel;
      legacyRefresh.dataset.state = snapshot.refresh;
    }
    const dot = document.querySelector("#refresh");
    if (dot) {
      dot.dataset.connection = snapshot.connection;
      dot.dataset.state = snapshot.refresh;
      dot.title = `${presentation.connectionLabel}; refresh ${presentation.refreshLabel}`;
    }
    const action = document.querySelector("#connection-action");
    if (action) {
      action.textContent = presentation.action ?? "";
      action.classList.toggle("hidden", presentation.action === null);
    }
    document.querySelector("#connect-another")?.classList.toggle("hidden", !presentation.canConnectAnother);
    const word = document.querySelector("#refresh-word");
    if (word) {
      word.textContent = snapshot.connection === "unauthorized" ? "authentication required" : snapshot.connection === "unreachable" ? "server unavailable" : snapshot.refresh === "degraded" ? "response error" : "";
    }
  }
  __name(renderConnectionChrome, "renderConnectionChrome");

  // src/http.ts
  var RequestFailure = class extends Error {
    constructor(kind, status) {
      super(
        kind === "unauthorized" ? "authentication required" : kind === "unreachable" ? "server unavailable" : "server response error"
      );
      this.kind = kind;
      this.status = status;
      this.name = "RequestFailure";
    }
    kind;
    status;
    static {
      __name(this, "RequestFailure");
    }
  };
  function failure(kind, status) {
    markRequestFailure(kind);
    return new RequestFailure(kind, status);
  }
  __name(failure, "failure");
  function expectedDocument(path) {
    const pathname = new URL(path, location.origin).pathname;
    const collections = {
      "/api/v2/profile": {
        schema: "pointbreak.inspect-reader-profile",
        version: 1
      },
      "/api/v2/changes": {
        schema: "pointbreak.inspect-changes-page",
        version: 1
      },
      "/api/v2/attention": {
        schema: "pointbreak.inspect-attention",
        version: 2
      },
      "/api/attention": { schema: "pointbreak.inspect-attention" },
      "/api/derived-access/status": {
        schema: "pointbreak.inspect-derived-access-status",
        version: 1
      },
      "/api/freshness": {
        schema: "pointbreak.inspect-freshness",
        version: 1
      },
      "/api/history": { schema: "pointbreak.inspect-history" },
      "/api/history/new-count": {
        schema: "pointbreak.inspect-history-new-count"
      },
      "/api/identity": { schema: "pointbreak.inspect-identity" },
      "/api/revisions": { schema: "pointbreak.inspect-revisions-page.v1" },
      "/api/threads": { schema: "pointbreak.inspect-threads" },
      "/api/version": { schema: "pointbreak.version", version: 1 }
    };
    if (collections[pathname]) return collections[pathname];
    if (pathname === "/api/derived-access/cancel" || pathname === "/api/derived-access/retry") {
      return {
        schema: "pointbreak.inspect-derived-access-status",
        version: 1
      };
    }
    if (/^\/api\/revisions\/[^/]+$/.test(pathname)) {
      return { schema: "pointbreak.review-revision", version: 2 };
    }
    if (/^\/api\/snapshots\/[^/]+$/.test(pathname)) {
      return { schema: "pointbreak.review-snapshot", version: 1 };
    }
    if (/^\/api\/v2\/changes\/[^/]+$/.test(pathname)) {
      return { schema: "pointbreak.review-change", version: 1 };
    }
    if (/^\/api\/v2\/changes\/[^/]+\/revisions\/[^/]+$/.test(pathname)) {
      return { schema: "pointbreak.review-change-revision", version: 1 };
    }
    if (/^\/api\/v2\/changes\/[^/]+\/revisions\/[^/]+\/resource$/.test(pathname)) {
      return { schema: "pointbreak.review-revision-resource", version: 1 };
    }
    if (/^\/api\/v2\/changes\/[^/]+\/interdiff\/[^/]+\/[^/]+$/.test(pathname)) {
      return { schema: "pointbreak.review-revision-interdiff", version: 1 };
    }
    return null;
  }
  __name(expectedDocument, "expectedDocument");
  function isExpectedDocument(data, expected) {
    if (typeof data !== "object" || data === null) return false;
    const document2 = data;
    return document2.schema === expected.schema && (expected.version === void 0 || document2.version === expected.version);
  }
  __name(isExpectedDocument, "isExpectedDocument");
  function hasPayloadError(data) {
    return typeof data === "object" && data !== null && "error" in data && Boolean(data.error);
  }
  __name(hasPayloadError, "hasPayloadError");
  async function fetchOnce(path, method) {
    const headers = {};
    const token = getSessionToken();
    if (token) headers.Authorization = `Bearer ${token}`;
    let response;
    try {
      response = await fetch(path, {
        method,
        cache: "no-store",
        credentials: "omit",
        referrerPolicy: "no-referrer",
        headers
      });
    } catch {
      throw failure("unreachable");
    }
    if (response.status === 401) throw new RequestFailure("unauthorized", 401);
    let text;
    try {
      text = await response.text();
    } catch {
      throw failure("protocol", response.status);
    }
    if (!response.ok) throw failure("protocol", response.status);
    let data;
    try {
      data = JSON.parse(text);
    } catch {
      throw failure("protocol", response.status);
    }
    const expected = expectedDocument(path);
    if (hasPayloadError(data) || expected !== null && !isExpectedDocument(data, expected)) {
      throw failure("protocol", response.status);
    }
    markRequestSuccess();
    return data;
  }
  __name(fetchOnce, "fetchOnce");
  async function fetchJSON(path, method = "GET") {
    const requestCredentialVersion = sessionCredentialVersion();
    try {
      return await fetchOnce(path, method);
    } catch (error) {
      if (!(error instanceof RequestFailure) || error.kind !== "unauthorized") {
        throw error;
      }
    }
    const credentialAlreadyRenewed = sessionCredentialVersion() !== requestCredentialVersion;
    if (credentialAlreadyRenewed || await recoverUnauthorized()) {
      try {
        return await fetchOnce(path, method);
      } catch (error) {
        if (error instanceof RequestFailure && error.kind === "unauthorized") {
          throw failure("unauthorized", 401);
        }
        throw error;
      }
    }
    throw failure("unauthorized", 401);
  }
  __name(fetchJSON, "fetchJSON");

  // src/change-bootstrap.ts
  var REQUIRED_DOCUMENTS = {
    "pointbreak.inspect-reader-profile": 1,
    "pointbreak.inspect-changes-page": 1,
    "pointbreak.review-change": 1,
    "pointbreak.review-change-revision": 1,
    "pointbreak.review-revision": 3,
    "pointbreak.review-revision-resource": 1,
    "pointbreak.review-association-comparison": 1,
    "pointbreak.review-revision-interdiff": 1,
    "pointbreak.inspect-attention": 2,
    "pointbreak.reader-upgrade-required": 1,
    "pointbreak.store-migration-required": 1,
    "pointbreak.store-migration-in-progress": 1
  };
  var pollTimer = null;
  var generation = 0;
  var connectionControlsInitialized = false;
  async function bootstrapChangeReader(options = {}) {
    stopChangeReader();
    const capability = bootstrapCapability();
    if (capability.token !== null) {
      (options.reload ?? (() => location.reload()))();
      return;
    }
    installDefaultAuthCoordinator();
    const retry = /* @__PURE__ */ __name(() => bootstrapChangeReader(options), "retry");
    configureConnectionActions({
      retry,
      reconnect: /* @__PURE__ */ __name(async () => {
        if (await requestReconnect()) await retry();
      }, "reconnect")
    });
    if (!connectionControlsInitialized) {
      initConnectionControls();
      connectionControlsInitialized = true;
    }
    try {
      const profile = validateProfile(
        await fetchJSON("/api/v2/profile")
      );
      prepareChangeShell();
      if (profile.availability !== "ready") {
        renderUnavailable(profile.availability);
        return;
      }
      await loadGeneration(profile);
      if (options.poll !== false) {
        pollTimer = setInterval(() => {
          void refresh();
        }, 3e3);
      }
    } catch (error) {
      renderRefusal(error);
    }
  }
  __name(bootstrapChangeReader, "bootstrapChangeReader");
  function stopChangeReader() {
    generation += 1;
    if (pollTimer !== null) {
      clearInterval(pollTimer);
      pollTimer = null;
    }
  }
  __name(stopChangeReader, "stopChangeReader");
  async function refresh() {
    try {
      const profile = validateProfile(
        await fetchJSON("/api/v2/profile")
      );
      if (profile.availability !== "ready") {
        renderUnavailable(profile.availability);
        stopChangeReader();
        return;
      }
      await loadGeneration(profile);
    } catch (error) {
      renderRefusal(error);
    }
  }
  __name(refresh, "refresh");
  function validateProfile(profile) {
    if (profile.schema !== "pointbreak.inspect-reader-profile" || profile.version !== 1 || !["migration_required", "migration_in_progress", "ready"].includes(
      profile.availability
    )) {
      throw new Error("incompatible Inspector reader profile");
    }
    for (const [schema, version] of Object.entries(REQUIRED_DOCUMENTS)) {
      if (profile.documents?.[schema] !== version) {
        throw new Error(`reader profile is missing ${schema} v${version}`);
      }
    }
    if (profile.availability === "ready" && profile.minimumReaderProfile !== "review_change_revision_v1") {
      throw new Error("incompatible minimum reader profile");
    }
    return profile;
  }
  __name(validateProfile, "validateProfile");
  async function loadGeneration(profile) {
    const requestedGeneration = ++generation;
    const [changes, attention] = await Promise.all([
      fetchJSON("/api/v2/changes"),
      fetchJSON("/api/v2/attention")
    ]);
    if (requestedGeneration !== generation) return;
    if (changes.schema !== "pointbreak.inspect-changes-page" || changes.version !== 1 || attention.schema !== "pointbreak.inspect-attention" || attention.version !== 2 || !changes.projectionStamp || changes.projectionStamp !== attention.projectionStamp || changes.changes.some(
      (change) => change.projectionStamp !== changes.projectionStamp
    ) || attention.changes.some(
      (change) => change.projectionStamp !== changes.projectionStamp
    )) {
      throw new Error("Change documents do not form one coherent generation");
    }
    renderGeneration(profile, changes, attention);
  }
  __name(loadGeneration, "loadGeneration");
  function prepareChangeShell() {
    document.querySelector("#toolbar")?.classList.add("hidden");
    document.querySelector("#view-controls")?.classList.add("hidden");
    document.querySelector("#derived-access-status")?.classList.add("hidden");
    const switcher = document.querySelector("#lens-switcher");
    if (switcher) {
      switcher.replaceChildren();
      const label = document.createElement("strong");
      label.textContent = "Changes";
      switcher.append(label);
    }
    const master = document.querySelector("#master");
    master?.setAttribute("aria-label", "Changes");
    const detail = document.querySelector("#detail-body");
    if (detail) {
      detail.replaceChildren(message("Select a Change or exact Revision."));
    }
  }
  __name(prepareChangeShell, "prepareChangeShell");
  function renderUnavailable(availability) {
    clearSemanticPresentation();
    const master = document.querySelector("#master");
    if (!master) return;
    master.replaceChildren(
      message(
        availability === "migration_required" ? "Store migration required. No Change state was loaded." : "Store migration in progress. Partial Change state is unavailable."
      )
    );
  }
  __name(renderUnavailable, "renderUnavailable");
  function renderGeneration(profile, page, attention) {
    const master = document.querySelector("#master");
    if (!master) return;
    const fragment = document.createDocumentFragment();
    const heading2 = document.createElement("h1");
    heading2.textContent = `Changes · ${page.changes.length}`;
    fragment.append(heading2);
    for (const change of page.changes) {
      const card = document.createElement("article");
      card.dataset.changeId = change.changeId;
      card.className = "unit-card";
      const open = document.createElement("button");
      open.type = "button";
      open.className = "ghost mono";
      open.textContent = change.changeId;
      open.addEventListener("click", () => {
        void loadChangeDetail(change);
      });
      card.append(open);
      card.append(
        line(`topology: ${words(change.topology)}`),
        line(`lifecycle: ${words(change.lifecycle)}`),
        line(`attention: ${words(change.attentionSummary)}`),
        line(`availability: ${words(change.availabilitySummary)}`)
      );
      const revisions = document.createElement("div");
      revisions.className = "change-current-revisions";
      for (const revision of change.currentRevisionRefs) {
        const select = document.createElement("button");
        select.type = "button";
        select.className = "ghost mono";
        select.dataset.revisionId = revision.revisionId;
        select.textContent = revision.revisionId;
        select.addEventListener("click", () => {
          void loadRevisionDetail(change, revision);
        });
        revisions.append(select);
      }
      card.append(revisions);
      if (change.currentRevisionRefs.length > 1) {
        const compare = document.createElement("button");
        compare.type = "button";
        compare.className = "ghost";
        compare.textContent = "Compare exact Revisions";
        compare.addEventListener("click", () => {
          void loadInterdiff(
            change,
            change.currentRevisionRefs[0],
            change.currentRevisionRefs[1]
          );
        });
        card.append(compare);
      }
      fragment.append(card);
    }
    if (page.changes.length === 0) fragment.append(message("No Changes."));
    master.replaceChildren(fragment);
    setText(
      "#stat-events",
      `${profile.authorityCursor.eventCount ?? "—"} events`
    );
    setText("#stat-units", `${page.changes.length} Changes`);
    setText("#stat-threads", `${attention.changes.length} need attention`);
    setText("#stat-hash", page.projectionStamp);
  }
  __name(renderGeneration, "renderGeneration");
  async function loadChangeDetail(change) {
    try {
      const detail = await fetchJSON(
        `/api/v2/changes/${encodeURIComponent(change.changeId)}`
      );
      if (detail.summary.changeId !== change.changeId || detail.projectionStamp !== change.projectionStamp) {
        throw new Error("Change detail generation is stale; refresh and retry");
      }
      const body = detailBody();
      body.append(
        heading(change.changeId),
        line(`topology: ${words(detail.summary.topology)}`),
        line(`lifecycle: ${words(detail.summary.lifecycle)}`)
      );
      const relations = document.createElement("section");
      relations.append(heading("Relation claims", 3));
      for (const claim of detail.relationClaims) {
        const supports = claim.supports.map((support) => `${support.actorId}/${support.eventId}`).join(", ");
        const withdrawals = claim.withdrawals.map((withdrawal) => `${withdrawal.actorId}/${withdrawal.eventId}`).join(", ");
        relations.append(
          line(
            `${claim.active ? "active" : "withdrawn"} ${claim.claimId}: ${claim.successor.revisionId} replaces ${claim.predecessor.revisionId} · support ${supports || "none"} · withdrawal ${withdrawals || "none"}`
          )
        );
      }
      if (detail.relationClaims.length === 0) {
        relations.append(message("No relation claims."));
      }
      body.append(relations);
    } catch (error) {
      renderRefusal(error);
    }
  }
  __name(loadChangeDetail, "loadChangeDetail");
  async function loadRevisionDetail(change, revision) {
    try {
      const params = new URLSearchParams({
        artifactHash: revision.objectArtifactContentHash
      });
      const detail = await fetchJSON(
        `/api/v2/changes/${encodeURIComponent(change.changeId)}/revisions/${encodeURIComponent(revision.revisionId)}?${params}`
      );
      if (detail.changeId !== change.changeId || !sameRevision(detail.revision, revision) || detail.projectionStamp !== change.projectionStamp || detail.associations.some(
        (association) => !sameRevision(association.comparison.revision, revision)
      )) {
        throw new Error("Revision detail generation is stale; refresh and retry");
      }
      const body = detailBody();
      body.append(
        heading(revision.revisionId),
        line(`currency: ${words(detail.revisionCurrency)}`),
        line(`relation: ${words(detail.relationClassification)}`),
        line(`captured resource: ${words(detail.availability)}`)
      );
      const facts = document.createElement("section");
      facts.append(heading("Facts", 3));
      for (const fact of detail.factPresentations) {
        facts.append(
          line(
            `${fact.family}: ${fact.factId} · origin ${fact.originRevision.revisionId} · ${words(fact.revisionCurrency)} · ${words(fact.familyState)} · ${words(fact.availability)}`
          )
        );
      }
      if (detail.factPresentations.length === 0)
        facts.append(message("No facts."));
      body.append(facts);
      const associations = document.createElement("section");
      associations.append(heading("Association comparisons", 3));
      for (const association of detail.associations) {
        associations.append(
          line(
            `${association.comparison.commitOid} · ${words(association.state)} · proof ${words(association.proofAvailability)}`
          )
        );
      }
      if (detail.associations.length === 0) {
        associations.append(message("No association comparisons."));
      }
      body.append(associations);
      const resource = document.createElement("button");
      resource.type = "button";
      resource.className = "ghost";
      resource.textContent = "Open exact captured resource";
      resource.addEventListener("click", () => {
        void loadRevisionResource(change, revision);
      });
      body.append(resource);
    } catch (error) {
      renderRefusal(error);
    }
  }
  __name(loadRevisionDetail, "loadRevisionDetail");
  async function loadRevisionResource(change, revision) {
    try {
      const params = new URLSearchParams({
        artifactHash: revision.objectArtifactContentHash
      });
      const resource = await fetchJSON(
        `/api/v2/changes/${encodeURIComponent(change.changeId)}/revisions/${encodeURIComponent(revision.revisionId)}/resource?${params}`
      );
      if (!sameRevision(resource.resource.revision, revision)) {
        throw new Error(
          "captured resource identity does not match its exact route"
        );
      }
      const body = detailBody();
      body.append(
        heading(`Captured resource · ${revision.revisionId}`),
        line(`availability: ${words(resource.availability)}`)
      );
      if (resource.capturedDocumentHash) {
        body.append(line(`document hash: ${resource.capturedDocumentHash}`));
      }
      if (resource.capturedDocument !== void 0) {
        const captured = document.createElement("pre");
        captured.textContent = JSON.stringify(resource.capturedDocument, null, 2);
        body.append(captured);
      }
      for (const diagnostic of resource.diagnostics)
        body.append(line(diagnostic));
    } catch (error) {
      renderRefusal(error);
    }
  }
  __name(loadRevisionResource, "loadRevisionResource");
  async function loadInterdiff(change, from, to) {
    try {
      const params = new URLSearchParams({
        fromArtifactHash: from.objectArtifactContentHash,
        toArtifactHash: to.objectArtifactContentHash
      });
      const interdiff = await fetchJSON(
        `/api/v2/changes/${encodeURIComponent(change.changeId)}/interdiff/${encodeURIComponent(from.revisionId)}/${encodeURIComponent(to.revisionId)}?${params}`
      );
      if (!sameRevision(interdiff.interdiff.from, from) || !sameRevision(interdiff.interdiff.to, to)) {
        throw new Error(
          "Revision interdiff identity does not match its exact route"
        );
      }
      const body = detailBody();
      body.append(
        heading(`Revision interdiff · ${from.revisionId} → ${to.revisionId}`),
        line(`availability: ${words(interdiff.availability)}`)
      );
      if (interdiff.comparison !== void 0) {
        const comparison = document.createElement("pre");
        comparison.textContent = JSON.stringify(interdiff.comparison, null, 2);
        body.append(comparison);
      }
      for (const diagnostic of interdiff.diagnostics)
        body.append(line(diagnostic));
    } catch (error) {
      renderRefusal(error);
    }
  }
  __name(loadInterdiff, "loadInterdiff");
  function detailBody() {
    const body = document.querySelector("#detail-body");
    if (!body) throw new Error("Inspector detail container is absent");
    body.replaceChildren();
    return body;
  }
  __name(detailBody, "detailBody");
  function renderRefusal(error) {
    const text = error instanceof Error ? error.message : String(error);
    clearSemanticPresentation();
    const master = document.querySelector("#master");
    master?.replaceChildren(message(`Reader refused: ${text}`));
    const banner = document.querySelector("#error");
    if (banner) {
      banner.textContent = `error: ${text}`;
      banner.classList.remove("hidden");
    }
  }
  __name(renderRefusal, "renderRefusal");
  function clearSemanticPresentation() {
    document.querySelector("#master")?.replaceChildren();
    document.querySelector("#detail-body")?.replaceChildren();
    for (const selector of [
      "#stat-events",
      "#stat-units",
      "#stat-threads",
      "#stat-hash"
    ]) {
      setText(selector, "—");
    }
  }
  __name(clearSemanticPresentation, "clearSemanticPresentation");
  function sameRevision(left, right) {
    return left.revisionId === right.revisionId && left.objectArtifactContentHash === right.objectArtifactContentHash;
  }
  __name(sameRevision, "sameRevision");
  function words(value) {
    return value.replaceAll("_", " ");
  }
  __name(words, "words");
  function setText(selector, value) {
    const element = document.querySelector(selector);
    if (element) element.textContent = value;
  }
  __name(setText, "setText");
  function message(text) {
    const paragraph = document.createElement("p");
    paragraph.className = "empty";
    paragraph.textContent = text;
    return paragraph;
  }
  __name(message, "message");
  function line(text) {
    const paragraph = document.createElement("p");
    paragraph.textContent = text;
    return paragraph;
  }
  __name(line, "line");
  function heading(text, level = 2) {
    const element = document.createElement(`h${level}`);
    element.textContent = text;
    return element;
  }
  __name(heading, "heading");

  // src/entry.ts
  void bootstrapChangeReader();
})();
