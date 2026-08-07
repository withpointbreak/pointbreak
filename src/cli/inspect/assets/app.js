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
  function resolveReconnectInput(input, currentOrigin, currentRoute2) {
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
      url: `${url.origin}/${routeWithToken(currentRoute2, extraction.token)}`
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
    const refresh = document.querySelector("#refresh-status");
    if (refresh) refresh.textContent = presentation.refreshLabel;
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

  // src/change-inspector-http.ts
  var ChangeInspectorRequestFailure = class extends Error {
    constructor(kind, status) {
      super(
        kind === "unauthorized" ? "authentication required" : kind === "unreachable" ? "server unavailable" : "server response error"
      );
      this.kind = kind;
      this.status = status;
    }
    kind;
    status;
    static {
      __name(this, "ChangeInspectorRequestFailure");
    }
  };
  var ChangeInspectorPageFailure = class extends ChangeInspectorRequestFailure {
    constructor(code, status) {
      super("protocol", status);
      this.code = code;
    }
    code;
    static {
      __name(this, "ChangeInspectorPageFailure");
    }
  };
  function failure(kind, status) {
    markRequestFailure(kind);
    return new ChangeInspectorRequestFailure(kind, status);
  }
  __name(failure, "failure");
  function typedPageFailure(value, status) {
    if (typeof value !== "object" || value === null) return null;
    const document2 = value;
    if (document2.schema !== "pointbreak.inspect-change-page-error" || document2.version !== 1)
      return null;
    if (document2.code === "invalid_query" && status === 400)
      return new ChangeInspectorPageFailure("invalid_query", status);
    if (document2.code === "stale_projection" && status === 409)
      return new ChangeInspectorPageFailure("stale_projection", status);
    return null;
  }
  __name(typedPageFailure, "typedPageFailure");
  async function fetchOnce(path) {
    const headers = {};
    const token = getSessionToken();
    if (token) headers.Authorization = `Bearer ${token}`;
    let response;
    try {
      response = await fetch(path, {
        method: "GET",
        cache: "no-store",
        credentials: "omit",
        referrerPolicy: "no-referrer",
        headers
      });
    } catch {
      throw failure("unreachable");
    }
    if (response.status === 401)
      throw new ChangeInspectorRequestFailure("unauthorized", 401);
    let data;
    try {
      data = JSON.parse(await response.text());
    } catch {
      throw failure("protocol", response.status);
    }
    if (!response.ok)
      throw typedPageFailure(data, response.status) ?? failure("protocol", response.status);
    if (typeof data !== "object" || data === null || "error" in data && Boolean(data.error)) {
      throw failure("protocol", response.status);
    }
    markRequestSuccess();
    return data;
  }
  __name(fetchOnce, "fetchOnce");
  async function fetchChangeInspectorJSON(path) {
    const credentialVersion2 = sessionCredentialVersion();
    try {
      return await fetchOnce(path);
    } catch (error) {
      if (!(error instanceof ChangeInspectorRequestFailure) || error.kind !== "unauthorized")
        throw error;
    }
    if (sessionCredentialVersion() !== credentialVersion2 || await recoverUnauthorized())
      return fetchOnce(path);
    throw failure("unauthorized", 401);
  }
  __name(fetchChangeInspectorJSON, "fetchChangeInspectorJSON");

  // src/change-inspector-cards.ts
  function words(value) {
    return value.replaceAll("_", " ");
  }
  __name(words, "words");
  function shortExact(revision) {
    const revisionId = revision.revisionId.length > 24 ? `${revision.revisionId.slice(0, 24)}…` : revision.revisionId;
    const artifact = revision.objectArtifactContentHash.length > 18 ? `${revision.objectArtifactContentHash.slice(0, 18)}…` : revision.objectArtifactContentHash;
    return `${revisionId} · ${artifact}`;
  }
  __name(shortExact, "shortExact");
  function changeCardPresentation(summary, presentation) {
    const byExactIdentity = new Map(
      (presentation?.currentRevisions ?? []).map((entry) => [
        `${entry.revision.revisionId}\0${entry.revision.objectArtifactContentHash}`,
        entry
      ])
    );
    return {
      changeId: summary.changeId,
      badges: [
        summary.topology,
        summary.lifecycle,
        summary.attentionSummary,
        summary.availabilitySummary
      ].map(words),
      peers: summary.currentRevisionRefs.map((revision) => {
        const entry = byExactIdentity.get(
          `${revision.revisionId}\0${revision.objectArtifactContentHash}`
        );
        const summaryLabel = entry?.summarySource === "revision_proposal_summary" ? entry.revisionProposalSummary : void 0;
        return {
          revision,
          label: summaryLabel ? `Current Revision — ${summaryLabel}` : `Current Revision — ${shortExact(revision)}`,
          copyText: `${revision.revisionId} ${revision.objectArtifactContentHash}`
        };
      })
    };
  }
  __name(changeCardPresentation, "changeCardPresentation");

  // src/change-inspector-router.ts
  var QUERY_KEYS = [
    "q",
    "topology",
    "lifecycle",
    "attention",
    "availability",
    "after",
    "limit",
    "order"
  ];
  var ROUTE_QUERY_KEYS = /* @__PURE__ */ new Set([...QUERY_KEYS, "artifactHash"]);
  function decodeSegment(value) {
    try {
      const decoded2 = decodeURIComponent(value);
      return decoded2.length > 0 ? decoded2 : null;
    } catch {
      return null;
    }
  }
  __name(decodeSegment, "decodeSegment");
  function validQueryEncoding(search) {
    if (!search) return true;
    return search.split("&").every((component) => {
      if (!component) return false;
      const separator = component.indexOf("=");
      const key = separator === -1 ? component : component.slice(0, separator);
      const value = separator === -1 ? "" : component.slice(separator + 1);
      try {
        decodeURIComponent(key.replaceAll("+", " "));
        decodeURIComponent(value.replaceAll("+", " "));
        return true;
      } catch {
        return false;
      }
    });
  }
  __name(validQueryEncoding, "validQueryEncoding");
  function parseQuery(search) {
    if (!validQueryEncoding(search)) {
      return { message: "Malformed route query encoding." };
    }
    const params = new URLSearchParams(search);
    const query = {};
    for (const key of params.keys()) {
      if (!ROUTE_QUERY_KEYS.has(key)) {
        return { message: `Unknown ${key} route query.` };
      }
    }
    for (const key of QUERY_KEYS) {
      const values = params.getAll(key);
      if (values.length > 1) return { message: `Duplicate ${key} route query.` };
      const value = values[0];
      if (value === void 0) continue;
      if (key === "limit") {
        const limit = Number(value);
        if (!Number.isInteger(limit))
          return { message: "Invalid limit route query." };
        query.limit = limit;
      } else if (key === "order") {
        if (value !== "change_id_asc") {
          return { message: "Invalid order route query." };
        }
        query.order = value;
      } else {
        query[key] = value;
      }
    }
    return { query, artifactHashes: params.getAll("artifactHash") };
  }
  __name(parseQuery, "parseQuery");
  function isParseError(value) {
    return "message" in value;
  }
  __name(isParseError, "isParseError");
  function parseChangeInspectorRoute(hash) {
    const raw = hash.startsWith("#") ? hash.slice(1) : hash;
    const separator = raw.indexOf("?");
    const path = separator === -1 ? raw : raw.slice(0, separator);
    const search = separator === -1 ? "" : raw.slice(separator + 1);
    const parsed = parseQuery(search);
    if (isParseError(parsed)) return { kind: "invalid", message: parsed.message };
    const { query, artifactHashes } = parsed;
    const segments = path.split("/").filter(Boolean);
    if (segments.length === 1 && (segments[0] === "changes" || segments[0] === "attention")) {
      if (artifactHashes.length > 0) {
        return {
          kind: "invalid",
          message: "artifactHash is only valid on an exact Revision route."
        };
      }
      return { kind: "lens", lens: segments[0], query };
    }
    if (segments[0] !== "changes")
      return { kind: "invalid", message: "Unknown Change Inspector route." };
    const changeId = decodeSegment(segments[1] ?? "");
    if (changeId === null)
      return { kind: "invalid", message: "Change routes require a Change ID." };
    if (segments.length === 2) {
      if (artifactHashes.length > 0) {
        return {
          kind: "invalid",
          message: "artifactHash is only valid on an exact Revision route."
        };
      }
      return { kind: "change", changeId, query };
    }
    if (segments.length !== 4 || segments[2] !== "revisions") {
      return { kind: "invalid", message: "Unknown Change Inspector route." };
    }
    const revisionId = decodeSegment(segments[3]);
    if (revisionId === null)
      return {
        kind: "invalid",
        message: "Revision routes require a Revision ID."
      };
    if (artifactHashes.length !== 1 || !artifactHashes[0])
      return {
        kind: "invalid",
        message: artifactHashes.length > 1 ? "Exact Revision routes require exactly one artifactHash." : "Exact Revision routes require artifactHash."
      };
    return {
      kind: "revision",
      changeId,
      revision: {
        revisionId,
        objectArtifactContentHash: artifactHashes[0]
      },
      query
    };
  }
  __name(parseChangeInspectorRoute, "parseChangeInspectorRoute");
  function appendQuery(query, params) {
    for (const key of QUERY_KEYS) {
      const value = query[key];
      if (value !== void 0) params.set(key, String(value));
    }
  }
  __name(appendQuery, "appendQuery");
  function formatChangeInspectorRoute(route) {
    const params = new URLSearchParams();
    appendQuery(route.query, params);
    if (route.kind === "revision")
      params.set("artifactHash", route.revision.objectArtifactContentHash);
    const suffix = params.size ? `?${params}` : "";
    if (route.kind === "lens") return `#/${route.lens}${suffix}`;
    const change = encodeURIComponent(route.changeId);
    if (route.kind === "change") return `#/changes/${change}${suffix}`;
    return `#/changes/${change}/revisions/${encodeURIComponent(route.revision.revisionId)}${suffix}`;
  }
  __name(formatChangeInspectorRoute, "formatChangeInspectorRoute");
  function lensForRoute(route) {
    return route.kind === "lens" ? route.lens : "changes";
  }
  __name(lensForRoute, "lensForRoute");

  // src/change-inspector-render.ts
  var FILTER_OPTIONS = [
    [
      "topology",
      [
        "initial",
        "replacement",
        "replacement_divergent",
        "consolidation",
        "parallel_current",
        "mixed",
        "incomplete",
        "cycle_conflicted"
      ]
    ],
    ["lifecycle", ["incomplete", "conflicted", "in_progress", "accepted"]],
    ["attention", ["clear", "in_progress", "incomplete", "conflicted"]],
    ["availability", ["available", "incomplete"]]
  ];
  function message(text) {
    const element = document.createElement("p");
    element.className = "empty";
    element.textContent = text;
    return element;
  }
  __name(message, "message");
  function setText(selector, value) {
    const element = document.querySelector(selector);
    if (element) element.textContent = value;
  }
  __name(setText, "setText");
  function prepareChangeInspectorShell(actions2) {
    document.querySelector("#view-controls")?.classList.add("hidden");
    document.querySelector("#derived-access-status")?.classList.add("hidden");
    document.querySelector("#follow-toggle")?.classList.add("hidden");
    const switcher = document.querySelector("#lens-switcher");
    if (switcher) {
      switcher.replaceChildren();
      for (const lens of ["changes", "attention"]) {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "lens-tab";
        button.dataset.lens = lens;
        button.textContent = lens === "changes" ? "Changes" : "Attention";
        button.addEventListener(
          "click",
          () => actions2.navigate({ kind: "lens", lens, query: {} })
        );
        switcher.append(button);
      }
    }
    const back = document.querySelector("#detail-back");
    if (back) {
      back.textContent = "‹ Changes";
      back.onclick = () => actions2.navigate({ kind: "lens", lens: "changes", query: {} });
    }
    const search = document.querySelector("#filter-text");
    if (search) search.placeholder = "Search Changes and current Revisions";
    const filterTypes = document.querySelector("#filter-types");
    if (filterTypes) {
      filterTypes.replaceChildren();
      const heading = document.createElement("h2");
      heading.id = "filter-types-label";
      heading.className = "control-heading";
      heading.textContent = "Change status";
      filterTypes.append(heading);
      for (const [name, values] of FILTER_OPTIONS) {
        const label = document.createElement("label");
        label.textContent = name.replaceAll("_", " ");
        const select = document.createElement("select");
        select.id = `change-filter-${name}`;
        for (const [labelText, value] of [
          ["Any", ""],
          ...values.map((value2) => [value2.replaceAll("_", " "), value2])
        ]) {
          const option = document.createElement("option");
          option.textContent = labelText;
          option.value = value;
          select.append(option);
        }
        select.addEventListener("change", () => {
          const current = parseChangeInspectorRoute(location.hash || "#/changes");
          const base = current.kind === "invalid" ? { kind: "lens", lens: "changes", query: {} } : current;
          actions2.navigate({
            ...base,
            query: {
              ...base.query,
              after: void 0,
              [name]: select.value || void 0
            }
          });
        });
        label.append(select);
        filterTypes.append(label);
      }
    }
    const clear = document.querySelector("#filter-clear");
    if (clear) {
      clear.onclick = () => {
        const current = parseChangeInspectorRoute(location.hash || "#/changes");
        const base = current.kind === "invalid" ? { kind: "lens", lens: "changes", query: {} } : current;
        actions2.navigate({
          ...base,
          query: {}
        });
      };
    }
  }
  __name(prepareChangeInspectorShell, "prepareChangeInspectorShell");
  function copyExact(value) {
    if (navigator.clipboard) void navigator.clipboard.writeText(value);
  }
  __name(copyExact, "copyExact");
  function clearError() {
    const banner = document.querySelector("#error");
    if (!banner) return;
    banner.textContent = "";
    banner.classList.add("hidden");
  }
  __name(clearError, "clearError");
  function filterValues(query) {
    const values = [];
    if (query.q) values.push(["search", query.q]);
    for (const [name] of FILTER_OPTIONS) {
      const value = query[name];
      if (value) values.push([name, value]);
    }
    return values;
  }
  __name(filterValues, "filterValues");
  function syncFilterChrome(route) {
    if (route.kind === "invalid") return;
    const input = document.querySelector("#filter-text");
    if (input) input.value = route.query.q ?? "";
    for (const [name] of FILTER_OPTIONS) {
      const select = document.querySelector(
        `#change-filter-${name}`
      );
      if (select) select.value = route.query[name] ?? "";
    }
    const values = filterValues(route.query);
    const chips = document.querySelector("#filter-chips");
    if (chips) {
      chips.replaceChildren(
        ...values.map(([name, value]) => {
          const chip = document.createElement("span");
          chip.className = "badge";
          chip.textContent = `${name}: ${value.replaceAll("_", " ")}`;
          return chip;
        })
      );
    }
    document.querySelector("#filter-chips-empty")?.classList.toggle("hidden", values.length > 0);
    const toggle = document.querySelector("#filters-toggle");
    if (toggle)
      toggle.textContent = values.length ? `Filters · ${values.length}` : "Filters";
  }
  __name(syncFilterChrome, "syncFilterChrome");
  function renderDetail(snapshot2, actions2) {
    const detail = document.querySelector("#detail-body");
    if (!detail) return;
    if (snapshot2.route.kind === "invalid") {
      detail.replaceChildren(message(snapshot2.route.message));
      return;
    }
    if (snapshot2.diagnostic) {
      detail.replaceChildren(message(snapshot2.diagnostic));
      return;
    }
    if (snapshot2.route.kind === "lens" || snapshot2.generation === null) {
      detail.replaceChildren(message("Select a Change or exact Revision."));
      return;
    }
    const heading = document.createElement("h2");
    heading.textContent = snapshot2.route.kind === "change" ? "Change" : "Exact Revision";
    const identity = document.createElement("p");
    identity.className = "mono";
    identity.textContent = snapshot2.route.kind === "change" ? `Change ID: ${snapshot2.route.changeId}` : `Revision ID: ${snapshot2.route.revision.revisionId} · artifact hash: ${snapshot2.route.revision.objectArtifactContentHash}`;
    const placeholder = message(
      snapshot2.route.kind === "change" ? "Select an explicit current Revision to inspect its exact context." : "Exact Revision selected. Rich facts and captured resources load in the next Inspector slice."
    );
    const copyLink = document.createElement("button");
    copyLink.type = "button";
    copyLink.className = "ghost";
    copyLink.textContent = "Copy link";
    copyLink.addEventListener("click", () => copyExact(location.href));
    const peers = document.createElement("section");
    if (snapshot2.route.kind === "change" && snapshot2.selected !== null) {
      const changeRoute = snapshot2.route;
      const peerHeading = document.createElement("h3");
      peerHeading.textContent = "Current Revisions";
      peers.append(peerHeading);
      for (const revision of snapshot2.selected.currentRevisionRefs) {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "ghost mono";
        button.textContent = revision.revisionId;
        button.addEventListener(
          "click",
          () => actions2.navigate({
            kind: "revision",
            changeId: changeRoute.changeId,
            revision,
            query: changeRoute.query
          })
        );
        peers.append(button);
      }
    }
    detail.replaceChildren(heading, identity, copyLink, placeholder, peers);
  }
  __name(renderDetail, "renderDetail");
  function renderChangeInspector(snapshot2, actions2) {
    const master = document.querySelector("#master");
    if (!master) return;
    const routeDiagnostic = document.querySelector("#route-diagnostic");
    if (routeDiagnostic) {
      routeDiagnostic.textContent = snapshot2.diagnostic ?? "";
      routeDiagnostic.classList.toggle("hidden", snapshot2.diagnostic === null);
    }
    syncFilterChrome(snapshot2.route);
    clearError();
    if (snapshot2.route.kind === "invalid") {
      master.replaceChildren(message("Cannot open this Inspector link."));
      renderDetail(snapshot2, actions2);
      return;
    }
    const route = snapshot2.route;
    if (snapshot2.generation === null) {
      master.replaceChildren(message("Loading Change generation…"));
      renderDetail(snapshot2, actions2);
      return;
    }
    const lens = lensForRoute(route);
    const page = lens === "changes" ? snapshot2.generation.changes : snapshot2.generation.attention;
    const list = document.createElement("section");
    list.className = "units";
    const heading = document.createElement("h1");
    heading.textContent = `${lens === "changes" ? "Changes" : "Attention"} · ${page.changes.length}`;
    list.append(heading);
    for (const summary of page.changes) {
      const card = changeCardPresentation(
        summary,
        page.presentations?.[summary.changeId]
      );
      const element = document.createElement("article");
      element.className = "unit-card";
      element.dataset.changeId = summary.changeId;
      const badges = document.createElement("p");
      badges.className = "change-card-badges";
      for (const value of card.badges) {
        const badge = document.createElement("span");
        badge.className = "badge";
        badge.textContent = value;
        badges.append(badge, " ");
      }
      element.append(badges);
      if (card.peers.length === 0) {
        const unavailable = document.createElement("h3");
        unavailable.textContent = "Current Revision unavailable";
        element.append(unavailable);
      } else if (card.peers.length > 1) {
        const peerHeading = document.createElement("h3");
        peerHeading.textContent = "Current Revisions";
        element.append(peerHeading);
      }
      for (const peer of card.peers) {
        const peerRow = document.createElement("div");
        peerRow.className = "change-card-peer";
        const choose = document.createElement("button");
        choose.type = "button";
        choose.className = "ghost change-card-peer-open";
        choose.textContent = peer.label;
        choose.title = peer.copyText;
        choose.addEventListener(
          "click",
          () => actions2.navigate({
            kind: "revision",
            changeId: summary.changeId,
            revision: peer.revision,
            query: route.query
          })
        );
        const copyPeer = document.createElement("button");
        copyPeer.type = "button";
        copyPeer.className = "ghost";
        copyPeer.textContent = "Copy exact Revision";
        copyPeer.addEventListener("click", () => copyExact(peer.copyText));
        peerRow.append(choose, copyPeer);
        element.append(peerRow);
      }
      const actionsElement = document.createElement("div");
      actionsElement.className = "actions change-card-actions";
      const open = document.createElement("button");
      open.type = "button";
      open.className = "ghost";
      open.textContent = "Open Change";
      open.addEventListener(
        "click",
        () => actions2.navigate({
          kind: "change",
          changeId: summary.changeId,
          query: route.query
        })
      );
      const changeIdentity = document.createElement("code");
      changeIdentity.className = "mono";
      changeIdentity.textContent = summary.changeId;
      const copyChange = document.createElement("button");
      copyChange.type = "button";
      copyChange.className = "ghost";
      copyChange.textContent = "Copy Change ID";
      copyChange.addEventListener("click", () => copyExact(summary.changeId));
      actionsElement.append(open, changeIdentity, copyChange);
      element.append(actionsElement);
      list.append(element);
    }
    if (page.changes.length === 0)
      list.append(
        message(
          lens === "changes" ? "No Changes." : "No Changes need attention."
        )
      );
    const nextPage = page.next;
    if (nextPage !== null) {
      const next = document.createElement("button");
      next.type = "button";
      next.className = "ghost";
      next.textContent = "Next page";
      next.addEventListener(
        "click",
        () => actions2.navigate({
          kind: "lens",
          lens,
          query: {
            ...route.query,
            after: nextPage
          }
        })
      );
      list.append(next);
    }
    master.replaceChildren(list);
    document.querySelectorAll("#lens-switcher [data-lens]").forEach((button) => {
      button.setAttribute("aria-pressed", String(button.dataset.lens === lens));
    });
    setText(
      "#stat-events",
      `${snapshot2.generation.profile.authorityCursor.eventCount ?? "—"} events`
    );
    setText(
      "#stat-units",
      `${snapshot2.generation.changes.changes.length} Changes`
    );
    setText(
      "#stat-threads",
      `${snapshot2.generation.attention.changes.length} need attention`
    );
    setText("#stat-hash", snapshot2.generation.changes.projectionStamp);
    renderDetail(snapshot2, actions2);
  }
  __name(renderChangeInspector, "renderChangeInspector");
  function renderChangeInspectorUnavailable(availability) {
    clearError();
    const master = document.querySelector("#master");
    master?.replaceChildren(
      message(
        availability === "migration_required" ? "Store migration required. No Change state was loaded." : "Store migration in progress. Partial Change state is unavailable."
      )
    );
    document.querySelector("#detail-body")?.replaceChildren(message("Change state is unavailable."));
  }
  __name(renderChangeInspectorUnavailable, "renderChangeInspectorUnavailable");
  function renderChangeInspectorRefusal(error) {
    const text = error instanceof Error ? error.message : String(error);
    document.querySelector("#master")?.replaceChildren(message(`Reader refused: ${text}`));
    document.querySelector("#detail-body")?.replaceChildren(message("Change state was not published."));
    const diagnostic = document.querySelector("#route-diagnostic");
    if (diagnostic) {
      diagnostic.textContent = "";
      diagnostic.classList.add("hidden");
    }
    const banner = document.querySelector("#error");
    if (banner) {
      banner.textContent = `error: ${text}`;
      banner.classList.remove("hidden");
    }
  }
  __name(renderChangeInspectorRefusal, "renderChangeInspectorRefusal");

  // ../../../documents/change_reader_profile_v1.json
  var change_reader_profile_v1_default = {
    minimumReaderProfile: "review_change_revision_v1",
    documents: {
      "pointbreak.attention-list": 2,
      "pointbreak.inspect-attention": 2,
      "pointbreak.inspect-changes-page": 1,
      "pointbreak.inspect-reader-profile": 1,
      "pointbreak.reader-upgrade-required": 1,
      "pointbreak.review-association-comparison": 1,
      "pointbreak.review-change": 1,
      "pointbreak.review-change-list": 1,
      "pointbreak.review-change-revision": 1,
      "pointbreak.review-revision": 3,
      "pointbreak.review-revision-interdiff": 1,
      "pointbreak.review-revision-resource": 1,
      "pointbreak.store-migration-in-progress": 1,
      "pointbreak.store-migration-required": 1
    }
  };

  // src/change-protocol.ts
  var CHANGE_PAGE_LIMIT = 50;
  var CHANGE_READER_PROFILE = change_reader_profile_v1_default.minimumReaderProfile;
  var CHANGE_READER_DOCUMENTS = change_reader_profile_v1_default.documents;
  var TOPOLOGY_VALUES = /* @__PURE__ */ new Set([
    "initial",
    "replacement",
    "replacement_divergent",
    "consolidation",
    "parallel_current",
    "mixed",
    "incomplete",
    "cycle_conflicted"
  ]);
  var LIFECYCLE_VALUES = /* @__PURE__ */ new Set([
    "incomplete",
    "conflicted",
    "in_progress",
    "accepted"
  ]);
  var ATTENTION_VALUES = /* @__PURE__ */ new Set([
    "clear",
    "in_progress",
    "incomplete",
    "conflicted"
  ]);
  var AVAILABILITY_VALUES = /* @__PURE__ */ new Set(["available", "incomplete"]);
  function buildChangePageUrl(lens, query = {}) {
    const limit = query.limit ?? CHANGE_PAGE_LIMIT;
    if (!Number.isInteger(limit) || limit < 1 || limit > 100) {
      throw new Error("Change page limit must be an integer from 1 through 100");
    }
    const params = new URLSearchParams({ limit: String(limit) });
    if (query.after !== void 0) {
      if (!query.after || new TextEncoder().encode(query.after).length > 4096) {
        throw new Error(
          "Change page continuation must be a non-empty opaque token"
        );
      }
      params.set("after", query.after);
    }
    if (query.q !== void 0) {
      const normalized = trimUnicodeWhitespace(query.q).toLowerCase();
      if (!normalized || new TextEncoder().encode(normalized).length > 256) {
        throw new Error(
          "Change page query must be non-empty and at most 256 bytes"
        );
      }
      params.set("q", normalized);
    }
    appendEnum(params, "topology", query.topology, TOPOLOGY_VALUES);
    appendEnum(params, "lifecycle", query.lifecycle, LIFECYCLE_VALUES);
    appendEnum(params, "attention", query.attention, ATTENTION_VALUES);
    appendEnum(params, "availability", query.availability, AVAILABILITY_VALUES);
    if (query.order !== void 0 && query.order !== "change_id_asc") {
      throw new Error("Change page order must be change_id_asc");
    }
    params.set("order", "change_id_asc");
    return `/api/v2/${lens}?${params}`;
  }
  __name(buildChangePageUrl, "buildChangePageUrl");
  function decodeReaderProfile(value) {
    const profile = object(value, "Inspector reader profile");
    const availability = profile.availability;
    const authorityCursor = profile.authorityCursor;
    const documents = profile.documents;
    const minimumReaderProfile = profile.minimumReaderProfile;
    const commitGraphStamp = profile.commitGraphStamp;
    if (profile.schema !== "pointbreak.inspect-reader-profile" || profile.version !== 1 || !isAvailability(availability) || !isRecord(authorityCursor) || !isDocumentMap(documents) || !sameDocumentMap(documents, CHANGE_READER_DOCUMENTS)) {
      throw new Error("incompatible Inspector reader profile");
    }
    if (availability === "ready" && (minimumReaderProfile !== CHANGE_READER_PROFILE || typeof commitGraphStamp !== "string" || commitGraphStamp.length === 0)) {
      throw new Error(
        "ready Inspector reader profile is missing capability or commit graph stamp"
      );
    }
    return {
      schema: "pointbreak.inspect-reader-profile",
      version: 1,
      availability,
      minimumReaderProfile: typeof minimumReaderProfile === "string" ? minimumReaderProfile : void 0,
      authorityCursor,
      commitGraphStamp: typeof commitGraphStamp === "string" ? commitGraphStamp : void 0,
      documents
    };
  }
  __name(decodeReaderProfile, "decodeReaderProfile");
  function decodeChangePage(value, expected) {
    const page = object(value, `${expected.lens} Change page`);
    const expectedSchema = expected.lens === "changes" ? "pointbreak.inspect-changes-page" : "pointbreak.inspect-attention";
    const expectedVersion = expected.lens === "changes" ? 1 : 2;
    const stamp = page.projectionStamp;
    const changes = page.changes;
    const diagnostics = page.diagnostics;
    const presentations = page.presentations;
    if (page.schema !== expectedSchema || page.version !== expectedVersion || !nonEmptyString(stamp) || !Array.isArray(changes) || expected.bounded && changes.length > 100 || !changes.every((change) => isChangeSummary(change, stamp)) || !isStrictlyAscending(changes.map((change) => change.changeId)) || new Set(changes.map((change) => change.changeId)).size !== changes.length || diagnostics !== void 0 && !isStringArray(diagnostics) || presentations !== void 0 && !isPresentations(presentations, changes)) {
      throw new Error(`invalid ${expected.lens} Change page DTO`);
    }
    const next = page.next;
    if (next !== void 0 && next !== null && !nonEmptyString(next)) {
      throw new Error("invalid Change page next continuation");
    }
    if (expected.bounded && next === void 0)
      throw new Error("bounded Change page is missing next continuation");
    const common = {
      changes,
      diagnostics,
      presentations,
      projectionStamp: stamp,
      next: next ?? null
    };
    return expected.lens === "changes" ? {
      schema: "pointbreak.inspect-changes-page",
      version: 1,
      ...common
    } : {
      schema: "pointbreak.inspect-attention",
      version: 2,
      ...common
    };
  }
  __name(decodeChangePage, "decodeChangePage");
  function requireCoherentGeneration(changes, attention) {
    if (changes.projectionStamp !== attention.projectionStamp) {
      throw new Error("Change documents do not form one coherent generation");
    }
  }
  __name(requireCoherentGeneration, "requireCoherentGeneration");
  function sameProfileGeneration(initial, postflight) {
    return initial.availability === postflight.availability && initial.minimumReaderProfile === postflight.minimumReaderProfile && initial.commitGraphStamp === postflight.commitGraphStamp && sameDocumentMap(initial.documents, postflight.documents) && canonicalJson(initial.authorityCursor) === canonicalJson(postflight.authorityCursor);
  }
  __name(sameProfileGeneration, "sameProfileGeneration");
  function trimUnicodeWhitespace(value) {
    return value.replace(/^\p{White_Space}+|\p{White_Space}+$/gu, "");
  }
  __name(trimUnicodeWhitespace, "trimUnicodeWhitespace");
  function appendEnum(params, name, value, values) {
    if (value === void 0) return;
    if (!values.has(value)) throw new Error(`invalid Change page ${name}`);
    params.set(name, value);
  }
  __name(appendEnum, "appendEnum");
  function isAvailability(value) {
    return value === "migration_required" || value === "migration_in_progress" || value === "ready";
  }
  __name(isAvailability, "isAvailability");
  function isChangeSummary(value, stamp) {
    if (!isRecord(value)) return false;
    return nonEmptyString(value.changeId) && isOneOf(value.topology, TOPOLOGY_VALUES) && isOneOf(value.lifecycle, LIFECYCLE_VALUES) && isOneOf(value.attentionSummary, ATTENTION_VALUES) && isOneOf(value.availabilitySummary, AVAILABILITY_VALUES) && value.projectionStamp === stamp && Array.isArray(value.currentRevisionRefs) && value.currentRevisionRefs.every(isRevisionRef) && uniqueRevisionKeys(value.currentRevisionRefs).size === value.currentRevisionRefs.length && (value.diagnostics === void 0 || isStringArray(value.diagnostics));
  }
  __name(isChangeSummary, "isChangeSummary");
  function isPresentations(value, changes) {
    if (!isRecord(value)) return false;
    const summaries = new Map(
      changes.map((change) => [change.changeId, change])
    );
    if (Object.keys(value).length !== summaries.size) return false;
    return Object.entries(value).every(([changeId, presentation]) => {
      const change = summaries.get(changeId);
      if (change === void 0 || !isRecord(presentation) || !Array.isArray(presentation.currentRevisions) || !presentation.currentRevisions.every(isPresentationRevision)) {
        return false;
      }
      const expected = uniqueRevisionKeys(change.currentRevisionRefs);
      const actual = uniqueRevisionKeys(
        presentation.currentRevisions.map((candidate) => candidate.revision)
      );
      return expected.size === change.currentRevisionRefs.length && actual.size === presentation.currentRevisions.length && expected.size === actual.size && [...expected].every((key) => actual.has(key));
    });
  }
  __name(isPresentations, "isPresentations");
  function isPresentationRevision(value) {
    return isRecord(value) && isRevisionRef(value.revision) && (value.summarySource === "revision_proposal_summary" && nonEmptyString(value.revisionProposalSummary) || value.summarySource === "absent" && value.revisionProposalSummary === void 0);
  }
  __name(isPresentationRevision, "isPresentationRevision");
  function isRevisionRef(value) {
    return isRecord(value) && nonEmptyString(value.revisionId) && nonEmptyString(value.objectArtifactContentHash);
  }
  __name(isRevisionRef, "isRevisionRef");
  function uniqueRevisionKeys(revisions) {
    return new Set(
      revisions.map(
        (revision) => `${revision.revisionId}\0${revision.objectArtifactContentHash}`
      )
    );
  }
  __name(uniqueRevisionKeys, "uniqueRevisionKeys");
  function isStrictlyAscending(values) {
    return values.every((value, index) => {
      const previous = values[index - 1];
      return index === 0 || previous !== void 0 && previous < value;
    });
  }
  __name(isStrictlyAscending, "isStrictlyAscending");
  function isOneOf(value, values) {
    return typeof value === "string" && values.has(value);
  }
  __name(isOneOf, "isOneOf");
  function isDocumentMap(value) {
    return isRecord(value) && Object.values(value).every((version) => Number.isInteger(version));
  }
  __name(isDocumentMap, "isDocumentMap");
  function sameDocumentMap(left, right) {
    const leftEntries = Object.entries(left).sort(
      ([a], [b]) => a.localeCompare(b)
    );
    const rightEntries = Object.entries(right).sort(
      ([a], [b]) => a.localeCompare(b)
    );
    return leftEntries.length === rightEntries.length && leftEntries.every(
      ([schema, version], index) => schema === rightEntries[index]?.[0] && version === rightEntries[index]?.[1]
    );
  }
  __name(sameDocumentMap, "sameDocumentMap");
  function canonicalJson(value) {
    if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
    if (isRecord(value)) {
      return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(",")}}`;
    }
    return JSON.stringify(value);
  }
  __name(canonicalJson, "canonicalJson");
  function object(value, name) {
    if (!isRecord(value)) throw new Error(`invalid ${name} DTO`);
    return value;
  }
  __name(object, "object");
  function isRecord(value) {
    return typeof value === "object" && value !== null && !Array.isArray(value);
  }
  __name(isRecord, "isRecord");
  function nonEmptyString(value) {
    return typeof value === "string" && value.length > 0;
  }
  __name(nonEmptyString, "nonEmptyString");
  function isStringArray(value) {
    return Array.isArray(value) && value.every((item) => typeof item === "string");
  }
  __name(isStringArray, "isStringArray");

  // src/change-inspector-state.ts
  var ChangeInspectorGenerationChanged = class extends Error {
    static {
      __name(this, "ChangeInspectorGenerationChanged");
    }
    constructor() {
      super("Change generation changed during staging");
    }
  };
  function sameRevision(left, right) {
    return left.revisionId === right.revisionId && left.objectArtifactContentHash === right.objectArtifactContentHash;
  }
  __name(sameRevision, "sameRevision");
  function selectedChange(generation, route) {
    if (route.kind !== "change" && route.kind !== "revision") return null;
    const all = [...generation.changes.changes, ...generation.attention.changes];
    const change = all.find((candidate) => candidate.changeId === route.changeId) ?? null;
    if (change === null || route.kind !== "revision") return change;
    return change.currentRevisionRefs.some(
      (candidate) => sameRevision(candidate, route.revision)
    ) ? change : null;
  }
  __name(selectedChange, "selectedChange");
  function selectionDiagnostic(route) {
    if (route.kind === "invalid") return route.message;
    return null;
  }
  __name(selectionDiagnostic, "selectionDiagnostic");
  function stageGeneration(profile, changes, attention, postflight) {
    requireCoherentGeneration(changes, attention);
    if (!sameProfileGeneration(profile, postflight)) {
      throw new ChangeInspectorGenerationChanged();
    }
    return { profile, changes, attention };
  }
  __name(stageGeneration, "stageGeneration");
  function createChangeInspectorState(initialRoute) {
    let generation = null;
    let route = initialRoute;
    const snapshot2 = /* @__PURE__ */ __name(() => {
      const selected = generation === null ? null : selectedChange(generation, route);
      return {
        generation,
        route,
        selected,
        diagnostic: selectionDiagnostic(route)
      };
    }, "snapshot");
    return {
      snapshot: snapshot2,
      publish(next) {
        generation = next;
        return snapshot2();
      },
      setRoute(next) {
        route = next;
        return snapshot2();
      },
      clearGeneration() {
        generation = null;
        return snapshot2();
      }
    };
  }
  __name(createChangeInspectorState, "createChangeInspectorState");

  // src/dom.ts
  function $(sel) {
    return document.querySelector(sel);
  }
  __name($, "$");

  // src/disclosure.ts
  var active = null;
  function createDisclosure({
    container,
    trigger,
    panel
  }) {
    let open = false;
    const controller = {
      isOpen: /* @__PURE__ */ __name(() => open, "isOpen"),
      open: /* @__PURE__ */ __name(() => {
        if (active && active !== controller) active.close();
        open = true;
        active = controller;
        controller.sync();
      }, "open"),
      close: /* @__PURE__ */ __name((returnFocus = false) => {
        open = false;
        if (active === controller) active = null;
        controller.sync();
        if (returnFocus) $(trigger)?.focus();
      }, "close"),
      toggle: /* @__PURE__ */ __name(() => {
        if (open) controller.close();
        else controller.open();
      }, "toggle"),
      sync: /* @__PURE__ */ __name(() => {
        $(panel)?.classList.toggle("hidden", !open);
        $(trigger)?.setAttribute("aria-expanded", String(open));
      }, "sync")
    };
    $(trigger)?.addEventListener("click", (event) => {
      event.stopPropagation();
      controller.toggle();
    });
    $(container)?.addEventListener("keydown", (event) => {
      if (event.key !== "Escape" || !open) return;
      event.preventDefault();
      event.stopPropagation();
      controller.close(true);
    });
    document.addEventListener(
      "click",
      (event) => {
        if (!open) return;
        const root = $(container);
        if (event.target instanceof Node && root?.contains(event.target)) return;
        controller.close();
      },
      true
    );
    controller.sync();
    return controller;
  }
  __name(createDisclosure, "createDisclosure");

  // src/change-inspector.ts
  var pollTimer = null;
  var routeListener = null;
  var filterInput = null;
  var filterInputListener = null;
  var connectionControlsInitialized = false;
  var filterDisclosureInitialized = false;
  var requestEpoch = 0;
  function currentRoute() {
    return parseChangeInspectorRoute(location.hash || "#/changes");
  }
  __name(currentRoute, "currentRoute");
  function stopChangeInspector() {
    requestEpoch += 1;
    if (pollTimer !== null) clearInterval(pollTimer);
    pollTimer = null;
    if (routeListener !== null)
      window.removeEventListener("hashchange", routeListener);
    routeListener = null;
    if (filterInput !== null && filterInputListener !== null) {
      filterInput.removeEventListener("change", filterInputListener);
    }
    filterInput = null;
    filterInputListener = null;
  }
  __name(stopChangeInspector, "stopChangeInspector");
  async function bootstrapChangeInspector(options = {}) {
    stopChangeInspector();
    const capability = bootstrapCapability();
    if (capability.token !== null) {
      (options.reload ?? (() => location.reload()))();
      return;
    }
    installDefaultAuthCoordinator();
    const state = createChangeInspectorState(currentRoute());
    const navigate = /* @__PURE__ */ __name((route) => {
      const hash = formatChangeInspectorRoute(route);
      if (location.hash !== hash) location.hash = hash;
      else void onRoute();
    }, "navigate");
    const paint = /* @__PURE__ */ __name(() => renderChangeInspector(state.snapshot(), { navigate }), "paint");
    const requestKey = /* @__PURE__ */ __name((query) => buildChangePageUrl("changes", query), "requestKey");
    let visibleRequest = "";
    const loadGeneration = /* @__PURE__ */ __name(async (route, restarted = false) => {
      const epoch = ++requestEpoch;
      try {
        const profile = decodeReaderProfile(
          await fetchChangeInspectorJSON("/api/v2/profile")
        );
        if (epoch !== requestEpoch) return;
        if (profile.availability !== "ready") {
          visibleRequest = "";
          state.clearGeneration();
          renderChangeInspectorUnavailable(profile.availability);
          return;
        }
        const query = route.query;
        const [changes, attention] = await Promise.all([
          fetchChangeInspectorJSON(buildChangePageUrl("changes", query)).then(
            (value) => decodeChangePage(value, { lens: "changes", bounded: true })
          ),
          fetchChangeInspectorJSON(buildChangePageUrl("attention", query)).then(
            (value) => decodeChangePage(value, { lens: "attention", bounded: true })
          )
        ]);
        const postflight = decodeReaderProfile(
          await fetchChangeInspectorJSON("/api/v2/profile")
        );
        if (epoch !== requestEpoch) return;
        state.publish(stageGeneration(profile, changes, attention, postflight));
        visibleRequest = requestKey(query);
        paint();
      } catch (error) {
        if (epoch !== requestEpoch) return;
        if (!restarted && (error instanceof ChangeInspectorPageFailure && error.code === "stale_projection" || error instanceof ChangeInspectorGenerationChanged)) {
          await loadGeneration(route, true);
          return;
        }
        visibleRequest = "";
        state.clearGeneration();
        renderChangeInspectorRefusal(error);
      }
    }, "loadGeneration");
    const onRoute = /* @__PURE__ */ __name(async () => {
      const route = currentRoute();
      state.setRoute(route);
      if (route.kind === "invalid") {
        visibleRequest = "";
        state.clearGeneration();
        paint();
        return;
      }
      let request;
      try {
        request = requestKey(route.query);
      } catch (error) {
        state.clearGeneration();
        renderChangeInspectorRefusal(error);
        return;
      }
      if (request === visibleRequest) paint();
      else {
        visibleRequest = "";
        state.clearGeneration();
        paint();
        await loadGeneration(route);
      }
    }, "onRoute");
    routeListener = /* @__PURE__ */ __name(() => {
      void onRoute();
    }, "routeListener");
    window.addEventListener("hashchange", routeListener);
    const reloadCurrent = /* @__PURE__ */ __name(async () => {
      const route = currentRoute();
      if (route.kind === "invalid") {
        await onRoute();
        return;
      }
      await loadGeneration(route);
    }, "reloadCurrent");
    configureConnectionActions({
      retry: reloadCurrent,
      reconnect: /* @__PURE__ */ __name(async () => {
        if (await requestReconnect()) await reloadCurrent();
      }, "reconnect")
    });
    if (!connectionControlsInitialized) {
      initConnectionControls();
      connectionControlsInitialized = true;
    }
    prepareChangeInspectorShell({ navigate });
    if (!filterDisclosureInitialized) {
      createDisclosure({
        container: "#filter-controls",
        trigger: "#filters-toggle",
        panel: "#filters-panel"
      });
      filterDisclosureInitialized = true;
    }
    filterInput = document.querySelector("#filter-text");
    filterInputListener = /* @__PURE__ */ __name(() => {
      const route = currentRoute();
      const base = route.kind === "invalid" ? { kind: "lens", lens: "changes", query: {} } : route;
      navigate({
        ...base,
        query: {
          ...base.query,
          after: void 0,
          q: filterInput?.value || void 0
        }
      });
    }, "filterInputListener");
    filterInput?.addEventListener("change", filterInputListener);
    await onRoute();
    if (options.poll !== false)
      pollTimer = setInterval(() => {
        const route = currentRoute();
        if (route.kind !== "invalid") void loadGeneration(route);
      }, 3e3);
  }
  __name(bootstrapChangeInspector, "bootstrapChangeInspector");

  // src/entry.ts
  void bootstrapChangeInspector();
})();
