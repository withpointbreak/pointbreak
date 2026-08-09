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
    const returnFocus = document.activeElement instanceof HTMLElement && document.activeElement !== document.body ? document.activeElement : null;
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
        dialog.removeEventListener("keydown", onKeyDown);
        input.value = "";
        dialog.classList.add("hidden");
        if (returnFocus?.isConnected) returnFocus.focus({ preventScroll: true });
        resolve(value);
      }, "finish");
      const onSubmit = /* @__PURE__ */ __name((event) => {
        event.preventDefault();
        finish(input.value);
      }, "onSubmit");
      const onCancel = /* @__PURE__ */ __name(() => finish(null), "onCancel");
      const onKeyDown = /* @__PURE__ */ __name((event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          finish(null);
          return;
        }
        if (event.key !== "Tab") return;
        const stops = Array.from(
          dialog.querySelectorAll(
            "button:not([disabled]), input:not([disabled]), [tabindex]:not([tabindex='-1'])"
          )
        );
        const first = stops[0];
        const last = stops.at(-1);
        if (!first || !last) return;
        const active3 = document.activeElement instanceof HTMLElement ? document.activeElement : null;
        if (event.shiftKey && (active3 === first || !dialog.contains(active3))) {
          event.preventDefault();
          last.focus();
        } else if (!event.shiftKey && (active3 === last || !dialog.contains(active3))) {
          event.preventDefault();
          first.focus();
        }
      }, "onKeyDown");
      form.addEventListener("submit", onSubmit);
      cancel.addEventListener("click", onCancel);
      dialog.addEventListener("keydown", onKeyDown);
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
      if (code === "moving_journal") {
        this.message = "Timeline journal changed while loading; retry";
      }
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
    if (document2.schema !== "pointbreak.inspect-change-page-error" && document2.schema !== "pointbreak.inspect-event-history-error" || document2.version !== 1)
      return null;
    if (document2.code === "invalid_query" && status === 400)
      return new ChangeInspectorPageFailure("invalid_query", status);
    if (document2.code === "stale_projection" && status === 409)
      return new ChangeInspectorPageFailure("stale_projection", status);
    if (document2.schema === "pointbreak.inspect-event-history-error" && document2.code === "moving_journal" && status === 503) {
      return new ChangeInspectorPageFailure("moving_journal", status);
    }
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
  var ROUTE_QUERY_KEYS = /* @__PURE__ */ new Set([
    ...QUERY_KEYS,
    "artifactHash",
    "fromArtifactHash",
    "toArtifactHash",
    "fact",
    "file",
    "fq"
  ]);
  var TIMELINE_QUERY_KEYS = [
    "limit",
    "after",
    "at",
    "q",
    "type",
    "track",
    "change",
    "revision",
    "artifactHash",
    "order"
  ];
  var TIMELINE_QUERY_KEY_SET = new Set(TIMELINE_QUERY_KEYS);
  function eventAnnotatedDiffRoute(context) {
    const changeId = context.changeIds[0];
    const revision2 = context.revisionRefs[0];
    if (context.changeIds.length !== 1 || context.revisionRefs.length !== 1 || context.unresolvedRevisionIds.length !== 0 || changeId === void 0 || revision2 === void 0) {
      return null;
    }
    return { kind: "diff", changeId, revision: revision2, query: {} };
  }
  __name(eventAnnotatedDiffRoute, "eventAnnotatedDiffRoute");
  function timelineEventRoute(eventId, historyQuery) {
    const { after: _after, at: _at, ...context } = historyQuery;
    return { kind: "event", eventId, historyQuery: context, query: {} };
  }
  __name(timelineEventRoute, "timelineEventRoute");
  function showChangeInTimelineRoute(changeId) {
    return { kind: "timeline", historyQuery: { change: changeId } };
  }
  __name(showChangeInTimelineRoute, "showChangeInTimelineRoute");
  function showRevisionInTimelineRoute(changeId, revision2) {
    return {
      kind: "timeline",
      historyQuery: {
        change: changeId,
        revision: revision2.revisionId,
        artifactHash: revision2.objectArtifactContentHash
      }
    };
  }
  __name(showRevisionInTimelineRoute, "showRevisionInTimelineRoute");
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
    return {
      query,
      artifactHashes: params.getAll("artifactHash"),
      fromArtifactHashes: params.getAll("fromArtifactHash"),
      toArtifactHashes: params.getAll("toArtifactHash"),
      facts: params.getAll("fact"),
      files: params.getAll("file"),
      fileQueries: params.getAll("fq")
    };
  }
  __name(parseQuery, "parseQuery");
  function isParseError(value) {
    return "message" in value;
  }
  __name(isParseError, "isParseError");
  function parseTimelineQuery(search) {
    if (!validQueryEncoding(search)) {
      return { message: "Malformed route query encoding." };
    }
    const params = new URLSearchParams(search);
    const query = {};
    for (const key of params.keys()) {
      if (!TIMELINE_QUERY_KEY_SET.has(key)) {
        return { message: `Unknown ${key} route query.` };
      }
      if (params.getAll(key).length !== 1) {
        return { message: `Duplicate ${key} route query.` };
      }
    }
    for (const key of TIMELINE_QUERY_KEYS) {
      const value = params.get(key);
      if (value === null) continue;
      if (!value) return { message: `Empty ${key} route query.` };
      if (key === "limit") {
        const limit = Number(value);
        if (!Number.isInteger(limit) || limit < 1 || limit > 100)
          return { message: "Invalid limit route query." };
        query.limit = limit;
      } else if (key === "order") {
        if (value !== "asc" && value !== "desc")
          return { message: "Invalid order route query." };
        query.order = value;
      } else if (key === "after" || key === "at" || key === "q" || key === "type" || key === "track" || key === "change" || key === "revision" || key === "artifactHash") {
        query[key] = value;
      }
    }
    if (query.revision === void 0 !== (query.artifactHash === void 0)) {
      return { message: "Timeline revision requires artifactHash." };
    }
    if (query.at !== void 0 && query.after !== void 0) {
      return { message: "Timeline at and after cannot be combined." };
    }
    return query;
  }
  __name(parseTimelineQuery, "parseTimelineQuery");
  function isTimelineParseError(value) {
    return "message" in value;
  }
  __name(isTimelineParseError, "isTimelineParseError");
  function parseChangeInspectorRoute(hash) {
    const raw = hash.startsWith("#") ? hash.slice(1) : hash;
    const separator = raw.indexOf("?");
    const path = separator === -1 ? raw : raw.slice(0, separator);
    const search = separator === -1 ? "" : raw.slice(separator + 1);
    const segments = path.split("/").filter(Boolean);
    if (segments.length === 0 || segments.length === 1 && segments[0] === "timeline") {
      const historyQuery = parseTimelineQuery(search);
      return isTimelineParseError(historyQuery) ? { kind: "invalid", message: historyQuery.message } : { kind: "timeline", historyQuery };
    }
    if (segments.length === 3 && segments[0] === "timeline" && segments[1] === "events") {
      const eventId = decodeSegment(segments[2]);
      if (eventId === null)
        return { kind: "invalid", message: "Event routes require an event ID." };
      const historyQuery = parseTimelineQuery(search);
      if (isTimelineParseError(historyQuery))
        return { kind: "invalid", message: historyQuery.message };
      if (historyQuery.at !== void 0)
        return {
          kind: "invalid",
          message: "Event routes select their anchor from the event ID."
        };
      return { kind: "event", eventId, historyQuery, query: {} };
    }
    const parsed = parseQuery(search);
    if (isParseError(parsed)) return { kind: "invalid", message: parsed.message };
    const {
      query,
      artifactHashes,
      fromArtifactHashes,
      toArtifactHashes,
      facts,
      files,
      fileQueries
    } = parsed;
    const focus = /* @__PURE__ */ __name((allowFileQuery = false) => {
      if (facts.length > 1 || files.length > 1 || fileQueries.length > 1 || facts.some((value) => !value) || files.some((value) => !value) || fileQueries.some((value) => !value) || !allowFileQuery && fileQueries.length > 0) {
        return null;
      }
      const selected = {
        ...facts[0] ? { factId: facts[0] } : {},
        ...files[0] ? { filePath: files[0] } : {},
        ...fileQueries[0] ? { fileQuery: fileQueries[0] } : {}
      };
      return Object.keys(selected).length ? selected : void 0;
    }, "focus");
    if (segments.length === 1 && (segments[0] === "changes" || segments[0] === "attention")) {
      if (artifactHashes.length > 0 || fromArtifactHashes.length > 0 || toArtifactHashes.length > 0 || facts.length > 0 || files.length > 0 || fileQueries.length > 0) {
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
      if (artifactHashes.length > 0 || fromArtifactHashes.length > 0 || toArtifactHashes.length > 0 || facts.length > 0 || files.length > 0 || fileQueries.length > 0) {
        return {
          kind: "invalid",
          message: "artifactHash is only valid on an exact Revision route."
        };
      }
      return { kind: "change", changeId, query };
    }
    const exactRevision = /* @__PURE__ */ __name((revisionId) => {
      if (revisionId === null || artifactHashes.length !== 1 || !artifactHashes[0])
        return null;
      return {
        revisionId,
        objectArtifactContentHash: artifactHashes[0]
      };
    }, "exactRevision");
    const exactFailure = /* @__PURE__ */ __name(() => ({
      kind: "invalid",
      message: artifactHashes.length > 1 ? "Exact Revision routes require exactly one artifactHash." : "Exact Revision routes require artifactHash."
    }), "exactFailure");
    if (segments[2] === "revisions" && segments.length >= 4) {
      const revision2 = exactRevision(decodeSegment(segments[3]));
      if (revision2 === null) return exactFailure();
      const exactFocus = focus(segments.length === 5 && segments[4] === "diff");
      if (exactFocus === null)
        return {
          kind: "invalid",
          message: "Exact route focus requires at most one non-empty fact and file."
        };
      if (fromArtifactHashes.length > 0 || toArtifactHashes.length > 0)
        return {
          kind: "invalid",
          message: "Revision routes do not accept interdiff hashes."
        };
      if (segments.length === 4)
        return {
          kind: "revision",
          changeId,
          revision: revision2,
          query,
          ...exactFocus ? { focus: exactFocus } : {}
        };
      if (segments.length === 5 && segments[4] === "resource")
        return {
          kind: "resource",
          changeId,
          revision: revision2,
          query,
          ...exactFocus ? { focus: exactFocus } : {}
        };
      if (segments.length === 5 && segments[4] === "diff")
        return {
          kind: "diff",
          changeId,
          revision: revision2,
          query,
          ...exactFocus ? { focus: exactFocus } : {}
        };
      if (segments.length === 5 && segments[4] === "association")
        return {
          kind: "association",
          changeId,
          revision: revision2,
          query,
          ...exactFocus ? { focus: exactFocus } : {}
        };
    }
    if (segments[2] === "interdiff" && segments.length === 5) {
      if (artifactHashes.length > 0)
        return {
          kind: "invalid",
          message: "Interdiff routes use endpoint artifact hashes."
        };
      const fromRevisionId = decodeSegment(segments[3]);
      const toRevisionId = decodeSegment(segments[4]);
      if (fromRevisionId === null || toRevisionId === null)
        return {
          kind: "invalid",
          message: "Interdiff routes require both Revision IDs."
        };
      if (fromArtifactHashes.length !== 1 || !fromArtifactHashes[0] || toArtifactHashes.length !== 1 || !toArtifactHashes[0])
        return {
          kind: "invalid",
          message: "Interdiff routes require exactly one artifact hash for each endpoint."
        };
      const exactFocus = focus();
      if (exactFocus === null)
        return {
          kind: "invalid",
          message: "Exact route focus requires at most one non-empty fact and file."
        };
      return {
        kind: "interdiff",
        changeId,
        from: {
          revisionId: fromRevisionId,
          objectArtifactContentHash: fromArtifactHashes[0]
        },
        to: {
          revisionId: toRevisionId,
          objectArtifactContentHash: toArtifactHashes[0]
        },
        query,
        ...exactFocus ? { focus: exactFocus } : {}
      };
    }
    return { kind: "invalid", message: "Unknown Change Inspector route." };
  }
  __name(parseChangeInspectorRoute, "parseChangeInspectorRoute");
  function appendQuery(query, params) {
    for (const key of QUERY_KEYS) {
      const value = query[key];
      if (value !== void 0) params.set(key, String(value));
    }
  }
  __name(appendQuery, "appendQuery");
  function appendTimelineQuery(query, params) {
    for (const key of TIMELINE_QUERY_KEYS) {
      const value = query[key];
      if (value !== void 0) params.set(key, String(value));
    }
  }
  __name(appendTimelineQuery, "appendTimelineQuery");
  function formatChangeInspectorRoute(route) {
    const params = new URLSearchParams();
    if (route.kind === "timeline") {
      appendTimelineQuery(route.historyQuery, params);
      return `#/timeline${params.size ? `?${params}` : ""}`;
    }
    if (route.kind === "event") {
      appendTimelineQuery(route.historyQuery, params);
      const eventId = encodeURIComponent(route.eventId);
      return `#/timeline/events/${eventId}${params.size ? `?${params}` : ""}`;
    }
    appendQuery(route.query, params);
    if (route.kind === "revision" || route.kind === "resource" || route.kind === "diff" || route.kind === "association")
      params.set("artifactHash", route.revision.objectArtifactContentHash);
    if (route.kind === "interdiff") {
      params.set("fromArtifactHash", route.from.objectArtifactContentHash);
      params.set("toArtifactHash", route.to.objectArtifactContentHash);
    }
    if ("focus" in route && route.focus?.factId)
      params.set("fact", route.focus.factId);
    if ("focus" in route && route.focus?.filePath)
      params.set("file", route.focus.filePath);
    if (route.kind === "diff" && route.focus?.fileQuery)
      params.set("fq", route.focus.fileQuery);
    const suffix = params.size ? `?${params}` : "";
    if (route.kind === "lens") return `#/${route.lens}${suffix}`;
    const change = encodeURIComponent(route.changeId);
    if (route.kind === "change") return `#/changes/${change}${suffix}`;
    if (route.kind === "revision")
      return `#/changes/${change}/revisions/${encodeURIComponent(route.revision.revisionId)}${suffix}`;
    if (route.kind === "resource")
      return `#/changes/${change}/revisions/${encodeURIComponent(route.revision.revisionId)}/resource${suffix}`;
    if (route.kind === "diff")
      return `#/changes/${change}/revisions/${encodeURIComponent(route.revision.revisionId)}/diff${suffix}`;
    if (route.kind === "association")
      return `#/changes/${change}/revisions/${encodeURIComponent(route.revision.revisionId)}/association${suffix}`;
    return `#/changes/${change}/interdiff/${encodeURIComponent(route.from.revisionId)}/${encodeURIComponent(route.to.revisionId)}${suffix}`;
  }
  __name(formatChangeInspectorRoute, "formatChangeInspectorRoute");
  function lensForRoute(route) {
    return route.kind === "timeline" || route.kind === "event" ? "timeline" : route.kind === "lens" ? route.lens : "changes";
  }
  __name(lensForRoute, "lensForRoute");
  function firstPageQuery(query) {
    const { after: _after, ...firstPage } = query;
    return firstPage;
  }
  __name(firstPageQuery, "firstPageQuery");
  function queryForExactNavigation(route) {
    if (route.kind === "timeline" || route.kind === "event") return {};
    if (route.kind !== "lens" || route.lens !== "attention") return route.query;
    return firstPageQuery(route.query);
  }
  __name(queryForExactNavigation, "queryForExactNavigation");

  // src/change-inspector-event-presentation.ts
  function words(value) {
    return value.replaceAll("_", " ");
  }
  __name(words, "words");
  function revision(reference) {
    return `${reference.revisionId} · ${reference.objectArtifactContentHash}`;
  }
  __name(revision, "revision");
  function fact(reference) {
    return reference.kind === "observation" ? `observation ${reference.observationId ?? "unknown"}` : `input request ${reference.inputRequestId ?? "unknown"}`;
  }
  __name(fact, "fact");
  function eventTargetLabel(target) {
    switch (target.kind) {
      case "revision":
        return `Revision ${target.revisionId}`;
      case "file":
        return `File ${target.filePath ?? "unknown"} in ${target.revisionId}`;
      case "range":
        return `${target.filePath ?? "unknown"}:${target.startLine ?? "?"}-${target.endLine ?? "?"} (${target.side ?? "?"}) in ${target.revisionId}`;
      case "observation":
        return `Observation ${target.observationId ?? "unknown"} in ${target.revisionId}`;
      case "input_request":
        return `Input request ${target.inputRequestId ?? "unknown"} in ${target.revisionId}`;
      case "assessment":
        return `Assessment ${target.assessmentId ?? "unknown"} in ${target.revisionId}`;
      case "event":
        return `Event ${target.eventId ?? "unknown"} in ${target.revisionId}`;
    }
  }
  __name(eventTargetLabel, "eventTargetLabel");
  function eventSubjectLabel(subject) {
    switch (subject.kind) {
      case "journal":
        return `Journal ${subject.journalId}`;
      case "review":
        return eventTargetLabel(subject.target);
      case "change":
        return `Change ${subject.changeId}`;
      case "change_membership_claim":
        return `Change membership claim ${subject.membershipClaimId}`;
      case "change_link_claim":
        return `Change link claim ${subject.linkClaimId}`;
      case "change_revision_relation_claim":
        return `Change Revision relation claim ${subject.relationClaimId}`;
      case "revision_relation_attestation":
        return `Revision relation attestation ${subject.relationAttestationId} for ${revision(subject.revision)}`;
      case "review_fact_port":
        return `Fact port ${subject.portId} from ${fact(subject.originFact)} on ${revision(subject.originRevision)}`;
    }
  }
  __name(eventSubjectLabel, "eventSubjectLabel");
  function eventTypeColor(eventType) {
    switch (eventType) {
      case "review_initialized":
      case "review_note_imported":
        return "var(--evt-init)";
      case "work_object_proposed":
      case "change_declared":
      case "change_membership_asserted":
        return "var(--evt-capture)";
      case "review_observation_recorded":
      case "revision_ref_associated":
      case "revision_commit_associated":
      case "review_fact_ported":
        return "var(--evt-observation)";
      case "review_assessment_recorded":
      case "revision_relation_attested":
        return "var(--evt-assessment)";
      case "input_request_opened":
      case "revision_ref_withdrawn":
      case "revision_commit_withdrawn":
      case "change_membership_withdrawn":
      case "change_revision_relation_withdrawn":
        return "var(--evt-request)";
      case "input_request_responded":
      case "change_link_asserted":
      case "change_revision_relation_asserted":
        return "var(--evt-response)";
      case "validation_check_recorded":
        return "var(--evt-validation)";
    }
  }
  __name(eventTypeColor, "eventTypeColor");
  function field(label2, value) {
    return value ? { label: label2, value } : null;
  }
  __name(field, "field");
  function fields(...values) {
    return values.filter(
      (value) => value !== null
    );
  }
  __name(fields, "fields");
  function presentEvent(entry) {
    const summary = entry.summary;
    switch (summary.kind) {
      case "review_initialized":
        return {
          label: "review initialized",
          title: "Review initialized",
          body: "The review journal was initialized.",
          fields: []
        };
      case "work_object_proposed": {
        const detail = summary.details;
        return {
          label: "Revision proposed",
          title: detail.summary || `Revision ${detail.revision.id} proposed`,
          body: `Captured ${detail.revision.objectId} as an exact Revision artifact.`,
          fields: fields(
            field("Revision", detail.revision.id),
            field("object", detail.revision.objectId),
            field("artifact", detail.objectArtifactContentHash),
            field("engagement", detail.engagementId),
            field(
              "supersedes",
              detail.supersedes.length ? detail.supersedes.join("; ") : "none"
            )
          )
        };
      }
      case "review_observation_recorded": {
        const detail = summary.details;
        return {
          label: "observation",
          title: detail.title,
          body: detail.body,
          fields: fields(
            field("observation", detail.observationId),
            field("target", eventTargetLabel(detail.target)),
            field("confidence", detail.confidence),
            field("tags", detail.tags?.join(", ")),
            field("supersedes", detail.supersedesObservationIds?.join("; ")),
            field("responds to", detail.respondsToObservationIds?.join("; "))
          )
        };
      }
      case "review_assessment_recorded": {
        const detail = summary.details;
        return {
          label: "assessment",
          title: `Assessment: ${words(detail.assessment)}`,
          body: detail.summary,
          fields: fields(
            field("assessment", detail.assessmentId),
            field("target", eventTargetLabel(detail.target)),
            field("replaces", detail.replacesAssessmentIds?.join("; ")),
            field("observations", detail.relatedObservationIds?.join("; ")),
            field("input requests", detail.relatedInputRequestIds?.join("; "))
          )
        };
      }
      case "input_request_opened": {
        const detail = summary.details;
        return {
          label: "input requested",
          title: detail.title,
          body: detail.body,
          fields: fields(
            field("input request", detail.inputRequestId),
            field("reason", words(detail.reasonCode)),
            field("target", eventTargetLabel(detail.target))
          )
        };
      }
      case "input_request_responded": {
        const detail = summary.details;
        return {
          label: "input response",
          title: `Input request ${words(detail.outcome)}`,
          body: detail.reason,
          fields: fields(
            field("response", detail.inputRequestResponseId),
            field("input request", detail.inputRequestId),
            field("Revision", detail.revisionId)
          )
        };
      }
      case "review_note_imported":
        return {
          label: "imported note",
          title: "Legacy review note imported",
          body: "A note from the retired import path remains in this journal.",
          fields: []
        };
      case "revision_ref_associated": {
        const detail = summary.details;
        return {
          label: "Git ref associated",
          title: `Associated ${detail.refName}`,
          fields: fields(
            field("association", detail.refAssociationId),
            field("target", eventTargetLabel(detail.target)),
            field("head", detail.headOid)
          )
        };
      }
      case "revision_ref_withdrawn": {
        const detail = summary.details;
        return {
          label: "Git ref withdrawn",
          title: "Withdrew a Git ref association",
          fields: fields(
            field("withdrawal", detail.refWithdrawalId),
            field("association", detail.refAssociationId),
            field("target", eventTargetLabel(detail.target))
          )
        };
      }
      case "revision_commit_associated": {
        const detail = summary.details;
        const endpoint = detail.commit.kind === "git_commit" ? `commit ${detail.commit.commitOid} · tree ${detail.commit.treeOid}` : detail.commit.kind === "git_working_tree" ? `working tree ${detail.commit.worktreeRoot}` : `${words(detail.commit.kind)} ${detail.commit.treeOid}`;
        return {
          label: "commit associated",
          title: `Associated ${endpoint}`,
          fields: fields(
            field("association", detail.commitAssociationId),
            field("target", eventTargetLabel(detail.target)),
            field("endpoint", endpoint)
          )
        };
      }
      case "revision_commit_withdrawn": {
        const detail = summary.details;
        return {
          label: "commit withdrawn",
          title: "Withdrew a commit association",
          fields: fields(
            field("withdrawal", detail.commitWithdrawalId),
            field("association", detail.commitAssociationId),
            field("target", eventTargetLabel(detail.target))
          )
        };
      }
      case "validation_check_recorded": {
        const detail = summary.details;
        return {
          label: "validation",
          title: `${detail.checkName}: ${words(detail.status)}`,
          body: detail.summary,
          fields: fields(
            field("validation", detail.validationCheckId),
            field("target", eventTargetLabel(detail.target)),
            field("command", detail.command),
            field("trigger", words(detail.trigger)),
            field(
              "exit code",
              detail.exitCode === void 0 ? void 0 : String(detail.exitCode)
            )
          )
        };
      }
      case "change_declared": {
        const detail = summary.details;
        const root = detail.identityDescriptor.kind === "root_revision" ? detail.identityDescriptor.revision_id : `opaque nonce ${detail.identityDescriptor.nonce}`;
        return {
          label: "Change declared",
          title: `Declared ${detail.changeId}`,
          body: `The stable Change identity is rooted in ${root}.`,
          fields: fields(
            field("declaration", detail.declarationClaimId),
            field("identity", root)
          )
        };
      }
      case "change_membership_asserted": {
        const detail = summary.details;
        return {
          label: "Change membership",
          title: `Added ${detail.revisionId} to a Change`,
          fields: fields(
            field("Change", detail.changeId),
            field("Revision", detail.revisionId),
            field("claim", detail.membershipClaimId)
          )
        };
      }
      case "change_membership_withdrawn": {
        const detail = summary.details;
        return {
          label: "membership withdrawn",
          title: "Withdrew a Change membership claim",
          fields: fields(
            field("claim", detail.membershipClaimId),
            field("withdrawal", detail.membershipWithdrawalId)
          )
        };
      }
      case "change_link_asserted": {
        const detail = summary.details;
        return {
          label: "Change link",
          title: `Linked Changes as ${words(detail.relation)}`,
          fields: fields(
            field("left Change", detail.leftChangeId),
            field("right Change", detail.rightChangeId),
            field("claim", detail.linkClaimId)
          )
        };
      }
      case "change_revision_relation_asserted": {
        const detail = summary.details;
        return {
          label: "Revision relation",
          title: "Asserted Revision supersession",
          body: `${detail.successor.revisionId} supersedes ${detail.predecessor.revisionId}.`,
          fields: fields(
            field("Change", detail.changeId),
            field("successor", revision(detail.successor)),
            field("predecessor", revision(detail.predecessor)),
            field("claim", detail.relationClaimId)
          )
        };
      }
      case "change_revision_relation_withdrawn": {
        const detail = summary.details;
        return {
          label: "relation withdrawn",
          title: "Withdrew a Revision-relation claim",
          fields: fields(
            field("claim", detail.relationClaimId),
            field("withdrawal", detail.relationWithdrawalId)
          )
        };
      }
      case "revision_relation_attested": {
        const detail = summary.details;
        return {
          label: "relation attested",
          title: `${words(detail.semanticRelation)}: ${words(detail.proofStatus)}`,
          body: `Proof method ${detail.proofMethod} (${detail.proofAlgorithmVersion}).`,
          fields: fields(
            field("Revision", revision(detail.revision)),
            field("commit association", detail.commitAssociationId),
            field("attestation", detail.relationAttestationId),
            field(
              "capture scope",
              detail.captureScope.join(", ") || "whole capture"
            ),
            field("comparison base", detail.comparisonBaseOrParent),
            field("endpoints", detail.endpointOids.join("; ")),
            field("evidence", detail.evidenceContentHash),
            field("result", detail.resultDigest)
          )
        };
      }
      case "review_fact_ported": {
        const detail = summary.details;
        return {
          label: "fact ported",
          title: `${words(detail.relation)} ${fact(detail.originFact)}`,
          body: `Ported review context from ${detail.originRevision.revisionId} to ${detail.targetRevision.revisionId}.`,
          fields: fields(
            field("port", detail.portId),
            field("origin Revision", revision(detail.originRevision)),
            field("origin fact", fact(detail.originFact)),
            field("target Revision", revision(detail.targetRevision)),
            field("target fact", detail.targetFact && fact(detail.targetFact)),
            field("Change context", detail.contextChangeId),
            field("rationale", detail.rationaleContentHash)
          )
        };
      }
    }
  }
  __name(presentEvent, "presentEvent");

  // src/dom.ts
  function $(sel) {
    return document.querySelector(sel);
  }
  __name($, "$");

  // src/prefs.ts
  var THEME_KEY = "shore-inspect-theme";
  var DENSITY_KEY = "shore-inspect-density";
  var SPLIT_KEY = "shore-inspect-split";
  var SPLIT_MIN = 25;
  var SPLIT_MAX = 75;
  var liveMediaQueries = [];
  var densityListeners = [];
  function registerDensityListener(listener) {
    densityListeners.push(listener);
  }
  __name(registerDensityListener, "registerDensityListener");
  function preferredThemeMode() {
    const stored = localStorage.getItem(THEME_KEY);
    return stored === "light" || stored === "dark" ? stored : "system";
  }
  __name(preferredThemeMode, "preferredThemeMode");
  function hasPinnedTheme() {
    return preferredThemeMode() !== "system";
  }
  __name(hasPinnedTheme, "hasPinnedTheme");
  function osTheme() {
    return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
  }
  __name(osTheme, "osTheme");
  function preferredTheme() {
    const mode = preferredThemeMode();
    return mode === "system" ? osTheme() : mode;
  }
  __name(preferredTheme, "preferredTheme");
  function syncChoice(name, value) {
    for (const input of document.querySelectorAll(
      `input[name="${name}"]`
    )) {
      input.checked = input.value === value;
    }
  }
  __name(syncChoice, "syncChoice");
  function applyTheme(theme) {
    document.documentElement.setAttribute("data-theme", theme);
    syncChoice("theme-mode", preferredThemeMode());
  }
  __name(applyTheme, "applyTheme");
  function setThemeMode(mode) {
    const next = mode === "light" || mode === "dark" ? mode : "system";
    localStorage.setItem(THEME_KEY, next);
    applyTheme(preferredTheme());
  }
  __name(setThemeMode, "setThemeMode");
  function preferredDensity() {
    return localStorage.getItem(DENSITY_KEY) || "comfortable";
  }
  __name(preferredDensity, "preferredDensity");
  function applyDensity(mode) {
    const value = mode === "compact" ? "compact" : "comfortable";
    document.documentElement.classList.toggle("compact", value === "compact");
    syncChoice("density-mode", value);
  }
  __name(applyDensity, "applyDensity");
  function setDensity(mode) {
    const next = mode === "compact" ? "compact" : "comfortable";
    localStorage.setItem(DENSITY_KEY, next);
    applyDensity(next);
  }
  __name(setDensity, "setDensity");
  function preferredSplit() {
    const raw = localStorage.getItem(SPLIT_KEY);
    const n = raw === null ? Number.NaN : Number.parseInt(raw, 10);
    return Number.isInteger(n) && n >= SPLIT_MIN && n <= SPLIT_MAX ? n : null;
  }
  __name(preferredSplit, "preferredSplit");
  function applySplit(pct) {
    if (pct === null) {
      document.documentElement.style.removeProperty("--split-master");
      localStorage.removeItem(SPLIT_KEY);
      return;
    }
    const clamped = Math.round(Math.min(SPLIT_MAX, Math.max(SPLIT_MIN, pct)));
    document.documentElement.style.setProperty("--split-master", `${clamped}%`);
    localStorage.setItem(SPLIT_KEY, String(clamped));
  }
  __name(applySplit, "applySplit");
  function applyPrefs() {
    applyTheme(preferredTheme());
    applyDensity(preferredDensity());
    const split = preferredSplit();
    if (split !== null) applySplit(split);
  }
  __name(applyPrefs, "applyPrefs");
  function watchColorScheme() {
    const query = window.matchMedia("(prefers-color-scheme: light)");
    liveMediaQueries.push(query);
    query.addEventListener("change", () => {
      if (hasPinnedTheme()) return;
      applyTheme(preferredTheme());
    });
  }
  __name(watchColorScheme, "watchColorScheme");

  // src/classNames.ts
  var CLASS = {
    // App chrome, master-detail panes, lens containers, and shared chips.
    units: "units",
    timeline: "timeline",
    empty: "empty",
    badge: "badge",
    tierMedium: "tier-medium",
    body: "body",
    title: "title",
    time: "time",
    eventDate: "event-date",
    rail: "rail",
    meta: "meta",
    type: "type",
    typeCount: "type-count",
    code: "code",
    dot: "dot",
    kv: "kv",
    ghost: "ghost",
    actions: "actions",
    timelineShell: "timeline-shell",
    timelineNewPill: "timeline-new-pill",
    // (The app-shell store-identity chip + detail popover is static markup in
    // index.html — `store-identity*` classes live there and in app.css, not here —
    // and its rows are `renderIdentity`-filled <dt>/<dd> styled via element selectors.
    // Issue #391.)
    // Fact cards (observation / input-request / assessment / validation / note).
    annoGroup: "anno-group",
    annoHead: "anno-head",
    annoLoc: "anno-loc",
    annoSummary: "anno-summary",
    annoTime: "anno-time",
    annoTitle: "anno-title",
    annoTrack: "anno-track",
    actorAttribution: "actor-attribution",
    factBodyRemoved: "fact-body-removed",
    factRel: "fact-rel",
    factResponse: "fact-response",
    factResponses: "fact-responses",
    factStaleContext: "fact-stale-context",
    factStatus: "fact-status",
    outcome: "outcome",
    advisoryNote: "advisory-note",
    validationNote: "validation-note",
    validationContinuity: "validation-continuity",
    validationContinuityNeutral: "validation-continuity-neutral",
    validationContinuityOutstanding: "validation-continuity-outstanding",
    readback: "readback",
    readbackRow: "readback-row",
    readerScopeNote: "reader-scope-note",
    rawEvent: "raw-event",
    rawEventActions: "raw-event-actions",
    // The current-assessment verdict block.
    verdictStatus: "verdict-status",
    verdictSummary: "verdict-summary",
    verdictValue: "verdict-value",
    // The advisory endorsement readback.
    endorseAttrs: "endorse-attrs",
    endorseLabel: "endorse-label",
    endorseList: "endorse-list",
    endorseWho: "endorse-who",
    endorsements: "endorsements",
    endorsementsLabel: "endorsements-label",
    // The revision-overview summary line.
    overviewAssessment: "overview-assessment",
    overviewCue: "overview-cue",
    overviewHistoryCue: "overview-history-cue",
    overviewCues: "overview-cues",
    overviewLabel: "overview-label",
    overviewLatest: "overview-latest",
    overviewMain: "overview-main",
    overviewMuted: "overview-muted",
    revisionDiagnostic: "revision-diagnostic",
    overviewStat: "overview-stat",
    overviewStats: "overview-stats",
    overviewSummary: "overview-summary",
    // The annotated snapshot diff: files, rows, and the navigator.
    dfileBody: "dfile-body",
    dfileHead: "dfile-head",
    dfileNotes: "dfile-notes",
    dfileSummary: "dfile-summary",
    dhunk: "dhunk",
    diffBtn: "diff-btn",
    diffAnchorReason: "diff-anchor-reason",
    diffDecisionContext: "diff-decision-context",
    diffDecisionContextNav: "diff-decision-context-nav",
    diffFactVicinity: "diff-fact-vicinity",
    diffFileNotice: "diff-file-notice",
    diffNavFact: "diff-nav-fact",
    diffNavFile: "diff-nav-file",
    diffNavFiles: "diff-nav-files",
    diffNavReason: "diff-nav-reason",
    diffNavSummary: "diff-nav-summary",
    diffUnanchored: "diff-unanchored",
    diffUnanchoredFacts: "diff-unanchored-facts",
    dpath: "dpath",
    drow: "drow",
    drowMeta: "drow-meta",
    dtext: "dtext",
    emph: "emph",
    ln: "ln",
    sign: "sign",
    // Revision list, supersession badges, and the laid-out DAG.
    unitCard: "unit-card",
    unitPage: "unit-page",
    unitPageTitle: "unit-page-title",
    supersessionBadges: "supersession-badges",
    competing: "competing",
    revisionSupersession: "revision-supersession",
    revisionHeads: "revision-heads",
    revisionSelf: "revision-self",
    dagEdge: "dag-edge",
    dagArrowHead: "dag-arrow-head",
    dagArrowHeadTraced: "dag-arrow-head-traced",
    revisionDag: "revision-dag",
    factDag: "fact-dag",
    head: "head",
    stale: "stale",
    superseded: "superseded",
    supersedes: "supersedes",
    upEmpty: "up-empty",
    upIdentity: "up-identity",
    upStat: "up-stat",
    upStats: "up-stats",
    // The applied-filter chip row (the toolbar's pure view of filterText).
    filterChips: "filter-chips",
    filterChipRemove: "filter-chip-remove",
    // The type facet section (the Timeline-only ?type= page-set control): static
    // container/list classes in index.html; rows are emitted via typeFacetRowClass.
    typeFacet: "type-facet",
    typeFacetMenu: "type-facet-menu",
    // The search-bar suggestion popover: static list container in index.html;
    // the rows are emitted via suggestionClass below.
    filterSuggestions: "filter-suggestions",
    suggestion: "suggestion",
    suggestionActive: "suggestion-active",
    // The command palette.
    cmdEmpty: "cmd-empty",
    cmdGroup: "cmd-group",
    cmdHint: "cmd-hint",
    cmdLabel: "cmd-label",
    // The attention lens: tiered cards over the outstanding review state.
    attentionCard: "attention-card",
    attentionTier: "attention-tier",
    attentionEmpty: "attention-empty",
    attentionOrderLabel: "attention-order-label",
    attentionKind: "attention-kind",
    attentionMeta: "attention-meta",
    attentionFreshness: "attention-freshness",
    attentionFocus: "attention-focus",
    attentionDelta: "attention-delta",
    // The attention tab's judgment-queue count badge (absent when both tiers are
    // empty) and the muted advisory count beside the needs-input number.
    attentionBadge: "attention-badge",
    attentionBadgeSecondary: "attention-badge-secondary",
    // The detail page's per-revision outstanding set (the scoped attention read);
    // absent when nothing is outstanding on the shown revision.
    outstandingSet: "outstanding-set",
    // Copyable CLI command handoffs (workflow-handoff.ts): the block, its label,
    // the command code, the visible placeholder marker, the clipboard-only copy
    // control, and the detail page's stage-template section host.
    workflowHandoff: "workflow-handoff",
    workflowHandoffLabel: "workflow-handoff-label",
    workflowCommand: "workflow-command",
    workflowPlaceholder: "workflow-placeholder",
    workflowCopy: "workflow-copy",
    workflowHandoffs: "workflow-handoffs"
  };
  var ANNO_KINDS = [
    "observation",
    "assessment",
    "input-request",
    "validation"
  ];
  var DIFF_ROW_KINDS = ["added", "removed", "context"];
  var TOKEN_KINDS = [
    "keyword",
    "string",
    "comment",
    "number",
    "type",
    "function",
    "constant",
    "operator",
    "punctuation",
    "variable"
  ];
  var DIFF_FILE_STATUSES = [
    "added",
    "deleted",
    "modified",
    "renamed",
    "copied"
  ];
  var VERIFY_STATUSES = [
    "valid",
    "invalid",
    "unsigned",
    "untrusted_key"
  ];
  var ENDORSE_CLASSES = [
    "endorsement-trusted",
    "ambiguous_endorser",
    "unknown_endorser"
  ];
  var VERDICT_ASSESSMENTS = [
    "accepted",
    "accepted_with_follow_up",
    "ambiguous",
    "needs_changes",
    "needs_clarification",
    "unassessed"
  ];
  var FACT_STATUSES = [
    "accepted",
    "accepted_with_follow_up",
    "ambiguous",
    "current",
    "errored",
    "failed",
    "needs_changes",
    "needs_clarification",
    "open",
    "passed",
    "replaced",
    "resolved",
    "responded",
    "skipped",
    "stale",
    "superseded",
    "unassessed"
  ];
  var REF_ID_PREFIXES = [
    "input-request-response",
    "input-request",
    "obs",
    "assess",
    "rev",
    "evt",
    "validation",
    "obj",
    "engagement",
    "checkpoint",
    "task-attempt",
    "assoc-commit",
    "assoc-ref",
    "withdraw-commit",
    "withdraw-ref"
  ];
  var REF_KINDS = [
    ...REF_ID_PREFIXES,
    "hash",
    "commit",
    "track",
    "actor"
  ];
  var annoContainerClass = /* @__PURE__ */ __name((kind) => `anno anno-${kind}`, "annoContainerClass");
  var annoKindClass = /* @__PURE__ */ __name((kind) => `anno-kind anno-kind-${kind}`, "annoKindClass");
  var drowClass = /* @__PURE__ */ __name((kind, noted) => `drow drow-${kind}${noted ? " drow-noted" : ""}`, "drowClass");
  var tokClass = /* @__PURE__ */ __name((kind) => `tok tok-${kind}`, "tokClass");
  var diffStatusClass = /* @__PURE__ */ __name((status) => `dstatus s-${status}`, "diffStatusClass");
  var verifyClass = /* @__PURE__ */ __name((status) => `verify verify-${status}`, "verifyClass");
  var endorseClass = /* @__PURE__ */ __name((cls) => `endorse endorse-${cls}`, "endorseClass");
  var verdictClass = /* @__PURE__ */ __name((assessment) => `verdict verdict-${assessment}`, "verdictClass");
  var factStatusClass = /* @__PURE__ */ __name((status) => `fact-status ${status}`, "factStatusClass");
  var refClass = /* @__PURE__ */ __name((kind) => `ref ref-${kind}`, "refClass");
  var dfileClass = /* @__PURE__ */ __name((lowSignal) => `dfile${lowSignal ? " dfile-lowsignal" : ""}`, "dfileClass");
  var dagNodeClass = /* @__PURE__ */ __name((o) => `dag-node${o.isHead ? " head" : ""}${o.isSuperseded ? " superseded" : ""}`, "dagNodeClass");
  var bodyClass = /* @__PURE__ */ __name((base, markdown) => `${base}${markdown ? " markdown-body" : ""}`, "bodyClass");
  var cmdItemClass = /* @__PURE__ */ __name((active3) => `cmd-item${active3 ? " active" : ""}`, "cmdItemClass");
  var filterChipClass = /* @__PURE__ */ __name((negated) => `filter-chip${negated ? " filter-chip-negated" : ""}`, "filterChipClass");
  var typeFacetRowClass = /* @__PURE__ */ __name((enabled) => `type-facet-row${enabled ? "" : " type-facet-row-off"}`, "typeFacetRowClass");
  var suggestionClass = /* @__PURE__ */ __name((active3) => `suggestion${active3 ? " suggestion-active" : ""}`, "suggestionClass");
  var tokensOf = /* @__PURE__ */ __name((classStrings) => classStrings.flatMap((s) => s.split(" ")), "tokensOf");
  var ALL_EMITTABLE_CLASSES = [
    ...new Set(
      tokensOf([
        ...Object.values(CLASS),
        ...ANNO_KINDS.map((k) => annoContainerClass(k)),
        ...ANNO_KINDS.map((k) => annoKindClass(k)),
        ...DIFF_ROW_KINDS.map((k) => drowClass(k, true)),
        ...TOKEN_KINDS.map((k) => tokClass(k)),
        ...DIFF_FILE_STATUSES.map((s) => diffStatusClass(s)),
        ...VERIFY_STATUSES.map((s) => verifyClass(s)),
        ...ENDORSE_CLASSES.map((c) => endorseClass(c)),
        ...VERDICT_ASSESSMENTS.map((a) => verdictClass(a)),
        ...FACT_STATUSES.map((s) => factStatusClass(s)),
        ...REF_KINDS.map((k) => refClass(k)),
        dfileClass(true),
        filterChipClass(true),
        typeFacetRowClass(true),
        typeFacetRowClass(false),
        suggestionClass(true),
        dagNodeClass({ isHead: true, isSuperseded: true }),
        bodyClass("anno-body", true),
        bodyClass("verdict-summary", true),
        cmdItemClass(true)
      ])
    )
  ];

  // src/escape.ts
  var ENTITIES = {
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    '"': "&quot;",
    "'": "&#39;"
  };
  function escapeHtml(value) {
    return String(value).replace(/[&<>"']/g, (char) => ENTITIES[char]);
  }
  __name(escapeHtml, "escapeHtml");

  // src/refs.ts
  function shortRef(id) {
    const value = String(id);
    let match = value.match(
      /^([a-z][a-z-]*):(?:git:|worktree:)?sha256:([0-9a-f]{6,})$/i
    );
    if (match) return `${match[1]}:${match[2].slice(0, 8)}`;
    match = value.match(/^sha256:([0-9a-f]{8,})$/i);
    if (match) return `sha256:${match[1].slice(0, 8)}`;
    if (/^[0-9a-f]{40}$/i.test(value)) return value.slice(0, 10);
    return value;
  }
  __name(shortRef, "shortRef");
  function shortExactRevision(revision2) {
    return `${shortRef(revision2.revisionId)} · ${shortRef(
      revision2.objectArtifactContentHash
    )}`;
  }
  __name(shortExactRevision, "shortExactRevision");
  function exactRevisionAccessibleIdentity(revision2) {
    return `exact Revision ${revision2.revisionId}; artifact ${revision2.objectArtifactContentHash}`;
  }
  __name(exactRevisionAccessibleIdentity, "exactRevisionAccessibleIdentity");
  var OPAQUE_ID_RE = /\b(?:[a-z][a-z-]*:(?:git:|worktree:)?sha256:[0-9a-f]{6,}|sha256:[0-9a-f]{16,}|[0-9a-f]{40})\b/gi;
  function compactIdentityText(value) {
    return value.replace(OPAQUE_ID_RE, (identity) => shortRef(identity));
  }
  __name(compactIdentityText, "compactIdentityText");
  var NON_CLICKABLE_KINDS = /* @__PURE__ */ new Set([
    "validation",
    "obj",
    "engagement",
    "checkpoint",
    "task-attempt",
    "assoc-commit",
    "assoc-ref",
    "withdraw-commit",
    "withdraw-ref"
  ]);
  function refInfo(token) {
    const match = token.match(
      /^([a-z][a-z-]*):(?:git:|worktree:)?sha256:[0-9a-f]+$/i
    );
    if (match) {
      const kind = match[1].toLowerCase();
      return { kind, clickable: !NON_CLICKABLE_KINDS.has(kind) };
    }
    if (/^sha256:[0-9a-f]+$/i.test(token))
      return { kind: "hash", clickable: false };
    if (/^[0-9a-f]{40}$/i.test(token))
      return { kind: "commit", clickable: false };
    if (/^(agent|human):[a-z0-9][a-z0-9_-]*$/i.test(token)) {
      return { kind: "track", clickable: true };
    }
    return null;
  }
  __name(refInfo, "refInfo");
  var REF_RE = new RegExp(
    `\\b(?:${REF_ID_PREFIXES.join("|")}):(?:git:|worktree:)?sha256:[0-9a-f]{6,}\\b|(?<!:)\\bsha256:[0-9a-f]{16,}\\b|\\b[0-9a-f]{40}\\b|\\b(?:agent|human):[a-z0-9][a-z0-9_-]*\\b`,
    "gi"
  );
  function linkifyEscaped(escaped, opts = {}) {
    const tabIndex = typeof opts === "object" ? opts.tabIndex ?? 0 : 0;
    return escaped.replace(REF_RE, (token) => {
      const info = refInfo(token);
      if (!info) return token;
      const display = escapeHtml(shortRef(token));
      if (!info.clickable) {
        return `<span class="${refClass(info.kind)}" data-ref-id="${escapeHtml(token)}" title="${escapeHtml(token)}" aria-label="${escapeHtml(token)}">${display}</span>`;
      }
      return `<span class="${refClass(info.kind)}" role="link" tabindex="${tabIndex}" data-ref-kind="${info.kind}" data-ref-id="${escapeHtml(token)}" title="${escapeHtml(token)}" aria-label="${escapeHtml(token)}">${display}</span>`;
    });
  }
  __name(linkifyEscaped, "linkifyEscaped");
  function linkify(text, opts = {}) {
    return linkifyEscaped(escapeHtml(String(text ?? "")), opts);
  }
  __name(linkify, "linkify");
  function actorChip(actorId, opts = {}) {
    if (!actorId) return "";
    const tabIndex = typeof opts === "object" ? opts.tabIndex ?? 0 : opts;
    const display = escapeHtml(actorId);
    return `<span class="${refClass("actor")}" role="link" tabindex="${tabIndex}" data-ref-kind="actor" data-ref-id="${escapeHtml(actorId)}" title="filter to ${escapeHtml(actorId)}">${display}</span>`;
  }
  __name(actorChip, "actorChip");
  function isMarkdownContentType(contentType) {
    return contentType === "text/markdown";
  }
  __name(isMarkdownContentType, "isMarkdownContentType");
  function safeMarkdownHref(href) {
    const raw = String(href ?? "").trim();
    if (/^(https?:|mailto:)/i.test(raw) || raw.startsWith("#"))
      return escapeHtml(raw);
    return "";
  }
  __name(safeMarkdownHref, "safeMarkdownHref");

  // src/change-inspector-timeline.ts
  var FALLBACK_ROW_HEIGHT = 72;
  var OVERSCAN = 8;
  var REMEASURE_SETTLE_MS = 150;
  var active = null;
  function label(value) {
    return value.replaceAll("_", " ");
  }
  __name(label, "label");
  var MAX_TIMELINE_TITLE = 120;
  var MAX_TIMELINE_EXCERPT = 180;
  function compactTimelineText(value, limit) {
    const compact = compactIdentityText(value).replace(/\s+/g, " ").trim();
    if (compact.length <= limit) return compact;
    return `${compact.slice(0, limit - 1).trimEnd()}…`;
  }
  __name(compactTimelineText, "compactTimelineText");
  function timelineTitle(value) {
    return compactTimelineText(value, MAX_TIMELINE_TITLE);
  }
  __name(timelineTitle, "timelineTitle");
  function timelineExcerpt(value) {
    return compactTimelineText(value, MAX_TIMELINE_EXCERPT);
  }
  __name(timelineExcerpt, "timelineExcerpt");
  function appendTimelineLink(parent, identity, kind, href) {
    const link = document.createElement("a");
    link.className = "ref";
    link.href = href;
    link.tabIndex = -1;
    link.title = identity;
    link.dataset.timelineContextKind = kind.toLowerCase();
    link.dataset.timelineContextId = identity;
    link.setAttribute("aria-label", `Open ${kind} ${identity}`);
    link.textContent = shortRef(identity);
    parent.append(link);
    return link;
  }
  __name(appendTimelineLink, "appendTimelineLink");
  function appendExactRevisionLink(parent, reference, route) {
    const link = appendTimelineLink(
      parent,
      reference.revisionId,
      "Revision",
      formatChangeInspectorRoute({
        kind: "timeline",
        historyQuery: {
          ...route.historyQuery,
          after: void 0,
          at: void 0,
          change: void 0,
          revision: reference.revisionId,
          artifactHash: reference.objectArtifactContentHash
        }
      })
    );
    const fullIdentity = exactRevisionAccessibleIdentity(reference);
    link.textContent = shortExactRevision(reference);
    link.title = fullIdentity;
    link.setAttribute("aria-label", `Filter Timeline to ${fullIdentity}`);
    link.dataset.revisionId = reference.revisionId;
    link.dataset.artifactHash = reference.objectArtifactContentHash;
  }
  __name(appendExactRevisionLink, "appendExactRevisionLink");
  function optionId(eventId) {
    return `timeline-event-${encodeURIComponent(eventId).replaceAll("%", "_")}`;
  }
  __name(optionId, "optionId");
  function rowSpacer(height) {
    const spacer = document.createElement("li");
    spacer.dataset.timelineSpacer = "true";
    spacer.setAttribute("aria-hidden", "true");
    spacer.style.height = `${height}px`;
    return spacer;
  }
  __name(rowSpacer, "rowSpacer");
  function appendChip(row, text) {
    const chip = document.createElement("span");
    chip.className = "badge";
    chip.textContent = text;
    row.append(chip);
  }
  __name(appendChip, "appendChip");
  function appendVerificationChip(row, status) {
    const chip = document.createElement("span");
    chip.className = `verify verify-${status}`;
    chip.title = "event signature verification status";
    chip.textContent = `verify: ${label(status)}`;
    row.append(chip);
  }
  __name(appendVerificationChip, "appendVerificationChip");
  function entryRow(entry, selectedEventId, route) {
    const presentation = presentEvent(entry);
    const row = document.createElement("li");
    row.className = "event";
    row.dataset.eventId = entry.eventId;
    row.id = optionId(entry.eventId);
    row.tabIndex = -1;
    row.setAttribute("role", "option");
    row.setAttribute("aria-selected", String(entry.eventId === selectedEventId));
    row.setAttribute(
      "aria-label",
      `${presentation.title}; ${entry.eventType}; writer ${entry.writer.actorId}; ${entry.occurredAt}; event ${entry.eventId}; Changes ${entry.changeIds.join(", ") || "none"}; exact Revisions ${entry.revisionRefs.map((reference) => `${reference.revisionId} ${reference.objectArtifactContentHash}`).join(", ") || "none"}; unresolved Revisions ${entry.unresolvedRevisionIds.join(", ") || "none"}`
    );
    const occurred = new Date(entry.occurredAt);
    const time = document.createElement("time");
    time.className = "time";
    time.dateTime = entry.occurredAt;
    if (Number.isNaN(occurred.valueOf())) {
      time.textContent = entry.occurredAt;
    } else {
      const date = document.createElement("span");
      date.className = "event-date";
      date.textContent = occurred.toLocaleDateString();
      const clock = document.createElement("span");
      clock.textContent = occurred.toLocaleTimeString([], {
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit"
      });
      time.append(date, clock);
    }
    const rail = document.createElement("span");
    rail.className = "rail";
    rail.style.background = eventTypeColor(entry.eventType);
    rail.setAttribute("aria-hidden", "true");
    const body = document.createElement("div");
    body.className = "body";
    const heading = document.createElement("h3");
    heading.className = "title";
    heading.textContent = timelineTitle(presentation.title);
    heading.title = presentation.title;
    if (presentation.body) {
      const summary = document.createElement("p");
      summary.className = "event-summary";
      summary.textContent = timelineExcerpt(presentation.body);
      body.append(heading, summary);
    } else {
      body.append(heading);
    }
    const meta = document.createElement("div");
    meta.className = "mono";
    meta.classList.add("meta");
    const eventType = document.createElement("span");
    eventType.className = "type";
    eventType.textContent = presentation.label;
    eventType.title = entry.eventType;
    eventType.style.color = eventTypeColor(entry.eventType);
    meta.append(eventType);
    appendVerificationChip(meta, entry.verificationStatus);
    if (entry.trackId) appendChip(meta, `track ${entry.trackId}`);
    const actor = document.createElement("span");
    actor.textContent = entry.writer.actorId;
    actor.title = `writer ${entry.writer.actorId}`;
    meta.append(actor);
    appendTimelineLink(
      meta,
      entry.eventId,
      "event",
      formatChangeInspectorRoute(
        timelineEventRoute(entry.eventId, route.historyQuery)
      )
    );
    const contexts = document.createElement("p");
    contexts.className = "event-context mono";
    if (entry.changeIds.length) {
      const changes = document.createElement("span");
      changes.textContent = "Changes ";
      contexts.append(changes);
      entry.changeIds.forEach((changeId, index) => {
        if (index) contexts.append(document.createTextNode(", "));
        appendTimelineLink(
          contexts,
          changeId,
          "Change",
          formatChangeInspectorRoute({ kind: "change", changeId, query: {} })
        );
      });
    }
    if (entry.revisionRefs.length) {
      if (contexts.childNodes.length)
        contexts.append(document.createTextNode(" · "));
      const revisions = document.createElement("span");
      revisions.textContent = "Revisions ";
      contexts.append(revisions);
      entry.revisionRefs.forEach((reference, index) => {
        if (index) contexts.append(document.createTextNode(", "));
        appendExactRevisionLink(contexts, reference, route);
      });
    }
    if (entry.unresolvedRevisionIds.length) {
      if (contexts.childNodes.length)
        contexts.append(document.createTextNode(" · "));
      const unresolved = document.createElement("span");
      unresolved.className = "warning";
      unresolved.title = entry.unresolvedRevisionIds.join(", ");
      unresolved.textContent = `unresolved ${entry.unresolvedRevisionIds.map(shortRef).join(", ")}`;
      contexts.append(unresolved);
    }
    body.append(meta);
    if (contexts.childNodes.length) body.append(contexts);
    row.append(time, rail, body);
    return row;
  }
  __name(entryRow, "entryRow");
  function paintVisible(view) {
    const { list, document: timeline, rowHeight } = view;
    const entries = timeline.entries;
    const viewport = list.clientHeight;
    const localStart = viewport > 0 ? Math.max(0, Math.floor(list.scrollTop / rowHeight) - OVERSCAN) : 0;
    const localEnd = viewport > 0 ? Math.min(
      entries.length,
      Math.ceil((list.scrollTop + viewport) / rowHeight) + OVERSCAN
    ) : entries.length;
    const top = rowSpacer(localStart * rowHeight);
    const bottom = rowSpacer(Math.max(0, entries.length - localEnd) * rowHeight);
    list.replaceChildren(
      top,
      ...entries.slice(localStart, localEnd).map((entry) => entryRow(entry, view.selectedEventId, view.route)),
      bottom
    );
    const activeOption = view.selectedEventId ? Array.from(list.querySelectorAll("[data-event-id]")).find(
      (row) => row.dataset.eventId === view.selectedEventId
    ) : null;
    if (activeOption) {
      list.setAttribute("aria-activedescendant", activeOption.id);
    } else {
      list.removeAttribute("aria-activedescendant");
    }
  }
  __name(paintVisible, "paintVisible");
  function anchoredScrollTop(view, nextRowHeight) {
    const { list, rowHeight: previousRowHeight } = view;
    const listTop = list.getBoundingClientRect().top;
    const leading = list.firstElementChild;
    const leadingHeight = leading?.dataset.timelineSpacer ? Number.parseFloat(leading.style.height) || 0 : 0;
    const paintStart = Math.round(leadingHeight / previousRowHeight);
    const rows = list.querySelectorAll("li.event[data-event-id]");
    let localIndex = 0;
    for (const row of rows) {
      const bounds = row.getBoundingClientRect();
      if (bounds.height > 0 && bounds.bottom > listTop) {
        return Math.max(
          0,
          (paintStart + localIndex) * nextRowHeight - (bounds.top - listTop)
        );
      }
      localIndex += 1;
    }
    return list.scrollTop / previousRowHeight * nextRowHeight;
  }
  __name(anchoredScrollTop, "anchoredScrollTop");
  function remeasureChangeInspectorTimelineRows() {
    const view = active;
    if (view === null || !view.list.isConnected) return false;
    const rows = Array.from(
      view.list.querySelectorAll("li.event[data-event-id]")
    );
    if (rows.length === 0) return false;
    const mean = rows.reduce((total, row) => total + row.getBoundingClientRect().height, 0) / rows.length;
    if (!Number.isFinite(mean) || mean <= 0) return false;
    if (Math.abs(mean - view.rowHeight) < 0.5) return false;
    const anchored = anchoredScrollTop(view, mean);
    view.rowHeight = mean;
    view.list.scrollTop = anchored;
    paintVisible(view);
    return true;
  }
  __name(remeasureChangeInspectorTimelineRows, "remeasureChangeInspectorTimelineRows");
  function scheduleChangeInspectorTimelineRemeasure() {
    const view = active;
    if (view === null) return;
    if (view.remeasureTimer !== null) clearTimeout(view.remeasureTimer);
    view.remeasureTimer = setTimeout(() => {
      view.remeasureTimer = null;
      if (active === view) remeasureChangeInspectorTimelineRows();
    }, REMEASURE_SETTLE_MS);
  }
  __name(scheduleChangeInspectorTimelineRemeasure, "scheduleChangeInspectorTimelineRemeasure");
  registerDensityListener(scheduleChangeInspectorTimelineRemeasure);
  function disposeActiveTimeline() {
    if (active === null) return;
    if (active.remeasureTimer !== null) clearTimeout(active.remeasureTimer);
    active.resizeObserver?.disconnect();
  }
  __name(disposeActiveTimeline, "disposeActiveTimeline");
  function renderChangeInspectorTimeline(master, timeline, actions2, route, selectedEventId = null) {
    const key = `${timeline.timelineProjectionStamp}\0${JSON.stringify(route.historyQuery)}`;
    if (master.dataset.timelineKey === key && active !== null) {
      const exactRouteChanged = selectedEventId !== active.routeSelectedEventId;
      active.document = timeline;
      active.route = route;
      active.list.dataset.timelineRoute = formatChangeInspectorRoute(route);
      active.routeSelectedEventId = selectedEventId;
      if (exactRouteChanged && selectedEventId !== null) {
        active.selectedEventId = selectedEventId;
      }
      paintVisible(active);
      if (exactRouteChanged && selectedEventId !== null) {
        revealChangeInspectorTimelineEvent(selectedEventId);
      }
      return;
    }
    const section = document.createElement("section");
    section.className = "timeline-shell";
    const heading = document.createElement("h1");
    heading.textContent = `Timeline · ${timeline.matchCount}`;
    const notice = document.createElement("p");
    notice.className = "timeline-summary dim";
    const loadedStart = timeline.entries.length ? timeline.offset + 1 : 0;
    const loadedEnd = timeline.offset + timeline.entries.length;
    notice.textContent = `${timeline.order === "desc" ? "Newest" : "Oldest"} first · loaded ${loadedStart}-${loadedEnd} of ${timeline.matchCount} matches · ${timeline.eventCount} recorded events. Presentation chronology uses writer timestamps; late events can backfill when writer clocks differ.`;
    const notices = document.createElement("div");
    notices.className = "timeline-notices";
    notices.setAttribute("aria-live", "polite");
    for (const message2 of timeline.queryNotices) {
      const line = document.createElement("p");
      line.className = "info";
      line.textContent = `Query notice: ${message2}`;
      notices.append(line);
    }
    for (const message2 of timeline.diagnostics) {
      const line = document.createElement("p");
      line.className = "warning";
      line.textContent = `Timeline diagnostic: ${message2}`;
      notices.append(line);
    }
    const page = document.createElement("div");
    page.className = "actions";
    if (timeline.previous) {
      const previousRoute = {
        kind: "timeline",
        historyQuery: {
          ...route.historyQuery,
          at: void 0,
          after: timeline.previous
        }
      };
      const previous = document.createElement("button");
      previous.type = "button";
      previous.className = "ghost";
      previous.dataset.timelinePage = "previous";
      previous.dataset.timelineTargetRoute = formatChangeInspectorRoute(previousRoute);
      previous.textContent = "Previous page";
      previous.addEventListener("click", () => actions2.navigate(previousRoute));
      page.append(previous);
    }
    if (timeline.next) {
      const nextRoute = {
        kind: "timeline",
        historyQuery: {
          ...route.historyQuery,
          at: void 0,
          after: timeline.next
        }
      };
      const next = document.createElement("button");
      next.type = "button";
      next.className = "ghost";
      next.dataset.timelinePage = "next";
      next.dataset.timelineTargetRoute = formatChangeInspectorRoute(nextRoute);
      next.textContent = "Next page";
      next.addEventListener("click", () => actions2.navigate(nextRoute));
      page.append(next);
    }
    const list = document.createElement("ol");
    list.id = "timeline";
    list.className = "timeline";
    list.dataset.timelineRoute = formatChangeInspectorRoute(route);
    list.tabIndex = timeline.entries.length ? 0 : -1;
    list.setAttribute("role", "listbox");
    list.setAttribute("aria-label", "event timeline");
    if (!timeline.entries.length) list.setAttribute("aria-disabled", "true");
    section.append(heading, notice, notices, page);
    if (timeline.matchCount === 0) {
      const empty = document.createElement("p");
      empty.className = "timeline-empty dim";
      empty.setAttribute("role", "status");
      empty.textContent = "No Timeline events match the current filters.";
      section.append(empty);
    }
    section.append(list);
    delete master.dataset.changeListKey;
    disposeActiveTimeline();
    master.replaceChildren(section);
    master.dataset.timelineKey = key;
    active = {
      document: timeline,
      list,
      remeasureTimer: null,
      resizeObserver: null,
      rowHeight: FALLBACK_ROW_HEIGHT,
      route,
      routeSelectedEventId: selectedEventId,
      selectedEventId
    };
    const view = active;
    list.addEventListener("scroll", () => {
      if (active === view) paintVisible(view);
    });
    if (typeof ResizeObserver !== "undefined") {
      view.resizeObserver = new ResizeObserver(() => {
        if (active === view) scheduleChangeInspectorTimelineRemeasure();
      });
      view.resizeObserver.observe(list);
    }
    paintVisible(view);
    if (selectedEventId !== null) {
      revealChangeInspectorTimelineEvent(selectedEventId);
    } else {
      remeasureChangeInspectorTimelineRows();
    }
  }
  __name(renderChangeInspectorTimeline, "renderChangeInspectorTimeline");
  function revealChangeInspectorTimelineEvent(eventId) {
    if (active === null) return false;
    const localIndex = active.document.entries.findIndex(
      (entry) => entry.eventId === eventId
    );
    if (localIndex < 0) return false;
    active.selectedEventId = eventId;
    remeasureChangeInspectorTimelineRows();
    const top = localIndex * active.rowHeight;
    const bottom = top + active.rowHeight;
    if (top < active.list.scrollTop) active.list.scrollTop = top;
    else if (bottom > active.list.scrollTop + active.list.clientHeight) {
      active.list.scrollTop = Math.max(0, bottom - active.list.clientHeight);
    }
    paintVisible(active);
    remeasureChangeInspectorTimelineRows();
    paintVisible(active);
    let selected = Array.from(
      active.list.querySelectorAll("li.event[data-event-id]")
    ).find((row) => row.dataset.eventId === eventId);
    if (selected === void 0) {
      if (localIndex === 0) active.list.scrollTop = 0;
      else if (localIndex === active.document.entries.length - 1) {
        active.list.scrollTop = active.list.scrollHeight;
      }
      paintVisible(active);
      selected = Array.from(
        active.list.querySelectorAll("li.event[data-event-id]")
      ).find((row) => row.dataset.eventId === eventId);
    }
    selected?.scrollIntoView({ block: "nearest", behavior: "auto" });
    return selected !== void 0;
  }
  __name(revealChangeInspectorTimelineEvent, "revealChangeInspectorTimelineEvent");

  // src/change-inspector-timeline-navigation.ts
  function selectedIndex(window2) {
    return window2.selectedEventId === null ? -1 : window2.eventIds.indexOf(window2.selectedEventId);
  }
  __name(selectedIndex, "selectedIndex");
  function moveTimelineSelection(window2, delta) {
    const { eventIds } = window2;
    if (eventIds.length === 0 || delta === 0) return null;
    const current = selectedIndex(window2);
    if (current < 0) {
      const eventId = eventIds[0];
      return eventId ? { kind: "select", eventId } : null;
    }
    const start = current;
    const target = start + delta;
    if (target >= 0 && target < eventIds.length) {
      const eventId = eventIds[target];
      return eventId ? { kind: "select", eventId } : null;
    }
    if (target >= eventIds.length) {
      return {
        kind: "adjacent-page",
        direction: "next",
        index: target - eventIds.length
      };
    }
    return {
      kind: "adjacent-page",
      direction: "previous",
      indexFromEnd: Math.abs(target) - 1
    };
  }
  __name(moveTimelineSelection, "moveTimelineSelection");
  function boundaryTimelineSelection(window2, boundary) {
    const eventId = boundary === "first" ? window2.eventIds[0] : window2.eventIds.at(-1);
    return eventId ? { kind: "select", eventId } : null;
  }
  __name(boundaryTimelineSelection, "boundaryTimelineSelection");
  function pageTimelineSelection(window2, direction, visibleRows, fraction) {
    const rows = Math.max(1, Math.floor(visibleRows));
    const delta = fraction === "full" ? rows : Math.max(1, Math.ceil(rows / 2));
    return moveTimelineSelection(
      window2,
      direction === "forward" ? delta : -delta
    );
  }
  __name(pageTimelineSelection, "pageTimelineSelection");
  function resolveTimelinePageSelection(eventIds, intent) {
    if (eventIds.length === 0) return null;
    if (intent.direction === "next") {
      return eventIds[Math.min(intent.index ?? 0, eventIds.length - 1)] ?? null;
    }
    return eventIds[Math.max(0, eventIds.length - 1 - (intent.indexFromEnd ?? 0))] ?? null;
  }
  __name(resolveTimelinePageSelection, "resolveTimelinePageSelection");

  // src/change-inspector-interaction.ts
  var colorSchemeWatcherInstalled = false;
  var HISTORY_ORIGIN_KEY = "__pointbreakChangeInspectorOrigin";
  var MASTER_SURFACE_KEYS = /* @__PURE__ */ new Set([
    "/",
    "j",
    "k",
    "ArrowDown",
    "ArrowUp",
    "g",
    "G",
    "f",
    "b",
    "d",
    "u",
    "F",
    "h",
    "l"
  ]);
  function isTextControl(target) {
    if (!(target instanceof HTMLElement)) return false;
    return target.isContentEditable || target.matches(
      "input, textarea, select, [role='textbox'], [role='combobox']"
    );
  }
  __name(isTextControl, "isTextControl");
  function isNativeActionControl(target) {
    return target instanceof Element && target.closest(
      "button, summary, a[href], [role='button'], [role='link'], [role='separator']"
    ) !== null;
  }
  __name(isNativeActionControl, "isNativeActionControl");
  function isTimelineListTarget(target) {
    return target instanceof Element && (target.matches("#timeline") || target.closest("#timeline") !== null);
  }
  __name(isTimelineListTarget, "isTimelineListTarget");
  function narrowDetailOwnsFocus(target) {
    return target instanceof Element && target.closest("#detail") !== null && window.matchMedia("(max-width: 760px)").matches;
  }
  __name(narrowDetailOwnsFocus, "narrowDetailOwnsFocus");
  function refineDiffFocus(route, patch) {
    const focus = { ...route.focus, ...patch };
    for (const key of Object.keys(focus)) {
      if (!focus[key]) delete focus[key];
    }
    return { ...route, ...Object.keys(focus).length ? { focus } : {} };
  }
  __name(refineDiffFocus, "refineDiffFocus");
  function moveDiffTarget(items, current, delta, identity) {
    if (!items.length) return null;
    const index = items.findIndex((item) => identity(item) === current);
    return items[Math.max(0, Math.min(items.length - 1, (index < 0 ? -1 : index) + delta))] ?? null;
  }
  __name(moveDiffTarget, "moveDiffTarget");
  function revisionRouteFromDiff(route) {
    return {
      kind: "revision",
      changeId: route.changeId,
      revision: route.revision,
      query: route.query,
      ...route.focus && (route.focus.factId || route.focus.filePath) ? {
        focus: {
          ...route.focus.factId ? { factId: route.focus.factId } : {},
          ...route.focus.filePath ? { filePath: route.focus.filePath } : {}
        }
      } : {}
    };
  }
  __name(revisionRouteFromDiff, "revisionRouteFromDiff");
  function exactActivationIdentity(route) {
    if (route?.kind === "event") return `event\0${route.eventId}`;
    if (route?.kind !== "revision") return null;
    return `revision\0${route.changeId}\0${route.revision.revisionId}\0${route.revision.objectArtifactContentHash}`;
  }
  __name(exactActivationIdentity, "exactActivationIdentity");
  function diffIdentity(route) {
    return {
      changeId: route.changeId,
      revisionId: route.revision.revisionId,
      objectArtifactContentHash: route.revision.objectArtifactContentHash
    };
  }
  __name(diffIdentity, "diffIdentity");
  function sameDiffIdentity(left, right) {
    return left !== null && right !== null && left.changeId === right.changeId && left.revisionId === right.revisionId && left.objectArtifactContentHash === right.objectArtifactContentHash;
  }
  __name(sameDiffIdentity, "sameDiffIdentity");
  function companionTimelineRoute(route) {
    if (route?.kind === "timeline") return route;
    if (route?.kind === "event") {
      return { kind: "timeline", historyQuery: route.historyQuery };
    }
    return null;
  }
  __name(companionTimelineRoute, "companionTimelineRoute");
  function setSelected(changeId) {
    document.querySelectorAll(".unit-card[data-change-id]").forEach((card) => {
      const selected = card.dataset.changeId === changeId;
      card.classList.toggle("change-card-selected", selected);
      card.setAttribute("aria-current", selected ? "true" : "false");
    });
  }
  __name(setSelected, "setSelected");
  function moveSelection(selectedChangeId, delta) {
    const cards = Array.from(
      document.querySelectorAll(".unit-card[data-change-id]")
    );
    if (!cards.length) return selectedChangeId;
    const current = cards.findIndex(
      (card2) => card2.dataset.changeId === selectedChangeId
    );
    const next = Math.max(
      0,
      Math.min(cards.length - 1, current < 0 ? 0 : current + delta)
    );
    const card = cards[next];
    const changeId = card.dataset.changeId ?? null;
    setSelected(changeId);
    card.scrollIntoView({ block: "nearest", behavior: "auto" });
    return changeId;
  }
  __name(moveSelection, "moveSelection");
  function moveSelectionToBoundary(selectedChangeId, boundary) {
    const cards = Array.from(
      document.querySelectorAll(".unit-card[data-change-id]")
    );
    const card = boundary === "first" ? cards[0] : cards.at(-1);
    if (!card) return selectedChangeId;
    const changeId = card.dataset.changeId ?? null;
    setSelected(changeId);
    card.scrollIntoView({ block: "nearest", behavior: "auto" });
    return changeId;
  }
  __name(moveSelectionToBoundary, "moveSelectionToBoundary");
  function timelineRows() {
    return Array.from(
      document.querySelectorAll("#timeline [data-event-id]")
    );
  }
  __name(timelineRows, "timelineRows");
  function timelineWindow(eventIds, selectedEventId) {
    return {
      eventIds: [...eventIds],
      selectedEventId
    };
  }
  __name(timelineWindow, "timelineWindow");
  function timelineRowId(eventId) {
    return `timeline-event-${encodeURIComponent(eventId).replaceAll("%", "_")}`;
  }
  __name(timelineRowId, "timelineRowId");
  function setTimelineSelected(eventId) {
    const list = document.querySelector("#timeline");
    const rows = timelineRows();
    let activeId = null;
    for (const row of rows) {
      const rowEventId = row.dataset.eventId ?? null;
      const selected = rowEventId === eventId;
      row.tabIndex = -1;
      row.setAttribute("role", "option");
      row.setAttribute("aria-selected", String(selected));
      if (selected && rowEventId !== null) {
        row.id = timelineRowId(rowEventId);
        activeId = row.id;
      }
    }
    if (!list) return;
    list.setAttribute("role", "listbox");
    list.tabIndex = rows.length > 0 ? 0 : -1;
    list.toggleAttribute("aria-disabled", rows.length === 0);
    if (activeId) list.setAttribute("aria-activedescendant", activeId);
    else list.removeAttribute("aria-activedescendant");
  }
  __name(setTimelineSelected, "setTimelineSelected");
  function focusTimelineSelection(eventId) {
    const row = timelineRows().find((item) => item.dataset.eventId === eventId);
    row?.scrollIntoView?.({ block: "nearest", behavior: "auto" });
    document.querySelector("#timeline")?.focus({ preventScroll: true });
  }
  __name(focusTimelineSelection, "focusTimelineSelection");
  function visibleTimelineRows() {
    const list = document.querySelector("#timeline");
    const row = timelineRows()[0];
    const rowHeight = row?.getBoundingClientRect().height ?? 0;
    if (!list || rowHeight <= 0 || list.clientHeight <= 0) return 10;
    return Math.max(1, Math.floor(list.clientHeight / rowHeight));
  }
  __name(visibleTimelineRows, "visibleTimelineRows");
  function timelinePager(direction) {
    const explicit = document.querySelector(
      `#timeline [data-timeline-page="${direction}"], [data-timeline-page="${direction}"]`
    );
    if (explicit) return explicit;
    return Array.from(
      document.querySelectorAll("#master button")
    ).find(
      (button2) => button2.textContent?.trim() === `${direction === "next" ? "Next" : "Previous"} page`
    ) ?? null;
  }
  __name(timelinePager, "timelinePager");
  function trapModalFocus(modal, event) {
    if (event.key !== "Tab") return;
    const stops = Array.from(
      modal.querySelectorAll(
        "button:not([disabled]), a[href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex='-1'])"
      )
    );
    if (!stops.length) return;
    const active3 = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const first = stops[0];
    const last = stops.at(-1) ?? first;
    if (event.shiftKey && (active3 === first || !modal.contains(active3))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (active3 === last || !modal.contains(active3))) {
      event.preventDefault();
      first.focus();
    }
  }
  __name(trapModalFocus, "trapModalFocus");
  function installChangeInspectorInteraction(actions2) {
    let selectedChangeId = null;
    let selectedTimelineEventId = null;
    let currentTimelineEventIds = [];
    let currentTimelineEntries = /* @__PURE__ */ new Map();
    let pendingTimelineSelection = null;
    let pendingGlobalTimelineSelection = null;
    let modalReturnFocus = null;
    let detailReturnFocus = null;
    let detailWasOpen = false;
    let currentRoute2 = null;
    let exactOriginLens = null;
    let timelineOriginRoute = null;
    let detailDomIdentity = null;
    let pendingDiffEntryFocus = null;
    let pendingDiffExitFocus = null;
    let pendingExactActivationFocus = null;
    let diffReturnRoute = null;
    const parkTimelineForReaderActivity = /* @__PURE__ */ __name(() => {
      actions2.parkTimelineMonitoring?.();
    }, "parkTimelineForReaderActivity");
    const navigateToTimelineEvent = /* @__PURE__ */ __name((eventId, historyQuery) => {
      const origin = companionTimelineRoute(currentRoute2);
      if (currentRoute2?.kind === "timeline") {
        timelineOriginRoute = currentRoute2;
      } else if (timelineOriginRoute === null && origin !== null) {
        timelineOriginRoute = origin;
      }
      actions2.navigate(timelineEventRoute(eventId, historyQuery));
    }, "navigateToTimelineEvent");
    const selectTimelineEvent = /* @__PURE__ */ __name((eventId) => {
      selectedTimelineEventId = eventId;
      actions2.revealTimelineEvent?.(eventId);
      setTimelineSelected(eventId);
      focusTimelineSelection(eventId);
    }, "selectTimelineEvent");
    const applyTimelineIntent = /* @__PURE__ */ __name((intent) => {
      if (intent === null) return false;
      parkTimelineForReaderActivity();
      if (intent.kind === "select") {
        pendingTimelineSelection = null;
        selectTimelineEvent(intent.eventId);
        return true;
      }
      const pager = timelinePager(intent.direction);
      const targetRoute = pager?.dataset.timelineTargetRoute;
      if (!pager || !targetRoute) return false;
      pendingTimelineSelection = {
        intent,
        route: targetRoute,
        restoreFocus: document.activeElement?.id === "timeline"
      };
      pager.click();
      return true;
    }, "applyTimelineIntent");
    const syncTimelineDom = /* @__PURE__ */ __name((route = currentRoute2) => {
      const window2 = timelineWindow(
        currentTimelineEventIds,
        selectedTimelineEventId
      );
      const routedTimeline = companionTimelineRoute(route);
      const routedTimelineKey = routedTimeline === null ? null : formatChangeInspectorRoute(routedTimeline);
      const mountedTimeline = document.querySelector("#timeline");
      const mountedTimelineRoute = mountedTimeline?.dataset.timelineRoute ?? null;
      if (pendingTimelineSelection !== null && (route?.kind !== "timeline" || routedTimelineKey !== pendingTimelineSelection.route)) {
        pendingTimelineSelection = null;
      }
      if (pendingGlobalTimelineSelection !== null && (route?.kind !== "timeline" || routedTimelineKey !== pendingGlobalTimelineSelection.route)) {
        pendingGlobalTimelineSelection = null;
      }
      let restoreTimelineFocus = false;
      if (pendingGlobalTimelineSelection !== null && route?.kind === "timeline" && mountedTimelineRoute === formatChangeInspectorRoute(route) && formatChangeInspectorRoute(route) === pendingGlobalTimelineSelection.route && window2.eventIds.length > 0) {
        selectedTimelineEventId = pendingGlobalTimelineSelection.boundary === "first" ? window2.eventIds[0] ?? null : window2.eventIds.at(-1) ?? null;
        restoreTimelineFocus = pendingGlobalTimelineSelection.restoreFocus;
        pendingGlobalTimelineSelection = null;
        if (selectedTimelineEventId !== null) {
          actions2.revealTimelineEvent?.(selectedTimelineEventId);
        }
      }
      if (pendingTimelineSelection !== null && route?.kind === "timeline" && mountedTimelineRoute === pendingTimelineSelection.route && window2.eventIds.length > 0) {
        selectedTimelineEventId = resolveTimelinePageSelection(
          window2.eventIds,
          pendingTimelineSelection.intent
        );
        restoreTimelineFocus = pendingTimelineSelection.restoreFocus;
        pendingTimelineSelection = null;
        if (selectedTimelineEventId !== null) {
          actions2.revealTimelineEvent?.(selectedTimelineEventId);
        }
      }
      if (selectedTimelineEventId !== null && !window2.eventIds.includes(selectedTimelineEventId)) {
        setTimelineSelected(null);
        return;
      }
      setTimelineSelected(selectedTimelineEventId);
      if (restoreTimelineFocus && selectedTimelineEventId !== null) {
        focusTimelineSelection(selectedTimelineEventId);
      }
    }, "syncTimelineDom");
    const applyGlobalTimelineBoundary = /* @__PURE__ */ __name((boundary, route) => {
      const navigateBoundary = actions2.navigateTimelineBoundary;
      if (navigateBoundary === void 0) {
        return applyTimelineIntent(
          boundaryTimelineSelection(
            timelineWindow(currentTimelineEventIds, selectedTimelineEventId),
            boundary
          )
        );
      }
      parkTimelineForReaderActivity();
      pendingTimelineSelection = null;
      const restoreFocus = document.activeElement?.id === "timeline";
      void navigateBoundary(boundary, route).then((target) => {
        if (target === null) return;
        const targetRoute = formatChangeInspectorRoute(target);
        pendingGlobalTimelineSelection = {
          boundary,
          route: targetRoute,
          restoreFocus
        };
        const mountedRoute = document.querySelector("#timeline")?.dataset.timelineRoute ?? null;
        if (currentRoute2?.kind === "timeline" && formatChangeInspectorRoute(currentRoute2) === targetRoute && mountedRoute === targetRoute) {
          syncTimelineDom();
        }
      }).catch(() => {
        pendingGlobalTimelineSelection = null;
      });
      return true;
    }, "applyGlobalTimelineBoundary");
    applyPrefs();
    if (!colorSchemeWatcherInstalled) {
      watchColorScheme();
      colorSchemeWatcherInstalled = true;
    }
    const historyOriginRecord = /* @__PURE__ */ __name((route) => {
      if (route.kind === "lens" || route.kind === "timeline") return null;
      const state = history.state;
      if (state === null || typeof state !== "object" || Array.isArray(state))
        return null;
      const origin = state[HISTORY_ORIGIN_KEY];
      if (origin === null || typeof origin !== "object") return null;
      const record = origin;
      if (record.route !== formatChangeInspectorRoute(route)) return null;
      return record;
    }, "historyOriginRecord");
    const historyOrigin = /* @__PURE__ */ __name((route) => {
      const record = historyOriginRecord(route);
      if (record === null) return null;
      return record.lens === "timeline" || record.lens === "changes" || record.lens === "attention" ? record.lens : null;
    }, "historyOrigin");
    const persistHistoryOrigin = /* @__PURE__ */ __name((route, lens, returnRoute = null) => {
      if (route.kind === "lens" || route.kind === "timeline") return;
      const state = history.state;
      const retained = state !== null && typeof state === "object" && !Array.isArray(state) ? state : {};
      history.replaceState(
        {
          ...retained,
          [HISTORY_ORIGIN_KEY]: {
            route: formatChangeInspectorRoute(route),
            lens,
            ...returnRoute ? { returnRoute: formatChangeInspectorRoute(returnRoute) } : {}
          }
        },
        "",
        location.href
      );
    }, "persistHistoryOrigin");
    const persistedDiffReturnRoute = /* @__PURE__ */ __name((route) => {
      const encoded2 = historyOriginRecord(route)?.returnRoute;
      if (typeof encoded2 !== "string") return null;
      const parsed = parseChangeInspectorRoute(encoded2);
      if (parsed.kind === "event") return parsed;
      if (parsed.kind === "revision" && parsed.changeId === route.changeId && parsed.revision.revisionId === route.revision.revisionId && parsed.revision.objectArtifactContentHash === route.revision.objectArtifactContentHash) {
        return parsed;
      }
      return null;
    }, "persistedDiffReturnRoute");
    const routeReturningFromDiff = /* @__PURE__ */ __name((route) => diffReturnRoute ?? revisionRouteFromDiff(route), "routeReturningFromDiff");
    const onDiffClose = /* @__PURE__ */ __name(() => {
      if (currentRoute2?.kind === "diff") {
        actions2.navigate(routeReturningFromDiff(currentRoute2));
      }
    }, "onDiffClose");
    const listRoute = /* @__PURE__ */ __name((route) => {
      if (route.kind === "timeline" || route.kind === "lens") return route;
      const lens = historyOrigin(route) ?? exactOriginLens ?? "timeline";
      if (lens !== "timeline") return { kind: "lens", lens, query: route.query };
      return timelineOriginRoute ?? (route.kind === "event" ? { kind: "timeline", historyQuery: route.historyQuery } : { kind: "timeline", historyQuery: {} });
    }, "listRoute");
    const focusFallback = /* @__PURE__ */ __name((route = currentRoute2) => {
      const target = route?.kind === "diff" ? document.querySelector("#diff-page-close") : route !== null && route.kind !== "lens" && route.kind !== "timeline" ? window.matchMedia("(max-width: 760px)").matches ? document.querySelector("#detail-back") : document.querySelector("#detail-close") : document.querySelector("#master");
      target?.focus({ preventScroll: true });
    }, "focusFallback");
    const setCoveredPageInert = /* @__PURE__ */ __name((covered) => {
      for (const selector of [
        "#topbar",
        "#toolbar",
        "#master-rail",
        "#master",
        ".divider"
      ]) {
        const element = document.querySelector(selector);
        if (element) element.inert = covered;
      }
    }, "setCoveredPageInert");
    const onViewportResize = /* @__PURE__ */ __name(() => {
      const covered = detailWasOpen && window.matchMedia("(max-width: 760px)").matches;
      setCoveredPageInert(covered);
      const detail = document.querySelector("#detail");
      const routeSurface = currentRoute2?.kind === "diff" ? document.querySelector("#diff-page") : detail;
      const active3 = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      if (!covered) {
        if (detailWasOpen && active3?.id === "detail-back") {
          document.querySelector("#detail-close")?.focus({ preventScroll: true });
        }
        return;
      }
      if (currentRoute2 !== null && currentRoute2.kind !== "diff" && currentRoute2.kind !== "lens" && currentRoute2.kind !== "timeline" && active3?.id === "detail-close") {
        focusFallback();
        return;
      }
      if (routeSurface !== null && (active3 === null || !routeSurface.contains(active3)) && (active3 === null || active3.closest(".modal:not(.hidden)") === null)) {
        focusFallback();
      }
    }, "onViewportResize");
    window.addEventListener("resize", onViewportResize);
    const closeModal = /* @__PURE__ */ __name((id) => {
      const modal = document.querySelector(id);
      if (!modal || modal.classList.contains("hidden")) return;
      modal.classList.add("hidden");
      const focus = modalReturnFocus;
      modalReturnFocus = null;
      if (focus?.isConnected === true) focus.focus({ preventScroll: true });
      else focusFallback();
    }, "closeModal");
    const openModal = /* @__PURE__ */ __name((id, initial) => {
      const modal = document.querySelector(id);
      if (!modal) return;
      modalReturnFocus = document.activeElement instanceof HTMLElement && document.activeElement !== document.body ? document.activeElement : null;
      modal.classList.remove("hidden");
      initial?.focus();
    }, "openModal");
    const changeTheme = /* @__PURE__ */ __name((event) => {
      const input = event.target;
      if (!input.checked) return;
      setThemeMode(input.value);
    }, "changeTheme");
    const changeDensity = /* @__PURE__ */ __name((event) => {
      const input = event.target;
      if (!input.checked) return;
      setDensity(input.value);
      scheduleChangeInspectorTimelineRemeasure();
    }, "changeDensity");
    document.querySelectorAll("input[name='theme-mode']").forEach((input) => {
      input.addEventListener("change", changeTheme);
    });
    document.querySelectorAll("input[name='density-mode']").forEach((input) => {
      input.addEventListener("change", changeDensity);
    });
    const paletteInput = document.querySelector("#cmd-input");
    const paletteResults = document.querySelector("#cmd-results");
    const paletteCommands = [
      ["Open Timeline", "timeline"],
      ["Open Changes", "changes"],
      ["Open Attention", "attention"]
    ];
    const renderPaletteResults = /* @__PURE__ */ __name(() => {
      if (paletteResults) {
        paletteResults.replaceChildren();
        const query = paletteInput?.value.trim().toLocaleLowerCase() ?? "";
        const matching = paletteCommands.filter(
          ([label2]) => label2.toLocaleLowerCase().includes(query)
        );
        for (const [label2, lens] of matching) {
          const button2 = document.createElement("button");
          button2.type = "button";
          button2.className = "ghost cmd-item";
          const commandLabel = document.createElement("span");
          commandLabel.className = "cmd-label";
          commandLabel.textContent = label2;
          button2.append(commandLabel);
          button2.addEventListener("click", () => {
            closeModal("#cmd-palette");
            const route = currentRoute2;
            if (route) {
              actions2.navigate(
                lens === "timeline" ? { kind: "timeline", historyQuery: {} } : {
                  kind: "lens",
                  lens,
                  query: route.kind === "timeline" ? {} : { ...route.query, after: void 0 }
                }
              );
            }
          });
          paletteResults.append(button2);
        }
        if (matching.length === 0) {
          const empty = document.createElement("p");
          empty.className = "cmd-empty";
          empty.setAttribute("role", "status");
          empty.textContent = "No matching commands.";
          paletteResults.append(empty);
        }
      }
    }, "renderPaletteResults");
    const openPalette = /* @__PURE__ */ __name(() => {
      if (paletteInput) paletteInput.value = "";
      renderPaletteResults();
      openModal("#cmd-palette", paletteInput);
    }, "openPalette");
    paletteInput?.addEventListener("input", renderPaletteResults);
    const helpClose = /* @__PURE__ */ __name(() => closeModal("#key-help"), "helpClose");
    const helpCloseButton = document.querySelector("#key-help-close");
    helpCloseButton?.addEventListener("click", helpClose);
    const readingButton = document.querySelector("#detail-read");
    const masterRail = document.querySelector("#master-rail");
    const setReading = /* @__PURE__ */ __name((enabled) => {
      const split = document.querySelector(".split");
      const detailViewport = document.querySelector("#detail-body");
      const scrollTop = detailViewport?.scrollTop ?? 0;
      split?.classList.toggle("reading", enabled);
      scheduleChangeInspectorTimelineRemeasure();
      if (readingButton) {
        readingButton.textContent = enabled ? "⤡" : "⤢";
        readingButton.setAttribute("aria-pressed", String(enabled));
        readingButton.setAttribute(
          "aria-label",
          enabled ? "Exit reading mode" : "Enter reading mode"
        );
        readingButton.title = enabled ? "Exit reading mode" : "Reading mode";
      }
      if (detailViewport) detailViewport.scrollTop = scrollTop;
    }, "setReading");
    const toggleReading = /* @__PURE__ */ __name(() => {
      setReading(
        !document.querySelector(".split")?.classList.contains("reading")
      );
    }, "toggleReading");
    readingButton?.addEventListener("click", toggleReading);
    const restoreMaster = /* @__PURE__ */ __name(() => {
      setReading(false);
      document.querySelector("#master")?.focus({ preventScroll: true });
    }, "restoreMaster");
    masterRail?.addEventListener("click", restoreMaster);
    const divider = document.querySelector(".divider");
    const updateSplit = /* @__PURE__ */ __name((value) => {
      applySplit(value);
      scheduleChangeInspectorTimelineRemeasure();
      divider?.setAttribute("aria-valuenow", String(preferredSplit() ?? 50));
    }, "updateSplit");
    updateSplit(preferredSplit());
    const onDividerKey = /* @__PURE__ */ __name((event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        event.stopPropagation();
        updateSplit(null);
        return;
      }
      if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
      event.preventDefault();
      event.stopPropagation();
      const value = (preferredSplit() ?? 50) + (event.key === "ArrowLeft" ? -5 : 5);
      updateSplit(value);
    }, "onDividerKey");
    divider?.addEventListener("keydown", onDividerKey);
    let activeDividerPointerId = null;
    const finishDividerDrag = /* @__PURE__ */ __name((event) => {
      if (!divider || activeDividerPointerId !== event.pointerId) return;
      activeDividerPointerId = null;
      divider.classList.remove("dragging");
      if (divider.hasPointerCapture?.(event.pointerId)) {
        divider.releasePointerCapture?.(event.pointerId);
      }
    }, "finishDividerDrag");
    const onDividerLostPointerCapture = /* @__PURE__ */ __name((event) => {
      if (!divider || activeDividerPointerId !== event.pointerId) return;
      activeDividerPointerId = null;
      divider.classList.remove("dragging");
    }, "onDividerLostPointerCapture");
    const onDividerPointerDown = /* @__PURE__ */ __name((event) => {
      if (!divider || activeDividerPointerId !== null || event.pointerType === "mouse" && event.button !== 0)
        return;
      event.preventDefault();
      divider.focus();
      activeDividerPointerId = event.pointerId;
      divider.setPointerCapture?.(event.pointerId);
      divider.classList.add("dragging");
    }, "onDividerPointerDown");
    const onDividerPointerMove = /* @__PURE__ */ __name((event) => {
      if (!divider?.classList.contains("dragging") || activeDividerPointerId !== event.pointerId)
        return;
      const split = document.querySelector(".split");
      const bounds = split?.getBoundingClientRect();
      if (!bounds || bounds.width <= 0) return;
      const value = (event.clientX - bounds.left) / bounds.width * 100;
      if (value < 15) {
        finishDividerDrag(event);
        setReading(true);
        return;
      }
      updateSplit(value);
    }, "onDividerPointerMove");
    const onDividerDoubleClick = /* @__PURE__ */ __name((event) => {
      event.preventDefault();
      updateSplit(null);
    }, "onDividerDoubleClick");
    divider?.addEventListener("pointerdown", onDividerPointerDown);
    divider?.addEventListener("pointermove", onDividerPointerMove);
    divider?.addEventListener("pointerup", finishDividerDrag);
    divider?.addEventListener("pointercancel", finishDividerDrag);
    divider?.addEventListener("lostpointercapture", onDividerLostPointerCapture);
    divider?.addEventListener("dblclick", onDividerDoubleClick);
    const onClick = /* @__PURE__ */ __name((event) => {
      const target = event.target instanceof Element ? event.target : null;
      const card = target?.closest(".unit-card[data-change-id]");
      if (card && !target?.closest("button, a, input, select, textarea")) {
        selectedChangeId = card.dataset.changeId ?? null;
        setSelected(selectedChangeId);
        return;
      }
      const timelineEvent = target?.closest(
        "#timeline [data-event-id]"
      );
      if (timelineEvent?.dataset.eventId && !target?.closest("button, a[href], input, select, textarea")) {
        parkTimelineForReaderActivity();
        const eventId = timelineEvent.dataset.eventId;
        selectTimelineEvent(eventId);
        const historyQuery = currentRoute2?.kind === "timeline" || currentRoute2?.kind === "event" ? currentRoute2.historyQuery : {};
        navigateToTimelineEvent(eventId, historyQuery);
      }
    }, "onClick");
    document.addEventListener("click", onClick);
    const onFocusIn = /* @__PURE__ */ __name((event) => {
      const target = event.target instanceof Element ? event.target : null;
      const timelineEvent = target?.closest(
        "#timeline [data-event-id]"
      );
      if (!timelineEvent?.dataset.eventId) return;
      selectedTimelineEventId = timelineEvent.dataset.eventId;
      setTimelineSelected(selectedTimelineEventId);
    }, "onFocusIn");
    document.addEventListener("focusin", onFocusIn);
    const onTimelineScroll = /* @__PURE__ */ __name((event) => {
      const target = event.target instanceof Element ? event.target : null;
      if (target?.closest("#timeline")) parkTimelineForReaderActivity();
    }, "onTimelineScroll");
    document.addEventListener("scroll", onTimelineScroll, true);
    const timelineDomObserver = new MutationObserver(() => {
      if (companionTimelineRoute(currentRoute2) !== null) syncTimelineDom();
    });
    const master = document.querySelector("#master");
    if (master) {
      timelineDomObserver.observe(master, {
        childList: true,
        subtree: true
      });
    }
    const onKey = /* @__PURE__ */ __name((event) => {
      const open = document.querySelector(
        "#cmd-palette:not(.hidden), #key-help:not(.hidden)"
      );
      if (open) {
        if (event.key === "Escape") {
          event.preventDefault();
          closeModal(`#${open.id}`);
        } else {
          trapModalFocus(open, event);
        }
        return;
      }
      if (document.querySelector("#reconnect-dialog:not(.hidden)")) return;
      const route = currentRoute2;
      if (route?.kind === "diff" && event.key === "Escape") {
        event.preventDefault();
        actions2.navigate(routeReturningFromDiff(route));
        return;
      }
      if (isTextControl(event.target)) return;
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k" || event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "p") {
        event.preventDefault();
        openPalette();
        return;
      }
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      if (!route) return;
      if (route.kind === "diff") {
        if (isNativeActionControl(event.target)) return;
        if (event.key === "]" || event.key === "[") {
          const next = moveDiffTarget(
            Array.from(
              document.querySelectorAll("#diff-page-body .dfile")
            ),
            route.focus?.filePath,
            event.key === "]" ? 1 : -1,
            (item) => item.dataset.filePath
          );
          const filePath = next?.dataset.filePath;
          if (filePath) {
            event.preventDefault();
            actions2.navigate(refineDiffFocus(route, { filePath }));
          }
          return;
        }
        if (event.key === "n" || event.key === "p") {
          const seen = /* @__PURE__ */ new Set();
          const facts = Array.from(
            document.querySelectorAll("#diff-page-body [data-anno]")
          ).filter((item) => {
            const id = item.dataset.anno;
            if (!id || seen.has(id)) return false;
            seen.add(id);
            return true;
          });
          const next = moveDiffTarget(
            facts,
            route.focus?.factId,
            event.key === "n" ? 1 : -1,
            (item) => item.dataset.anno
          );
          const factId = next?.dataset.anno;
          if (factId) {
            event.preventDefault();
            actions2.navigate(refineDiffFocus(route, { factId }));
          }
          return;
        }
        return;
      }
      if (event.key === "?") {
        event.preventDefault();
        openModal("#key-help", document.querySelector("#key-help-close"));
        return;
      }
      if (MASTER_SURFACE_KEYS.has(event.key) && narrowDetailOwnsFocus(event.target)) {
        return;
      }
      if (event.key === "/") {
        event.preventDefault();
        document.querySelector("#filter-text")?.focus();
        return;
      }
      const timelineRoute = companionTimelineRoute(route);
      if (timelineRoute !== null) {
        const timeline = timelineWindow(
          currentTimelineEventIds,
          selectedTimelineEventId
        );
        if ((event.key === "ArrowDown" || event.key === "ArrowUp") && !isTimelineListTarget(event.target)) {
          return;
        }
        const applyScopedTimelineIntent = /* @__PURE__ */ __name((intent) => {
          if (route.kind === "event" && intent?.kind === "adjacent-page") {
            return false;
          }
          return applyTimelineIntent(intent);
        }, "applyScopedTimelineIntent");
        if (event.key === "j" || event.key === "ArrowDown") {
          if (applyScopedTimelineIntent(moveTimelineSelection(timeline, 1))) {
            event.preventDefault();
          }
          return;
        }
        if (event.key === "k" || event.key === "ArrowUp") {
          if (applyScopedTimelineIntent(moveTimelineSelection(timeline, -1))) {
            event.preventDefault();
          }
          return;
        }
        if (event.key === "g") {
          if (route.kind === "timeline" && applyGlobalTimelineBoundary("first", timelineRoute)) {
            event.preventDefault();
          }
          return;
        }
        if (event.key === "G") {
          if (route.kind === "timeline" && applyGlobalTimelineBoundary("last", timelineRoute)) {
            event.preventDefault();
          }
          return;
        }
        if (event.key === "f") {
          if (applyScopedTimelineIntent(
            pageTimelineSelection(
              timeline,
              "forward",
              visibleTimelineRows(),
              "full"
            )
          )) {
            event.preventDefault();
          }
          return;
        }
        if (event.key === "b") {
          if (applyScopedTimelineIntent(
            pageTimelineSelection(
              timeline,
              "backward",
              visibleTimelineRows(),
              "full"
            )
          )) {
            event.preventDefault();
          }
          return;
        }
        if (event.key === "d" || event.key === "u") {
          if (applyScopedTimelineIntent(
            pageTimelineSelection(
              timeline,
              event.key === "d" ? "forward" : "backward",
              visibleTimelineRows(),
              "half"
            )
          )) {
            event.preventDefault();
          }
          return;
        }
        if (event.key === "F") {
          if (route.kind === "timeline") {
            event.preventDefault();
            actions2.toggleTimelineMonitoring?.();
          }
          return;
        }
        if (event.key === "Enter" && selectedTimelineEventId !== null && !isNativeActionControl(event.target)) {
          event.preventDefault();
          if (route.kind === "event" && route.eventId === selectedTimelineEventId) {
            const entry = currentTimelineEntries.get(selectedTimelineEventId);
            const diff = entry ? eventAnnotatedDiffRoute(entry) : null;
            if (diff === null) {
              document.querySelector("[data-event-diff-refusal]")?.focus({ preventScroll: true });
              return;
            }
            diffReturnRoute = route;
            actions2.navigate(diff);
            return;
          }
          navigateToTimelineEvent(
            selectedTimelineEventId,
            timelineRoute.historyQuery
          );
          return;
        }
      }
      if (event.key === "Enter" && route.kind === "revision" && !isNativeActionControl(event.target)) {
        event.preventDefault();
        diffReturnRoute = route;
        actions2.navigate({
          kind: "diff",
          changeId: route.changeId,
          revision: route.revision,
          query: route.query,
          ...route.focus ? { focus: route.focus } : {}
        });
        return;
      }
      if (event.key === "j" || event.key === "ArrowDown") {
        event.preventDefault();
        selectedChangeId = moveSelection(selectedChangeId, 1);
        return;
      }
      if (event.key === "k" || event.key === "ArrowUp") {
        event.preventDefault();
        selectedChangeId = moveSelection(selectedChangeId, -1);
        return;
      }
      if (event.key === "g") {
        event.preventDefault();
        selectedChangeId = moveSelectionToBoundary(selectedChangeId, "first");
        return;
      }
      if (event.key === "G") {
        event.preventDefault();
        selectedChangeId = moveSelectionToBoundary(selectedChangeId, "last");
        return;
      }
      if (event.key === "1") {
        event.preventDefault();
        actions2.navigate({ kind: "timeline", historyQuery: {} });
        return;
      }
      if (event.key === "2") {
        event.preventDefault();
        actions2.navigate({
          kind: "lens",
          lens: "changes",
          query: route.kind === "timeline" ? {} : { ...route.query, after: void 0 }
        });
        return;
      }
      if (event.key === "3") {
        event.preventDefault();
        actions2.navigate({
          kind: "lens",
          lens: "attention",
          query: route.kind === "timeline" ? {} : { ...route.query, after: void 0 }
        });
        return;
      }
      if (event.key === "Enter" && selectedChangeId && !isNativeActionControl(event.target)) {
        event.preventDefault();
        actions2.navigate({
          kind: "change",
          changeId: selectedChangeId,
          query: queryForExactNavigation(route)
        });
        return;
      }
      if (event.key === "h") {
        event.preventDefault();
        updateSplit((preferredSplit() ?? 50) - 5);
        return;
      }
      if (event.key === "l") {
        event.preventDefault();
        updateSplit((preferredSplit() ?? 50) + 5);
        return;
      }
      if (event.key === "Escape" && route.kind !== "lens" && route.kind !== "timeline") {
        event.preventDefault();
        actions2.navigate(listRoute(route));
      }
    }, "onKey");
    document.addEventListener("keydown", onKey);
    const onClose = /* @__PURE__ */ __name(() => {
      const route = currentRoute2;
      if (route) actions2.navigate(listRoute(route));
    }, "onClose");
    const closeButton = document.querySelector("#detail-close");
    const backButton = document.querySelector("#detail-back");
    if (closeButton) closeButton.onclick = onClose;
    if (backButton) backButton.onclick = onClose;
    const stop = /* @__PURE__ */ __name(() => {
      document.removeEventListener("click", onClick);
      document.removeEventListener("focusin", onFocusIn);
      document.removeEventListener("scroll", onTimelineScroll, true);
      document.removeEventListener("keydown", onKey);
      timelineDomObserver.disconnect();
      document.querySelectorAll("input[name='theme-mode']").forEach((input) => {
        input.removeEventListener("change", changeTheme);
      });
      document.querySelectorAll("input[name='density-mode']").forEach((input) => {
        input.removeEventListener("change", changeDensity);
      });
      helpCloseButton?.removeEventListener("click", helpClose);
      readingButton?.removeEventListener("click", toggleReading);
      masterRail?.removeEventListener("click", restoreMaster);
      divider?.removeEventListener("keydown", onDividerKey);
      divider?.removeEventListener("pointerdown", onDividerPointerDown);
      divider?.removeEventListener("pointermove", onDividerPointerMove);
      divider?.removeEventListener("pointerup", finishDividerDrag);
      divider?.removeEventListener("pointercancel", finishDividerDrag);
      divider?.removeEventListener(
        "lostpointercapture",
        onDividerLostPointerCapture
      );
      divider?.removeEventListener("dblclick", onDividerDoubleClick);
      window.removeEventListener("resize", onViewportResize);
      if (divider && activeDividerPointerId !== null && divider.hasPointerCapture?.(activeDividerPointerId)) {
        divider.releasePointerCapture?.(activeDividerPointerId);
      }
      activeDividerPointerId = null;
      divider?.classList.remove("dragging");
      paletteInput?.removeEventListener("input", renderPaletteResults);
      if (closeButton?.onclick === onClose) closeButton.onclick = null;
      if (backButton?.onclick === onClose) backButton.onclick = null;
      document.querySelector("#cmd-palette")?.classList.add("hidden");
      document.querySelector("#key-help")?.classList.add("hidden");
      paletteResults?.replaceChildren();
      selectedChangeId = null;
      selectedTimelineEventId = null;
      currentTimelineEventIds = [];
      currentTimelineEntries = /* @__PURE__ */ new Map();
      pendingTimelineSelection = null;
      pendingGlobalTimelineSelection = null;
      setSelected(null);
      setTimelineSelected(null);
      modalReturnFocus = null;
      detailReturnFocus = null;
      detailWasOpen = false;
      currentRoute2 = null;
      exactOriginLens = null;
      timelineOriginRoute = null;
      detailDomIdentity = null;
      pendingDiffEntryFocus = null;
      pendingDiffExitFocus = null;
      pendingExactActivationFocus = null;
      diffReturnRoute = null;
      const diffClose = document.querySelector("#diff-page-close");
      if (diffClose?.onclick === onDiffClose) diffClose.onclick = null;
      setCoveredPageInert(false);
    }, "stop");
    return {
      sync(snapshot2, timelinePage = snapshot2.generation?.history ?? null) {
        const nextRoute = snapshot2.route.kind === "invalid" ? null : snapshot2.route;
        const previousTimelineEntries = currentTimelineEntries;
        const timelineEntries = companionTimelineRoute(nextRoute) !== null && timelinePage !== null ? timelinePage.entries : [];
        currentTimelineEventIds = timelineEntries.map((entry) => entry.eventId);
        currentTimelineEntries = new Map(
          timelineEntries.map((entry) => [entry.eventId, entry])
        );
        if (nextRoute?.kind === "event" && (currentRoute2?.kind !== "event" || formatChangeInspectorRoute(currentRoute2) !== formatChangeInspectorRoute(nextRoute))) {
          selectedTimelineEventId = nextRoute.eventId;
        }
        if (nextRoute !== null && nextRoute.kind !== "lens" && nextRoute.kind !== "timeline") {
          if (nextRoute.kind === "diff") {
            const persisted = persistedDiffReturnRoute(nextRoute);
            if (persisted !== null) {
              diffReturnRoute = persisted;
            } else if (currentRoute2?.kind === "event") {
              const entry = previousTimelineEntries.get(currentRoute2.eventId);
              const eventDiff = entry ? eventAnnotatedDiffRoute(entry) : null;
              diffReturnRoute = eventDiff !== null && sameDiffIdentity(diffIdentity(eventDiff), diffIdentity(nextRoute)) ? currentRoute2 : null;
            } else if (currentRoute2?.kind === "revision" && currentRoute2.changeId === nextRoute.changeId && currentRoute2.revision.revisionId === nextRoute.revision.revisionId && currentRoute2.revision.objectArtifactContentHash === nextRoute.revision.objectArtifactContentHash) {
              diffReturnRoute = currentRoute2;
            } else if (currentRoute2?.kind !== "diff" || !sameDiffIdentity(
              diffIdentity(currentRoute2),
              diffIdentity(nextRoute)
            )) {
              diffReturnRoute = null;
            }
          }
          const persistedOrigin = historyOrigin(nextRoute);
          const origin = persistedOrigin ?? (currentRoute2?.kind === "lens" ? currentRoute2.lens : currentRoute2?.kind === "timeline" ? "timeline" : exactOriginLens ?? "timeline");
          if (currentRoute2?.kind === "timeline") {
            timelineOriginRoute = currentRoute2;
          }
          exactOriginLens = origin;
          if (persistedOrigin === null) {
            persistHistoryOrigin(
              nextRoute,
              origin,
              nextRoute.kind === "diff" ? diffReturnRoute : null
            );
          }
        } else {
          exactOriginLens = null;
        }
        const cards = Array.from(
          document.querySelectorAll(".unit-card[data-change-id]")
        );
        if (!cards.some((card) => card.dataset.changeId === selectedChangeId))
          selectedChangeId = null;
        setSelected(selectedChangeId);
        if (companionTimelineRoute(nextRoute) !== null) {
          syncTimelineDom(nextRoute);
        } else {
          selectedTimelineEventId = null;
          pendingTimelineSelection = null;
          pendingGlobalTimelineSelection = null;
        }
        const detailOpen = snapshot2.route.kind !== "lens" && snapshot2.route.kind !== "timeline" && snapshot2.route.kind !== "invalid";
        const detail = document.querySelector("#detail");
        const viewportIsNarrow = window.matchMedia("(max-width: 760px)").matches;
        const coveredPage = detailOpen && viewportIsNarrow;
        if (!coveredPage) setCoveredPageInert(false);
        const nextDetailDomIdentity = document.querySelector("#detail-body")?.firstChild ?? null;
        const routeSurface = nextRoute?.kind === "diff" ? document.querySelector("#diff-page") : detail;
        const detailDomChanged = detailDomIdentity !== nextDetailDomIdentity;
        const active3 = document.activeElement instanceof HTMLElement ? document.activeElement : null;
        const detailRouteChanged = currentRoute2 !== null && currentRoute2.kind !== "lens" && currentRoute2.kind !== "timeline" && nextRoute !== null && nextRoute.kind !== "lens" && nextRoute.kind !== "timeline" && formatChangeInspectorRoute(currentRoute2) !== formatChangeInspectorRoute(nextRoute);
        const nextDiffIdentity = nextRoute?.kind === "diff" ? diffIdentity(nextRoute) : null;
        const currentDiffIdentity = currentRoute2?.kind === "diff" ? diffIdentity(currentRoute2) : null;
        const nextExactActivationIdentity = exactActivationIdentity(nextRoute);
        const currentExactActivationIdentity = exactActivationIdentity(currentRoute2);
        if (nextExactActivationIdentity === null) {
          pendingExactActivationFocus = null;
        } else if (currentRoute2?.kind !== "diff" && nextExactActivationIdentity !== currentExactActivationIdentity) {
          pendingExactActivationFocus = nextExactActivationIdentity;
        }
        const exactActivationTarget = pendingExactActivationFocus === nextExactActivationIdentity ? document.querySelector(
          "#detail-body [data-exact-diff-activation], #detail-body [data-event-diff-refusal]"
        ) : null;
        if (nextDiffIdentity === null) {
          pendingDiffEntryFocus = null;
        } else if (!sameDiffIdentity(currentDiffIdentity, nextDiffIdentity)) {
          pendingDiffEntryFocus = nextDiffIdentity;
        }
        const diffClose = document.querySelector("#diff-page-close");
        if (nextRoute?.kind === "diff" && diffClose) {
          diffClose.onclick = onDiffClose;
        }
        const entersVisibleDiff = nextRoute?.kind === "diff" && sameDiffIdentity(pendingDiffEntryFocus, nextDiffIdentity) && document.querySelector("#diff-page:not(.hidden)") !== null;
        const leavesDiffForExactSurface = currentRoute2?.kind === "diff" && nextRoute !== null && nextRoute.kind !== "diff" && nextRoute.kind !== "lens" && nextRoute.kind !== "timeline";
        const leavesDiffForRevision = leavesDiffForExactSurface && nextRoute?.kind === "revision";
        const nextRevisionRoute = nextRoute?.kind === "revision" ? formatChangeInspectorRoute(nextRoute) : null;
        if (leavesDiffForRevision) {
          pendingDiffExitFocus = nextRevisionRoute;
        } else if (pendingDiffExitFocus !== nextRevisionRoute) {
          pendingDiffExitFocus = null;
        }
        const completesDiffExitFocus = pendingDiffExitFocus !== null && pendingDiffExitFocus === nextRevisionRoute && document.querySelector("#detail-body")?.dataset.changeReadingKey?.startsWith(`${pendingDiffExitFocus}:`) === true;
        document.querySelector(".split")?.classList.toggle("split-closed", !detailOpen);
        if (detail) {
          detail.inert = !detailOpen;
          if (detailOpen) detail.removeAttribute("aria-hidden");
          else detail.setAttribute("aria-hidden", "true");
        }
        if (exactActivationTarget !== null) {
          pendingExactActivationFocus = null;
          exactActivationTarget.focus({ preventScroll: true });
        } else if (entersVisibleDiff) {
          pendingDiffEntryFocus = null;
          focusFallback(nextRoute);
        } else if (leavesDiffForExactSurface) {
          if (completesDiffExitFocus) pendingDiffExitFocus = null;
          focusFallback(nextRoute);
        } else if (completesDiffExitFocus) {
          pendingDiffExitFocus = null;
          focusFallback(nextRoute);
        } else if (detailOpen && !detailWasOpen) {
          detailReturnFocus = active3 && active3 !== document.body ? active3 : null;
          if (viewportIsNarrow && nextRoute?.kind !== "diff") {
            document.querySelector("#detail-back")?.focus({ preventScroll: true });
          }
        } else if (detailOpen && detailWasOpen && detailRouteChanged && nextRoute?.kind !== "diff" && viewportIsNarrow && detail !== null && (active3 === null || !detail.contains(active3)) && (active3 === null || active3.closest(".modal:not(.hidden)") === null)) {
          focusFallback(nextRoute);
        } else if (detailOpen && detailWasOpen && detailDomChanged && (!(document.activeElement instanceof HTMLElement) || document.activeElement === document.body || !document.activeElement.isConnected)) {
          focusFallback(nextRoute);
        } else if (!detailOpen && detailWasOpen) {
          setReading(false);
          const candidate = detailReturnFocus?.isConnected === true ? detailReturnFocus : document.querySelector("#master");
          detailReturnFocus = null;
          candidate?.focus({ preventScroll: true });
        }
        if (coveredPage) {
          const coveredActive = document.activeElement instanceof HTMLElement ? document.activeElement : null;
          if (routeSurface !== null && (coveredActive === null || !routeSurface.contains(coveredActive)) && (coveredActive === null || coveredActive.closest(".modal:not(.hidden)") === null)) {
            focusFallback(nextRoute);
          }
          setCoveredPageInert(true);
        }
        detailWasOpen = detailOpen;
        currentRoute2 = nextRoute;
        detailDomIdentity = nextDetailDomIdentity;
      },
      stop
    };
  }
  __name(installChangeInspectorInteraction, "installChangeInspectorInteraction");

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
  var EVENT_HISTORY_EVENT_TYPES = [
    "review_initialized",
    "work_object_proposed",
    "review_observation_recorded",
    "review_assessment_recorded",
    "input_request_opened",
    "input_request_responded",
    "review_note_imported",
    "revision_ref_associated",
    "revision_ref_withdrawn",
    "revision_commit_associated",
    "revision_commit_withdrawn",
    "validation_check_recorded",
    "change_declared",
    "change_membership_asserted",
    "change_membership_withdrawn",
    "change_link_asserted",
    "change_revision_relation_asserted",
    "change_revision_relation_withdrawn",
    "revision_relation_attested",
    "review_fact_ported"
  ];
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
  var CONTENT_AVAILABILITY_VALUES = /* @__PURE__ */ new Set([
    "available",
    "removed",
    "missing",
    "mismatch",
    "non_textual"
  ]);
  var REVISION_CURRENCY_VALUES = /* @__PURE__ */ new Set([
    "current",
    "stale_by_supersession",
    "membership_incomplete",
    "membership_conflicted"
  ]);
  var FACT_FAMILY_STATE_VALUES = /* @__PURE__ */ new Set([
    "current",
    "stale",
    "withdrawn",
    "conflicted",
    "unavailable"
  ]);
  var ASSOCIATION_STATE_VALUES = /* @__PURE__ */ new Set([
    "unknown",
    "exact",
    "equivalent",
    "extension",
    "unavailable"
  ]);
  var ASSOCIATION_PROOF_VALUES = /* @__PURE__ */ new Set([
    "available",
    "missing",
    "mismatch",
    "not_requested"
  ]);
  var INTERDIFF_AVAILABILITY_VALUES = /* @__PURE__ */ new Set([
    "available",
    "unavailable",
    "endpoint_missing",
    "endpoint_mismatch",
    "non_textual"
  ]);
  function decodeChangeDetail(value) {
    const detail = object(value, "Change detail");
    const summary = detail.summary;
    const stamp = detail.projectionStamp;
    const memberRevisions = detail.memberRevisions;
    const unavailableMemberRevisions = detail.unavailableMemberRevisions;
    const membershipClaims = detail.membershipClaims;
    const membershipWithdrawals = detail.membershipWithdrawals;
    const relationClaims = detail.relationClaims;
    const relationWithdrawals = detail.relationWithdrawals;
    const links = detail.links;
    const effectiveSupersedes = detail.effectiveSupersedes;
    const pendingOrConflictingEdges = detail.pendingOrConflictingEdges;
    const currentRevisionRefs = detail.currentRevisionRefs;
    const perCurrentRevisionQualification = detail.perCurrentRevisionQualification;
    const operativeObligations = detail.operativeObligations;
    const diagnostics = detail.diagnostics;
    const inspectorPresentation = detail.inspectorPresentation;
    if (detail.schema !== "pointbreak.review-change" || detail.version !== 1 || !nonEmptyString(stamp) || !isChangeSummary(summary, stamp) || !isChangeMemberRevisions(memberRevisions) || !isUnavailableChangeMemberRevisions(unavailableMemberRevisions) || !isMembershipClaims(membershipClaims, summary.changeId) || !isClaimWithdrawals(membershipWithdrawals) || !Array.isArray(relationClaims) || !relationClaims.every(
      (claim) => isRelationClaim(claim, summary.changeId)
    ) || !isClaimWithdrawals(relationWithdrawals) || !isChangeLinks(links) || !isEffectiveSupersedes(effectiveSupersedes) || !Array.isArray(pendingOrConflictingEdges) || !pendingOrConflictingEdges.every(
      (claim) => isRelationClaim(claim, summary.changeId)
    ) || !Array.isArray(currentRevisionRefs) || !currentRevisionRefs.every(isRevisionRef) || !sameRevisionSet(currentRevisionRefs, summary.currentRevisionRefs) || !isRevisionQualifications(
      perCurrentRevisionQualification,
      currentRevisionRefs
    ) || !isStringArray(operativeObligations) || !isStringArray(diagnostics)) {
      throw new Error("invalid Change detail DTO");
    }
    if (!isChangeDetailInspectorPresentation(inspectorPresentation, {
      memberRevisions,
      currentRevisionRefs,
      effectiveSupersedes,
      pendingOrConflictingEdges,
      diagnostics
    })) {
      throw new Error("invalid Change detail DTO");
    }
    return {
      schema: "pointbreak.review-change",
      version: 1,
      summary,
      memberRevisions,
      unavailableMemberRevisions,
      membershipClaims,
      membershipWithdrawals,
      relationClaims,
      relationWithdrawals,
      links,
      effectiveSupersedes,
      pendingOrConflictingEdges,
      currentRevisionRefs,
      perCurrentRevisionQualification,
      operativeObligations,
      diagnostics,
      projectionStamp: stamp,
      inspectorPresentation
    };
  }
  __name(decodeChangeDetail, "decodeChangeDetail");
  function decodeChangeRevisionDetail(value) {
    const detail = object(value, "Change Revision detail");
    const revision2 = detail.revision;
    const factPresentations = detail.factPresentations;
    const factContentPresentations = detail.factContentPresentations;
    const exactRevisionDocument = detail.exactRevisionDocument;
    const membershipSupport = detail.membershipSupport;
    const factPorts = detail.factPorts;
    const associations = detail.associations;
    const diagnostics = detail.diagnostics;
    const revisionCurrency = detail.revisionCurrency;
    const relationClassification = detail.relationClassification;
    const availability = detail.availability;
    const inspectorPresentation = detail.inspectorPresentation;
    if (detail.schema !== "pointbreak.review-change-revision" || detail.version !== 1 || !nonEmptyString(detail.changeId) || !isRevisionRef(revision2) || typeof revisionCurrency !== "string" || !REVISION_CURRENCY_VALUES.has(revisionCurrency) || relationClassification !== "current" && relationClassification !== "superseded" || typeof availability !== "string" || !CONTENT_AVAILABILITY_VALUES.has(availability) || !isRevisionResource(exactRevisionDocument) || !sameRevision(exactRevisionDocument.resource.revision, revision2) || availability !== exactRevisionDocument.availability || !isMembershipClaims(membershipSupport, detail.changeId) || !Array.isArray(factPresentations) || !factPresentations.every(isFactPresentation) || !uniqueFactPresentationIds(factPresentations) || factContentPresentations !== void 0 && !isFactContentPresentations(factContentPresentations) || factContentPresentations !== void 0 && !sameFactIds(factPresentations, factContentPresentations) || !isFactPortPresentations(
      factPorts,
      detail.changeId,
      factPresentations,
      revision2
    ) || !Array.isArray(associations) || !associations.every(isAssociation) || !isStringArray(diagnostics) || !nonEmptyString(detail.projectionStamp)) {
      throw new Error("invalid Change Revision detail DTO");
    }
    if (!isChangeRevisionDetailInspectorPresentation(inspectorPresentation, {
      revision: revision2,
      factPresentations,
      factPorts
    })) {
      throw new Error("invalid Change Revision detail DTO");
    }
    return {
      schema: "pointbreak.review-change-revision",
      version: 1,
      changeId: detail.changeId,
      revision: revision2,
      membershipSupport,
      revisionCurrency,
      relationClassification,
      availability,
      exactRevisionDocument,
      factPresentations,
      factContentPresentations,
      factPorts,
      associations,
      diagnostics,
      projectionStamp: detail.projectionStamp,
      inspectorPresentation
    };
  }
  __name(decodeChangeRevisionDetail, "decodeChangeRevisionDetail");
  function decodeRevisionResource(value) {
    const document2 = object(value, "Revision resource");
    const resource = document2.resource;
    const projection = document2.projection;
    const diagnostics = document2.diagnostics;
    const availability = document2.availability;
    const capturedDocumentHash = document2.capturedDocumentHash;
    const projectionStamp = document2.projectionStamp;
    const cacheKey = document2.cacheKey;
    if (document2.schema !== "pointbreak.review-revision-resource" || document2.version !== 1 || !isRecord(resource) || !isRevisionRef(resource.revision) || !nonEmptyString(resource.objectId) || !isResourceProjection(projection) || !isOneOf(availability, CONTENT_AVAILABILITY_VALUES) || capturedDocumentHash !== void 0 && !nonEmptyString(capturedDocumentHash) || availability === "available" && (capturedDocumentHash === void 0 || !isCapturedReviewSnapshot(
      document2.capturedDocument,
      resource.revision.objectArtifactContentHash,
      resource.objectId
    )) || availability !== "available" && (capturedDocumentHash !== void 0 || document2.capturedDocument !== void 0) || !nonEmptyString(projectionStamp) || !nonEmptyString(cacheKey) || !isStringArray(diagnostics)) {
      throw new Error("invalid Revision resource DTO");
    }
    return {
      schema: "pointbreak.review-revision-resource",
      version: 1,
      resource: { revision: resource.revision, objectId: resource.objectId },
      projection,
      availability,
      capturedDocumentHash,
      capturedDocument: document2.capturedDocument,
      diagnostics,
      projectionStamp,
      cacheKey
    };
  }
  __name(decodeRevisionResource, "decodeRevisionResource");
  function decodeRevisionInterdiff(value) {
    const document2 = object(value, "Revision interdiff");
    const interdiff = document2.interdiff;
    const diagnostics = document2.diagnostics;
    const availability = document2.availability;
    const projectionStamp = document2.projectionStamp;
    const cacheKey = document2.cacheKey;
    if (document2.schema !== "pointbreak.review-revision-interdiff" || document2.version !== 1 || !isRecord(interdiff) || !isRevisionRef(interdiff.from) || !isRevisionRef(interdiff.to) || !nonEmptyString(interdiff.algorithmVersion) || !isStringArray(interdiff.scope) || !isOneOf(availability, INTERDIFF_AVAILABILITY_VALUES) || !isStringArray(diagnostics) || !nonEmptyString(projectionStamp) || !nonEmptyString(cacheKey) || availability === "available" !== (document2.comparison !== void 0)) {
      throw new Error("invalid Revision interdiff DTO");
    }
    return {
      schema: "pointbreak.review-revision-interdiff",
      version: 1,
      interdiff: {
        from: interdiff.from,
        to: interdiff.to,
        algorithmVersion: interdiff.algorithmVersion,
        scope: interdiff.scope
      },
      availability,
      comparison: document2.comparison,
      diagnostics,
      projectionStamp,
      cacheKey
    };
  }
  __name(decodeRevisionInterdiff, "decodeRevisionInterdiff");
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
  function buildEventHistoryUrl(query = {}) {
    const limit = query.limit ?? 100;
    if (!Number.isInteger(limit) || limit < 1 || limit > 100) {
      throw new Error("Timeline limit must be an integer from 1 through 100");
    }
    if (query.after !== void 0 && query.at !== void 0) {
      throw new Error("Timeline at and after are mutually exclusive");
    }
    if (query.revision === void 0 !== (query.artifactHash === void 0) || query.revision !== void 0 && (!query.revision || !query.artifactHash)) {
      throw new Error("Timeline revision requires an exact artifact hash");
    }
    const eventTypes = query.type?.split(",");
    if (eventTypes?.some((eventType) => !isEventHistoryEventType(eventType))) {
      throw new Error("Timeline type contains an unknown event type");
    }
    if (eventTypes && new Set(eventTypes).size !== eventTypes.length) {
      throw new Error("Timeline type contains a duplicate event type");
    }
    const canonicalTypes = eventTypes?.sort().join(",");
    const params = new URLSearchParams({ limit: String(limit) });
    const textFields = [
      "after",
      "at",
      "q",
      "track",
      "change",
      "revision",
      "artifactHash"
    ];
    for (const field2 of textFields) {
      const value = query[field2];
      if (value === void 0) continue;
      if (!value) throw new Error(`Timeline ${field2} must be non-empty`);
      params.set(
        field2,
        field2 === "q" ? trimUnicodeWhitespace(value).toLowerCase() : value
      );
    }
    if (canonicalTypes) params.set("type", canonicalTypes);
    if (query.order !== void 0 && query.order !== "asc" && query.order !== "desc") {
      throw new Error("Timeline order must be asc or desc");
    }
    params.set("order", query.order ?? "desc");
    return `/api/v2/history?${params}`;
  }
  __name(buildEventHistoryUrl, "buildEventHistoryUrl");
  function isEventHistoryRevisionRef(value) {
    return isRecord(value) && nonEmptyString(value.revisionId) && nonEmptyString(value.objectArtifactContentHash);
  }
  __name(isEventHistoryRevisionRef, "isEventHistoryRevisionRef");
  var EVENT_HISTORY_EVENT_TYPE_VALUES = new Set(
    EVENT_HISTORY_EVENT_TYPES
  );
  function isEventHistoryEventType(value) {
    return typeof value === "string" && EVENT_HISTORY_EVENT_TYPE_VALUES.has(value);
  }
  __name(isEventHistoryEventType, "isEventHistoryEventType");
  function isEventHistoryWriter(value) {
    return isRecord(value) && nonEmptyString(value.actorId) && isRecord(value.producer) && nonEmptyString(value.producer.name) && nonEmptyString(value.producer.version);
  }
  __name(isEventHistoryWriter, "isEventHistoryWriter");
  function isReviewEndpoint(value) {
    if (!isRecord(value)) return false;
    switch (value.kind) {
      case "git_commit":
        return nonEmptyString(value.commitOid) && nonEmptyString(value.treeOid);
      case "git_tree":
      case "git_index":
        return nonEmptyString(value.treeOid);
      case "git_working_tree":
        return nonEmptyString(value.worktreeRoot);
      default:
        return false;
    }
  }
  __name(isReviewEndpoint, "isReviewEndpoint");
  function isEventHistorySubject(value) {
    if (!isRecord(value)) return false;
    switch (value.kind) {
      case "journal":
        return nonEmptyString(value.journalId);
      case "review":
        return isFactTarget(value.target);
      case "change":
        return nonEmptyString(value.changeId);
      case "change_membership_claim":
        return nonEmptyString(value.membershipClaimId);
      case "change_link_claim":
        return nonEmptyString(value.linkClaimId);
      case "change_revision_relation_claim":
        return nonEmptyString(value.relationClaimId);
      case "revision_relation_attestation":
        return nonEmptyString(value.relationAttestationId) && isEventHistoryRevisionRef(value.revision);
      case "review_fact_port":
        return nonEmptyString(value.portId) && isEventHistoryRevisionRef(value.originRevision) && isFactRef(value.originFact);
      default:
        return false;
    }
  }
  __name(isEventHistorySubject, "isEventHistorySubject");
  function isOptionalStringArray(value) {
    return value === void 0 || isStringArray(value);
  }
  __name(isOptionalStringArray, "isOptionalStringArray");
  function isNullableString(value) {
    return value === null || typeof value === "string";
  }
  __name(isNullableString, "isNullableString");
  function isReviewTargetSummary(value) {
    return isFactTarget(value);
  }
  __name(isReviewTargetSummary, "isReviewTargetSummary");
  function isEventHistorySummary(value, eventType) {
    if (!isRecord(value) || value.kind !== eventType) return false;
    if (eventType === "review_initialized" || eventType === "review_note_imported") {
      return value.details === void 0;
    }
    const details = value.details;
    if (!isRecord(details)) return false;
    switch (eventType) {
      case "work_object_proposed":
        return nonEmptyString(details.engagementId) && isRecord(details.revision) && nonEmptyString(details.revision.id) && nonEmptyString(details.revision.objectId) && isNullableString(details.summary) && nonEmptyString(details.objectArtifactContentHash) && isStringArray(details.supersedes);
      case "review_observation_recorded":
        return nonEmptyString(details.observationId) && isReviewTargetSummary(details.target) && nonEmptyString(details.title) && optionalString(details.body) && isOptionalStringArray(details.tags) && optionalString(details.confidence) && isOptionalStringArray(details.supersedesObservationIds) && isOptionalStringArray(details.respondsToObservationIds);
      case "review_assessment_recorded":
        return nonEmptyString(details.assessmentId) && isReviewTargetSummary(details.target) && (details.assessment === "accepted" || details.assessment === "accepted_with_follow_up" || details.assessment === "needs_changes" || details.assessment === "needs_clarification") && optionalString(details.summary) && isOptionalStringArray(details.replacesAssessmentIds) && isOptionalStringArray(details.relatedObservationIds) && isOptionalStringArray(details.relatedInputRequestIds);
      case "input_request_opened":
        return nonEmptyString(details.inputRequestId) && isReviewTargetSummary(details.target) && (details.reasonCode === "ambiguous_state" || details.reasonCode === "unsafe_action" || details.reasonCode === "stale_revision" || details.reasonCode === "failed_gate" || details.reasonCode === "external_side_effect" || details.reasonCode === "conflicting_event" || details.reasonCode === "missing_permission" || details.reasonCode === "manual_decision_required" || details.reasonCode === "insufficient_evidence") && nonEmptyString(details.title) && optionalString(details.body);
      case "input_request_responded":
        return nonEmptyString(details.inputRequestResponseId) && nonEmptyString(details.inputRequestId) && nonEmptyString(details.revisionId) && (details.outcome === "approved" || details.outcome === "rejected" || details.outcome === "dismissed" || details.outcome === "superseded" || details.outcome === "abandoned") && optionalString(details.reason);
      case "revision_ref_associated":
        return nonEmptyString(details.refAssociationId) && isReviewTargetSummary(details.target) && nonEmptyString(details.refName) && nonEmptyString(details.headOid);
      case "revision_ref_withdrawn":
        return nonEmptyString(details.refWithdrawalId) && isReviewTargetSummary(details.target) && nonEmptyString(details.refAssociationId);
      case "revision_commit_associated":
        return nonEmptyString(details.commitAssociationId) && isReviewTargetSummary(details.target) && isReviewEndpoint(details.commit);
      case "revision_commit_withdrawn":
        return nonEmptyString(details.commitWithdrawalId) && isReviewTargetSummary(details.target) && nonEmptyString(details.commitAssociationId);
      case "validation_check_recorded":
        return nonEmptyString(details.validationCheckId) && isRecord(details.target) && details.target.kind === "revision" && nonEmptyString(details.target.revisionId) && nonEmptyString(details.checkName) && optionalString(details.command) && (details.status === "passed" || details.status === "failed" || details.status === "errored" || details.status === "skipped") && (details.exitCode === void 0 || typeof details.exitCode === "number" && Number.isSafeInteger(details.exitCode)) && (details.trigger === "manual" || details.trigger === "push" || details.trigger === "pull_request") && optionalString(details.summary);
      case "change_declared":
        return details.schema === "pointbreak.change-declared" && details.version === 1 && nonEmptyString(details.declarationClaimId) && nonEmptyString(details.changeId) && isRecord(details.identityDescriptor) && details.identityDescriptor.schema === "pointbreak.change-identity.v1" && (details.identityDescriptor.kind === "opaque_nonce" && nonEmptyString(details.identityDescriptor.nonce) || details.identityDescriptor.kind === "root_revision" && nonEmptyString(details.identityDescriptor.revision_id)) && nonEmptyString(details.claimNonce);
      case "change_membership_asserted":
        return details.schema === "pointbreak.change-membership-asserted" && details.version === 1 && nonEmptyString(details.membershipClaimId) && nonEmptyString(details.changeId) && nonEmptyString(details.revisionId) && nonEmptyString(details.claimNonce);
      case "change_membership_withdrawn":
        return details.schema === "pointbreak.change-membership-withdrawn" && details.version === 1 && nonEmptyString(details.membershipWithdrawalId) && nonEmptyString(details.membershipClaimId) && nonEmptyString(details.claimNonce);
      case "change_link_asserted":
        return details.schema === "pointbreak.change-link-asserted" && details.version === 1 && nonEmptyString(details.linkClaimId) && nonEmptyString(details.leftChangeId) && nonEmptyString(details.rightChangeId) && (details.relation === "same_work" || details.relation === "related_work") && nonEmptyString(details.claimNonce);
      case "change_revision_relation_asserted":
        return details.schema === "pointbreak.change-revision-relation-asserted" && details.version === 1 && nonEmptyString(details.relationClaimId) && nonEmptyString(details.changeId) && isEventHistoryRevisionRef(details.successor) && isEventHistoryRevisionRef(details.predecessor) && details.relation === "supersedes" && nonEmptyString(details.claimNonce);
      case "change_revision_relation_withdrawn":
        return details.schema === "pointbreak.change-revision-relation-withdrawn" && details.version === 1 && nonEmptyString(details.relationWithdrawalId) && nonEmptyString(details.relationClaimId) && nonEmptyString(details.claimNonce);
      case "revision_relation_attested":
        return details.schema === "pointbreak.revision-relation-attested" && details.version === 1 && nonEmptyString(details.relationAttestationId) && isEventHistoryRevisionRef(details.revision) && nonEmptyString(details.commitAssociationId) && (details.semanticRelation === "exact_materialization" || details.semanticRelation === "equivalent_rewrite" || details.semanticRelation === "content_preserving_extension" || details.semanticRelation === "landing_provenance" || details.semanticRelation === "related_provenance" || details.semanticRelation === "unknown") && (details.proofStatus === "verified" || details.proofStatus === "asserted" || details.proofStatus === "unverified" || details.proofStatus === "indeterminate" || details.proofStatus === "refuted") && nonEmptyString(details.proofMethod) && nonEmptyString(details.proofAlgorithmVersion) && isStringArray(details.captureScope) && isNullableString(details.comparisonBaseOrParent) && isStringArray(details.endpointOids) && isNullableString(details.evidenceContentHash) && nonEmptyString(details.resultDigest);
      case "review_fact_ported":
        return details.schema === "pointbreak.review-fact-ported" && details.version === 1 && nonEmptyString(details.portId) && isEventHistoryRevisionRef(details.originRevision) && isFactRef(details.originFact) && isEventHistoryRevisionRef(details.targetRevision) && (details.relation === "context_only" || details.relation === "reanchored_as" || details.relation === "carried_open_as" || details.relation === "resolved_by") && (details.targetFact === null || isFactRef(details.targetFact)) && isNullableString(details.rationaleContentHash) && isNullableString(details.contextChangeId);
    }
  }
  __name(isEventHistorySummary, "isEventHistorySummary");
  function isEventHistoryEntry(value) {
    if (!isRecord(value) || !isEventHistoryEventType(value.eventType)) {
      return false;
    }
    return nonEmptyString(value.eventId) && nonEmptyString(value.occurredAt) && nonEmptyString(value.payloadHash) && nonEmptyString(value.journalId) && optionalString(value.trackId) && isEventHistoryWriter(value.writer) && (value.verificationStatus === "valid" || value.verificationStatus === "invalid" || value.verificationStatus === "untrusted_key" || value.verificationStatus === "unsigned") && (value.assertionMode === "advisory" || value.assertionMode === "operative") && optionalString(value.signer) && (value.sourceRef === void 0 || isRecord(value.sourceRef) && nonEmptyString(value.sourceRef.sourceSystem) && nonEmptyString(value.sourceRef.sourceId)) && (value.ingest === void 0 || isRecord(value.ingest) && (value.ingest.via === "ingest-events" || value.ingest.via === "bundle-apply") && nonEmptyString(value.ingest.receivedAt)) && isEventHistorySubject(value.subject) && isStringArray(value.changeIds) && Array.isArray(value.revisionRefs) && value.revisionRefs.every(isEventHistoryRevisionRef) && isStringArray(value.unresolvedRevisionIds) && isEventHistorySummary(value.summary, value.eventType);
  }
  __name(isEventHistoryEntry, "isEventHistoryEntry");
  function decodeEventHistory(value) {
    const document2 = object(value, "event history");
    const completion = document2.completion;
    const authorityCursor = decodeAuthorityCursorV2(document2.authorityCursor);
    if (document2.schema !== "pointbreak.inspect-event-history" || document2.version !== 1 || !nonEmptyString(document2.sourceChangeProjectionStamp) || !nonEmptyString(document2.timelineProjectionStamp) || document2.order !== "asc" && document2.order !== "desc" || !Number.isSafeInteger(document2.eventCount) || document2.eventCount < 0 || document2.eventCount !== authorityCursor.eventCount || !Number.isSafeInteger(document2.matchCount) || document2.matchCount < 0 || !Number.isSafeInteger(document2.offset) || document2.offset < 0 || document2.matchIndex !== void 0 && (!Number.isSafeInteger(document2.matchIndex) || document2.matchIndex < 0) || !isRecord(document2.facets) || !Object.entries(document2.facets).every(
      ([eventType, count]) => isEventHistoryEventType(eventType) && typeof count === "number" && Number.isSafeInteger(count) && count >= 0
    ) || !isRecord(completion) || !isStringArray(completion.eventTypes) || !completion.eventTypes.every(isEventHistoryEventType) || new Set(completion.eventTypes).size !== completion.eventTypes.length || !isStringArray(completion.trackIds) || !isStringArray(completion.changeIds) || !Array.isArray(completion.revisionRefs) || !completion.revisionRefs.every(isEventHistoryRevisionRef) || !isStringArray(completion.unresolvedRevisionIds) || !isStringArray(document2.diagnostics) || !isStringArray(document2.queryNotices) || !Array.isArray(document2.entries) || document2.entries.length > 100 || !document2.entries.every(isEventHistoryEntry) || document2.matchCount > document2.eventCount || document2.offset > document2.matchCount || document2.offset + document2.entries.length > document2.matchCount || document2.previous !== void 0 && !nonEmptyString(document2.previous) || document2.next !== void 0 && !nonEmptyString(document2.next)) {
      throw new Error("invalid event history DTO");
    }
    if (document2.offset + document2.entries.length > document2.matchCount) {
      throw new Error("event history page exceeds its match count");
    }
    return {
      ...document2,
      authorityCursor
    };
  }
  __name(decodeEventHistory, "decodeEventHistory");
  var AUTHORITY_CURSOR_V2_KEYS = /* @__PURE__ */ new Set([
    "schema",
    "journalRecordCount",
    "eventCount",
    "journalRecordSetHash",
    "eventSetHash",
    "capabilitySetHash"
  ]);
  var PREFIXED_SHA256 = /^sha256:[0-9a-f]{64}$/;
  function decodeAuthorityCursorV2(value) {
    const cursor = object(value, "authority cursor");
    if (!hasExactKeys(cursor, AUTHORITY_CURSOR_V2_KEYS) || cursor.schema !== "pointbreak.authority-cursor.v2" || !isNonnegativeSafeInteger(cursor.journalRecordCount) || !isNonnegativeSafeInteger(cursor.eventCount) || cursor.eventCount > cursor.journalRecordCount || typeof cursor.journalRecordSetHash !== "string" || !PREFIXED_SHA256.test(cursor.journalRecordSetHash) || typeof cursor.eventSetHash !== "string" || !PREFIXED_SHA256.test(cursor.eventSetHash) || typeof cursor.capabilitySetHash !== "string" || !PREFIXED_SHA256.test(cursor.capabilitySetHash)) {
      throw new Error("invalid authority cursor DTO");
    }
    return cursor;
  }
  __name(decodeAuthorityCursorV2, "decodeAuthorityCursorV2");
  function decodeReaderProfile(value) {
    const profile = object(value, "Inspector reader profile");
    const availability = profile.availability;
    const authorityCursor = decodeAuthorityCursorV2(profile.authorityCursor);
    const documents = profile.documents;
    const minimumReaderProfile = profile.minimumReaderProfile;
    const commitGraphStamp = profile.commitGraphStamp;
    if (profile.schema !== "pointbreak.inspect-reader-profile" || profile.version !== 1 || !isReaderProfileAvailability(availability) || !isDocumentMap(documents) || !sameDocumentMap(documents, CHANGE_READER_DOCUMENTS)) {
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
    if (page.schema !== expectedSchema || page.version !== expectedVersion || !nonEmptyString(stamp) || !Array.isArray(changes) || expected.bounded && changes.length > 100 || !changes.every((change) => isChangeSummary(change, stamp)) || !isStrictlyAscending(changes.map((change) => change.changeId)) || new Set(changes.map((change) => change.changeId)).size !== changes.length || diagnostics !== void 0 && !isStringArray(diagnostics) || presentations !== void 0 && !isPresentations(presentations, changes, expected.lens)) {
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
    return initial.availability === postflight.availability && initial.minimumReaderProfile === postflight.minimumReaderProfile && initial.commitGraphStamp === postflight.commitGraphStamp && sameDocumentMap(initial.documents, postflight.documents) && sameAuthorityCursor(initial.authorityCursor, postflight.authorityCursor);
  }
  __name(sameProfileGeneration, "sameProfileGeneration");
  function sameAuthorityCursor(left, right) {
    return canonicalJson(left) === canonicalJson(right);
  }
  __name(sameAuthorityCursor, "sameAuthorityCursor");
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
  function isReaderProfileAvailability(value) {
    return value === "migration_required" || value === "migration_in_progress" || value === "ready";
  }
  __name(isReaderProfileAvailability, "isReaderProfileAvailability");
  function isChangeSummary(value, stamp) {
    if (!isRecord(value)) return false;
    return nonEmptyString(value.changeId) && (value.declarationState === "authoritative" || value.declarationState === "incomplete" || value.declarationState === "conflicted") && isStringArray(value.titleAssertions) && typeof value.memberCount === "number" && Number.isSafeInteger(value.memberCount) && value.memberCount >= 0 && isOneOf(value.topology, TOPOLOGY_VALUES) && isOneOf(value.lifecycle, LIFECYCLE_VALUES) && isOneOf(value.attentionSummary, ATTENTION_VALUES) && isOneOf(value.availabilitySummary, AVAILABILITY_VALUES) && value.projectionStamp === stamp && Array.isArray(value.currentRevisionRefs) && value.currentRevisionRefs.every(isRevisionRef) && uniqueRevisionKeys(value.currentRevisionRefs).size === value.currentRevisionRefs.length && (value.diagnostics === void 0 || isStringArray(value.diagnostics));
  }
  __name(isChangeSummary, "isChangeSummary");
  function isClaimSupport(value) {
    return isRecord(value) && nonEmptyString(value.eventId) && nonEmptyString(value.actorId) && optionalString(value.trackId);
  }
  __name(isClaimSupport, "isClaimSupport");
  function isChangeMemberRevisions(value) {
    return Array.isArray(value) && value.every(
      (member) => isRecord(member) && isRevisionRef(member.revision) && isStringArray(member.supportingClaimIds)
    );
  }
  __name(isChangeMemberRevisions, "isChangeMemberRevisions");
  function isUnavailableChangeMemberRevisions(value) {
    return Array.isArray(value) && value.every(
      (member) => isRecord(member) && nonEmptyString(member.revisionId) && (member.reason === "invalid_revision_id" || member.reason === "invalid_object_artifact_content_hash") && isStringArray(member.supportingClaimIds)
    );
  }
  __name(isUnavailableChangeMemberRevisions, "isUnavailableChangeMemberRevisions");
  function isMembershipClaims(value, changeId) {
    return Array.isArray(value) && value.every(
      (claim) => isRecord(claim) && nonEmptyString(claim.claimId) && claim.changeId === changeId && nonEmptyString(claim.revisionId) && Array.isArray(claim.supports) && claim.supports.every(isClaimSupport) && Array.isArray(claim.withdrawals) && claim.withdrawals.every(isClaimSupport) && typeof claim.active === "boolean" && isStringArray(claim.diagnostics)
    );
  }
  __name(isMembershipClaims, "isMembershipClaims");
  function isClaimWithdrawals(value) {
    return Array.isArray(value) && value.every(
      (withdrawal) => isRecord(withdrawal) && nonEmptyString(withdrawal.claimId) && Array.isArray(withdrawal.supports) && withdrawal.supports.every(isClaimSupport) && isStringArray(withdrawal.diagnostics)
    );
  }
  __name(isClaimWithdrawals, "isClaimWithdrawals");
  function isChangeLinks(value) {
    return Array.isArray(value) && value.every(
      (link) => isRecord(link) && nonEmptyString(link.leftChangeId) && nonEmptyString(link.rightChangeId) && nonEmptyString(link.relation)
    );
  }
  __name(isChangeLinks, "isChangeLinks");
  function isEffectiveSupersedes(value) {
    return Array.isArray(value) && value.every(
      (edge) => Array.isArray(edge) && edge.length === 2 && isRevisionRef(edge[0]) && isRevisionRef(edge[1])
    );
  }
  __name(isEffectiveSupersedes, "isEffectiveSupersedes");
  function isRevisionQualifications(value, currentRevisionRefs) {
    if (!Array.isArray(value)) return false;
    const qualifications = [];
    for (const candidate of value) {
      if (!isRecord(candidate) || !isRevisionRef(candidate.revision)) {
        return false;
      }
      const revision2 = candidate.revision;
      if (typeof candidate.qualified !== "boolean" || !currentRevisionRefs.some((current) => sameRevision(current, revision2)))
        return false;
      qualifications.push({
        revision: revision2,
        qualified: candidate.qualified
      });
    }
    return sameRevisionSet(
      qualifications.map((qualification) => qualification.revision),
      currentRevisionRefs
    );
  }
  __name(isRevisionQualifications, "isRevisionQualifications");
  function sameRevisionSet(left, right) {
    const leftKeys = uniqueRevisionKeys(left);
    const rightKeys = uniqueRevisionKeys(right);
    return leftKeys.size === left.length && rightKeys.size === right.length && leftKeys.size === rightKeys.size && [...leftKeys].every((key) => rightKeys.has(key));
  }
  __name(sameRevisionSet, "sameRevisionSet");
  function isPresentations(value, changes, lens) {
    if (!isRecord(value)) return false;
    const summaries = new Map(
      changes.map((change) => [change.changeId, change])
    );
    if (Object.keys(value).length !== summaries.size) return false;
    return Object.entries(value).every(([changeId, presentation]) => {
      const change = summaries.get(changeId);
      if (change === void 0 || !isRecord(presentation) || !Array.isArray(presentation.currentRevisions) || !presentation.currentRevisions.every(isPresentationRevision) || (lens === "attention" ? !isAttentionPresentation(presentation.attention) : presentation.attention !== void 0)) {
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
  function isAttentionPresentation(value) {
    if (!isRecord(value) || !isAttentionReason(value.primaryReason) || !Array.isArray(value.reasons) || value.reasons.length === 0 || !value.reasons.every(isAttentionReason) || !sameAttentionReason(value.primaryReason, value.reasons[0]) || value.diagnostics !== void 0 && !isStringArray(value.diagnostics)) {
      return false;
    }
    return true;
  }
  __name(isAttentionPresentation, "isAttentionPresentation");
  function isAttentionReason(value) {
    if (!isRecord(value)) return false;
    switch (value.kind) {
      case "conflicted":
      case "incomplete":
      case "no_current_revision":
        return Object.keys(value).length === 1;
      case "unresolved_operative_requests":
        return Object.keys(value).length === 2 && Array.isArray(value.requestIds) && value.requestIds.length > 0 && value.requestIds.every(nonEmptyString) && new Set(value.requestIds).size === value.requestIds.length;
      case "current_revisions_need_assessment":
        return Object.keys(value).length === 2 && Array.isArray(value.revisions) && value.revisions.length > 0 && value.revisions.every(isRevisionRef) && uniqueRevisionKeys(value.revisions).size === value.revisions.length;
      default:
        return false;
    }
  }
  __name(isAttentionReason, "isAttentionReason");
  function sameAttentionReason(left, right) {
    if (left.kind !== right.kind) return false;
    if (left.kind === "unresolved_operative_requests" && right.kind === "unresolved_operative_requests") {
      return left.requestIds.length === right.requestIds.length && left.requestIds.every(
        (requestId, index) => requestId === right.requestIds[index]
      );
    }
    if (left.kind === "current_revisions_need_assessment" && right.kind === "current_revisions_need_assessment") {
      return left.revisions.length === right.revisions.length && left.revisions.every(
        (revision2, index) => sameRevision(revision2, right.revisions[index])
      );
    }
    return true;
  }
  __name(sameAttentionReason, "sameAttentionReason");
  function isPresentationRevision(value) {
    return isRecord(value) && isRevisionRef(value.revision) && (value.summarySource === "revision_proposal_summary" && nonEmptyString(value.revisionProposalSummary) || value.summarySource === "absent" && value.revisionProposalSummary === void 0);
  }
  __name(isPresentationRevision, "isPresentationRevision");
  function isRevisionRef(value) {
    return isRecord(value) && nonEmptyString(value.revisionId) && nonEmptyString(value.objectArtifactContentHash);
  }
  __name(isRevisionRef, "isRevisionRef");
  function isChangeDetailInspectorPresentation(value, detail) {
    if (value === void 0) return true;
    if (!isRecord(value) || !isChangeRevisionGraphPresentation(value.revisionGraph))
      return false;
    const graph = value.revisionGraph;
    const expectedMembers = new Set(
      detail.memberRevisions.map(
        (member) => revisionGraphNodeId(member.revision)
      )
    );
    const expectedCurrent = new Set(
      detail.currentRevisionRefs.map(revisionGraphNodeId)
    );
    const expectedNodes = new Set(expectedMembers);
    for (const claim of detail.pendingOrConflictingEdges) {
      expectedNodes.add(revisionGraphNodeId(claim.successor));
      expectedNodes.add(revisionGraphNodeId(claim.predecessor));
    }
    const actualNodes = new Map(graph.nodes.map((node) => [node.id, node]));
    if (actualNodes.size !== expectedNodes.size || ![...expectedNodes].every((id) => actualNodes.has(id)) || !graph.nodes.every(
      (node) => node.isMember === expectedMembers.has(node.id) && node.isCurrent === expectedCurrent.has(node.id)
    ) || !sameStringArray(graph.diagnostics ?? [], detail.diagnostics)) {
      return false;
    }
    const effective = new Set(
      detail.effectiveSupersedes.map(
        ([successor, predecessor]) => graphEdgeKey(
          revisionGraphNodeId(successor),
          revisionGraphNodeId(predecessor)
        )
      )
    );
    const graphEffective = new Set(
      graph.effectiveSupersedes.map((edge) => graphEdgeKey(edge.from, edge.to))
    );
    if (!sameStringSet(effective, graphEffective)) return false;
    const pending = new Map(
      detail.pendingOrConflictingEdges.map((claim) => [claim.claimId, claim])
    );
    return pending.size === graph.pendingOrConflictingClaims.length && graph.pendingOrConflictingClaims.every((edge) => {
      const claim = pending.get(edge.claimId);
      return claim !== void 0 && sameRevision(edge.successor, claim.successor) && sameRevision(edge.predecessor, claim.predecessor) && sameStringArray(edge.diagnostics, claim.diagnostics);
    });
  }
  __name(isChangeDetailInspectorPresentation, "isChangeDetailInspectorPresentation");
  function isChangeRevisionDetailInspectorPresentation(value, detail) {
    if (value === void 0) return true;
    if (!isRecord(value) || !isFactRelationshipGraphPresentation(value.factGraph))
      return false;
    const graph = value.factGraph;
    const expectedActivation = /* @__PURE__ */ new Map();
    const expectedNodes = /* @__PURE__ */ new Set();
    for (const fact2 of detail.factPresentations) {
      const id = factGraphNodeId(fact2.originRevision, fact2.family, fact2.factId);
      expectedNodes.add(id);
      if (sameRevision(fact2.originRevision, detail.revision) || fact2.presentedInRevision !== void 0 && sameRevision(fact2.presentedInRevision, detail.revision)) {
        expectedActivation.set(id, detail.revision);
      }
    }
    for (const port of detail.factPorts) {
      expectedNodes.add(
        factGraphNodeId(
          port.originRevision,
          port.originFact.kind,
          factRefId(port.originFact)
        )
      );
      const targetId = port.targetFact === void 0 ? revisionGraphNodeId(port.targetRevision) : factGraphNodeId(
        port.targetRevision,
        port.targetFact.kind,
        factRefId(port.targetFact)
      );
      expectedNodes.add(targetId);
      if (port.targetFact === void 0 && sameRevision(port.targetRevision, detail.revision)) {
        expectedActivation.set(targetId, detail.revision);
      }
    }
    const actualNodes = new Map(graph.nodes.map((node) => [node.id, node]));
    if (actualNodes.size !== expectedNodes.size || ![...expectedNodes].every((id) => actualNodes.has(id)) || !graph.nodes.every((node) => {
      const activation = expectedActivation.get(node.id);
      return activation === void 0 ? node.contextAvailability === "relationship_context_only" && node.activationRevision === void 0 : node.contextAvailability === "available" && node.activationRevision !== void 0 && sameRevision(node.activationRevision, activation);
    })) {
      return false;
    }
    const ports = new Map(detail.factPorts.map((port) => [port.portId, port]));
    return ports.size === graph.factPorts.length && graph.factPorts.every((edge) => {
      const port = ports.get(edge.portId);
      return port !== void 0 && sameRevision(edge.originRevision, port.originRevision) && sameFactRef(edge.originFact, port.originFact) && sameRevision(edge.targetRevision, port.targetRevision) && sameOptionalFactRef(edge.targetFact, port.targetFact) && edge.relation === port.relation && edge.applicability === port.applicability && sameStringArray(edge.diagnostics ?? [], port.diagnostics);
    });
  }
  __name(isChangeRevisionDetailInspectorPresentation, "isChangeRevisionDetailInspectorPresentation");
  function isChangeRevisionGraphPresentation(value) {
    if (!isRecord(value) || !Array.isArray(value.nodes) || value.nodes.length === 0 || !value.nodes.every(isChangeRevisionGraphNode) || !Array.isArray(value.effectiveSupersedes) || !Array.isArray(value.pendingOrConflictingClaims) || !isGraphBounds(value.bounds) || value.diagnostics !== void 0 && !isStringArray(value.diagnostics)) {
      return false;
    }
    const nodes = new Map(value.nodes.map((node) => [node.id, node]));
    return nodes.size === value.nodes.length && value.effectiveSupersedes.every(
      (edge) => isChangeRevisionGraphEffectiveEdge(edge, nodes)
    ) && uniqueGraphEdgeEndpoints(value.effectiveSupersedes) && value.pendingOrConflictingClaims.every(
      (edge) => isChangeRevisionGraphClaimEdge(edge, nodes)
    ) && new Set(value.pendingOrConflictingClaims.map((edge) => edge.claimId)).size === value.pendingOrConflictingClaims.length;
  }
  __name(isChangeRevisionGraphPresentation, "isChangeRevisionGraphPresentation");
  function isChangeRevisionGraphNode(value) {
    return isRecord(value) && nonEmptyString(value.id) && isRevisionRef(value.revision) && value.id === revisionGraphNodeId(value.revision) && isFiniteGeometry(value) && typeof value.isCurrent === "boolean" && typeof value.isMember === "boolean" && isGraphContext(value) && (value.isMember ? value.contextAvailability === "available" && isRevisionRef(value.activationRevision) && sameRevision(value.activationRevision, value.revision) : value.contextAvailability === "relationship_context_only" && value.activationRevision === void 0);
  }
  __name(isChangeRevisionGraphNode, "isChangeRevisionGraphNode");
  function isChangeRevisionGraphEffectiveEdge(value, nodes) {
    return isRecord(value) && nonEmptyString(value.from) && nonEmptyString(value.to) && isRevisionRef(value.successor) && isRevisionRef(value.predecessor) && value.from === revisionGraphNodeId(value.successor) && value.to === revisionGraphNodeId(value.predecessor) && nodes.has(value.from) && nodes.has(value.to) && isGraphPath(value.path);
  }
  __name(isChangeRevisionGraphEffectiveEdge, "isChangeRevisionGraphEffectiveEdge");
  function isChangeRevisionGraphClaimEdge(value, nodes) {
    if (!isRecord(value) || !isChangeRevisionGraphEffectiveEdge(value, nodes))
      return false;
    return nonEmptyString(value.claimId) && isStringArray(value.diagnostics);
  }
  __name(isChangeRevisionGraphClaimEdge, "isChangeRevisionGraphClaimEdge");
  function isFactRelationshipGraphPresentation(value) {
    if (!isRecord(value) || !Array.isArray(value.nodes) || value.nodes.length === 0 || !value.nodes.every(isFactRelationshipGraphNode) || !Array.isArray(value.observationSupersedes) || !Array.isArray(value.assessmentReplaces) || !Array.isArray(value.factPorts) || !isGraphBounds(value.bounds)) {
      return false;
    }
    const nodes = new Map(value.nodes.map((node) => [node.id, node]));
    return nodes.size === value.nodes.length && value.observationSupersedes.every(
      (edge) => isFactRelationshipEdge(edge, "observation", nodes)
    ) && uniqueGraphEdgeEndpoints(value.observationSupersedes) && value.assessmentReplaces.every(
      (edge) => isFactRelationshipEdge(edge, "assessment", nodes)
    ) && uniqueGraphEdgeEndpoints(value.assessmentReplaces) && value.factPorts.every((edge) => isFactPortRelationshipEdge(edge, nodes)) && new Set(value.factPorts.map((edge) => edge.portId)).size === value.factPorts.length;
  }
  __name(isFactRelationshipGraphPresentation, "isFactRelationshipGraphPresentation");
  function isFactRelationshipGraphNode(value) {
    if (!isRecord(value) || !nonEmptyString(value.id) || !isRevisionRef(value.revision) || !isFiniteGeometry(value) || !isGraphContext(value)) {
      return false;
    }
    if (value.kind === "fact") {
      return nonEmptyString(value.factId) && nonEmptyString(value.family) && value.id === factGraphNodeId(value.revision, value.family, value.factId);
    }
    return value.kind === "revision" && value.factId === void 0 && value.family === void 0 && value.id === revisionGraphNodeId(value.revision);
  }
  __name(isFactRelationshipGraphNode, "isFactRelationshipGraphNode");
  function isGraphContext(value) {
    if (value.contextAvailability === "available") {
      return isRevisionRef(value.activationRevision);
    }
    return value.contextAvailability === "relationship_context_only" && value.activationRevision === void 0;
  }
  __name(isGraphContext, "isGraphContext");
  function isFactRelationshipEdge(value, family, nodes) {
    return isRecord(value) && nonEmptyString(value.from) && nonEmptyString(value.to) && isRevisionRef(value.originRevision) && nonEmptyString(value.fromFactId) && nonEmptyString(value.toFactId) && value.from === factGraphNodeId(value.originRevision, family, value.fromFactId) && value.to === factGraphNodeId(value.originRevision, family, value.toFactId) && nodes.has(value.from) && nodes.has(value.to) && isGraphPath(value.path);
  }
  __name(isFactRelationshipEdge, "isFactRelationshipEdge");
  function isFactPortRelationshipEdge(value, nodes) {
    if (!isRecord(value) || !nonEmptyString(value.portId) || !nonEmptyString(value.from) || !nonEmptyString(value.to) || !isRevisionRef(value.originRevision) || !isFactRef(value.originFact) || !isRevisionRef(value.targetRevision) || value.targetFact !== void 0 && !isFactRef(value.targetFact) || value.relation !== "context_only" && value.relation !== "reanchored_as" && value.relation !== "carried_open_as" && value.relation !== "resolved_by" || value.applicability !== "applicable" && value.applicability !== "conflicted" && value.applicability !== "unavailable" || !isGraphPath(value.path) || value.diagnostics !== void 0 && !isStringArray(value.diagnostics)) {
      return false;
    }
    const from = factGraphNodeId(
      value.originRevision,
      value.originFact.kind,
      factRefId(value.originFact)
    );
    const to = value.targetFact === void 0 ? revisionGraphNodeId(value.targetRevision) : factGraphNodeId(
      value.targetRevision,
      value.targetFact.kind,
      factRefId(value.targetFact)
    );
    return value.from === from && value.to === to && nodes.has(from) && nodes.has(to);
  }
  __name(isFactPortRelationshipEdge, "isFactPortRelationshipEdge");
  function isGraphBounds(value) {
    return isRecord(value) && isFiniteNumber(value.w) && isFiniteNumber(value.h);
  }
  __name(isGraphBounds, "isGraphBounds");
  function isFiniteGeometry(value) {
    return isFiniteNumber(value.x) && isFiniteNumber(value.y) && isFiniteNumber(value.w) && isFiniteNumber(value.h);
  }
  __name(isFiniteGeometry, "isFiniteGeometry");
  function isGraphPath(value) {
    return Array.isArray(value) && value.length > 0 && value.every(
      (point) => Array.isArray(point) && point.length === 2 && isFiniteNumber(point[0]) && isFiniteNumber(point[1])
    );
  }
  __name(isGraphPath, "isGraphPath");
  function isFiniteNumber(value) {
    return typeof value === "number" && Number.isFinite(value);
  }
  __name(isFiniteNumber, "isFiniteNumber");
  function uniqueGraphEdgeEndpoints(edges) {
    return new Set(edges.map((edge) => `${edge.from}\0${edge.to}`)).size === edges.length;
  }
  __name(uniqueGraphEdgeEndpoints, "uniqueGraphEdgeEndpoints");
  function revisionGraphNodeId(revision2) {
    return `revision:${revision2.revisionId}@${revision2.objectArtifactContentHash}`;
  }
  __name(revisionGraphNodeId, "revisionGraphNodeId");
  function factGraphNodeId(revision2, family, factId) {
    return `${revisionGraphNodeId(revision2).replace("revision:", "fact:")}:${family}:${factId}`;
  }
  __name(factGraphNodeId, "factGraphNodeId");
  function graphEdgeKey(from, to) {
    return `${from}\0${to}`;
  }
  __name(graphEdgeKey, "graphEdgeKey");
  function sameStringArray(left, right) {
    return left.length === right.length && left.every((value, index) => value === right[index]);
  }
  __name(sameStringArray, "sameStringArray");
  function sameStringSet(left, right) {
    return left.size === right.size && [...left].every((value) => right.has(value));
  }
  __name(sameStringSet, "sameStringSet");
  function sameFactRef(left, right) {
    return left.kind === right.kind && factRefId(left) === factRefId(right);
  }
  __name(sameFactRef, "sameFactRef");
  function sameOptionalFactRef(left, right) {
    return left === void 0 && right === void 0 || left !== void 0 && right !== void 0 && sameFactRef(left, right);
  }
  __name(sameOptionalFactRef, "sameOptionalFactRef");
  function uniqueRevisionKeys(revisions) {
    return new Set(
      revisions.map(
        (revision2) => `${revision2.revisionId}\0${revision2.objectArtifactContentHash}`
      )
    );
  }
  __name(uniqueRevisionKeys, "uniqueRevisionKeys");
  function isRelationClaim(value, changeId) {
    return isRecord(value) && nonEmptyString(value.claimId) && value.changeId === changeId && typeof value.active === "boolean" && isRevisionRef(value.successor) && isRevisionRef(value.predecessor) && Array.isArray(value.supports) && value.supports.every(isClaimSupport) && Array.isArray(value.withdrawals) && value.withdrawals.every(isClaimSupport) && isStringArray(value.diagnostics);
  }
  __name(isRelationClaim, "isRelationClaim");
  function isFactPresentation(value) {
    return isRecord(value) && nonEmptyString(value.factId) && nonEmptyString(value.family) && isRevisionRef(value.originRevision) && (value.target === void 0 || isFactTarget(value.target)) && (value.contextChangeId === void 0 || nonEmptyString(value.contextChangeId)) && (value.presentedInRevision === void 0 || isRevisionRef(value.presentedInRevision)) && (value.portRelation === void 0 || value.portRelation === "context_only" || value.portRelation === "reanchored_as" || value.portRelation === "carried_open_as" || value.portRelation === "resolved_by") && nonEmptyString(value.actorId) && (value.trackId === void 0 || nonEmptyString(value.trackId)) && isOneOf(value.revisionCurrency, REVISION_CURRENCY_VALUES) && isOneOf(value.familyState, FACT_FAMILY_STATE_VALUES) && isOneOf(value.availability, CONTENT_AVAILABILITY_VALUES);
  }
  __name(isFactPresentation, "isFactPresentation");
  function isFactTarget(value) {
    if (!isRecord(value) || !nonEmptyString(value.revisionId)) return false;
    if (value.kind === "revision") return true;
    if (value.kind === "file") return nonEmptyString(value.filePath);
    if (value.kind === "range") {
      return nonEmptyString(value.filePath) && (value.side === "old" || value.side === "new") && Number.isSafeInteger(value.startLine) && value.startLine > 0 && Number.isSafeInteger(value.endLine) && value.endLine >= value.startLine;
    }
    if (value.kind === "observation") return nonEmptyString(value.observationId);
    if (value.kind === "input_request")
      return nonEmptyString(value.inputRequestId);
    if (value.kind === "assessment") return nonEmptyString(value.assessmentId);
    return value.kind === "event" && nonEmptyString(value.eventId);
  }
  __name(isFactTarget, "isFactTarget");
  function uniqueFactPresentationIds(facts) {
    return new Set(facts.map((fact2) => fact2.factId)).size === facts.length;
  }
  __name(uniqueFactPresentationIds, "uniqueFactPresentationIds");
  function isResourceProjection(value) {
    return isRecord(value) && typeof value.includeBody === "boolean" && (value.trackId === void 0 || nonEmptyString(value.trackId));
  }
  __name(isResourceProjection, "isResourceProjection");
  function isCapturedReviewSnapshot(value, expectedContentHash, expectedObjectId) {
    if (!isRecord(value) || value.schema !== "pointbreak.review-snapshot" || value.version !== 1 || value.contentHash !== expectedContentHash || !isRecord(value.snapshot)) {
      return false;
    }
    return nonEmptyString(value.snapshot.review_id) && value.snapshot.object_id === expectedObjectId && Array.isArray(value.snapshot.files);
  }
  __name(isCapturedReviewSnapshot, "isCapturedReviewSnapshot");
  function isFactContentPresentations(value) {
    return isRecord(value) && Object.values(value).every(
      (presentation) => isRecord(presentation) && (presentation.contentType === "text/plain" || presentation.contentType === "text/markdown") && (presentation.bodyContentState === "present" || presentation.bodyContentState === "suppressed_present" || presentation.bodyContentState === "physically_removed") && isFactContent(presentation.content)
    );
  }
  __name(isFactContentPresentations, "isFactContentPresentations");
  function sameFactIds(facts, content) {
    const expected = new Set(facts.map((fact2) => fact2.factId));
    const actual = Object.keys(content);
    return expected.size === facts.length && expected.size === actual.length && actual.every((factId) => expected.has(factId));
  }
  __name(sameFactIds, "sameFactIds");
  function isRevisionResource(value) {
    try {
      decodeRevisionResource(value);
      return true;
    } catch {
      return false;
    }
  }
  __name(isRevisionResource, "isRevisionResource");
  function isFactPortPresentations(value, changeId, facts, selectedRevision) {
    if (!Array.isArray(value) || !value.every(isFactPortPresentation))
      return false;
    if (new Set(value.map((port) => port.portId)).size !== value.length)
      return false;
    return value.every(
      (port) => (port.contextChangeId === void 0 || port.contextChangeId === changeId) && port.sourceEventIds.length > 0 && new Set(port.sourceEventIds).size === port.sourceEventIds.length && port.trackId !== void 0 && (port.applicability !== "applicable" || applicableFactPortHasExactEndpoints(port, facts, selectedRevision))
    );
  }
  __name(isFactPortPresentations, "isFactPortPresentations");
  function factRefId(fact2) {
    return fact2.kind === "observation" ? fact2.observationId ?? "" : fact2.inputRequestId ?? "";
  }
  __name(factRefId, "factRefId");
  function applicableFactPortHasExactEndpoints(port, facts, selectedRevision) {
    if (!sameRevision(port.targetRevision, selectedRevision)) return false;
    const matchingOrigin = facts.filter(
      (fact2) => fact2.factId === factRefId(port.originFact) && fact2.family === port.originFact.kind && sameRevision(fact2.originRevision, port.originRevision) && fact2.presentedInRevision !== void 0 && sameRevision(fact2.presentedInRevision, selectedRevision)
    );
    if (matchingOrigin.length !== 1) return false;
    const targetFact = port.targetFact;
    if (targetFact === void 0) return true;
    return facts.filter(
      (fact2) => fact2.factId === factRefId(targetFact) && fact2.family === targetFact.kind && sameRevision(fact2.originRevision, selectedRevision)
    ).length === 1;
  }
  __name(applicableFactPortHasExactEndpoints, "applicableFactPortHasExactEndpoints");
  function isFactPortPresentation(value) {
    return isRecord(value) && nonEmptyString(value.portId) && isRevisionRef(value.originRevision) && isFactRef(value.originFact) && isRevisionRef(value.targetRevision) && (value.relation === "context_only" || value.relation === "reanchored_as" || value.relation === "carried_open_as" || value.relation === "resolved_by") && (value.targetFact === void 0 || isFactRef(value.targetFact)) && optionalString(value.rationaleContentHash) && optionalString(value.contextChangeId) && nonEmptyString(value.actorId) && nonEmptyString(value.trackId) && isStringArray(value.sourceEventIds) && (value.applicability === "applicable" || value.applicability === "conflicted" || value.applicability === "unavailable") && isStringArray(value.diagnostics);
  }
  __name(isFactPortPresentation, "isFactPortPresentation");
  function isFactRef(value) {
    if (!isRecord(value)) return false;
    if (value.kind === "observation") {
      return nonEmptyString(value.observationId) && value.inputRequestId === void 0;
    }
    if (value.kind === "input_request") {
      return nonEmptyString(value.inputRequestId) && value.observationId === void 0;
    }
    return false;
  }
  __name(isFactRef, "isFactRef");
  function isFactContent(value) {
    if (!isRecord(value)) return false;
    switch (value.kind) {
      case "observation":
        return nonEmptyString(value.title) && optionalString(value.body);
      case "input_request":
        return nonEmptyString(value.title) && optionalString(value.body) && nonEmptyString(value.status) && (value.responses === void 0 || Array.isArray(value.responses) && value.responses.every(isFactResponse));
      case "assessment":
        return nonEmptyString(value.assessment) && optionalString(value.summary);
      case "validation":
        return nonEmptyString(value.checkName) && optionalString(value.command) && nonEmptyString(value.status) && optionalString(value.summary);
      default:
        return false;
    }
  }
  __name(isFactContent, "isFactContent");
  function isFactResponse(value) {
    return isRecord(value) && nonEmptyString(value.responseId) && nonEmptyString(value.outcome) && optionalString(value.reason) && (value.contentType === "text/plain" || value.contentType === "text/markdown") && (value.bodyContentState === "present" || value.bodyContentState === "suppressed_present" || value.bodyContentState === "physically_removed") && isOneOf(value.availability, CONTENT_AVAILABILITY_VALUES);
  }
  __name(isFactResponse, "isFactResponse");
  function isAssociation(value) {
    return isRecord(value) && value.schema === "pointbreak.review-association-comparison" && value.version === 1 && isOneOf(value.state, ASSOCIATION_STATE_VALUES) && isOneOf(value.proofAvailability, ASSOCIATION_PROOF_VALUES) && isRecord(value.comparison) && isRevisionRef(value.comparison.revision) && nonEmptyString(value.comparison.associationId) && nonEmptyString(value.comparison.commitOid) && nonEmptyString(value.comparison.comparisonBase) && nonEmptyString(value.comparison.viewKind) && optionalString(value.comparison.proofRef) && isStringArray(value.diagnostics) && nonEmptyString(value.cacheKey);
  }
  __name(isAssociation, "isAssociation");
  function sameRevision(left, right) {
    return left.revisionId === right.revisionId && left.objectArtifactContentHash === right.objectArtifactContentHash;
  }
  __name(sameRevision, "sameRevision");
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
  function hasExactKeys(value, expected) {
    const actual = Object.keys(value);
    return actual.length === expected.size && actual.every((key) => expected.has(key));
  }
  __name(hasExactKeys, "hasExactKeys");
  function isNonnegativeSafeInteger(value) {
    return Number.isSafeInteger(value) && value >= 0;
  }
  __name(isNonnegativeSafeInteger, "isNonnegativeSafeInteger");
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
  function optionalString(value) {
    return value === void 0 || typeof value === "string";
  }
  __name(optionalString, "optionalString");
  function isStringArray(value) {
    return Array.isArray(value) && value.every((item) => typeof item === "string");
  }
  __name(isStringArray, "isStringArray");

  // src/change-inspector-reading.ts
  function sameExactRevision(left, right) {
    return left.revisionId === right.revisionId && left.objectArtifactContentHash === right.objectArtifactContentHash;
  }
  __name(sameExactRevision, "sameExactRevision");
  function encoded(value) {
    return encodeURIComponent(value);
  }
  __name(encoded, "encoded");
  function revisionPath(changeId, revision2) {
    return `/api/v2/changes/${encoded(changeId)}/revisions/${encoded(revision2.revisionId)}?artifactHash=${encoded(revision2.objectArtifactContentHash)}`;
  }
  __name(revisionPath, "revisionPath");
  function resourcePath(changeId, revision2) {
    return `/api/v2/changes/${encoded(changeId)}/revisions/${encoded(revision2.revisionId)}/resource?artifactHash=${encoded(revision2.objectArtifactContentHash)}`;
  }
  __name(resourcePath, "resourcePath");
  function assertStamp(stamp, expected, surface) {
    if (stamp !== expected) {
      throw new Error(
        `${surface} projection stamp does not match the staged Change generation`
      );
    }
  }
  __name(assertStamp, "assertStamp");
  function assertRevisionDetail(document2, route, stamp) {
    if (document2.changeId !== route.changeId) {
      throw new Error(
        "contextual Revision detail Change ID does not match its exact route"
      );
    }
    if (!sameExactRevision(document2.revision, route.revision)) {
      throw new Error(
        "contextual Revision detail does not match its exact route"
      );
    }
    if (!sameExactRevision(
      document2.exactRevisionDocument.resource.revision,
      route.revision
    )) {
      throw new Error(
        "embedded captured resource does not match its exact Revision route"
      );
    }
    if (document2.factPresentations.some(
      (fact2) => fact2.contextChangeId !== route.changeId || fact2.presentedInRevision !== void 0 && !sameExactRevision(fact2.presentedInRevision, route.revision)
    )) {
      throw new Error(
        "fact presentation does not match its Change and exact Revision context"
      );
    }
    if (document2.factPorts.some(
      (port) => !sameExactRevision(port.targetRevision, route.revision)
    )) {
      throw new Error("fact port does not target the selected exact Revision");
    }
    if (document2.associations.some(
      (association) => !sameExactRevision(association.comparison.revision, route.revision)
    )) {
      throw new Error(
        "association comparison does not target the selected exact Revision"
      );
    }
    if (document2.exactRevisionDocument.projectionStamp !== document2.projectionStamp) {
      throw new Error(
        "embedded captured resource is from another projection stamp"
      );
    }
    assertStamp(document2.projectionStamp, stamp, "contextual Revision detail");
  }
  __name(assertRevisionDetail, "assertRevisionDetail");
  async function loadChangeInspectorReading(route, expectedProjectionStamp) {
    if (route.kind === "change") {
      const document3 = decodeChangeDetail(
        await fetchChangeInspectorJSON(
          `/api/v2/changes/${encoded(route.changeId)}`
        )
      );
      if (document3.summary.changeId !== route.changeId) {
        throw new Error("Change detail does not match its route");
      }
      assertStamp(
        document3.projectionStamp,
        expectedProjectionStamp,
        "Change detail"
      );
      return { kind: "change", document: document3 };
    }
    if (route.kind === "revision" || route.kind === "diff" || route.kind === "association") {
      const document3 = decodeChangeRevisionDetail(
        await fetchChangeInspectorJSON(
          revisionPath(route.changeId, route.revision)
        )
      );
      assertRevisionDetail(document3, route, expectedProjectionStamp);
      return { kind: route.kind, document: document3 };
    }
    if (route.kind === "resource") {
      const document3 = decodeRevisionResource(
        await fetchChangeInspectorJSON(
          resourcePath(route.changeId, route.revision)
        )
      );
      if (!sameExactRevision(document3.resource.revision, route.revision)) {
        throw new Error(
          "captured resource does not match its exact Revision route"
        );
      }
      assertStamp(
        document3.projectionStamp,
        expectedProjectionStamp,
        "captured resource"
      );
      return { kind: "resource", document: document3 };
    }
    const params = new URLSearchParams({
      fromArtifactHash: route.from.objectArtifactContentHash,
      toArtifactHash: route.to.objectArtifactContentHash
    });
    const document2 = decodeRevisionInterdiff(
      await fetchChangeInspectorJSON(
        `/api/v2/changes/${encoded(route.changeId)}/interdiff/${encoded(route.from.revisionId)}/${encoded(route.to.revisionId)}?${params}`
      )
    );
    if (!sameExactRevision(document2.interdiff.from, route.from) || !sameExactRevision(document2.interdiff.to, route.to)) {
      throw new Error(
        "ordered Revision interdiff does not match its exact route"
      );
    }
    assertStamp(
      document2.projectionStamp,
      expectedProjectionStamp,
      "Revision interdiff"
    );
    return { kind: "interdiff", document: document2 };
  }
  __name(loadChangeInspectorReading, "loadChangeInspectorReading");

  // src/change-inspector-cards.ts
  function words2(value) {
    return value.replaceAll("_", " ");
  }
  __name(words2, "words");
  function exactRevisionCopyText(revisions) {
    return revisions.map(
      (revision2) => `${revision2.revisionId} ${revision2.objectArtifactContentHash}`
    ).join("\n");
  }
  __name(exactRevisionCopyText, "exactRevisionCopyText");
  function attentionReasonCopy(reason) {
    switch (reason.kind) {
      case "conflicted":
        return {
          kind: reason.kind,
          reason: "Conflicting Change state",
          ask: "Resolve the conflicting Change state.",
          actionLabel: "Review conflict",
          accessibleName: "Conflicting Change state. Resolve the conflicting Change state.",
          title: "Conflicting Change state",
          copyText: "conflicted"
        };
      case "incomplete":
        return {
          kind: reason.kind,
          reason: "Incomplete Change state",
          ask: "Complete the missing Change state.",
          actionLabel: "Review incomplete Change",
          accessibleName: "Incomplete Change state. Complete the missing Change state.",
          title: "Incomplete Change state",
          copyText: "incomplete"
        };
      case "no_current_revision":
        return {
          kind: reason.kind,
          reason: "No current Revision",
          ask: "Establish one exact current Revision before review can continue.",
          actionLabel: "Review Change",
          accessibleName: "No current Revision. Establish one exact current Revision before review can continue.",
          title: "No current Revision",
          copyText: "no_current_revision"
        };
      case "unresolved_operative_requests": {
        const visibleRequestIds = reason.requestIds.map(shortRef);
        const requestList = visibleRequestIds.join(", ");
        const fullRequestList = reason.requestIds.join(", ");
        return {
          kind: reason.kind,
          reason: "Unresolved operative requests",
          ask: `Respond to operative requests: ${requestList}.`,
          actionLabel: "Respond to requests",
          accessibleName: `Unresolved operative requests. Respond to operative requests: ${fullRequestList}.`,
          title: `Operative requests: ${fullRequestList}`,
          copyText: reason.requestIds.join("\n")
        };
      }
      case "current_revisions_need_assessment": {
        const visibleRevisions = reason.revisions.map(shortExactRevision).join(", ");
        const fullRevisions = reason.revisions.map(exactRevisionAccessibleIdentity).join("; ");
        return {
          kind: reason.kind,
          reason: "Current Revisions need assessment",
          ask: `Assess current Revisions: ${visibleRevisions}.`,
          actionLabel: "Assess current Revisions",
          accessibleName: `Current Revisions need assessment. Assess ${fullRevisions}.`,
          title: fullRevisions,
          copyText: exactRevisionCopyText(reason.revisions)
        };
      }
    }
  }
  __name(attentionReasonCopy, "attentionReasonCopy");
  function attentionPresentation(attention) {
    if (attention === void 0) return void 0;
    const primary = attentionReasonCopy(attention.primaryReason);
    return {
      primary,
      reason: primary.reason,
      ask: primary.ask,
      actionLabel: primary.actionLabel,
      additionalReasons: attention.reasons.slice(1).map(attentionReasonCopy),
      ...attention.diagnostics === void 0 ? {} : { diagnostics: attention.diagnostics }
    };
  }
  __name(attentionPresentation, "attentionPresentation");
  function changeCardPresentation(summary, presentation) {
    const byExactIdentity = new Map(
      (presentation?.currentRevisions ?? []).map((entry) => [
        `${entry.revision.revisionId}\0${entry.revision.objectArtifactContentHash}`,
        entry
      ])
    );
    const peers = summary.currentRevisionRefs.map((revision2) => {
      const entry = byExactIdentity.get(
        `${revision2.revisionId}\0${revision2.objectArtifactContentHash}`
      );
      const summaryLabel = entry?.summarySource === "revision_proposal_summary" ? entry.revisionProposalSummary : void 0;
      const identity = exactRevisionAccessibleIdentity(revision2);
      return {
        revision: revision2,
        label: summaryLabel || "Current Revision",
        visibleIdentity: shortExactRevision(revision2),
        accessibleName: summaryLabel ? `Current Revision — ${summaryLabel}; ${identity}` : `Current Revision — ${identity}`,
        title: identity,
        copyText: exactRevisionCopyText([revision2])
      };
    });
    const onlyPeer = peers.length === 1 ? peers[0] : void 0;
    const headline = onlyPeer === void 0 ? peers.length === 0 ? "Current Revision unavailable" : "Multiple current Revisions need selection" : onlyPeer.label;
    const currentRevisionName = peers.length === 0 ? "Current Revision unavailable" : peers.length === 1 ? peers[0].accessibleName : `Current Revisions — ${peers.map(
      (peer) => peer.accessibleName.replace(/^Current Revision — /, "")
    ).join("; ")}`;
    const unavailableReason = peers.length === 0 ? "No exact current Revision is available for this Change." : void 0;
    return {
      changeId: summary.changeId,
      visibleChangeId: shortRef(summary.changeId),
      accessibleName: `${headline}; ${currentRevisionName}; Change ${summary.changeId}`,
      title: `Change ${summary.changeId}`,
      copyText: summary.changeId,
      headline,
      stateAxes: [
        { label: "Topology", value: words2(summary.topology) },
        { label: "Lifecycle", value: words2(summary.lifecycle) },
        { label: "Attention", value: words2(summary.attentionSummary) },
        { label: "Availability", value: words2(summary.availabilitySummary) }
      ],
      peers,
      ...unavailableReason === void 0 ? {} : { unavailableReason },
      primaryAction: {
        kind: "open_change",
        label: peers.length > 1 ? "Review current Revisions" : "Review Change"
      },
      ...presentation?.attention === void 0 ? {} : { attention: attentionPresentation(presentation.attention) }
    };
  }
  __name(changeCardPresentation, "changeCardPresentation");

  // src/format.ts
  var RFC3339_UTC = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:\.(\d+))?Z$/;
  function parseRfc3339UtcMillis(value) {
    const match = value.match(RFC3339_UTC);
    if (!match) return null;
    const [
      ,
      yearText,
      monthText,
      dayText,
      hourText,
      minuteText,
      secondText,
      fraction
    ] = match;
    const year = Number(yearText);
    const month = Number(monthText);
    const day = Number(dayText);
    const hour = Number(hourText);
    const minute = Number(minuteText);
    const second = Number(secondText);
    const leapYear = year % 4 === 0 && year % 100 !== 0 || year % 400 === 0;
    const daysInMonth = [
      31,
      leapYear ? 29 : 28,
      31,
      30,
      31,
      30,
      31,
      31,
      30,
      31,
      30,
      31
    ];
    if (month < 1 || month > 12 || day < 1 || day > daysInMonth[month - 1] || hour > 23 || minute > 59 || second > 60) {
      return null;
    }
    const millis = Number((fraction ?? "").padEnd(3, "0").slice(0, 3));
    const date = /* @__PURE__ */ new Date(0);
    date.setUTCFullYear(year, month - 1, day);
    date.setUTCHours(hour, minute, Math.min(second, 59), millis);
    return date.getTime() + (second === 60 ? 1e3 : 0);
  }
  __name(parseRfc3339UtcMillis, "parseRfc3339UtcMillis");
  function parseMs(occurredAt) {
    if (typeof occurredAt !== "string") return null;
    if (occurredAt.startsWith("unix-ms:")) {
      const unixMillis = occurredAt.match(/^unix-ms:([+-]?\d+)$/);
      return unixMillis ? Number(unixMillis[1]) : null;
    }
    if (/^\d{4}-\d{2}-\d{2}T/.test(occurredAt))
      return parseRfc3339UtcMillis(occurredAt);
    const match = occurredAt.match(/(\d+)\s*$/);
    return match ? Number(match[1]) : null;
  }
  __name(parseMs, "parseMs");
  function fmtDateTime(occurredAt) {
    const ms = parseMs(occurredAt);
    if (ms == null) return occurredAt || "";
    return new Date(ms).toLocaleString([], { hour12: false });
  }
  __name(fmtDateTime, "fmtDateTime");

  // src/markdown.ts
  function renderBodyContent(text, contentType) {
    if (!text) return "";
    const cls = bodyClass("anno-body", isMarkdownContentType(contentType));
    return `<div class="${cls}">${renderContentHtml(text, contentType)}</div>`;
  }
  __name(renderBodyContent, "renderBodyContent");
  function renderContentHtml(text, contentType) {
    return isMarkdownContentType(contentType) ? renderMarkdown(text) : linkify(text);
  }
  __name(renderContentHtml, "renderContentHtml");
  function renderMarkdown(text) {
    const lines = String(text ?? "").replace(/\r\n?/g, "\n").split("\n");
    const out = [];
    let paragraph = [];
    let listKind = null;
    let listItems = [];
    const flushParagraph = /* @__PURE__ */ __name(() => {
      if (!paragraph.length) return;
      out.push(`<p>${renderMarkdownInline(paragraph.join(" "))}</p>`);
      paragraph = [];
    }, "flushParagraph");
    const flushList = /* @__PURE__ */ __name(() => {
      if (!listKind) return;
      out.push(
        `<${listKind}>${listItems.map((item) => `<li>${renderMarkdownInline(item)}</li>`).join("")}</${listKind}>`
      );
      listKind = null;
      listItems = [];
    }, "flushList");
    const flushBlocks = /* @__PURE__ */ __name(() => {
      flushParagraph();
      flushList();
    }, "flushBlocks");
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      const fence = line.match(/^\s*```/);
      if (fence) {
        flushBlocks();
        const code = [];
        i++;
        while (i < lines.length && !/^\s*```/.test(lines[i])) {
          code.push(lines[i]);
          i++;
        }
        out.push(`<pre><code>${escapeHtml(code.join("\n"))}</code></pre>`);
        continue;
      }
      if (!line.trim()) {
        flushBlocks();
        continue;
      }
      const heading = line.match(/^(#{1,6})\s+(.+)$/);
      if (heading) {
        flushBlocks();
        const level = heading[1].length;
        out.push(
          `<h${level}>${renderMarkdownInline(heading[2].trim())}</h${level}>`
        );
        continue;
      }
      const unordered = line.match(/^\s*[-*]\s+(.+)$/);
      if (unordered) {
        flushParagraph();
        if (listKind && listKind !== "ul") flushList();
        listKind = "ul";
        listItems.push(unordered[1]);
        continue;
      }
      const ordered = line.match(/^\s*\d+[.)]\s+(.+)$/);
      if (ordered) {
        flushParagraph();
        if (listKind && listKind !== "ol") flushList();
        listKind = "ol";
        listItems.push(ordered[1]);
        continue;
      }
      if (listKind) flushList();
      paragraph.push(line.trim());
    }
    flushBlocks();
    return out.join("");
  }
  __name(renderMarkdown, "renderMarkdown");
  function renderMarkdownInline(text) {
    const placeholders = [];
    const stash = /* @__PURE__ */ __name((html2) => {
      const token = `\0MD${placeholders.length}\0`;
      placeholders.push([token, html2]);
      return token;
    }, "stash");
    let html = escapeHtml(String(text ?? ""));
    html = protectBackslashEscapes(html, stash, (character) => character === "`");
    html = html.replace(
      /`([^`]+)`/g,
      (_, code) => stash(`<code>${code}</code>`)
    );
    html = protectBackslashEscapes(html, stash);
    html = html.replace(
      /\[([^\]]+)\]\(([^)\s]+)\)/g,
      (_, label2, href) => {
        const safe = safeMarkdownHref(href);
        const labelHtml = renderMarkdownInline(label2);
        return safe ? stash(
          `<a href="${safe}" target="_blank" rel="noreferrer">${labelHtml}</a>`
        ) : labelHtml;
      }
    );
    html = html.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>").replace(/\*([^*]+)\*/g, "<em>$1</em>");
    html = linkifyEscaped(html);
    for (const [token, replacement] of placeholders.reverse()) {
      html = html.split(token).join(replacement);
    }
    return html;
  }
  __name(renderMarkdownInline, "renderMarkdownInline");
  function protectBackslashEscapes(html, stash, shouldProtect = isAsciiPunctuation) {
    let protectedHtml = "";
    for (let index = 0; index < html.length; index++) {
      const character = html[index];
      const escaped = html[index + 1];
      if (character === "\\" && escaped && shouldProtect(escaped)) {
        protectedHtml += stash(escaped);
        index++;
      } else {
        protectedHtml += character;
      }
    }
    return protectedHtml;
  }
  __name(protectBackslashEscapes, "protectBackslashEscapes");
  function isAsciiPunctuation(character) {
    const codePoint = character.codePointAt(0) ?? 0;
    return codePoint >= 33 && codePoint <= 47 || codePoint >= 58 && codePoint <= 64 || codePoint >= 91 && codePoint <= 96 || codePoint >= 123 && codePoint <= 126;
  }
  __name(isAsciiPunctuation, "isAsciiPunctuation");

  // src/types.ts
  var TYPES = [
    { id: "review_initialized", label: "init", color: "var(--evt-init)" },
    { id: "work_object_proposed", label: "capture", color: "var(--evt-capture)" },
    {
      id: "review_observation_recorded",
      label: "observation",
      color: "var(--evt-observation)"
    },
    {
      id: "review_assessment_recorded",
      label: "assessment",
      color: "var(--evt-assessment)"
    },
    { id: "input_request_opened", label: "request", color: "var(--evt-request)" },
    {
      id: "input_request_responded",
      label: "response",
      color: "var(--evt-response)"
    },
    { id: "review_note_imported", label: "note", color: "var(--evt-note)" },
    {
      id: "validation_check_recorded",
      label: "validation",
      color: "var(--evt-validation)"
    }
  ];
  var TYPE_MAP = Object.fromEntries(TYPES.map((type) => [type.id, type]));
  var VERIFICATION_LABELS = {
    valid: "signature valid",
    invalid: "signature invalid",
    untrusted_key: "untrusted key",
    unsigned: "unsigned"
  };
  var ENDORSEMENT_LABELS = {
    "endorsement-trusted": "trusted endorsement",
    unknown_endorser: "unknown endorser",
    ambiguous_endorser: "ambiguous endorser"
  };
  var ASSESSMENT_LABELS = {
    accepted: "accepted",
    accepted_with_follow_up: "accepted-with-follow-up",
    needs_changes: "needs-changes",
    needs_clarification: "needs-clarification"
  };
  var REVISION_ATTENTION_VALUES = [
    "open-request",
    "unassessed",
    "validation-context",
    "follow-up",
    "stale-fact"
  ];
  var DEFAULT_OPEN_FILES = 10;
  var LARGE_FILE_ROWS = 500;

  // src/query.ts
  function tokenizeQuery(q) {
    const out = [];
    const re = /-?(?:[a-z]+:)?"[^"]*"|\S+/gi;
    let m = re.exec(q);
    while (m !== null) {
      out.push(m[0]);
      m = re.exec(q);
    }
    return out;
  }
  __name(tokenizeQuery, "tokenizeQuery");

  // src/projection.ts
  function verificationChip(status) {
    if (!status) return "";
    const label2 = VERIFICATION_LABELS[status] || status;
    return `<span class="${verifyClass(escapeHtml(status))}" title="advisory signature readback — reader-relative, never gates a write">${escapeHtml(label2)}</span>`;
  }
  __name(verificationChip, "verificationChip");
  function endorserDisplay(actorId) {
    return actorId.replace(/^actor:git-(email|name):/, "");
  }
  __name(endorserDisplay, "endorserDisplay");
  function endorsementRow(en) {
    const cls = en.classification || "";
    const label2 = ENDORSEMENT_LABELS[cls] || cls;
    const parts = [
      `<span class="${CLASS.endorseLabel}">${escapeHtml(label2)}</span>`
    ];
    if (en.endorser) {
      parts.push(
        `<span class="${CLASS.endorseWho}">${escapeHtml(endorserDisplay(en.endorser))}</span>`
      );
    }
    const attrs = en.endorserAttributes || {};
    const attrBits = [];
    if (attrs.kind) attrBits.push(attrs.kind);
    const roles = attrs.roles || [];
    if (roles.length) attrBits.push(roles.join(", "));
    if (attrBits.length) {
      parts.push(
        `<span class="${CLASS.endorseAttrs}">${escapeHtml(attrBits.join(" · "))}</span>`
      );
    }
    return `<li class="${endorseClass(escapeHtml(cls))}">${parts.join(" ")}</li>`;
  }
  __name(endorsementRow, "endorsementRow");
  function endorsementsBlock(endorsements) {
    const list = endorsements || [];
    if (!list.length) return "";
    const rows = list.map(endorsementRow).join("");
    return `<div class="${CLASS.endorsements}" title="advisory endorsement readback — reader-relative, never gates a write">
    <span class="${CLASS.endorsementsLabel}">endorsements</span>
    <ul class="${CLASS.endorseList}">${rows}</ul>
  </div>`;
  }
  __name(endorsementsBlock, "endorsementsBlock");
  function assessmentDisplayLabel(value) {
    return ASSESSMENT_LABELS[value] || value || "";
  }
  __name(assessmentDisplayLabel, "assessmentDisplayLabel");
  var [
    ATTENTION_OPEN_REQUEST,
    ATTENTION_UNASSESSED,
    ATTENTION_VALIDATION_CONTEXT,
    ATTENTION_FOLLOW_UP,
    ATTENTION_STALE_FACT
  ] = REVISION_ATTENTION_VALUES;

  // src/cards.ts
  var VALIDATION_DISPOSITION_LABELS = {
    outstanding: "outstanding",
    current: "current result",
    resolved_by_later_pass: "resolved by strictly later pass",
    historical: "historical",
    skipped: "skipped"
  };
  function renderActorAttribution(label2, writer) {
    const actorId = writer?.actorId ?? "";
    if (!actorId) return "";
    return `<span class="${CLASS.actorAttribution}">${label2} ${actorChip(actorId)}</span>`;
  }
  __name(renderActorAttribution, "renderActorAttribution");
  function renderRecordedTime(createdAt) {
    if (!createdAt) return "";
    return `<span class="${CLASS.annoTime}" title="${escapeHtml(createdAt)}">${escapeHtml(fmtDateTime(createdAt))}</span>`;
  }
  __name(renderRecordedTime, "renderRecordedTime");
  function targetLabel(t) {
    const tt = t ?? {};
    switch (tt.kind) {
      case "range":
        return `${escapeHtml(tt.filePath)}:${tt.startLine}-${tt.endLine ?? tt.startLine} (${escapeHtml(tt.side || "new")})`;
      case "file":
        return escapeHtml(tt.filePath || "");
      case "revision":
        return "whole revision";
      case "observation":
        return `→ ${linkify(tt.observationId)}`;
      case "input_request":
        return `→ ${linkify(tt.inputRequestId)}`;
      case "assessment":
        return `→ ${linkify(tt.assessmentId)}`;
      case "event":
        return `→ ${linkify(tt.eventId)}`;
      default:
        return escapeHtml(tt.kind || "");
    }
  }
  __name(targetLabel, "targetLabel");
  function removedBodyCue(state) {
    if (state !== "suppressed_present" && state !== "physically_removed") {
      return null;
    }
    const title = state === "suppressed_present" ? "removal recorded; bytes still stored until compact" : "removed; bytes swept from the store";
    return `<div class="${CLASS.factBodyRemoved}" title="${title}">content removed</div>`;
  }
  __name(removedBodyCue, "removedBodyCue");
  function factCard(kind, opts) {
    const tags = (opts.tags || []).filter(Boolean).map((t) => `<span class="${CLASS.badge}">${escapeHtml(t)}</span>`).join(" ");
    const body = removedBodyCue(opts.bodyContentState) ?? renderBodyContent(opts.body, opts.bodyContentType);
    const annotationId = opts.annotationId ? ` data-anno="${escapeHtml(opts.annotationId)}"` : "";
    return `<div class="${annoContainerClass(kind)}"${annotationId}>
    <div class="${CLASS.annoHead}">
      <span class="${annoKindClass(kind)}">${kind}</span>
      <span class="${CLASS.annoTrack}">${escapeHtml(opts.track || "")}</span>
      ${renderActorAttribution("writer", opts.writer)}
      <span class="${CLASS.annoTitle}">${linkify(opts.title || "")}</span>
      ${opts.status ? `<span class="${factStatusClass(escapeHtml(opts.status))}">${escapeHtml(opts.status)}</span>` : ""}
      ${opts.target ? `<span class="${CLASS.annoLoc}">${opts.target}</span>` : ""}
      ${tags}
      ${opts.verify || ""}
      ${renderRecordedTime(opts.createdAt)}
    </div>
    ${body}
    ${opts.endorsements || ""}
    ${opts.extra || ""}</div>`;
  }
  __name(factCard, "factCard");
  function renderObservationCard(o) {
    const supersedes = o.supersedes ?? [];
    const extra = supersedes.length ? `<div class="${CLASS.factRel}">supersedes ${supersedes.map(linkify).join(", ")}</div>` : "";
    return factCard("observation", {
      annotationId: o.id,
      track: o.trackId,
      title: o.title,
      status: o.status,
      target: targetLabel(o.target),
      tags: o.tags,
      body: o.body,
      bodyContentType: o.bodyContentType,
      bodyContentState: o.bodyContentState,
      createdAt: o.createdAt,
      verify: verificationChip(o.verificationStatus ?? ""),
      endorsements: endorsementsBlock(o.endorsements),
      writer: o.writer,
      extra
    });
  }
  __name(renderObservationCard, "renderObservationCard");
  function renderInputRequestResponse(r) {
    const reason = removedBodyCue(r.reasonContentState) ?? (r.reason ? renderBodyContent(r.reason, r.reasonContentType) : "");
    return `<div class="${CLASS.factResponse}">
    <div class="${CLASS.annoHead}">
      <span class="${CLASS.outcome}">${escapeHtml(r.outcome)}</span>
      ${r.id ? `<span class="${CLASS.annoLoc}">${linkify(r.id)}</span>` : ""}
      ${renderActorAttribution("answered by", r.writer)}
      ${verificationChip(r.verificationStatus ?? "")}
      ${renderRecordedTime(r.createdAt)}
    </div>
    ${reason}
    ${endorsementsBlock(r.endorsements)}
  </div>`;
  }
  __name(renderInputRequestResponse, "renderInputRequestResponse");
  function renderInputRequestCard(ir) {
    const responses = (ir.responses ?? []).map(renderInputRequestResponse).join("");
    return factCard("input-request", {
      annotationId: ir.id,
      track: ir.trackId,
      title: ir.title,
      status: ir.status,
      target: targetLabel(ir.target),
      tags: [ir.mode, ir.reasonCode],
      body: ir.body,
      bodyContentType: ir.bodyContentType,
      bodyContentState: ir.bodyContentState,
      createdAt: ir.createdAt,
      verify: verificationChip(ir.verificationStatus ?? ""),
      endorsements: endorsementsBlock(ir.endorsements),
      writer: ir.writer,
      extra: responses ? `<div class="${CLASS.factResponses}">${responses}</div>` : ""
    });
  }
  __name(renderInputRequestCard, "renderInputRequestCard");
  function renderAssessmentCard(a) {
    const rel = [];
    const replaces = a.replaces ?? [];
    const relatedObservations = a.relatedObservations ?? [];
    const relatedInputRequests = a.relatedInputRequests ?? [];
    if (replaces.length) rel.push(`replaces ${replaces.map(linkify).join(", ")}`);
    if (relatedObservations.length) {
      rel.push(`re ${relatedObservations.map(linkify).join(", ")}`);
    }
    if (relatedInputRequests.length) {
      rel.push(`re ${relatedInputRequests.map(linkify).join(", ")}`);
    }
    return factCard("assessment", {
      annotationId: a.id,
      track: a.trackId,
      title: assessmentDisplayLabel(a.assessment ?? ""),
      status: a.status,
      target: targetLabel(a.target),
      body: a.summary,
      bodyContentType: a.summaryContentType,
      bodyContentState: a.summaryContentState,
      createdAt: a.createdAt,
      verify: verificationChip(a.verificationStatus ?? ""),
      endorsements: endorsementsBlock(a.endorsements),
      writer: a.writer,
      extra: rel.length ? `<div class="${CLASS.factRel}">${rel.join(" · ")}</div>` : ""
    });
  }
  __name(renderAssessmentCard, "renderAssessmentCard");
  function renderValidationCheckCard(v, disposition) {
    const rel = [];
    const logs = v.logArtifactContentHashes ?? [];
    if (v.command) rel.push(escapeHtml(v.command));
    if (logs.length) rel.push(`logs ${logs.map(linkify).join(", ")}`);
    const continuity = disposition ? `<div class="${CLASS.validationContinuity} ${disposition === "outstanding" ? CLASS.validationContinuityOutstanding : CLASS.validationContinuityNeutral}" title="server-projected validation continuity">${escapeHtml(VALIDATION_DISPOSITION_LABELS[disposition])}</div>` : "";
    const related = rel.length ? `<div class="${CLASS.factRel}">${rel.join(" · ")}</div>` : "";
    return factCard("validation", {
      annotationId: v.id,
      track: v.trackId,
      title: v.checkName,
      status: v.status,
      // passed | failed | errored | skipped → .fact-status.<status>
      target: targetLabel(v.target),
      tags: [v.trigger, v.exitCode != null ? `exit ${v.exitCode}` : null],
      body: v.summary || "",
      bodyContentType: v.summaryContentType,
      bodyContentState: v.summaryContentState,
      createdAt: v.completedAt || v.createdAt,
      verify: verificationChip(v.verificationStatus ?? ""),
      endorsements: endorsementsBlock(v.endorsements),
      writer: v.writer,
      extra: continuity + related
    });
  }
  __name(renderValidationCheckCard, "renderValidationCheckCard");

  // src/diff/highlight.ts
  function validChannel(spans, len) {
    let cursor = 0;
    for (const span of spans) {
      if (!Number.isInteger(span.start) || !Number.isInteger(span.end) || span.start < cursor || span.end < span.start || span.end > len) {
        return false;
      }
      cursor = span.end;
    }
    return true;
  }
  __name(validChannel, "validChannel");
  function segClass(kind, isEmph) {
    const parts = [
      kind ? tokClass(kind) : null,
      isEmph ? CLASS.emph : null
    ].filter(Boolean);
    return parts.length > 0 ? parts.join(" ") : null;
  }
  __name(segClass, "segClass");
  function highlightRowText(text, tokens, emphasis) {
    const toks = tokens && validChannel(tokens, text.length) ? tokens : [];
    const emph = emphasis && validChannel(emphasis, text.length) ? emphasis : [];
    if (toks.length === 0 && emph.length === 0) return escapeHtml(text);
    const points = [
      .../* @__PURE__ */ new Set([
        0,
        text.length,
        ...toks.flatMap((t) => [t.start, t.end]),
        ...emph.flatMap((e) => [e.start, e.end])
      ])
    ].sort((a, b) => a - b);
    let out = "";
    for (let i = 0; i + 1 < points.length; i++) {
      const a = points[i];
      const b = points[i + 1];
      if (a >= b) continue;
      const seg = escapeHtml(text.slice(a, b));
      const kind = toks.find((t) => t.start <= a && a < t.end)?.kind;
      const isEmph = emph.some((e) => e.start <= a && a < e.end);
      const cls = segClass(kind, isEmph);
      out += cls ? `<span class="${cls}">${seg}</span>` : seg;
    }
    return out;
  }
  __name(highlightRowText, "highlightRowText");

  // src/diff/render.ts
  function filePathLabel(f) {
    const oldp = f.old_path;
    const newp = f.new_path;
    return oldp && newp && oldp !== newp ? `${oldp} → ${newp}` : newp || oldp || "(unknown path)";
  }
  __name(filePathLabel, "filePathLabel");
  function fileRowCount(f) {
    return (f.hunks ?? []).reduce((n, h) => n + (h.rows ? h.rows.length : 0), 0);
  }
  __name(fileRowCount, "fileRowCount");
  function classifyLowSignal(f) {
    if (f.is_binary) return "binary";
    if (f.is_mode_only) return "mode change only";
    const hunks = f.hunks ?? [];
    const renamed = f.status === "renamed" || !!f.old_path && !!f.new_path && f.old_path !== f.new_path;
    if (renamed && !hunks.length) {
      return f.similarity != null ? `rename ${f.similarity}%` : "rename";
    }
    if (fileRowCount(f) > LARGE_FILE_ROWS) return "large file";
    return null;
  }
  __name(classifyLowSignal, "classifyLowSignal");
  function fileFactCount(f, anchored) {
    const oldp = f.old_path;
    const newp = f.new_path;
    let n = 0;
    for (const a of anchored) {
      const p = a.target?.filePath;
      if (p === newp || p === oldp) n += 1;
    }
    return n;
  }
  __name(fileFactCount, "fileFactCount");
  function fileForFact(files, filePath) {
    return files.find((f) => f.new_path === filePath || f.old_path === filePath) ?? null;
  }
  __name(fileForFact, "fileForFact");
  function rangeTouchesCapturedRows(a, file) {
    if (!file) return false;
    const t = a.target ?? {};
    if (t.kind !== "range" || t.startLine == null) return true;
    const start = t.startLine;
    const side = t.side === "old" ? "old" : "new";
    const end = t.endLine ?? start;
    for (const h of file.hunks ?? []) {
      for (const r of h.rows ?? []) {
        const line = side === "old" ? r.old_line : r.new_line;
        if (line != null && line >= start && line <= end) return true;
      }
    }
    return false;
  }
  __name(rangeTouchesCapturedRows, "rangeTouchesCapturedRows");
  function renderAnnotation(a, showLocation) {
    const target = showLocation && a.target?.filePath ? a.target : void 0;
    switch (a.kind) {
      case "observation":
        return renderObservationCard({
          id: a.id,
          trackId: a.track,
          title: a.title,
          status: a.status,
          target,
          tags: a.tags,
          body: a.body,
          bodyContentType: a.bodyContentType,
          bodyContentState: a.bodyContentState,
          createdAt: a.createdAt,
          verificationStatus: a.verificationStatus,
          endorsements: a.endorsements,
          supersedes: a.supersedes,
          writer: a.writer
        });
      case "input-request":
        return renderInputRequestCard({
          id: a.id,
          trackId: a.track,
          title: a.title,
          status: a.status,
          target,
          mode: a.mode,
          reasonCode: a.reasonCode,
          body: a.body,
          bodyContentType: a.bodyContentType,
          bodyContentState: a.bodyContentState,
          createdAt: a.createdAt,
          verificationStatus: a.verificationStatus,
          endorsements: a.endorsements,
          responses: a.responses,
          writer: a.writer
        });
      case "assessment":
        return renderAssessmentCard({
          id: a.id,
          trackId: a.track,
          assessment: a.assessment ?? a.title.replace(/^assessment:\s*/, "").trim(),
          status: a.status,
          target,
          summary: a.body,
          summaryContentType: a.bodyContentType,
          summaryContentState: a.bodyContentState,
          createdAt: a.createdAt,
          verificationStatus: a.verificationStatus,
          endorsements: a.endorsements,
          replaces: a.replaces,
          relatedObservations: a.relatedObservations,
          relatedInputRequests: a.relatedInputRequests,
          writer: a.writer
        });
      case "validation":
        return renderValidationCheckCard(
          {
            id: a.id,
            trackId: a.track,
            checkName: a.title,
            status: a.status,
            target,
            trigger: a.trigger,
            exitCode: a.exitCode,
            summary: a.body,
            summaryContentType: a.bodyContentType,
            summaryContentState: a.bodyContentState,
            completedAt: a.completedAt,
            createdAt: a.createdAt,
            verificationStatus: a.verificationStatus,
            endorsements: a.endorsements,
            command: a.command,
            logArtifactContentHashes: a.logArtifactContentHashes,
            writer: a.writer
          },
          a.continuity
        );
      default:
        return factCard(a.kind, {
          annotationId: a.id,
          track: a.track,
          title: a.title,
          status: a.status,
          body: a.body,
          bodyContentType: a.bodyContentType,
          bodyContentState: a.bodyContentState,
          createdAt: a.createdAt,
          writer: a.writer,
          tags: a.tags
        });
    }
  }
  __name(renderAnnotation, "renderAnnotation");
  function renderDiffFactVicinity(f, anchored) {
    const facts = anchored.filter((a) => {
      const p = a.target?.filePath;
      return p === f.new_path || p === f.old_path;
    });
    return `<div class="${CLASS.diffFactVicinity}" data-fact-vicinity="true">
    <p>Large annotated file: showing review facts first.</p>
    <button type="button" data-render-diff-file="true">Render all rows</button>
    ${facts.map((a) => renderAnnotation(a, true)).join("")}
  </div>`;
  }
  __name(renderDiffFactVicinity, "renderDiffFactVicinity");
  function renderDiffFileHeader(f, anchored, reason, open) {
    const n = fileFactCount(f, anchored);
    const summary = reason ? `<span class="${CLASS.dfileSummary}">${escapeHtml(reason)}</span>` : "";
    return `<header class="${CLASS.dfileHead}" role="button" tabindex="0" aria-expanded="${open}">
    <span class="${diffStatusClass(escapeHtml(f.status))}">${escapeHtml(f.status)}</span>
    <span class="${CLASS.dpath}">${escapeHtml(filePathLabel(f))}</span>${summary}
    ${n ? `<span class="${CLASS.dfileNotes}">${n} note${n === 1 ? "" : "s"}</span>` : ""}</header>`;
  }
  __name(renderDiffFileHeader, "renderDiffFileHeader");
  function renderDiffFileBody(f, anchored) {
    const oldp = f.old_path;
    const newp = f.new_path;
    const fileFacts = anchored.filter((a) => {
      const p = a.target?.filePath;
      return p === newp || p === oldp;
    });
    const rangeFacts = fileFacts.filter((a) => a.target?.kind === "range");
    const fileLevelFacts = fileFacts.filter((a) => a.target?.kind === "file");
    const emitted = /* @__PURE__ */ new Set();
    let html = "";
    for (const a of fileLevelFacts) {
      html += renderAnnotation(a, false);
      emitted.add(a.id);
    }
    for (const m of f.metadata_rows ?? []) {
      html += `<div class="${CLASS.drow} ${CLASS.drowMeta}"><span class="${CLASS.dtext}">${escapeHtml(m.text)}</span></div>`;
    }
    const factsByLine = /* @__PURE__ */ new Map();
    for (const a of rangeFacts) {
      const t = a.target ?? {};
      if (t.startLine == null) continue;
      const start = t.startLine;
      const side = t.side === "old" ? "old" : "new";
      const end = t.endLine ?? start;
      for (let line = start; line <= end; line++) {
        const key = `${side}:${line}`;
        const bucket = factsByLine.get(key);
        if (bucket) bucket.push(a);
        else factsByLine.set(key, [a]);
      }
    }
    const hunks = f.hunks ?? [];
    for (const h of hunks) {
      html += `<div class="${CLASS.dhunk}">${escapeHtml(h.header)}</div>`;
      for (const r of h.rows ?? []) {
        const matching = [];
        const seen = /* @__PURE__ */ new Set();
        const collect = /* @__PURE__ */ __name((key) => {
          const bucket = factsByLine.get(key);
          if (!bucket) return;
          for (const a of bucket) {
            if (!seen.has(a.id)) {
              seen.add(a.id);
              matching.push(a);
            }
          }
        }, "collect");
        if (r.old_line != null) collect(`old:${r.old_line}`);
        if (r.new_line != null) collect(`new:${r.new_line}`);
        const sign = r.kind === "added" ? "+" : r.kind === "removed" ? "-" : " ";
        const noted = matching.length > 0;
        const notedAttrs = noted ? ` data-anno="${escapeHtml(matching[0].id)}" tabindex="0" role="button"` : "";
        html += `<div class="${drowClass(escapeHtml(r.kind), noted)}"${notedAttrs}>
        <span class="${CLASS.ln}">${r.old_line ?? ""}</span>
        <span class="${CLASS.ln}">${r.new_line ?? ""}</span>
        <span class="${CLASS.sign}">${sign}</span>
        <span class="${CLASS.dtext}">${highlightRowText(r.text, r.tokens, r.emphasis)}</span></div>`;
        for (const a of matching) {
          if (!emitted.has(a.id)) {
            html += renderAnnotation(a, false);
            emitted.add(a.id);
          }
        }
      }
    }
    for (const a of rangeFacts) {
      if (!emitted.has(a.id)) {
        html += renderAnnotation(a, true);
        emitted.add(a.id);
      }
    }
    if (!hunks.length && !(f.metadata_rows ?? []).length) {
      if (!classifyLowSignal(f)) {
        html += `<div class="${CLASS.drow} ${CLASS.drowMeta}"><span class="${CLASS.dtext}">(no captured content)</span></div>`;
      }
    }
    return html;
  }
  __name(renderDiffFileBody, "renderDiffFileBody");
  function partitionAnnotations(files, annotations) {
    const anchored = [];
    const decisionContext = [];
    const unanchored = [];
    for (const annotation of annotations) {
      const target = annotation.target ?? {};
      if (annotation.kind === "validation") {
        decisionContext.push(annotation);
        continue;
      }
      if (target.kind !== "range" && target.kind !== "file") {
        decisionContext.push(annotation);
        continue;
      }
      if (!target.filePath) {
        unanchored.push(annotation);
        continue;
      }
      const file = fileForFact(files, target.filePath);
      if (!file || target.kind === "range" && !rangeTouchesCapturedRows(annotation, file)) {
        unanchored.push(annotation);
        continue;
      }
      anchored.push(annotation);
    }
    return { anchored, decisionContext, unanchored };
  }
  __name(partitionAnnotations, "partitionAnnotations");
  function renderDecisionContext(annotations) {
    if (!annotations.length) return "";
    return `<section class="${CLASS.diffDecisionContext}" aria-label="Decision context">
    <h2>Decision context (${annotations.length})</h2>
    <p>Review evidence and recorded assessments for this revision. Validation remains context only.</p>
    <div class="${CLASS.annoGroup}">${annotations.map((annotation) => renderAnnotation(annotation, true)).join("")}</div>
  </section>`;
  }
  __name(renderDecisionContext, "renderDecisionContext");
  function renderUnanchoredFacts(annotations, filePaths) {
    if (!annotations.length) return "";
    return `<section class="${CLASS.diffUnanchoredFacts}" aria-label="Unanchored facts">
    <h2>Unanchored facts (${annotations.length})</h2>
    <div class="${CLASS.annoGroup}">${annotations.map(
      (annotation) => `<div><p class="${CLASS.diffAnchorReason}">${escapeHtml(unanchoredReason(annotation, filePaths))}</p>${renderAnnotation(annotation, true)}</div>`
    ).join("")}</div>
  </section>`;
  }
  __name(renderUnanchoredFacts, "renderUnanchoredFacts");
  function renderDiff(snapshotId, artifact, annotations) {
    const annos = annotations ?? [];
    const files = artifact.snapshot?.files ?? [];
    const filePaths = /* @__PURE__ */ new Set();
    for (const f of files) {
      if (f.new_path) filePaths.add(f.new_path);
      if (f.old_path) filePaths.add(f.old_path);
    }
    const { anchored, decisionContext, unanchored } = partitionAnnotations(
      files,
      annos
    );
    const ctx = {
      snapshotId,
      files,
      anchored,
      decisionContext,
      unanchored,
      filePaths
    };
    const counts = {};
    for (const a of annos) {
      counts[a.kind] = (counts[a.kind] ?? 0) + 1;
    }
    const breakdown = Object.entries(counts).map(([k, n]) => `${n} ${k}${n === 1 ? "" : "s"}`).join(", ");
    let html = `<div class="${CLASS.annoSummary}">${annos.length} review fact${annos.length === 1 ? "" : "s"} on this revision${breakdown ? ` · ${breakdown}` : ""}${unanchored.length ? ` · ${unanchored.length} not anchored to a diff line` : ""}</div>`;
    html += renderDecisionContext(decisionContext);
    html += renderUnanchoredFacts(unanchored, filePaths);
    if (!files.length) {
      return {
        html: `${html}<p class="${CLASS.empty}">No files captured in this snapshot.</p>`,
        ctx
      };
    }
    let openBudget = DEFAULT_OPEN_FILES;
    html += files.map((f, i) => {
      const reason = classifyLowSignal(f);
      const annotated = fileFactCount(f, anchored) > 0;
      const annotatedLarge = annotated && fileRowCount(f) > LARGE_FILE_ROWS;
      const open = annotated && !annotatedLarge || (reason ? false : openBudget-- > 0);
      const expanded = annotatedLarge || open;
      const body = annotatedLarge ? renderDiffFactVicinity(f, anchored) : open ? renderDiffFileBody(f, anchored) : "";
      const lowAttr = reason ? ` data-lowsignal="${escapeHtml(reason)}"` : "";
      const bodyAttr = annotatedLarge ? ` data-fact-vicinity="true"` : open ? ` data-rendered="1"` : "";
      return `<section class="${dfileClass(!!reason)}" data-dfile="${i}" data-expanded="${expanded}"${lowAttr}>${renderDiffFileHeader(f, anchored, reason, expanded)}<div class="${CLASS.dfileBody}" data-dfile-body="${i}"${bodyAttr}>${body}</div></section>`;
    }).join("");
    return { html, ctx };
  }
  __name(renderDiff, "renderDiff");
  function renderDiffNavSummary(summary) {
    return `<div class="${CLASS.diffNavSummary}" aria-label="diff summary">
    <span><b>${summary.fileCount}</b> files</span>
    <span><b>${summary.factCount}</b> facts</span>
    <span><b>${summary.decisionContextCount}</b> context</span>
    <span><b>${summary.unanchoredCount}</b> unanchored</span>
  </div>`;
  }
  __name(renderDiffNavSummary, "renderDiffNavSummary");
  function unanchoredReason(a, filePaths) {
    const t = a.target ?? {};
    if (t.kind !== "range" && t.kind !== "file") {
      return "not a file or range target";
    }
    if (!t.filePath) return "target missing file path";
    if (t.kind === "range" && filePaths.has(t.filePath)) {
      return "line outside captured rows";
    }
    if (!filePaths.has(t.filePath)) return "file missing from snapshot";
    return "not anchored to a diff line";
  }
  __name(unanchoredReason, "unanchoredReason");
  var DIFF_FILE_QUERY_KEYS = ["path", "change", "has", "is"];
  var DIFF_FILE_CHANGE_VALUES = [
    "added",
    "deleted",
    "modified",
    "renamed",
    "copied"
  ];
  var DIFF_FILE_HAS_VALUES = ["facts"];
  var DIFF_FILE_IS_VALUES = ["unanchored"];
  function parseDiffFileQuery(query) {
    const clauses = [];
    const freeText = [];
    const diagnostics = [];
    for (const tok of tokenizeQuery(query || "")) {
      const colon = tok.indexOf(":");
      const field2 = colon > 0 ? tok.slice(0, colon).toLowerCase() : "";
      const rawValue = colon > 0 ? tok.slice(colon + 1).replace(/^"|"$/g, "").toLowerCase() : "";
      if (field2 === "status") {
        diagnostics.push({
          code: "unsupported-qualifier",
          key: "status",
          message: "status: isn't valid in the diff file search — use change: (added, deleted, modified, renamed, copied)"
        });
        continue;
      }
      if (DIFF_FILE_QUERY_KEYS.includes(field2)) {
        const key = field2;
        if (key === "change" && !DIFF_FILE_CHANGE_VALUES.includes(rawValue)) {
          diagnostics.push({
            code: "unsupported-value",
            key: "change",
            message: `change: has no value "${rawValue}" — expected one of ${DIFF_FILE_CHANGE_VALUES.join(", ")}`
          });
          continue;
        }
        if (key === "has" && !DIFF_FILE_HAS_VALUES.includes(rawValue)) {
          diagnostics.push({
            code: "unsupported-value",
            key: "has",
            message: `has: has no value "${rawValue}" — expected "facts"`
          });
          continue;
        }
        if (key === "is" && !DIFF_FILE_IS_VALUES.includes(rawValue)) {
          diagnostics.push({
            code: "unsupported-value",
            key: "is",
            message: `is: has no value "${rawValue}" — expected "unanchored"`
          });
          continue;
        }
        clauses.push({ field: key, value: rawValue });
        continue;
      }
      const term = tok.replace(/^"|"$/g, "").toLowerCase();
      if (term) freeText.push(term);
    }
    return { clauses, freeText, diagnostics };
  }
  __name(parseDiffFileQuery, "parseDiffFileQuery");
  function matchDiffFiles(ctx, query) {
    const { clauses, freeText, diagnostics } = parseDiffFileQuery(query);
    const files = ctx.files.filter((f) => {
      const label2 = filePathLabel(f).toLowerCase();
      for (const term of freeText) {
        if (!label2.includes(term)) return false;
      }
      for (const c of clauses) {
        if (c.field === "path" && !label2.includes(c.value)) return false;
        if (c.field === "change" && f.status !== c.value) return false;
        if (c.field === "has" && fileFactCount(f, ctx.anchored) <= 0)
          return false;
        if (c.field === "is" && fileFactCount(f, ctx.unanchored) <= 0)
          return false;
      }
      return true;
    });
    return { files, diagnostics };
  }
  __name(matchDiffFiles, "matchDiffFiles");

  // src/change-inspector-diff.ts
  function capturedDiffArtifact(value) {
    if (typeof value !== "object" || value === null) return null;
    const snapshot2 = value.snapshot;
    if (typeof snapshot2 !== "object" || snapshot2 === null) return null;
    return Array.isArray(snapshot2.files) ? value : null;
  }
  __name(capturedDiffArtifact, "capturedDiffArtifact");
  function annotationTarget(target) {
    return {
      kind: target.kind,
      filePath: target.filePath,
      startLine: target.startLine,
      endLine: target.endLine,
      side: target.side,
      observationId: target.observationId,
      inputRequestId: target.inputRequestId,
      assessmentId: target.assessmentId,
      eventId: target.eventId
    };
  }
  __name(annotationTarget, "annotationTarget");
  function annotationBody(content) {
    return content.kind === "observation" || content.kind === "input_request" ? content.body : content.kind === "assessment" || content.kind === "validation" ? content.summary : void 0;
  }
  __name(annotationBody, "annotationBody");
  function annotationForFact(detail, fact2) {
    const presentation = detail.factContentPresentations?.[fact2.factId];
    const content = presentation?.content;
    if (!content || content.kind !== fact2.family || fact2.originRevision.revisionId !== detail.revision.revisionId || fact2.originRevision.objectArtifactContentHash !== detail.revision.objectArtifactContentHash || fact2.target !== void 0 && fact2.target.revisionId !== detail.revision.revisionId) {
      return null;
    }
    const annotation = {
      id: fact2.factId,
      kind: content.kind === "input_request" ? "input-request" : content.kind,
      title: content.kind === "assessment" ? `assessment: ${content.assessment}` : content.kind === "validation" ? content.checkName : content.title,
      track: fact2.trackId ?? "untracked",
      body: annotationBody(content),
      bodyContentType: presentation.contentType,
      bodyContentState: presentation.bodyContentState,
      ...fact2.target ? { target: annotationTarget(fact2.target) } : {}
    };
    if (content.kind === "input_request") {
      annotation.status = content.status;
      annotation.responses = content.responses?.map((response) => ({
        id: response.responseId,
        outcome: response.outcome,
        reason: response.reason,
        reasonContentType: response.contentType,
        reasonContentState: response.bodyContentState,
        verificationStatus: response.availability
      }));
    } else if (content.kind === "assessment") {
      annotation.assessment = content.assessment;
      annotation.status = fact2.familyState;
    } else if (content.kind === "validation") {
      annotation.status = content.status;
      annotation.command = content.command;
    }
    return annotation;
  }
  __name(annotationForFact, "annotationForFact");
  function annotationsForExactRevision(detail) {
    return detail.factPresentations.flatMap((fact2) => {
      const annotation = annotationForFact(detail, fact2);
      return annotation ? [annotation] : [];
    });
  }
  __name(annotationsForExactRevision, "annotationsForExactRevision");
  function exactRoute(route, focus) {
    return { ...route, ...focus ? { focus } : {} };
  }
  __name(exactRoute, "exactRoute");
  function updateFocus(route, patch) {
    const next = { ...route.focus, ...patch };
    for (const key of Object.keys(next)) {
      if (!next[key]) delete next[key];
    }
    return exactRoute(route, Object.keys(next).length ? next : void 0);
  }
  __name(updateFocus, "updateFocus");
  function expandFile(section, file, ctx) {
    const body = section.querySelector("[data-dfile-body]");
    if (!body) return;
    if (body.dataset.rendered !== "1") {
      body.innerHTML = renderDiffFileBody(file, ctx.anchored);
      body.dataset.rendered = "1";
    }
    section.dataset.expanded = "true";
    section.querySelector(".dfile-head")?.setAttribute("aria-expanded", "true");
  }
  __name(expandFile, "expandFile");
  function factTarget(root, factId) {
    const matching = Array.from(
      root.querySelectorAll("[data-anno]")
    ).filter((element) => element.dataset.anno === factId);
    return matching.find((element) => element.classList.contains("anno")) ?? matching[0] ?? null;
  }
  __name(factTarget, "factTarget");
  function focusFile(body, ctx, filePath) {
    const index = ctx.files.findIndex(
      (file) => file.new_path === filePath || file.old_path === filePath
    );
    if (index < 0) return;
    const section = body.querySelector(`[data-dfile="${index}"]`);
    if (!section) return;
    expandFile(section, ctx.files[index], ctx);
    section.dataset.exactFocus = "true";
    section.scrollIntoView({ block: "start", behavior: "auto" });
    section.focus({ preventScroll: true });
  }
  __name(focusFile, "focusFile");
  function focusFact(body, ctx, factId) {
    const fact2 = [
      ...ctx.anchored,
      ...ctx.decisionContext,
      ...ctx.unanchored
    ].find((item) => item.id === factId);
    if (fact2?.target?.filePath) focusFile(body, ctx, fact2.target.filePath);
    const target = factTarget(body, factId);
    if (!target) return;
    target.dataset.exactFocus = "true";
    target.tabIndex = -1;
    target.scrollIntoView({ block: "center", behavior: "auto" });
    target.focus({ preventScroll: true });
  }
  __name(focusFact, "focusFact");
  function button(label2, className = "ghost") {
    const element = document.createElement("button");
    element.type = "button";
    element.className = className;
    element.textContent = label2;
    return element;
  }
  __name(button, "button");
  function renderNavigator(nav, route, actions2, ctx) {
    const query = route.focus?.fileQuery ?? "";
    const match = matchDiffFiles(ctx, query);
    nav.replaceChildren();
    const summary = document.createElement("div");
    summary.innerHTML = renderDiffNavSummary({
      fileCount: ctx.files.length,
      factCount: ctx.anchored.length + ctx.decisionContext.length + ctx.unanchored.length,
      decisionContextCount: ctx.decisionContext.length,
      unanchoredCount: ctx.unanchored.length
    });
    nav.append(summary);
    for (const diagnostic of match.diagnostics) {
      const notice = document.createElement("p");
      notice.className = "diff-file-notice";
      notice.textContent = diagnostic.message;
      nav.append(notice);
    }
    const files = document.createElement("ol");
    files.className = "diff-nav-files";
    for (const file of match.files) {
      const index = ctx.files.indexOf(file);
      const item = document.createElement("li");
      const trigger = button(filePathLabel(file), "diff-nav-file");
      trigger.dataset.navFile = String(index);
      const count = fileFactCount(file, ctx.anchored);
      trigger.setAttribute(
        "aria-label",
        `${filePathLabel(file)}${count ? `, ${count} inline facts` : ""}`
      );
      trigger.addEventListener("click", () => {
        const filePath = file.new_path ?? file.old_path;
        if (!filePath) return;
        actions2.navigate(updateFocus(route, { filePath }));
      });
      item.append(trigger);
      files.append(item);
    }
    nav.append(files);
    const facts = [...ctx.anchored, ...ctx.decisionContext, ...ctx.unanchored];
    if (facts.length > 0) {
      const heading = document.createElement("h3");
      heading.textContent = "Facts";
      nav.append(heading);
      const list = document.createElement("ol");
      for (const fact2 of facts) {
        const item = document.createElement("li");
        const trigger = button(fact2.title, "diff-nav-fact");
        trigger.dataset.anno = fact2.id;
        trigger.addEventListener(
          "click",
          () => actions2.navigate(updateFocus(route, { factId: fact2.id }))
        );
        item.append(trigger);
        list.append(item);
      }
      nav.append(list);
    }
  }
  __name(renderNavigator, "renderNavigator");
  function bindDiffBody(body, route, actions2, ctx) {
    ctx.files.forEach((file, index) => {
      const section = body.querySelector(`[data-dfile="${index}"]`);
      if (!section) return;
      const path = file.new_path ?? file.old_path;
      if (path) section.dataset.filePath = path;
      if (file.old_path) section.dataset.oldFilePath = file.old_path;
      if (file.new_path) section.dataset.newFilePath = file.new_path;
      section.tabIndex = -1;
      const toggle = /* @__PURE__ */ __name(() => {
        if (section.dataset.expanded === "true") {
          section.dataset.expanded = "false";
          section.querySelector(".dfile-head")?.setAttribute("aria-expanded", "false");
        } else {
          expandFile(section, file, ctx);
        }
      }, "toggle");
      const header = section.querySelector(".dfile-head");
      header?.addEventListener("click", toggle);
      header?.addEventListener("keydown", (event) => {
        if (event.key !== "Enter" && event.key !== " ") return;
        event.preventDefault();
        toggle();
      });
    });
    const activateBodyTarget = /* @__PURE__ */ __name((target) => {
      const renderAll = target?.closest("[data-render-diff-file]");
      if (renderAll) {
        const section = renderAll.closest(".dfile");
        const index = Number(section?.dataset.dfile);
        if (section && Number.isInteger(index))
          expandFile(section, ctx.files[index], ctx);
        return;
      }
      const noted = target?.closest(".drow-noted[data-anno]");
      if (noted?.dataset.anno)
        actions2.navigate(updateFocus(route, { factId: noted.dataset.anno }));
    }, "activateBodyTarget");
    body.onclick = (event) => activateBodyTarget(event.target instanceof Element ? event.target : null);
    body.onkeydown = (event) => {
      if (event.key !== "Enter" && event.key !== " ") return;
      const target = event.target instanceof Element ? event.target : null;
      if (!target?.closest(".drow-noted[data-anno]")) return;
      event.preventDefault();
      activateBodyTarget(target);
    };
  }
  __name(bindDiffBody, "bindDiffBody");
  function renderChangeInspectorDiffPage(detail, route, actions2) {
    const page = document.querySelector("#diff-page");
    const toolbar = document.querySelector("#toolbar");
    const split = document.querySelector(".split");
    const title = document.querySelector("#diff-page-title");
    const close = document.querySelector("#diff-page-close");
    const input = document.querySelector("#diff-file-query");
    const nav = document.querySelector("#diff-page-nav-list");
    const body = document.querySelector("#diff-page-body");
    if (!page || !toolbar || !split || !title || !close || !input || !nav || !body)
      return false;
    page.classList.remove("hidden");
    toolbar.classList.add("hidden");
    split.classList.add("hidden");
    title.textContent = `Annotated diff · ${shortRef(detail.revision.revisionId)}`;
    title.title = `exact Revision ${detail.revision.revisionId}; artifact ${detail.revision.objectArtifactContentHash}`;
    title.setAttribute(
      "aria-label",
      `Annotated diff for exact Revision ${detail.revision.revisionId}; artifact ${detail.revision.objectArtifactContentHash}`
    );
    close.onclick = () => actions2.navigate({
      kind: "revision",
      changeId: route.changeId,
      revision: route.revision,
      query: route.query,
      ...route.focus && (route.focus.factId || route.focus.filePath) ? {
        focus: {
          ...route.focus.factId ? { factId: route.focus.factId } : {},
          ...route.focus.filePath ? { filePath: route.focus.filePath } : {}
        }
      } : {}
    });
    input.value = route.focus?.fileQuery ?? "";
    input.oninput = () => actions2.replace?.(updateFocus(route, { fileQuery: input.value }));
    const resource = detail.exactRevisionDocument;
    if (resource.availability !== "available") {
      body.replaceChildren(
        Object.assign(document.createElement("p"), {
          className: "empty",
          textContent: "Captured bytes are unavailable. The Inspector will not reconstruct a diff from Git or an associated commit."
        })
      );
      nav.replaceChildren();
      return true;
    }
    const artifact = capturedDiffArtifact(resource.capturedDocument);
    if (!artifact) {
      body.replaceChildren(
        Object.assign(document.createElement("p"), {
          className: "empty",
          textContent: "This exact resource does not contain a captured snapshot."
        })
      );
      nav.replaceChildren();
      return true;
    }
    const rendered = renderDiff(
      resource.resource.objectId,
      artifact,
      annotationsForExactRevision(detail)
    );
    body.innerHTML = rendered.html;
    bindDiffBody(body, route, actions2, rendered.ctx);
    renderNavigator(nav, route, actions2, rendered.ctx);
    if (route.focus?.filePath)
      focusFile(body, rendered.ctx, route.focus.filePath);
    if (route.focus?.factId) focusFact(body, rendered.ctx, route.focus.factId);
    return true;
  }
  __name(renderChangeInspectorDiffPage, "renderChangeInspectorDiffPage");
  function hideChangeInspectorDiffPage() {
    document.querySelector("#diff-page")?.classList.add("hidden");
    document.querySelector("#toolbar")?.classList.remove("hidden");
    document.querySelector(".split")?.classList.remove("hidden");
  }
  __name(hideChangeInspectorDiffPage, "hideChangeInspectorDiffPage");

  // src/change-inspector-graphs.ts
  var SVG_NS = "http://www.w3.org/2000/svg";
  function svgElement(document2, name, attributes = {}) {
    const element = document2.createElementNS(SVG_NS, name);
    for (const [attribute, value] of Object.entries(attributes)) {
      element.setAttribute(attribute, String(value));
    }
    return element;
  }
  __name(svgElement, "svgElement");
  function words3(value) {
    return value.replaceAll("_", " ");
  }
  __name(words3, "words");
  function exactRevisionIdentity(revision2) {
    return `exact Revision ${revision2.revisionId}; artifact ${revision2.objectArtifactContentHash}`;
  }
  __name(exactRevisionIdentity, "exactRevisionIdentity");
  function exactFactIdentity(focus) {
    return `${words3(focus.family)} ${focus.factId}; ${exactRevisionIdentity(focus.revision)}`;
  }
  __name(exactFactIdentity, "exactFactIdentity");
  function exactFactData(focus) {
    return JSON.stringify({
      revisionId: focus.revision.revisionId,
      objectArtifactContentHash: focus.revision.objectArtifactContentHash,
      family: focus.family,
      factId: focus.factId
    });
  }
  __name(exactFactData, "exactFactData");
  function setRevisionData(element, revision2) {
    element.setAttribute("data-revision-id", revision2.revisionId);
    element.setAttribute(
      "data-artifact-hash",
      revision2.objectArtifactContentHash
    );
  }
  __name(setRevisionData, "setRevisionData");
  function wireAction(element, action, keyboard) {
    element.addEventListener("click", action);
    if (!keyboard) return;
    element.addEventListener("keydown", (event) => {
      if (!(event instanceof KeyboardEvent)) return;
      if (event.key !== "Enter" && event.key !== " ") return;
      event.preventDefault();
      action();
    });
  }
  __name(wireAction, "wireAction");
  function appendTitle(document2, parent, value) {
    const title = svgElement(document2, "title");
    title.textContent = value;
    parent.append(title);
  }
  __name(appendTitle, "appendTitle");
  function marker(document2, id, kind) {
    const result = svgElement(document2, "marker", {
      id,
      markerWidth: 9,
      markerHeight: 9,
      refX: 8,
      refY: 4.5,
      orient: "auto",
      markerUnits: "userSpaceOnUse"
    });
    const path = svgElement(document2, "path", {
      d: "M0,0 L8,4.5 L0,9 z",
      fill: kind === "solid" ? "currentColor" : "none",
      stroke: "currentColor",
      "stroke-width": kind === "solid" ? 0 : 1.5
    });
    result.append(path);
    return result;
  }
  __name(marker, "marker");
  function graphRoot(document2, className, label2, bounds) {
    return svgElement(document2, "svg", {
      class: className,
      width: bounds.w,
      height: bounds.h,
      viewBox: `0 0 ${bounds.w} ${bounds.h}`,
      preserveAspectRatio: "xMinYMin meet",
      role: "group",
      "aria-label": label2
    });
  }
  __name(graphRoot, "graphRoot");
  function sorted(values, key) {
    return [...values].sort(
      (left, right) => key(left).localeCompare(key(right), "en")
    );
  }
  __name(sorted, "sorted");
  function edgePoints(path, from) {
    let points = path;
    if (from && path.length > 1) {
      const distance = /* @__PURE__ */ __name(([x, y]) => (x - from.x) ** 2 + (y - from.y) ** 2, "distance");
      if (distance(path[0]) < distance(path[path.length - 1])) {
        points = [...path].reverse();
      }
    }
    return points.map(([x, y]) => `${x},${y}`).join(" ");
  }
  __name(edgePoints, "edgePoints");
  function relationshipGroup(document2, accessibleName, attributes, path, from, markerId, strokeDasharray) {
    const group = svgElement(document2, "g", {
      role: "group",
      "aria-label": accessibleName,
      ...attributes
    });
    appendTitle(document2, group, accessibleName);
    group.append(
      svgElement(document2, "polyline", {
        points: edgePoints(path, from),
        fill: "none",
        stroke: "currentColor",
        "stroke-width": 2,
        "stroke-dasharray": strokeDasharray ?? "none",
        "vector-effect": "non-scaling-stroke",
        "marker-end": `url(#${markerId})`,
        "aria-hidden": true
      })
    );
    return group;
  }
  __name(relationshipGroup, "relationshipGroup");
  function textualEquivalent(document2, label2) {
    const root = document2.createElement("details");
    root.className = "relationship-graph-text";
    root.dataset.graphTextualEquivalent = "true";
    const summary = document2.createElement("summary");
    summary.textContent = `${label2} as text`;
    const nodeHeading = document2.createElement("h4");
    nodeHeading.textContent = "Exact identities";
    const nodes = document2.createElement("ul");
    nodes.dataset.graphTextNodes = "true";
    const edgeHeading = document2.createElement("h4");
    edgeHeading.textContent = "Relationships";
    const edges = document2.createElement("ul");
    edges.dataset.graphTextEdges = "true";
    root.append(summary, nodeHeading, nodes, edgeHeading, edges);
    return { root, nodes, edges };
  }
  __name(textualEquivalent, "textualEquivalent");
  function actionItem(document2, text, accessibleName, action) {
    const item = document2.createElement("li");
    const button2 = document2.createElement("button");
    button2.type = "button";
    button2.textContent = text;
    button2.title = accessibleName;
    button2.setAttribute("aria-label", accessibleName);
    wireAction(button2, action, false);
    item.append(button2);
    return item;
  }
  __name(actionItem, "actionItem");
  function textItem(document2, value) {
    const item = document2.createElement("li");
    item.textContent = value;
    return item;
  }
  __name(textItem, "textItem");
  function renderChangeRevisionGraph(graph, options) {
    const { document: document2 } = options;
    const figure = document2.createElement("figure");
    figure.className = "change-revision-graph";
    figure.setAttribute("aria-label", "Change Revision relationships");
    const svg = graphRoot(
      document2,
      "change-revision-graph-svg",
      "Change Revision relationship graph",
      graph.bounds
    );
    const defs = svgElement(document2, "defs");
    defs.append(
      marker(document2, "change-effective-arrow", "solid"),
      marker(document2, "change-claim-arrow", "open")
    );
    svg.append(defs);
    const text = textualEquivalent(document2, "Change Revision relationships");
    const nodesById = new Map(graph.nodes.map((node) => [node.id, node]));
    for (const edge of sorted(
      graph.effectiveSupersedes,
      (candidate) => `${candidate.from}\0${candidate.to}`
    )) {
      const accessibleName = `Effective supersedes relationship: ${exactRevisionIdentity(edge.successor)} supersedes ${exactRevisionIdentity(edge.predecessor)}`;
      const group = relationshipGroup(
        document2,
        accessibleName,
        {
          class: "change-revision-edge change-revision-edge-effective",
          "data-edge-kind": "effective-supersedes",
          "data-from": edge.from,
          "data-to": edge.to
        },
        edge.path,
        nodesById.get(edge.from),
        "change-effective-arrow"
      );
      setRevisionData(group, edge.successor);
      group.setAttribute(
        "data-predecessor-revision-id",
        edge.predecessor.revisionId
      );
      group.setAttribute(
        "data-predecessor-artifact-hash",
        edge.predecessor.objectArtifactContentHash
      );
      svg.append(group);
      text.edges.append(textItem(document2, accessibleName));
    }
    for (const edge of sorted(
      graph.pendingOrConflictingClaims,
      (candidate) => `${candidate.claimId}\0${candidate.from}\0${candidate.to}`
    )) {
      const diagnostics = (edge.diagnostics ?? []).length ? ` Diagnostics: ${edge.diagnostics?.join("; ")}` : "";
      const accessibleName = `Pending or conflicting supersedes claim ${edge.claimId}: ${exactRevisionIdentity(edge.successor)} claims to supersede ${exactRevisionIdentity(edge.predecessor)}.${diagnostics}`;
      const group = relationshipGroup(
        document2,
        accessibleName,
        {
          class: "change-revision-edge change-revision-edge-claim",
          "data-edge-kind": "pending-or-conflicting-claim",
          "data-claim-id": edge.claimId,
          "data-from": edge.from,
          "data-to": edge.to
        },
        edge.path,
        nodesById.get(edge.from),
        "change-claim-arrow",
        "7 5"
      );
      setRevisionData(group, edge.successor);
      group.setAttribute(
        "data-predecessor-revision-id",
        edge.predecessor.revisionId
      );
      group.setAttribute(
        "data-predecessor-artifact-hash",
        edge.predecessor.objectArtifactContentHash
      );
      svg.append(group);
      text.edges.append(textItem(document2, accessibleName));
    }
    for (const node of sorted(graph.nodes, (candidate) => candidate.id)) {
      const activationRevision = node.activationRevision;
      const canActivate = node.contextAvailability === "available" && activationRevision !== void 0;
      const state = [
        node.isCurrent ? "current" : "not current",
        node.isMember ? "Change member" : "claim-only context",
        canActivate ? "exact Change context available" : "relationship context only; no exact Change route is available"
      ].join("; ");
      const accessibleName = `${exactRevisionIdentity(node.revision)}; ${state}`;
      const group = svgElement(document2, "g", {
        class: `change-revision-node${node.isCurrent ? " is-current" : ""}${node.isMember ? " is-member" : " is-context"}`,
        role: canActivate ? "link" : "group",
        "aria-label": accessibleName,
        "data-graph-node-id": node.id,
        "data-current": node.isCurrent,
        "data-member": node.isMember,
        "data-context-availability": node.contextAvailability
      });
      if (canActivate) {
        group.setAttribute("tabindex", "0");
      } else {
        group.setAttribute("aria-disabled", "true");
      }
      setRevisionData(group, node.revision);
      group.setAttribute("title", accessibleName);
      appendTitle(document2, group, accessibleName);
      group.append(
        svgElement(document2, "rect", {
          x: node.x - node.w / 2,
          y: node.y - node.h / 2,
          width: node.w,
          height: node.h,
          rx: 6,
          fill: "none",
          stroke: "currentColor",
          "stroke-width": node.isCurrent ? 3 : node.isMember ? 2 : 1,
          "stroke-dasharray": node.isMember ? "none" : "4 3",
          "vector-effect": "non-scaling-stroke",
          "aria-hidden": true
        })
      );
      const label2 = svgElement(document2, "text", {
        x: node.x,
        y: node.y,
        "text-anchor": "middle",
        "dominant-baseline": "middle",
        "aria-hidden": true
      });
      label2.textContent = `${node.isCurrent ? "current · " : ""}${canActivate ? "" : "context · "}${shortRef(node.revision.revisionId)}`;
      group.append(label2);
      const activate = canActivate ? () => options.onActivateRevision(activationRevision) : void 0;
      if (activate) wireAction(group, activate, true);
      svg.append(group);
      text.nodes.append(
        activate ? actionItem(
          document2,
          `${node.isCurrent ? "Current " : ""}${shortRef(node.revision.revisionId)}`,
          `Open ${accessibleName}`,
          activate
        ) : textItem(document2, accessibleName)
      );
    }
    for (const diagnostic of graph.diagnostics ?? []) {
      text.edges.append(textItem(document2, `Graph diagnostic: ${diagnostic}`));
    }
    figure.append(svg, text.root);
    return figure;
  }
  __name(renderChangeRevisionGraph, "renderChangeRevisionGraph");
  function factRefIdentity(reference) {
    if (reference.kind === "observation") {
      return { family: reference.kind, factId: reference.observationId ?? "" };
    }
    return { family: reference.kind, factId: reference.inputRequestId ?? "" };
  }
  __name(factRefIdentity, "factRefIdentity");
  function appendFactEdge(document2, svg, text, nodesById, edge, accessibleName, kind, markerId, dash) {
    const group = relationshipGroup(
      document2,
      accessibleName,
      {
        class: `fact-relationship-edge fact-relationship-edge-${kind}`,
        "data-edge-kind": kind,
        "data-from": edge.from,
        "data-to": edge.to
      },
      edge.path,
      nodesById.get(edge.from),
      markerId,
      dash
    );
    svg.append(group);
    text.append(textItem(document2, accessibleName));
    return group;
  }
  __name(appendFactEdge, "appendFactEdge");
  function renderFactRelationshipGraph(graph, options) {
    const { document: document2 } = options;
    const figure = document2.createElement("figure");
    figure.className = "fact-relationship-graph";
    figure.setAttribute("aria-label", "Exact fact relationships");
    const svg = graphRoot(
      document2,
      "fact-relationship-graph-svg",
      "Exact fact relationship graph",
      graph.bounds
    );
    const defs = svgElement(document2, "defs");
    defs.append(
      marker(document2, "fact-observation-arrow", "solid"),
      marker(document2, "fact-assessment-arrow", "open"),
      marker(document2, "fact-port-arrow", "open")
    );
    svg.append(defs);
    const text = textualEquivalent(document2, "Exact fact relationships");
    const nodesById = new Map(graph.nodes.map((node) => [node.id, node]));
    for (const edge of sorted(
      graph.observationSupersedes,
      (candidate) => `${candidate.from}\0${candidate.to}`
    )) {
      const accessibleName = `Observation supersedes relationship: observation ${edge.fromFactId}; ${exactRevisionIdentity(edge.originRevision)} supersedes observation ${edge.toFactId}; ${exactRevisionIdentity(edge.originRevision)}`;
      const group = appendFactEdge(
        document2,
        svg,
        text.edges,
        nodesById,
        edge,
        accessibleName,
        "observation-supersedes",
        "fact-observation-arrow"
      );
      setRevisionData(group, edge.originRevision);
      group.setAttribute("data-graph-from-fact-id", edge.fromFactId);
      group.setAttribute("data-graph-to-fact-id", edge.toFactId);
    }
    for (const edge of sorted(
      graph.assessmentReplaces,
      (candidate) => `${candidate.from}\0${candidate.to}`
    )) {
      const accessibleName = `Assessment replaces relationship: assessment ${edge.fromFactId}; ${exactRevisionIdentity(edge.originRevision)} replaces assessment ${edge.toFactId}; ${exactRevisionIdentity(edge.originRevision)}`;
      const group = appendFactEdge(
        document2,
        svg,
        text.edges,
        nodesById,
        edge,
        accessibleName,
        "assessment-replaces",
        "fact-assessment-arrow",
        "10 4"
      );
      setRevisionData(group, edge.originRevision);
      group.setAttribute("data-graph-from-fact-id", edge.fromFactId);
      group.setAttribute("data-graph-to-fact-id", edge.toFactId);
    }
    for (const edge of sorted(
      graph.factPorts,
      (candidate) => `${candidate.portId}\0${candidate.from}\0${candidate.to}`
    )) {
      const origin = factRefIdentity(edge.originFact);
      const originIdentity = exactFactIdentity({
        revision: edge.originRevision,
        ...origin
      });
      const target = edge.targetFact ? exactFactIdentity({
        revision: edge.targetRevision,
        ...factRefIdentity(edge.targetFact)
      }) : exactRevisionIdentity(edge.targetRevision);
      const diagnostics = (edge.diagnostics ?? []).length ? ` Diagnostics: ${edge.diagnostics?.join("; ")}` : "";
      const accessibleName = `Fact port ${edge.portId}: ${originIdentity} ${words3(edge.relation)} ${target}; applicability ${words3(edge.applicability)}.${diagnostics}`;
      const group = appendFactEdge(
        document2,
        svg,
        text.edges,
        nodesById,
        edge,
        accessibleName,
        "fact-port",
        "fact-port-arrow",
        "2 4"
      );
      group.setAttribute("data-port-id", edge.portId);
      group.setAttribute("data-port-relation", edge.relation);
      group.setAttribute("data-port-applicability", edge.applicability);
      setRevisionData(group, edge.originRevision);
      group.setAttribute(
        "data-target-revision-id",
        edge.targetRevision.revisionId
      );
      group.setAttribute(
        "data-target-artifact-hash",
        edge.targetRevision.objectArtifactContentHash
      );
    }
    for (const node of sorted(graph.nodes, (candidate) => candidate.id)) {
      const isFact = node.kind === "fact";
      const focus = isFact && node.factId !== void 0 && node.family !== void 0 ? {
        revision: node.revision,
        family: node.family,
        factId: node.factId
      } : void 0;
      const activationRevision = node.activationRevision;
      const canActivate = node.contextAvailability === "available" && activationRevision !== void 0;
      const availability = canActivate ? `exact Change context available in ${exactRevisionIdentity(activationRevision)}` : "relationship context only; no exact Change route is available";
      const accessibleName = focus ? `${canActivate ? "Focus" : "Relationship context for"} exact fact ${exactFactIdentity(focus)}; ${availability}` : `${canActivate ? "Open" : "Relationship context for"} ${exactRevisionIdentity(node.revision)} fact-port anchor; ${availability}`;
      const group = svgElement(document2, "g", {
        class: `fact-relationship-node fact-relationship-node-${isFact ? "fact" : "revision"}`,
        role: canActivate ? "link" : "group",
        "aria-label": accessibleName,
        "data-graph-node-id": node.id,
        "data-node-kind": node.kind,
        "data-context-availability": node.contextAvailability
      });
      if (canActivate) {
        group.setAttribute("tabindex", "0");
      } else {
        group.setAttribute("aria-disabled", "true");
      }
      setRevisionData(group, node.revision);
      group.setAttribute("title", accessibleName);
      if (focus) {
        group.setAttribute("data-graph-family", focus.family);
        group.setAttribute("data-graph-fact-id", focus.factId);
        group.setAttribute("data-graph-fact-focus", exactFactData(focus));
      }
      appendTitle(document2, group, accessibleName);
      group.append(
        svgElement(document2, "rect", {
          x: node.x - node.w / 2,
          y: node.y - node.h / 2,
          width: node.w,
          height: node.h,
          rx: isFact ? 6 : 0,
          fill: "none",
          stroke: "currentColor",
          "stroke-width": 2,
          "stroke-dasharray": isFact ? "none" : "4 3",
          "vector-effect": "non-scaling-stroke",
          "aria-hidden": true
        })
      );
      const label2 = svgElement(document2, "text", {
        x: node.x,
        y: node.y,
        "text-anchor": "middle",
        "dominant-baseline": "middle",
        "aria-hidden": true
      });
      label2.textContent = focus ? `${words3(focus.family)} · ${shortRef(focus.factId)}` : `Revision · ${shortRef(node.revision.revisionId)}`;
      group.append(label2);
      const activate = canActivate ? focus ? () => options.onFocusFact({
        ...focus,
        revision: activationRevision
      }) : () => options.onActivateRevision(activationRevision) : void 0;
      if (activate) wireAction(group, activate, true);
      svg.append(group);
      text.nodes.append(
        activate ? actionItem(
          document2,
          focus ? `${words3(focus.family)} ${shortRef(focus.factId)}` : `Revision ${shortRef(node.revision.revisionId)}`,
          accessibleName,
          activate
        ) : textItem(document2, accessibleName)
      );
    }
    figure.append(svg, text.root);
    return figure;
  }
  __name(renderFactRelationshipGraph, "renderFactRelationshipGraph");

  // src/change-inspector-render.ts
  function routeForLens(lens, current) {
    if (lens === "timeline") {
      return {
        kind: "timeline",
        historyQuery: current.kind === "timeline" || current.kind === "event" ? { ...current.historyQuery, after: void 0, at: void 0 } : {}
      };
    }
    return {
      kind: "lens",
      lens,
      query: current.kind === "invalid" || current.kind === "timeline" || current.kind === "event" ? {} : { ...current.query, after: void 0 }
    };
  }
  __name(routeForLens, "routeForLens");
  function syncLensLinks(active3, current) {
    document.querySelectorAll("#lens-switcher a[data-lens]").forEach((link) => {
      const lens = link.dataset.lens;
      if (lens !== "timeline" && lens !== "changes" && lens !== "attention")
        return;
      link.href = formatChangeInspectorRoute(routeForLens(lens, current));
      if (lens === active3) link.setAttribute("aria-current", "page");
      else link.removeAttribute("aria-current");
    });
  }
  __name(syncLensLinks, "syncLensLinks");
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
  function setCompactIdentityText(element, text, accessibleName = text) {
    const visible = compactIdentityText(text);
    element.textContent = visible;
    if (visible !== text) {
      element.title = text;
      element.setAttribute("aria-label", accessibleName);
    }
  }
  __name(setCompactIdentityText, "setCompactIdentityText");
  function message(text) {
    const element = document.createElement("p");
    element.className = "empty";
    setCompactIdentityText(element, text);
    return element;
  }
  __name(message, "message");
  function selectOption(label2, value, artifactHash, revisionId, title, accessibleName) {
    const option = document.createElement("option");
    option.textContent = label2;
    option.value = value;
    if (artifactHash) option.dataset.artifactHash = artifactHash;
    if (revisionId) option.dataset.revisionId = revisionId;
    if (title) option.title = title;
    if (accessibleName) option.setAttribute("aria-label", accessibleName);
    return option;
  }
  __name(selectOption, "selectOption");
  function exactRevisionOptionValue(revisionId, objectArtifactContentHash) {
    return JSON.stringify([revisionId, objectArtifactContentHash]);
  }
  __name(exactRevisionOptionValue, "exactRevisionOptionValue");
  function setText(selector, value) {
    const element = document.querySelector(selector);
    if (element) element.textContent = value;
  }
  __name(setText, "setText");
  function replaceMasterWith(...children) {
    const master = document.querySelector("#master");
    if (!master) return;
    delete master.dataset.changeListKey;
    delete master.dataset.timelineKey;
    master.replaceChildren(...children);
  }
  __name(replaceMasterWith, "replaceMasterWith");
  function replaceDetailWith(...children) {
    const detail = document.querySelector("#detail-body");
    if (!detail) return;
    delete detail.dataset.changeReadingKey;
    detail.replaceChildren(...children);
  }
  __name(replaceDetailWith, "replaceDetailWith");
  function prepareChangeInspectorShell(actions2) {
    document.querySelector("#view-controls")?.classList.remove("hidden");
    setText("#view-toggle", "View");
    document.querySelector("#view-order-section")?.classList.add("hidden");
    document.querySelector("#view-sort-section")?.classList.add("hidden");
    document.querySelector("#jump-latest")?.closest(".control-section")?.classList.add("hidden");
    document.querySelector("#derived-access-status")?.classList.add("hidden");
    const follow = document.querySelector("#follow-toggle");
    if (follow) {
      follow.classList.add("hidden");
      follow.onclick = () => actions2.toggleTimelineMonitoring?.();
    }
    const switcher = document.querySelector("#lens-switcher");
    if (switcher) {
      switcher.replaceChildren();
      for (const lens of ["timeline", "changes", "attention"]) {
        const link = document.createElement("a");
        link.className = "lens-tab";
        link.dataset.lens = lens;
        link.textContent = lens === "timeline" ? "Timeline" : lens === "changes" ? "Changes" : "Attention";
        const destination = /* @__PURE__ */ __name(() => {
          const current = parseChangeInspectorRoute(
            location.hash || "#/timeline"
          );
          return routeForLens(lens, current);
        }, "destination");
        link.href = formatChangeInspectorRoute(destination());
        link.addEventListener("click", (event) => {
          if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey)
            return;
          event.preventDefault();
          actions2.navigate(destination());
        });
        switcher.append(link);
      }
    }
    const back = document.querySelector("#detail-back");
    if (back) {
      back.textContent = "‹ Timeline";
    }
    const search = document.querySelector("#filter-text");
    if (search)
      search.placeholder = "Search Timeline, Changes, and exact Revisions";
    const filterTypes = document.querySelector("#filter-types");
    if (filterTypes) {
      filterTypes.replaceChildren();
      const heading = document.createElement("h2");
      heading.id = "filter-types-label";
      heading.className = "control-heading change-filter";
      heading.textContent = "Change status";
      filterTypes.append(heading);
      const timelineHeading = document.createElement("h2");
      timelineHeading.className = "control-heading timeline-filter hidden";
      timelineHeading.textContent = "Event types";
      const timelineMenu = document.createElement("ul");
      timelineMenu.id = "filter-types-menu";
      timelineMenu.className = "type-facet-menu timeline-filter hidden";
      timelineMenu.setAttribute("aria-label", "event types");
      filterTypes.append(timelineHeading, timelineMenu);
      for (const [name, values] of FILTER_OPTIONS) {
        const label2 = document.createElement("label");
        label2.className = "change-filter";
        label2.textContent = name.replaceAll("_", " ");
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
          const current = parseChangeInspectorRoute(
            location.hash || "#/timeline"
          );
          const base = current.kind === "invalid" || current.kind === "timeline" || current.kind === "event" ? { kind: "lens", lens: "changes", query: {} } : current;
          actions2.navigate({
            ...base,
            query: {
              ...base.query,
              after: void 0,
              [name]: select.value || void 0
            }
          });
        });
        label2.append(select);
        filterTypes.append(label2);
      }
      const timelineChoice = /* @__PURE__ */ __name((label2, id, update) => {
        const choiceLabel = document.createElement("label");
        choiceLabel.className = "timeline-filter hidden";
        choiceLabel.textContent = label2;
        const select = document.createElement("select");
        select.id = id;
        select.append(selectOption("Any", ""));
        select.addEventListener("change", () => {
          const current = parseChangeInspectorRoute(
            location.hash || "#/timeline"
          );
          if (current.kind !== "timeline" && current.kind !== "event") return;
          const query = {
            ...current.historyQuery,
            after: void 0,
            at: void 0
          };
          update(query);
          actions2.navigate({ kind: "timeline", historyQuery: query });
        });
        choiceLabel.append(select);
        filterTypes.append(choiceLabel);
      }, "timelineChoice");
      timelineChoice("track", "timeline-filter-track", (query) => {
        query.track = document.querySelector("#timeline-filter-track")?.value || void 0;
      });
      timelineChoice("Change", "timeline-filter-change", (query) => {
        query.change = document.querySelector("#timeline-filter-change")?.value || void 0;
      });
      timelineChoice("exact Revision", "timeline-filter-revision", (query) => {
        const select = document.querySelector(
          "#timeline-filter-revision"
        );
        const selected = select?.selectedOptions[0];
        query.revision = selected?.dataset.revisionId;
        query.artifactHash = selected?.dataset.artifactHash;
      });
    }
    for (const input of document.querySelectorAll(
      "input[name='view-order']"
    )) {
      input.addEventListener("change", () => {
        if (!input.checked) return;
        const current = parseChangeInspectorRoute(location.hash || "#/timeline");
        if (current.kind !== "timeline" && current.kind !== "event") return;
        actions2.navigate({
          kind: "timeline",
          historyQuery: {
            ...current.historyQuery,
            after: void 0,
            at: void 0,
            order: input.value === "asc" ? "asc" : "desc"
          }
        });
      });
    }
    const clear = document.querySelector("#filter-clear");
    if (clear) {
      clear.onclick = () => {
        const current = parseChangeInspectorRoute(location.hash || "#/timeline");
        if (current.kind === "timeline" || current.kind === "event") {
          actions2.navigate({
            kind: "timeline",
            historyQuery: {
              limit: current.historyQuery.limit,
              order: current.historyQuery.order
            }
          });
          return;
        }
        const base = current.kind === "invalid" ? { kind: "lens", lens: "changes", query: {} } : current;
        actions2.navigate({
          ...base,
          // Clear only reader filters and the now-invalid continuation. Keep the
          // bounded page shape in the URL so reset does not silently change the
          // caller's explicit limit or stable ordering contract.
          query: {
            limit: base.query.limit,
            order: base.query.order
          }
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
  function replaceTimelineSelectOptions(id, options, selected) {
    const select = document.querySelector(`#${id}`);
    if (!select) return;
    const key = JSON.stringify(options);
    if (select.dataset.timelineOptions !== key) {
      select.replaceChildren(selectOption("Any", ""));
      for (const option of options) {
        select.append(
          selectOption(
            option.label,
            option.value,
            option.artifactHash,
            option.revisionId,
            option.title,
            option.accessibleName
          )
        );
      }
      select.dataset.timelineOptions = key;
    }
    select.value = selected ?? "";
  }
  __name(replaceTimelineSelectOptions, "replaceTimelineSelectOptions");
  function syncTimelineTypeFacets(route, history2, actions2) {
    const menu = document.querySelector("#filter-types-menu");
    if (!menu) return;
    const selected = new Set(
      route.historyQuery.type?.split(",").filter(Boolean) ?? []
    );
    const rows = history2.completion.eventTypes.map((eventType) => {
      const item = document.createElement("li");
      const button2 = document.createElement("button");
      button2.type = "button";
      button2.className = `type-facet-row${selected.has(eventType) ? "" : " type-facet-row-off"}`;
      button2.dataset.eventType = eventType;
      button2.setAttribute("aria-pressed", String(selected.has(eventType)));
      const dot = document.createElement("span");
      dot.className = "dot";
      dot.style.background = eventTypeColor(eventType);
      dot.setAttribute("aria-hidden", "true");
      const name = document.createElement("span");
      name.textContent = eventType.replaceAll("_", " ");
      const count = document.createElement("span");
      count.className = "type-count";
      count.textContent = String(history2.facets[eventType] ?? 0);
      button2.append(dot, name, count);
      button2.addEventListener("click", () => {
        const nextTypes = new Set(selected);
        if (nextTypes.has(eventType)) nextTypes.delete(eventType);
        else nextTypes.add(eventType);
        actions2.navigate({
          kind: "timeline",
          historyQuery: {
            ...route.historyQuery,
            after: void 0,
            at: void 0,
            type: nextTypes.size ? [...nextTypes].sort().join(",") : void 0
          }
        });
      });
      item.append(button2);
      return item;
    });
    if (!rows.length) {
      const empty = document.createElement("li");
      empty.className = "dim";
      empty.textContent = "No event types available";
      rows.push(empty);
    }
    menu.replaceChildren(...rows);
  }
  __name(syncTimelineTypeFacets, "syncTimelineTypeFacets");
  function syncFilterChrome(route, history2, actions2) {
    if (route.kind === "invalid") return;
    if (route.kind === "timeline" || route.kind === "event") {
      const input2 = document.querySelector("#filter-text");
      if (input2) input2.value = route.historyQuery.q ?? "";
      const chips2 = document.querySelector("#filter-chips");
      const values2 = [
        ["search", route.historyQuery.q],
        ["type", route.historyQuery.type],
        ["track", route.historyQuery.track],
        ["change", route.historyQuery.change],
        ["revision", route.historyQuery.revision]
      ].filter((value) => Boolean(value[1]));
      chips2?.replaceChildren(
        ...values2.map(([name, value]) => {
          const chip = document.createElement("button");
          chip.type = "button";
          chip.className = "badge";
          const exactRevision = name === "revision" && route.historyQuery.artifactHash ? {
            revisionId: value,
            objectArtifactContentHash: route.historyQuery.artifactHash
          } : null;
          const visible = exactRevision ? shortExactRevision(exactRevision) : compactIdentityText(value);
          const full = exactRevision ? `${value} · ${exactRevision.objectArtifactContentHash}` : value;
          chip.textContent = `${name}: ${visible} ×`;
          chip.title = full;
          chip.setAttribute(
            "aria-label",
            exactRevision ? `Remove revision filter: ${value}; artifact ${exactRevision.objectArtifactContentHash}` : `Remove ${name} filter: ${value}`
          );
          chip.addEventListener("click", () => {
            const next = {
              ...route.historyQuery,
              after: void 0,
              at: void 0,
              [name === "search" ? "q" : name]: void 0
            };
            if (name === "revision") next.artifactHash = void 0;
            actions2.navigate({ kind: "timeline", historyQuery: next });
          });
          return chip;
        })
      );
      document.querySelector("#filter-chips-empty")?.classList.toggle("hidden", values2.length > 0);
      const toggle2 = document.querySelector("#filters-toggle");
      if (toggle2)
        toggle2.textContent = values2.length ? `Filters · ${values2.length}` : "Filters";
      document.querySelectorAll(".timeline-filter").forEach((element) => {
        element.classList.remove("hidden");
      });
      document.querySelectorAll(".change-filter").forEach((element) => {
        element.classList.add("hidden");
      });
      if (history2 !== null) {
        syncTimelineTypeFacets(route, history2, actions2);
        replaceTimelineSelectOptions(
          "timeline-filter-track",
          history2.completion.trackIds.map((trackId) => ({
            value: trackId,
            label: trackId
          })),
          route.historyQuery.track
        );
        replaceTimelineSelectOptions(
          "timeline-filter-change",
          history2.completion.changeIds.map((changeId) => ({
            value: changeId,
            label: shortRef(changeId),
            title: changeId,
            accessibleName: `Change ${changeId}`
          })),
          route.historyQuery.change
        );
        replaceTimelineSelectOptions(
          "timeline-filter-revision",
          history2.completion.revisionRefs.map((revision2) => ({
            value: exactRevisionOptionValue(
              revision2.revisionId,
              revision2.objectArtifactContentHash
            ),
            label: shortExactRevision(revision2),
            artifactHash: revision2.objectArtifactContentHash,
            revisionId: revision2.revisionId,
            title: exactRevisionAccessibleIdentity(revision2),
            accessibleName: exactRevisionAccessibleIdentity(revision2)
          })),
          route.historyQuery.revision && route.historyQuery.artifactHash ? exactRevisionOptionValue(
            route.historyQuery.revision,
            route.historyQuery.artifactHash
          ) : void 0
        );
      }
      document.querySelector("#view-order-section")?.classList.remove("hidden");
      const newest = document.querySelector("#order-newest");
      const oldest = document.querySelector("#order-oldest");
      if (newest) newest.checked = route.historyQuery.order !== "asc";
      if (oldest) oldest.checked = route.historyQuery.order === "asc";
      return;
    }
    document.querySelectorAll(".timeline-filter").forEach((element) => {
      element.classList.add("hidden");
    });
    document.querySelectorAll(".change-filter").forEach((element) => {
      element.classList.remove("hidden");
    });
    document.querySelector("#view-order-section")?.classList.add("hidden");
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
  function appendDefinition(list, label2, value, accessibleName = value) {
    const term = document.createElement("dt");
    term.textContent = label2;
    const definition = document.createElement("dd");
    setCompactIdentityText(definition, value, accessibleName);
    list.append(term, definition);
    return definition;
  }
  __name(appendDefinition, "appendDefinition");
  function renderEventDetail(event, actions2) {
    const presentation = presentEvent(event);
    const heading = detailHeading("Event");
    const identity = detailLine(event.eventId, "mono");
    identity.title = event.eventId;
    identity.setAttribute("aria-label", `event ${event.eventId}`);
    identity.dataset.eventId = event.eventId;
    const summary = document.createElement("section");
    summary.className = "event-detail-summary";
    summary.append(detailHeading(presentation.title, 3));
    if (presentation.body) summary.append(detailLine(presentation.body));
    const summaryFacts = document.createElement("dl");
    summaryFacts.className = "kv";
    for (const item of presentation.fields) {
      appendDefinition(summaryFacts, item.label, item.value);
    }
    if (presentation.fields.length) summary.append(summaryFacts);
    const attribution = document.createElement("section");
    attribution.className = "event-detail-attribution";
    attribution.append(detailHeading("Subject and attribution", 3));
    const attributionFacts = document.createElement("dl");
    attributionFacts.className = "kv";
    const attributionAdd = /* @__PURE__ */ __name((name, value) => appendDefinition(attributionFacts, name, value), "attributionAdd");
    attributionAdd("subject", eventSubjectLabel(event.subject));
    attributionAdd("writer", event.writer.actorId);
    attributionAdd(
      "producer",
      `${event.writer.producer.name} ${event.writer.producer.version}`
    );
    attributionAdd("assertion", event.assertionMode.replaceAll("_", " "));
    if (event.signer) attributionAdd("signer", event.signer);
    if (event.sourceRef) {
      attributionAdd(
        "source",
        `${event.sourceRef.sourceSystem} · ${event.sourceRef.sourceId}`
      );
    }
    if (event.ingest) {
      attributionAdd(
        "ingest",
        `${event.ingest.via} · ${event.ingest.receivedAt}`
      );
    }
    attribution.append(attributionFacts);
    const record = document.createElement("section");
    record.className = "event-detail-record";
    record.append(detailHeading("Event record", 3));
    const facts = document.createElement("dl");
    facts.className = "kv";
    const add = /* @__PURE__ */ __name((name, value, accessibleName = value) => appendDefinition(facts, name, value, accessibleName), "add");
    add("type", event.eventType.replaceAll("_", " "));
    add("occurred", event.occurredAt);
    add("verification", event.verificationStatus.replaceAll("_", " "));
    const payload = add(
      "event payload",
      event.payloadHash,
      `artifact ${event.payloadHash}`
    );
    payload.dataset.artifactHash = event.payloadHash;
    const journal = add("journal", event.journalId, `journal ${event.journalId}`);
    journal.dataset.journalId = event.journalId;
    if (event.trackId) add("track", event.trackId);
    if (event.signer) add("signer", event.signer);
    add("Changes", event.changeIds.join("; ") || "none");
    add(
      "exact Revisions",
      event.revisionRefs.map(
        (revision2) => `${revision2.revisionId} · ${revision2.objectArtifactContentHash}`
      ).join("; ") || "none"
    );
    if (event.unresolvedRevisionIds.length)
      add("unresolved Revisions", event.unresolvedRevisionIds.join("; "));
    record.append(facts);
    const context = document.createElement("section");
    context.className = "actions event-detail-actions";
    const onlyChange = event.changeIds.length === 1 ? event.changeIds[0] : null;
    const annotatedDiff = eventAnnotatedDiffRoute(event);
    if (annotatedDiff) {
      const activation = document.createElement("button");
      activation.type = "button";
      activation.className = "ghost detail-action-primary";
      activation.dataset.exactDiffActivation = "true";
      activation.textContent = "Open annotated diff";
      activation.addEventListener("click", () => actions2.navigate(annotatedDiff));
      context.append(activation);
    }
    for (const changeId of event.changeIds) {
      const change = document.createElement("button");
      change.type = "button";
      change.className = "ghost";
      change.dataset.eventChangeChoice = changeId;
      change.dataset.changeId = changeId;
      change.textContent = event.changeIds.length === 1 ? "Open Change" : `Open Change ${shortRef(changeId)}`;
      change.title = `Change ${changeId}`;
      change.setAttribute("aria-label", `Open Change ${changeId}`);
      change.addEventListener(
        "click",
        () => actions2.navigate({ kind: "change", changeId, query: {} })
      );
      context.append(change);
    }
    if (onlyChange) {
      for (const exactRevision of event.revisionRefs) {
        const revision2 = document.createElement("button");
        revision2.type = "button";
        revision2.className = "ghost";
        revision2.dataset.eventRevisionChoice = formatChangeInspectorRoute({
          kind: "revision",
          changeId: onlyChange,
          revision: exactRevision,
          query: {}
        });
        revision2.textContent = event.revisionRefs.length === 1 ? "Open exact Revision" : `Open exact Revision ${shortExact(exactRevision)}`;
        revision2.title = exactRevisionAccessibleIdentity(exactRevision);
        revision2.setAttribute(
          "aria-label",
          `Open ${exactRevisionAccessibleIdentity(exactRevision)} for Change ${onlyChange}`
        );
        revision2.dataset.changeId = onlyChange;
        revision2.dataset.revisionId = exactRevision.revisionId;
        revision2.dataset.artifactHash = exactRevision.objectArtifactContentHash;
        revision2.addEventListener(
          "click",
          () => actions2.navigate({
            kind: "revision",
            changeId: onlyChange,
            revision: exactRevision,
            query: {}
          })
        );
        context.append(revision2);
      }
    }
    if (!annotatedDiff) {
      const refusal = detailLine(
        "Opening an annotated diff requires exactly one Change and one exact Revision, with no unresolved Revisions. Choose an explicit context where available.",
        "dim"
      );
      refusal.dataset.eventDiffRefusal = "true";
      refusal.setAttribute("role", "status");
      refusal.tabIndex = -1;
      context.append(refusal);
    }
    const copyLink = document.createElement("button");
    copyLink.type = "button";
    copyLink.className = "ghost";
    copyLink.textContent = "Copy link";
    copyLink.addEventListener("click", () => copyExact(location.href));
    context.append(copyLink);
    const structured = document.createElement("details");
    structured.className = "event-structured";
    const structuredLabel = document.createElement("summary");
    structuredLabel.textContent = "Structured event data";
    const raw = document.createElement("pre");
    raw.className = "anno-body mono";
    raw.textContent = JSON.stringify(
      {
        summary: event.summary,
        subject: event.subject,
        writer: event.writer,
        sourceRef: event.sourceRef,
        ingest: event.ingest
      },
      null,
      2
    );
    structured.append(structuredLabel, raw);
    return [heading, identity, summary, attribution, record, context, structured];
  }
  __name(renderEventDetail, "renderEventDetail");
  function detailHeading(text, level = 2) {
    const heading = document.createElement(`h${level}`);
    setCompactIdentityText(heading, text);
    return heading;
  }
  __name(detailHeading, "detailHeading");
  function detailLine(text, className) {
    const line = document.createElement("p");
    if (className) line.className = className;
    setCompactIdentityText(line, text);
    return line;
  }
  __name(detailLine, "detailLine");
  function detailState(entries) {
    const list = document.createElement("dl");
    list.className = "detail-state";
    for (const [label2, value] of entries) {
      const term = document.createElement("dt");
      term.textContent = label2;
      const description = document.createElement("dd");
      setCompactIdentityText(description, value);
      list.append(term, description);
    }
    return list;
  }
  __name(detailState, "detailState");
  function detailIdentity(revision2) {
    const line = document.createElement("p");
    line.className = "detail-identity";
    line.append(exactRevisionIdentity2(revision2));
    return line;
  }
  __name(detailIdentity, "detailIdentity");
  function detailActions(...controls) {
    const actions2 = document.createElement("div");
    actions2.className = "detail-actions";
    actions2.append(...controls);
    return actions2;
  }
  __name(detailActions, "detailActions");
  function showChangeInTimeline(changeId) {
    const link = document.createElement("a");
    link.className = "ghost";
    link.textContent = "Show in Timeline";
    link.href = formatChangeInspectorRoute(showChangeInTimelineRoute(changeId));
    link.setAttribute("aria-label", `Show Change ${changeId} in Timeline`);
    return link;
  }
  __name(showChangeInTimeline, "showChangeInTimeline");
  function showRevisionInTimeline(changeId, revision2) {
    const link = document.createElement("a");
    link.className = "ghost";
    link.textContent = "Show in Timeline";
    link.href = formatChangeInspectorRoute(
      showRevisionInTimelineRoute(changeId, revision2)
    );
    link.setAttribute(
      "aria-label",
      `Show exact Revision ${revision2.revisionId} with artifact ${revision2.objectArtifactContentHash} for Change ${changeId} in Timeline`
    );
    return link;
  }
  __name(showRevisionInTimeline, "showRevisionInTimeline");
  function shortExact(revision2) {
    return shortExactRevision(revision2);
  }
  __name(shortExact, "shortExact");
  function exactRevisionText(revision2) {
    return `${revision2.revisionId} · ${revision2.objectArtifactContentHash}`;
  }
  __name(exactRevisionText, "exactRevisionText");
  function exactRevisionIdentity2(revision2, className = "mono") {
    const identity = document.createElement("code");
    identity.className = className;
    identity.textContent = shortExact(revision2);
    identity.title = exactRevisionAccessibleIdentity(revision2);
    identity.setAttribute(
      "aria-label",
      exactRevisionAccessibleIdentity(revision2)
    );
    identity.dataset.revisionId = revision2.revisionId;
    identity.dataset.artifactHash = revision2.objectArtifactContentHash;
    return identity;
  }
  __name(exactRevisionIdentity2, "exactRevisionIdentity");
  function renderedFactBody(content, contentType) {
    const body = document.createElement("div");
    body.className = "anno-body";
    const text = content.kind === "observation" || content.kind === "input_request" ? content.body : content.kind === "assessment" || content.kind === "validation" ? content.summary : void 0;
    if (text) body.innerHTML = renderBodyContent(text, contentType);
    return body;
  }
  __name(renderedFactBody, "renderedFactBody");
  function renderFacts(reading, route, actions2) {
    const facts = document.createElement("section");
    facts.className = "detail-facts";
    facts.append(detailHeading("Facts", 3));
    const groups = /* @__PURE__ */ new Map();
    for (const fact2 of reading.document.factPresentations) {
      const family = groups.get(fact2.family) ?? [];
      family.push(fact2);
      groups.set(fact2.family, family);
    }
    for (const [family, items] of groups) {
      const group = document.createElement("section");
      group.append(detailHeading(family.replaceAll("_", " "), 4));
      for (const fact2 of items) {
        const card = document.createElement("article");
        card.className = "unit-card";
        card.dataset.factId = fact2.factId;
        card.tabIndex = -1;
        const content = reading.document.factContentPresentations?.[fact2.factId];
        if (content) {
          const heading = content.content.kind === "assessment" ? `Assessment: ${content.content.assessment}` : content.content.kind === "validation" ? content.content.checkName : content.content.title;
          card.append(detailHeading(heading, 5));
        }
        const factIdentity = document.createElement("p");
        factIdentity.className = "detail-fact-identity";
        const factCode = document.createElement("code");
        factCode.textContent = shortRef(fact2.factId);
        factCode.title = fact2.factId;
        factCode.setAttribute("aria-label", `${fact2.family} ${fact2.factId}`);
        factIdentity.append(factCode);
        card.append(
          factIdentity,
          detailLine(
            `origin: ${exactRevisionText(fact2.originRevision)} · context: ${fact2.contextChangeId ?? "unavailable"} · currency: ${fact2.revisionCurrency.replaceAll("_", " ")}`
          ),
          detailLine(
            `family: ${fact2.familyState.replaceAll("_", " ")} · availability: ${fact2.availability.replaceAll("_", " ")} · actor: ${fact2.actorId}${fact2.trackId ? ` · track: ${fact2.trackId}` : ""}`
          )
        );
        const presentedInRevision = fact2.presentedInRevision;
        if (presentedInRevision) {
          const applicablePort = reading.document.factPorts.find(
            (port) => port.applicability === "applicable" && port.originRevision.revisionId === fact2.originRevision.revisionId && port.originRevision.objectArtifactContentHash === fact2.originRevision.objectArtifactContentHash && port.targetRevision.revisionId === presentedInRevision.revisionId && port.targetRevision.objectArtifactContentHash === presentedInRevision.objectArtifactContentHash && factRefLabel(port.originFact) === factRefLabelFromFactId(fact2)
          );
          card.append(
            detailLine(
              `presented in: ${exactRevisionText(presentedInRevision)} · port: ${fact2.portRelation?.replaceAll("_", " ") ?? (applicablePort ? `${applicablePort.relation.replaceAll("_", " ")} (${applicablePort.portId})` : "see Fact ports")}`
            )
          );
        }
        if (content) {
          card.append(
            detailLine(
              `body: ${content.bodyContentState.replaceAll("_", " ")} · ${content.contentType}`
            ),
            renderedFactBody(content.content, content.contentType)
          );
        }
        const focus = document.createElement("button");
        focus.type = "button";
        focus.className = "ghost";
        focus.textContent = "Focus fact";
        focus.addEventListener(
          "click",
          () => actions2.navigate({
            kind: route.kind,
            changeId: route.changeId,
            revision: route.revision,
            query: queryForExactNavigation(route),
            focus: { factId: fact2.factId }
          })
        );
        card.append(focus);
        group.append(card);
      }
      facts.append(group);
    }
    if (groups.size === 0) facts.append(message("No facts."));
    return facts;
  }
  __name(renderFacts, "renderFacts");
  function factRefLabel(fact2) {
    return fact2.kind === "observation" ? `observation: ${fact2.observationId}` : `input request: ${fact2.inputRequestId}`;
  }
  __name(factRefLabel, "factRefLabel");
  function factRefLabelFromFactId(fact2) {
    return fact2.family === "observation" ? `observation: ${fact2.factId}` : fact2.family === "input_request" ? `input request: ${fact2.factId}` : "";
  }
  __name(factRefLabelFromFactId, "factRefLabelFromFactId");
  function renderFactPorts(reading) {
    const ports = document.createElement("section");
    ports.append(detailHeading("Fact ports", 3));
    for (const port of reading.document.factPorts) {
      const item = document.createElement("article");
      item.className = "unit-card";
      item.append(
        detailLine(port.portId, "mono"),
        detailLine(
          `origin: ${factRefLabel(port.originFact)} · ${exactRevisionText(port.originRevision)}`
        ),
        detailLine(
          `target: ${exactRevisionText(port.targetRevision)} · ${port.relation.replaceAll("_", " ")}`
        ),
        detailLine(
          `target fact: ${port.targetFact ? factRefLabel(port.targetFact) : "none"} · applicability: ${port.applicability.replaceAll("_", " ")}`
        ),
        detailLine(
          `actor: ${port.actorId}${port.trackId ? ` · track: ${port.trackId}` : ""}${port.contextChangeId ? ` · context: ${port.contextChangeId}` : ""}`
        )
      );
      if (port.rationaleContentHash)
        item.append(
          detailLine(`rationale: ${port.rationaleContentHash}`, "mono")
        );
      for (const diagnostic of port.diagnostics)
        item.append(detailLine(diagnostic));
      ports.append(item);
    }
    if (reading.document.factPorts.length === 0)
      ports.append(message("No fact ports."));
    return ports;
  }
  __name(renderFactPorts, "renderFactPorts");
  function openCapturedResource(route, actions2) {
    const button2 = document.createElement("button");
    button2.type = "button";
    button2.className = "ghost";
    button2.textContent = "Open authoritative captured diff";
    button2.addEventListener(
      "click",
      () => actions2.navigate({
        kind: "resource",
        changeId: route.changeId,
        revision: route.revision,
        query: route.query,
        ...route.focus ? { focus: route.focus } : {}
      })
    );
    return button2;
  }
  __name(openCapturedResource, "openCapturedResource");
  function openAnnotatedDiff(route, actions2) {
    const button2 = document.createElement("button");
    button2.type = "button";
    button2.className = "ghost detail-action-primary";
    button2.dataset.exactDiffActivation = "true";
    button2.textContent = "Open annotated diff";
    button2.addEventListener(
      "click",
      () => actions2.navigate({
        kind: "diff",
        changeId: route.changeId,
        revision: route.revision,
        query: route.query,
        ...route.focus ? { focus: route.focus } : {}
      })
    );
    return button2;
  }
  __name(openAnnotatedDiff, "openAnnotatedDiff");
  function renderAssociations(reading, route, actions2) {
    const section = document.createElement("section");
    section.append(detailHeading("Association comparisons", 3));
    for (const association of reading.document.associations) {
      const item = document.createElement("article");
      item.className = "unit-card";
      item.append(
        detailLine(association.comparison.associationId, "mono"),
        detailLine(
          `commit: ${association.comparison.commitOid} · base: ${association.comparison.comparisonBase}`
        ),
        detailLine(
          `view: ${association.comparison.viewKind} · state: ${association.state} · proof: ${association.proofAvailability}`
        )
      );
      if (association.comparison.proofRef)
        item.append(
          detailLine(`proof: ${association.comparison.proofRef}`, "mono")
        );
      for (const diagnostic of association.diagnostics)
        item.append(detailLine(diagnostic));
      item.append(openCapturedResource(route, actions2));
      section.append(item);
    }
    if (reading.document.associations.length === 0)
      section.append(message("No association comparisons."));
    return section;
  }
  __name(renderAssociations, "renderAssociations");
  function capturedDiffArtifact2(documentValue) {
    if (typeof documentValue !== "object" || documentValue === null) return null;
    const documentRecord = documentValue;
    const snapshot2 = documentRecord.snapshot;
    if (typeof snapshot2 !== "object" || snapshot2 === null) return null;
    const files = snapshot2.files;
    return Array.isArray(files) ? documentValue : null;
  }
  __name(capturedDiffArtifact2, "capturedDiffArtifact");
  function bindCapturedDiffInteractions(diff, rendered) {
    rendered.ctx.files.forEach((file, index) => {
      const section = diff.querySelector(`[data-dfile="${index}"]`);
      const header = section?.querySelector(".dfile-head");
      const body = section?.querySelector(
        `[data-dfile-body="${index}"]`
      );
      const renderBody = /* @__PURE__ */ __name(() => {
        if (!section || !body) return;
        body.innerHTML = renderDiffFileBody(file, rendered.ctx.anchored);
        body.dataset.rendered = "1";
        section.dataset.expanded = "true";
      }, "renderBody");
      const toggle = /* @__PURE__ */ __name(() => {
        if (!section || !body) return;
        if (section.dataset.expanded === "true") {
          section.dataset.expanded = "false";
          return;
        }
        renderBody();
      }, "toggle");
      header?.addEventListener("click", toggle);
      header?.addEventListener("keydown", (event) => {
        if (event.key !== "Enter" && event.key !== " ") return;
        event.preventDefault();
        toggle();
      });
      body?.addEventListener("click", (event) => {
        const trigger = event.target?.closest(
          "[data-render-diff-file]"
        );
        if (!trigger) return;
        event.preventDefault();
        renderBody();
      });
    });
  }
  __name(bindCapturedDiffInteractions, "bindCapturedDiffInteractions");
  function renderCapturedDiff(resource) {
    const artifact = capturedDiffArtifact2(resource.capturedDocument);
    if (artifact === null) {
      const refusal = document.createElement("section");
      refusal.className = "detail-facts";
      refusal.append(
        message(
          "This exact resource does not contain a captured snapshot. The Inspector will not reconstruct a diff from Git or an associated commit."
        )
      );
      return refusal;
    }
    const diff = document.createElement("section");
    diff.className = "captured-diff";
    const rendered = renderDiff(resource.resource.objectId, artifact, []);
    diff.innerHTML = rendered.html;
    rendered.ctx.files.forEach((file, index) => {
      const section = diff.querySelector(`[data-dfile="${index}"]`);
      const path = file.new_path ?? file.old_path;
      if (section && path) {
        section.dataset.filePath = path;
        if (file.old_path) section.dataset.oldFilePath = file.old_path;
        if (file.new_path) section.dataset.newFilePath = file.new_path;
        section.tabIndex = -1;
      }
    });
    bindCapturedDiffInteractions(diff, rendered);
    return diff;
  }
  __name(renderCapturedDiff, "renderCapturedDiff");
  function renderCapturedResource(resource, route, actions2) {
    const nodes = [
      detailHeading("Authoritative captured diff"),
      detailLine(exactRevisionText(resource.resource.revision), "mono"),
      detailLine(`availability: ${resource.availability.replaceAll("_", " ")}`)
    ];
    if (resource.availability !== "available") {
      nodes.push(
        message(
          "Captured bytes are unavailable. No live or associated-commit bytes were substituted."
        )
      );
    } else {
      nodes.push(
        detailLine(`captured document: ${resource.capturedDocumentHash}`, "mono")
      );
      nodes.push(renderCapturedDiff(resource));
    }
    for (const diagnostic of resource.diagnostics)
      nodes.push(detailLine(diagnostic));
    const back = document.createElement("button");
    back.type = "button";
    back.className = "ghost";
    back.textContent = "Back to exact Revision";
    back.addEventListener(
      "click",
      () => actions2.navigate({
        kind: "revision",
        changeId: route.changeId,
        revision: route.revision,
        query: route.query,
        ...route.focus ? { focus: route.focus } : {}
      })
    );
    nodes.push(back);
    return nodes;
  }
  __name(renderCapturedResource, "renderCapturedResource");
  function renderCurrentRevisionChoices(changeId, revisions, query, actions2) {
    const choices = document.createElement("section");
    choices.className = "detail-current-revisions";
    choices.append(detailHeading("Current Revisions", 3));
    if (revisions.length === 0) {
      choices.append(message("No current Revision is available."));
      return choices;
    }
    for (const revision2 of revisions) {
      const button2 = document.createElement("button");
      button2.type = "button";
      button2.className = "ghost mono";
      button2.textContent = shortExact(revision2);
      button2.title = exactRevisionAccessibleIdentity(revision2);
      button2.setAttribute(
        "aria-label",
        `Current Revision: open ${exactRevisionAccessibleIdentity(revision2)}; for Change ${changeId}`
      );
      button2.dataset.changeId = changeId;
      button2.dataset.revisionId = revision2.revisionId;
      button2.dataset.artifactHash = revision2.objectArtifactContentHash;
      button2.addEventListener(
        "click",
        () => actions2.navigate({
          kind: "revision",
          changeId,
          revision: revision2,
          query
        })
      );
      choices.append(button2);
    }
    return choices;
  }
  __name(renderCurrentRevisionChoices, "renderCurrentRevisionChoices");
  function renderChangeRelationshipGraph(detail, route, actions2) {
    const graph = detail.inspectorPresentation?.revisionGraph;
    if (!graph) return null;
    const section = document.createElement("section");
    section.className = "detail-relationships";
    section.append(
      detailHeading("Revision relationships", 3),
      renderChangeRevisionGraph(graph, {
        document,
        onActivateRevision: /* @__PURE__ */ __name((revision2) => actions2.navigate({
          kind: "revision",
          changeId: route.changeId,
          revision: revision2,
          query: route.query
        }), "onActivateRevision")
      })
    );
    return section;
  }
  __name(renderChangeRelationshipGraph, "renderChangeRelationshipGraph");
  function renderExactFactRelationshipGraph(detail, route, actions2) {
    const graph = detail.inspectorPresentation?.factGraph;
    if (!graph) return null;
    const section = document.createElement("section");
    section.className = "detail-relationships";
    section.append(
      detailHeading("Fact relationships", 3),
      renderFactRelationshipGraph(graph, {
        document,
        onActivateRevision: /* @__PURE__ */ __name((revision2) => actions2.navigate({
          kind: "revision",
          changeId: route.changeId,
          revision: revision2,
          query: queryForExactNavigation(route)
        }), "onActivateRevision"),
        onFocusFact: /* @__PURE__ */ __name((focus) => actions2.navigate({
          kind: "revision",
          changeId: route.changeId,
          revision: focus.revision,
          query: queryForExactNavigation(route),
          focus: { factId: focus.factId }
        }), "onFocusFact")
      })
    );
    return section;
  }
  __name(renderExactFactRelationshipGraph, "renderExactFactRelationshipGraph");
  function renderChangeDetail(detail, route, actions2) {
    const changeIdentity = document.createElement("p");
    changeIdentity.className = "detail-identity";
    const changeCode = document.createElement("code");
    changeCode.textContent = shortRef(detail.summary.changeId);
    changeCode.title = detail.summary.changeId;
    changeCode.setAttribute("aria-label", `Change ${detail.summary.changeId}`);
    changeIdentity.append(changeCode);
    const nodes = [
      detailHeading(
        detail.summary.titleAssertions.length === 1 ? detail.summary.titleAssertions[0] : "Change"
      ),
      changeIdentity,
      detailState([
        ["Topology", detail.summary.topology.replaceAll("_", " ")],
        ["Lifecycle", detail.summary.lifecycle.replaceAll("_", " ")],
        ["Attention", detail.summary.attentionSummary.replaceAll("_", " ")],
        ["Availability", detail.summary.availabilitySummary.replaceAll("_", " ")],
        ["Members", String(detail.summary.memberCount)],
        ["Current peers", String(detail.currentRevisionRefs.length)]
      ]),
      detailActions(showChangeInTimeline(route.changeId)),
      renderCurrentRevisionChoices(
        route.changeId,
        detail.currentRevisionRefs,
        route.query,
        actions2
      )
    ];
    const relationships = renderChangeRelationshipGraph(detail, route, actions2);
    if (relationships) nodes.push(relationships);
    if (detail.summary.titleAssertions.length > 1) {
      const titles = document.createElement("section");
      titles.className = "detail-notice";
      titles.append(detailHeading("Title assertions", 3));
      for (const title of detail.summary.titleAssertions)
        titles.append(detailLine(title));
      nodes.push(titles);
    }
    if (detail.operativeObligations.length > 0) {
      const obligations = document.createElement("section");
      obligations.className = "detail-notice detail-notice-warning";
      obligations.append(detailHeading("What needs attention", 3));
      for (const obligation of detail.operativeObligations)
        obligations.append(detailLine(obligation));
      nodes.push(obligations);
    }
    const sections = [
      [
        "Member Revisions",
        detail.memberRevisions.map(
          (member) => `${exactRevisionText(member.revision)} · support: ${member.supportingClaimIds.join(", ") || "none"}`
        )
      ],
      [
        "Unavailable Members",
        detail.unavailableMemberRevisions.map(
          (member) => `${member.revisionId} · ${member.reason.replaceAll("_", " ")} · support: ${member.supportingClaimIds.join(", ") || "none"}`
        )
      ],
      [
        "Membership Claims",
        detail.membershipClaims.map(
          (claim) => `${claim.claimId} · revision: ${claim.revisionId} · ${claim.active ? "active" : "inactive"} · supports: ${claim.supports.length} · withdrawals: ${claim.withdrawals.length}`
        )
      ],
      [
        "Membership Withdrawals",
        detail.membershipWithdrawals.map(
          (withdrawal) => `${withdrawal.claimId} · support carriers: ${withdrawal.supports.length}`
        )
      ],
      [
        "Revision Relation Claims",
        detail.relationClaims.map(
          (claim) => `${claim.claimId} · ${exactRevisionText(claim.predecessor)} → ${exactRevisionText(claim.successor)} · ${claim.active ? "active" : "inactive"}`
        )
      ],
      [
        "Relation Withdrawals",
        detail.relationWithdrawals.map(
          (withdrawal) => `${withdrawal.claimId} · support carriers: ${withdrawal.supports.length}`
        )
      ],
      [
        "Effective Supersedes",
        detail.effectiveSupersedes.map(
          ([successor, predecessor]) => `${exactRevisionText(predecessor)} → ${exactRevisionText(successor)}`
        )
      ],
      [
        "Pending or Conflicting Edges",
        detail.pendingOrConflictingEdges.map(
          (claim) => `${claim.claimId} · ${exactRevisionText(claim.predecessor)} → ${exactRevisionText(claim.successor)} · ${claim.active ? "active" : "inactive"}`
        )
      ],
      [
        "Change Links",
        detail.links.map(
          (link) => `${link.leftChangeId} · ${link.relation.replaceAll("_", " ")} · ${link.rightChangeId}`
        )
      ],
      [
        "Current Revision Qualification",
        detail.perCurrentRevisionQualification.map(
          (qualification) => `${exactRevisionText(qualification.revision)} · ${qualification.qualified ? "qualified" : "not qualified"}`
        )
      ],
      ["Diagnostics", detail.diagnostics]
    ];
    const record = document.createElement("details");
    record.className = "detail-record";
    const recordLabel = document.createElement("summary");
    recordLabel.textContent = "Recorded claims and diagnostics";
    record.append(recordLabel);
    for (const [title, entries] of sections) {
      const section = document.createElement("section");
      section.append(detailHeading(title, 3));
      if (entries.length === 0) section.append(message("None."));
      for (const entry of entries) section.append(detailLine(entry));
      record.append(section);
    }
    nodes.push(record);
    return nodes;
  }
  __name(renderChangeDetail, "renderChangeDetail");
  function renderReading(reading, snapshot2, actions2) {
    const route = snapshot2.route;
    const copy = document.createElement("button");
    copy.type = "button";
    copy.className = "ghost";
    copy.textContent = "Copy link";
    copy.addEventListener("click", () => copyExact(location.href));
    if (reading.kind === "change" && route.kind === "change") {
      return [...renderChangeDetail(reading.document, route, actions2), copy];
    }
    if (reading.kind === "resource" && route.kind === "resource")
      return [...renderCapturedResource(reading.document, route, actions2), copy];
    if (reading.kind === "interdiff" && route.kind === "interdiff") {
      const nodes = [
        detailHeading("Ordered Revision interdiff"),
        detailLine(
          `${exactRevisionText(reading.document.interdiff.from)} → ${exactRevisionText(reading.document.interdiff.to)}`,
          "mono"
        ),
        detailLine(
          `availability: ${reading.document.availability.replaceAll("_", " ")} · algorithm: ${reading.document.interdiff.algorithmVersion}`
        ),
        detailLine("This is a comparison, not the authoritative captured diff.")
      ];
      if (reading.document.comparison !== void 0) {
        const comparison = document.createElement("pre");
        comparison.textContent = JSON.stringify(
          reading.document.comparison,
          null,
          2
        );
        nodes.push(comparison);
      }
      for (const diagnostic of reading.document.diagnostics)
        nodes.push(detailLine(diagnostic));
      for (const revision2 of [route.from, route.to]) {
        const button2 = document.createElement("button");
        button2.type = "button";
        button2.className = "ghost";
        button2.textContent = `Open authoritative captured diff: ${shortExact(revision2)}`;
        button2.title = exactRevisionAccessibleIdentity(revision2);
        button2.setAttribute(
          "aria-label",
          `Open authoritative captured diff for ${exactRevisionAccessibleIdentity(revision2)}; Change ${route.changeId}`
        );
        button2.dataset.changeId = route.changeId;
        button2.dataset.revisionId = revision2.revisionId;
        button2.dataset.artifactHash = revision2.objectArtifactContentHash;
        button2.addEventListener(
          "click",
          () => actions2.navigate({
            kind: "resource",
            changeId: route.changeId,
            revision: revision2,
            query: route.query
          })
        );
        nodes.push(button2);
      }
      nodes.push(copy);
      return nodes;
    }
    if ((reading.kind === "revision" || reading.kind === "association") && (route.kind === "revision" || route.kind === "association")) {
      const document2 = reading.document;
      const nodes = [
        detailHeading(
          reading.kind === "association" ? "Association comparisons" : "Exact Revision"
        ),
        detailIdentity(document2.revision),
        detailState([
          ["Currency", document2.revisionCurrency.replaceAll("_", " ")],
          ["Relation", document2.relationClassification.replaceAll("_", " ")],
          ["Captured resource", document2.availability.replaceAll("_", " ")],
          ["Facts", String(document2.factPresentations.length)],
          ["Associations", String(document2.associations.length)]
        ])
      ];
      if (reading.kind === "revision") {
        const factRelationships = route.kind === "revision" ? renderExactFactRelationshipGraph(document2, route, actions2) : null;
        nodes.push(
          detailActions(
            openAnnotatedDiff(route, actions2),
            openCapturedResource(route, actions2),
            showRevisionInTimeline(route.changeId, route.revision)
          ),
          ...factRelationships ? [factRelationships] : [],
          renderFacts(reading, route, actions2),
          renderFactPorts(reading)
        );
      }
      nodes.push(renderAssociations(reading, route, actions2));
      if (reading.kind === "association")
        nodes.push(
          detailActions(
            openAnnotatedDiff(route, actions2),
            openCapturedResource(route, actions2)
          )
        );
      nodes.push(copy);
      return nodes;
    }
    return [
      message("Reading surface no longer matches the selected exact route.")
    ];
  }
  __name(renderReading, "renderReading");
  function exactFocusTarget(detail, route) {
    if (route.kind !== "revision" && route.kind !== "resource" && route.kind !== "association" && route.kind !== "interdiff") {
      return null;
    }
    const focus = route.focus;
    if (!focus) return null;
    const targets = [];
    if (focus.factId) {
      const fact2 = Array.from(
        detail.querySelectorAll("[data-fact-id], [data-anno]")
      ).find(
        (element) => element.dataset.factId === focus.factId || element.dataset.anno === focus.factId
      );
      if (fact2) targets.push(fact2);
    }
    if (focus.filePath) {
      const file = Array.from(
        detail.querySelectorAll("[data-file-path]")
      ).find(
        (element) => element.dataset.filePath === focus.filePath || element.dataset.oldFilePath === focus.filePath || element.dataset.newFilePath === focus.filePath
      );
      if (file) targets.push(file);
    }
    for (const target of targets) {
      target.dataset.exactFocus = "true";
      target.setAttribute("aria-current", "true");
    }
    return targets[0] ?? null;
  }
  __name(exactFocusTarget, "exactFocusTarget");
  function applyExactFocus(detail, route) {
    const target = exactFocusTarget(detail, route);
    if (!target) return;
    if (target.dataset.dfile !== void 0 && target.dataset.expanded !== "true") {
      target.querySelector(".dfile-head")?.click();
    }
    target.scrollIntoView?.({ block: "center", behavior: "auto" });
    target.focus({ preventScroll: true });
  }
  __name(applyExactFocus, "applyExactFocus");
  function renderDetail(snapshot2, actions2, presentation) {
    const detail = document.querySelector("#detail-body");
    if (!detail) return;
    if (snapshot2.route.kind === "invalid") {
      replaceDetailWith(message(snapshot2.route.message));
      return;
    }
    if (snapshot2.diagnostic) {
      replaceDetailWith(message(snapshot2.diagnostic));
      return;
    }
    if (snapshot2.route.kind === "timeline" || snapshot2.route.kind === "lens" || snapshot2.generation === null) {
      replaceDetailWith(message("Select a Change or exact Revision."));
      return;
    }
    if (snapshot2.route.kind === "event") {
      const route = snapshot2.route;
      const event = snapshot2.generation.history?.entries.find(
        (entry) => entry.eventId === route.eventId
      );
      replaceDetailWith(
        ...event ? renderEventDetail(event, actions2) : [
          detailHeading("Event"),
          message(
            "This exact event was not present in the bounded Timeline response."
          )
        ]
      );
      return;
    }
    if (presentation.refusal !== null) {
      replaceDetailWith(
        message(`Reader refused this exact surface: ${presentation.refusal}`)
      );
      return;
    }
    if (presentation.reading !== null) {
      const readingKey = `${formatChangeInspectorRoute(snapshot2.route)}:${presentation.reading.document.projectionStamp}`;
      if (detail.dataset.changeReadingKey !== readingKey) {
        detail.replaceChildren(
          ...renderReading(presentation.reading, snapshot2, actions2)
        );
        detail.dataset.changeReadingKey = readingKey;
        applyExactFocus(detail, snapshot2.route);
      }
      return;
    }
    const heading = document.createElement("h2");
    heading.textContent = snapshot2.route.kind === "change" ? "Change" : snapshot2.route.kind === "resource" ? "Captured resource" : snapshot2.route.kind === "association" ? "Association comparison" : snapshot2.route.kind === "interdiff" ? "Revision interdiff" : "Exact Revision";
    const identity = document.createElement("p");
    identity.className = "mono";
    const identityText = snapshot2.route.kind === "change" ? `Change ID: ${snapshot2.route.changeId}` : snapshot2.route.kind === "interdiff" ? `From: ${snapshot2.route.from.revisionId} · ${snapshot2.route.from.objectArtifactContentHash}
To: ${snapshot2.route.to.revisionId} · ${snapshot2.route.to.objectArtifactContentHash}` : `Revision ID: ${snapshot2.route.revision.revisionId} · artifact hash: ${snapshot2.route.revision.objectArtifactContentHash}`;
    setCompactIdentityText(identity, identityText);
    const placeholder = message(
      snapshot2.route.kind === "change" ? "Select an explicit current Revision to inspect its exact context." : "Exact reading surface is loading."
    );
    const copyLink = document.createElement("button");
    copyLink.type = "button";
    copyLink.className = "ghost";
    copyLink.textContent = "Copy link";
    copyLink.addEventListener("click", () => copyExact(location.href));
    const peers = document.createElement("section");
    if (snapshot2.route.kind === "change" && snapshot2.selected !== null) {
      const changeRoute = snapshot2.route;
      peers.append(
        renderCurrentRevisionChoices(
          changeRoute.changeId,
          snapshot2.selected.currentRevisionRefs,
          changeRoute.query,
          actions2
        )
      );
    }
    replaceDetailWith(heading, identity, copyLink, placeholder, peers);
  }
  __name(renderDetail, "renderDetail");
  function renderChangeInspector(snapshot2, actions2, presentation = {
    reading: null,
    refusal: null
  }) {
    const master = document.querySelector("#master");
    if (!master) return;
    if (snapshot2.route.kind === "diff" && presentation.reading?.kind === "diff") {
      renderChangeInspectorDiffPage(
        presentation.reading.document,
        snapshot2.route,
        actions2
      );
      return;
    }
    hideChangeInspectorDiffPage();
    const routeDiagnostic = document.querySelector("#route-diagnostic");
    if (routeDiagnostic) {
      routeDiagnostic.textContent = snapshot2.diagnostic ?? "";
      routeDiagnostic.classList.toggle("hidden", snapshot2.diagnostic === null);
    }
    syncFilterChrome(
      snapshot2.route,
      snapshot2.generation?.history ?? null,
      actions2
    );
    if (snapshot2.route.kind !== "timeline") {
      document.querySelector("#follow-toggle")?.classList.add("hidden");
    }
    clearError();
    if (snapshot2.route.kind === "invalid") {
      replaceMasterWith(message("Cannot open this Inspector link."));
      renderDetail(snapshot2, actions2, presentation);
      return;
    }
    const route = snapshot2.route;
    if (snapshot2.generation === null) {
      replaceMasterWith(message("Loading Change generation…"));
      renderDetail(snapshot2, actions2, presentation);
      return;
    }
    if (route.kind === "timeline" || route.kind === "event") {
      if (snapshot2.generation.history === null) {
        replaceMasterWith(message("Loading Timeline…"));
        renderDetail(snapshot2, actions2, presentation);
        return;
      }
      const monitor = route.kind === "timeline" ? presentation.timeline ?? null : null;
      const history2 = monitor?.display ?? snapshot2.generation.history;
      const follow = document.querySelector("#follow-toggle");
      if (follow) {
        follow.classList.toggle("hidden", monitor === null);
        if (monitor !== null) {
          const parked = monitor.mode === "parked";
          follow.setAttribute("aria-pressed", String(!parked));
          follow.textContent = parked ? monitor.newCount > 0 ? `Show ${monitor.newCount} new ${monitor.newCount === 1 ? "event" : "events"}` : "Parked" : "Following";
          follow.setAttribute(
            "aria-label",
            parked ? "Show the latest filtered Timeline events and resume following" : "Park the Timeline at the current events"
          );
        }
      }
      const timelineRoute = route.kind === "timeline" ? route : { kind: "timeline", historyQuery: route.historyQuery };
      renderChangeInspectorTimeline(
        master,
        history2,
        actions2,
        timelineRoute,
        route.kind === "event" ? route.eventId : null
      );
      syncLensLinks("timeline", snapshot2.route);
      setText("#stat-events", `${history2.eventCount} events`);
      setText(
        "#stat-units",
        `${snapshot2.generation.changes.changes.length} Changes`
      );
      setText(
        "#stat-threads",
        `${snapshot2.generation.attention.changes.length} need attention`
      );
      setText("#stat-hash", history2.timelineProjectionStamp);
      renderDetail(snapshot2, actions2, presentation);
      return;
    }
    const lens = lensForRoute(route);
    const page = lens === "changes" ? snapshot2.generation.changes : snapshot2.generation.attention;
    const listKey = JSON.stringify({
      lens,
      query: route.query,
      projectionStamp: page.projectionStamp,
      changes: page.changes.map((change) => change.changeId)
    });
    if (master.dataset.changeListKey !== listKey) {
      const list = document.createElement("section");
      list.className = "units";
      const heading = document.createElement("h1");
      heading.textContent = `${lens === "changes" ? "Changes" : "Attention"} · ${page.changes.length}`;
      list.append(heading);
      for (const summary of page.changes.slice(0, 150)) {
        const card = changeCardPresentation(
          summary,
          page.presentations?.[summary.changeId]
        );
        const element = document.createElement("article");
        element.className = "unit-card";
        element.dataset.changeId = summary.changeId;
        element.setAttribute("aria-label", card.accessibleName);
        const primary = document.createElement("button");
        primary.type = "button";
        primary.className = "change-card-primary";
        primary.setAttribute(
          "aria-label",
          `${card.primaryAction.label}. ${card.accessibleName}`
        );
        primary.title = card.title;
        const headline = document.createElement("span");
        headline.className = "change-card-headline";
        headline.textContent = card.headline;
        const identity = document.createElement("code");
        identity.className = "change-card-id mono";
        identity.textContent = card.visibleChangeId;
        identity.title = card.title;
        primary.append(headline, identity);
        primary.addEventListener(
          "click",
          () => actions2.navigate({
            kind: "change",
            changeId: summary.changeId,
            query: queryForExactNavigation(route)
          })
        );
        element.append(primary);
        if (card.attention) {
          const attention = document.createElement("section");
          attention.className = "change-card-attention";
          attention.setAttribute(
            "aria-label",
            card.attention.primary.accessibleName
          );
          const reason = document.createElement("strong");
          reason.className = "change-card-attention-reason";
          reason.textContent = card.attention.reason;
          reason.title = card.attention.primary.title;
          const ask = document.createElement("p");
          ask.className = "change-card-attention-ask";
          ask.textContent = card.attention.ask;
          const action = document.createElement("p");
          action.className = "change-card-attention-action";
          action.textContent = `Next: ${card.attention.actionLabel}`;
          attention.append(reason, ask, action);
          if (card.attention.additionalReasons.length > 0) {
            const additional = document.createElement("ul");
            additional.className = "change-card-attention-additional";
            additional.setAttribute("aria-label", "Additional reasons");
            for (const item of card.attention.additionalReasons) {
              const row = document.createElement("li");
              row.textContent = `${item.reason}: ${item.ask}`;
              row.title = item.title;
              row.setAttribute("aria-label", item.accessibleName);
              additional.append(row);
            }
            attention.append(additional);
          }
          if (card.attention.diagnostics?.length) {
            const details = document.createElement("section");
            details.className = "change-card-attention-diagnostics";
            details.setAttribute("aria-label", "Attention details");
            const detailsHeading = document.createElement("strong");
            detailsHeading.textContent = "Details";
            const diagnostics = document.createElement("ul");
            for (const diagnostic of card.attention.diagnostics) {
              const row = document.createElement("li");
              row.textContent = diagnostic;
              diagnostics.append(row);
            }
            details.append(detailsHeading, diagnostics);
            attention.append(details);
          }
          element.append(attention);
        }
        const state = document.createElement("dl");
        state.className = "change-card-state";
        for (const axis of card.stateAxes) {
          const label2 = document.createElement("dt");
          label2.textContent = axis.label;
          const value = document.createElement("dd");
          value.textContent = axis.value;
          state.append(label2, value);
        }
        element.append(state);
        if (card.unavailableReason) {
          const unavailable = document.createElement("p");
          unavailable.className = "change-card-unavailable";
          unavailable.textContent = card.unavailableReason;
          element.append(unavailable);
        } else if (card.peers.length === 1) {
          const peer = card.peers[0];
          const current = document.createElement("p");
          current.className = "change-card-current";
          current.append("Current Revision · ");
          const exact = exactRevisionIdentity2(peer.revision);
          current.append(exact);
          element.append(current);
        } else {
          const peers = document.createElement("section");
          peers.className = "change-card-peers";
          const peerHeading = document.createElement("h3");
          peerHeading.textContent = "Choose an exact current Revision";
          peers.append(peerHeading);
          for (const peer of card.peers) {
            const choose = document.createElement("button");
            choose.type = "button";
            choose.className = "ghost change-card-peer-open";
            choose.textContent = `${peer.label} · ${peer.visibleIdentity}`;
            choose.title = peer.title;
            choose.setAttribute(
              "aria-label",
              `${peer.accessibleName}; open for Change ${summary.changeId}`
            );
            choose.dataset.changeId = summary.changeId;
            choose.dataset.revisionId = peer.revision.revisionId;
            choose.dataset.artifactHash = peer.revision.objectArtifactContentHash;
            choose.addEventListener(
              "click",
              () => actions2.navigate({
                kind: "revision",
                changeId: summary.changeId,
                revision: peer.revision,
                query: queryForExactNavigation(route)
              })
            );
            peers.append(choose);
          }
          element.append(peers);
        }
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
      master.dataset.changeListKey = listKey;
    }
    syncLensLinks(lens, snapshot2.route);
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
    renderDetail(snapshot2, actions2, presentation);
  }
  __name(renderChangeInspector, "renderChangeInspector");
  function renderChangeInspectorUnavailable(availability) {
    clearError();
    replaceMasterWith(
      message(
        availability === "migration_required" ? "Store migration required. No Change state was loaded." : "Store migration in progress. Partial Change state is unavailable."
      )
    );
    replaceDetailWith(message("Change state is unavailable."));
  }
  __name(renderChangeInspectorUnavailable, "renderChangeInspectorUnavailable");
  function renderChangeInspectorRefusal(error) {
    const text = error instanceof Error ? error.message : String(error);
    replaceMasterWith(message(`Reader refused: ${text}`));
    replaceDetailWith(message("Change state was not published."));
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

  // src/change-inspector-state.ts
  var ChangeInspectorGenerationChanged = class extends Error {
    static {
      __name(this, "ChangeInspectorGenerationChanged");
    }
    constructor() {
      super("Change generation changed during staging");
    }
  };
  function sameRevision2(left, right) {
    return left.revisionId === right.revisionId && left.objectArtifactContentHash === right.objectArtifactContentHash;
  }
  __name(sameRevision2, "sameRevision");
  function selectedChange(generation, route) {
    if (route.kind !== "change" && route.kind !== "revision") return null;
    const all = [...generation.changes.changes, ...generation.attention.changes];
    const change = all.find((candidate) => candidate.changeId === route.changeId) ?? null;
    if (change === null || route.kind !== "revision") return change;
    return change.currentRevisionRefs.some(
      (candidate) => sameRevision2(candidate, route.revision)
    ) ? change : null;
  }
  __name(selectedChange, "selectedChange");
  function selectionDiagnostic(route) {
    if (route.kind === "invalid") return route.message;
    return null;
  }
  __name(selectionDiagnostic, "selectionDiagnostic");
  function stageGeneration(profile, changes, attention, postflight, history2 = null) {
    requireCoherentGeneration(changes, attention);
    if (!sameProfileGeneration(profile, postflight)) {
      throw new ChangeInspectorGenerationChanged();
    }
    if (history2 !== null && (history2.sourceChangeProjectionStamp !== changes.projectionStamp || !sameAuthorityCursor(history2.authorityCursor, profile.authorityCursor) || !sameAuthorityCursor(history2.authorityCursor, postflight.authorityCursor))) {
      throw new ChangeInspectorGenerationChanged();
    }
    return { profile, changes, attention, history: history2 };
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

  // src/change-inspector-timeline-boundary.ts
  var ChangeInspectorTimelineTraversalRefused = class extends Error {
    static {
      __name(this, "ChangeInspectorTimelineTraversalRefused");
    }
  };
  function canonical(value) {
    if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
    if (value !== null && typeof value === "object") {
      const entries = Object.entries(value).sort(
        ([left], [right]) => left.localeCompare(right)
      );
      return `{${entries.map(([key, nested]) => `${JSON.stringify(key)}:${canonical(nested)}`).join(",")}}`;
    }
    return JSON.stringify(value) ?? "null";
  }
  __name(canonical, "canonical");
  function firstTimelineRoute(route) {
    return {
      kind: "timeline",
      historyQuery: {
        ...route.historyQuery,
        after: void 0,
        at: void 0
      }
    };
  }
  __name(firstTimelineRoute, "firstTimelineRoute");
  function requireSameTimelineGeneration(anchor, page) {
    if (page.sourceChangeProjectionStamp !== anchor.sourceChangeProjectionStamp || page.timelineProjectionStamp !== anchor.timelineProjectionStamp || page.eventCount !== anchor.eventCount || page.matchCount !== anchor.matchCount || page.order !== anchor.order || canonical(page.authorityCursor) !== canonical(anchor.authorityCursor)) {
      throw new ChangeInspectorGenerationChanged();
    }
  }
  __name(requireSameTimelineGeneration, "requireSameTimelineGeneration");
  async function traverseTimelineTail(route, anchor, load) {
    const first = firstTimelineRoute(route);
    const limit = first.historyQuery.limit ?? 100;
    const maximumPages = Math.max(1, Math.ceil(anchor.matchCount / limit));
    let page = await load(first.historyQuery);
    requireSameTimelineGeneration(anchor, page);
    if (page.offset !== 0) {
      throw new ChangeInspectorTimelineTraversalRefused(
        "Timeline first page did not begin at the filtered head"
      );
    }
    let pageCount = 1;
    let finalAfter;
    const seen = /* @__PURE__ */ new Set();
    while (page.next !== void 0) {
      const token = page.next;
      if (seen.has(token)) {
        throw new ChangeInspectorTimelineTraversalRefused(
          "Timeline continuation chain contained a cycle"
        );
      }
      if (pageCount >= maximumPages || page.entries.length === 0) {
        throw new ChangeInspectorTimelineTraversalRefused(
          "Timeline continuation chain exceeded its bounded match count"
        );
      }
      seen.add(token);
      const expectedOffset = page.offset + page.entries.length;
      const next = await load({ ...first.historyQuery, after: token });
      requireSameTimelineGeneration(anchor, next);
      if (next.offset !== expectedOffset) {
        throw new ChangeInspectorTimelineTraversalRefused(
          "Timeline continuation chain was not contiguous"
        );
      }
      page = next;
      finalAfter = token;
      pageCount += 1;
    }
    return {
      route: {
        kind: "timeline",
        historyQuery: { ...first.historyQuery, after: finalAfter }
      },
      page,
      pageCount
    };
  }
  __name(traverseTimelineTail, "traverseTimelineTail");

  // src/change-inspector-timeline-monitor.ts
  function monitorKey(route) {
    const query = route.historyQuery;
    if (query.after !== void 0 || query.at !== void 0 || query.order === "asc")
      return null;
    return formatChangeInspectorRoute(route);
  }
  __name(monitorKey, "monitorKey");
  function newEventsAheadOfParkedHead(parked, latest) {
    const parkedIds = new Set(parked.entries.map((entry) => entry.eventId));
    if (parkedIds.size > 0) {
      const retainedIndex = latest.entries.findIndex(
        (entry) => parkedIds.has(entry.eventId)
      );
      if (retainedIndex >= 0) return retainedIndex;
    }
    const countDelta = Math.max(0, latest.matchCount - parked.matchCount);
    if (countDelta > 0) return countDelta;
    return parkedIds.size > 0 ? latest.entries.length : 0;
  }
  __name(newEventsAheadOfParkedHead, "newEventsAheadOfParkedHead");
  function createTimelineMonitor() {
    let key = null;
    let latest = null;
    let parked = null;
    let following = true;
    const snapshot2 = /* @__PURE__ */ __name(() => {
      if (key === null) return null;
      const display = following ? latest : parked;
      if (display === null) return null;
      return {
        mode: following ? "following" : "parked",
        newCount: !following && latest !== null ? newEventsAheadOfParkedHead(display, latest) : 0,
        display
      };
    }, "snapshot");
    const park = /* @__PURE__ */ __name(() => {
      if (latest === null) return null;
      if (following) {
        parked = latest;
        following = false;
      }
      return snapshot2();
    }, "park");
    const follow = /* @__PURE__ */ __name(() => {
      if (latest === null) return null;
      parked = null;
      following = true;
      return snapshot2();
    }, "follow");
    return {
      observe(route, document2) {
        const nextKey = monitorKey(route);
        if (nextKey === null) {
          key = null;
          latest = document2;
          parked = null;
          following = true;
          return null;
        }
        if (key !== nextKey) {
          key = nextKey;
          parked = null;
          following = true;
        }
        latest = document2;
        return snapshot2();
      },
      toggle() {
        if (latest === null) return null;
        if (following) {
          park();
        } else {
          follow();
        }
        return snapshot2();
      },
      /** Idempotently retain the reader's current head window. */
      park,
      /** Explicit catch-up resumes the newest successfully loaded head page. */
      follow,
      snapshot: snapshot2
    };
  }
  __name(createTimelineMonitor, "createTimelineMonitor");

  // src/disclosure.ts
  var active2 = null;
  function createDisclosure({
    container,
    trigger,
    panel
  }) {
    let open = false;
    const triggerElement = $(trigger);
    const containerElement = $(container);
    let controller;
    const onTriggerClick = /* @__PURE__ */ __name((event) => {
      event.stopPropagation();
      controller.toggle();
    }, "onTriggerClick");
    const onContainerKeydown = /* @__PURE__ */ __name((event) => {
      if (event.key !== "Escape" || !open) return;
      event.preventDefault();
      event.stopPropagation();
      controller.close(true);
    }, "onContainerKeydown");
    const onDocumentClick = /* @__PURE__ */ __name((event) => {
      if (!open) return;
      const root = $(container);
      if (event.target instanceof Node && root?.contains(event.target)) return;
      controller.close();
    }, "onDocumentClick");
    controller = {
      isOpen: /* @__PURE__ */ __name(() => open, "isOpen"),
      open: /* @__PURE__ */ __name(() => {
        if (active2 && active2 !== controller) active2.close();
        open = true;
        active2 = controller;
        controller.sync();
      }, "open"),
      close: /* @__PURE__ */ __name((returnFocus = false) => {
        open = false;
        if (active2 === controller) active2 = null;
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
      }, "sync"),
      dispose: /* @__PURE__ */ __name(() => {
        triggerElement?.removeEventListener("click", onTriggerClick);
        containerElement?.removeEventListener("keydown", onContainerKeydown);
        document.removeEventListener("click", onDocumentClick, true);
        if (active2 === controller) active2 = null;
        open = false;
        controller.sync();
      }, "dispose")
    };
    triggerElement?.addEventListener("click", onTriggerClick);
    containerElement?.addEventListener("keydown", onContainerKeydown);
    document.addEventListener("click", onDocumentClick, true);
    controller.sync();
    return controller;
  }
  __name(createDisclosure, "createDisclosure");

  // src/change-inspector.ts
  var EXACT_READING_TIMEOUT_MS = 1e4;
  var POLL_CYCLE_TIMEOUT_MS = 15e3;
  var ChangeInspectorTimeout = class extends Error {
    static {
      __name(this, "ChangeInspectorTimeout");
    }
  };
  var pollTimer = null;
  var routeListener = null;
  var filterInput = null;
  var filterInputListener = null;
  var connectionControlsInitialized = false;
  var filterDisclosure = null;
  var viewDisclosure = null;
  var interactionStop = null;
  var pollCoordinatorStop = null;
  var requestEpoch = 0;
  function currentRoute() {
    return parseChangeInspectorRoute(location.hash || "#/timeline");
  }
  __name(currentRoute, "currentRoute");
  function newProjectionRetryBudget() {
    return { remaining: 1 };
  }
  __name(newProjectionRetryBudget, "newProjectionRetryBudget");
  function consumeProjectionRetry(budget) {
    if (budget.remaining === 0) return false;
    budget.remaining -= 1;
    return true;
  }
  __name(consumeProjectionRetry, "consumeProjectionRetry");
  function snapshotFilterDraft(input, restoreFocus) {
    return {
      input,
      restoreFocus,
      value: input.value,
      selectionStart: input.selectionStart,
      selectionEnd: input.selectionEnd,
      selectionDirection: input.selectionDirection
    };
  }
  __name(snapshotFilterDraft, "snapshotFilterDraft");
  function capturePollFilterDraft() {
    if (filterInput === null) return null;
    const focused = document.activeElement === filterInput;
    return snapshotFilterDraft(filterInput, focused);
  }
  __name(capturePollFilterDraft, "capturePollFilterDraft");
  async function withinTimeout(operation, timeoutMs, message2) {
    let timer = null;
    try {
      return await Promise.race([
        operation,
        new Promise((_resolve, reject) => {
          timer = setTimeout(
            () => reject(new ChangeInspectorTimeout(message2)),
            timeoutMs
          );
        })
      ]);
    } finally {
      if (timer !== null) clearTimeout(timer);
    }
  }
  __name(withinTimeout, "withinTimeout");
  function stopChangeInspector() {
    requestEpoch += 1;
    if (pollTimer !== null) clearInterval(pollTimer);
    pollTimer = null;
    pollCoordinatorStop?.();
    pollCoordinatorStop = null;
    if (routeListener !== null)
      window.removeEventListener("hashchange", routeListener);
    routeListener = null;
    if (filterInput !== null && filterInputListener !== null) {
      filterInput.removeEventListener("change", filterInputListener);
    }
    filterInput = null;
    filterInputListener = null;
    filterDisclosure?.dispose();
    filterDisclosure = null;
    viewDisclosure?.dispose();
    viewDisclosure = null;
    interactionStop?.();
    interactionStop = null;
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
    const replace = /* @__PURE__ */ __name((route) => {
      const hash = formatChangeInspectorRoute(route);
      if (location.hash === hash) return;
      history.replaceState(history.state, "", hash);
      void onRoute();
    }, "replace");
    let reading = null;
    let readingRefusal = null;
    let visibleReading = "";
    const timelineMonitor = createTimelineMonitor();
    const parkTimelineMonitoring = /* @__PURE__ */ __name(() => {
      if (timelineMonitor.park() !== null) paint();
    }, "parkTimelineMonitoring");
    const paint = /* @__PURE__ */ __name((pollDraft = null) => {
      const draft = pollDraft !== null && filterInput === pollDraft.input ? snapshotFilterDraft(
        filterInput,
        document.activeElement === filterInput || document.activeElement === document.body && pollDraft.restoreFocus
      ) : null;
      const snapshot2 = state.snapshot();
      const monitor = timelineMonitor.snapshot();
      renderChangeInspector(
        snapshot2,
        { navigate, replace, parkTimelineMonitoring },
        {
          reading,
          refusal: readingRefusal,
          timeline: monitor
        }
      );
      if (draft !== null && filterInput !== null) {
        filterInput.value = draft.value;
        if (draft.restoreFocus) filterInput.focus({ preventScroll: true });
        if (draft.selectionStart !== null && draft.selectionEnd !== null) {
          filterInput.setSelectionRange(
            draft.selectionStart,
            draft.selectionEnd,
            draft.selectionDirection ?? void 0
          );
        }
      }
      const interactiveTimeline = (snapshot2.route.kind === "timeline" || snapshot2.route.kind === "event") && snapshot2.generation !== null ? snapshot2.route.kind === "timeline" ? monitor?.display ?? snapshot2.generation.history : snapshot2.generation.history : null;
      interaction?.sync(snapshot2, interactiveTimeline);
    }, "paint");
    let interaction = null;
    const requestKey = /* @__PURE__ */ __name((route) => route.kind === "timeline" || route.kind === "event" ? buildEventHistoryUrl(
      route.kind === "event" ? { ...route.historyQuery, after: void 0, at: route.eventId } : route.historyQuery
    ) : buildChangePageUrl("changes", route.query), "requestKey");
    let visibleRequest = "";
    let pendingReading = null;
    let releaseQueuedPoll = /* @__PURE__ */ __name(() => {
    }, "releaseQueuedPoll");
    const readingKey = /* @__PURE__ */ __name((route, projectionStamp) => `${formatChangeInspectorRoute(route)}\0${projectionStamp}`, "readingKey");
    const clearReading = /* @__PURE__ */ __name(() => {
      reading = null;
      readingRefusal = null;
      visibleReading = "";
    }, "clearReading");
    const loadReading = /* @__PURE__ */ __name(async (route, expectedProjectionStamp, epoch, retryBudget, pollDraft = null) => {
      if (route.kind === "lens" || route.kind === "timeline" || route.kind === "event") {
        clearReading();
        return;
      }
      const requested = formatChangeInspectorRoute(route);
      const requestedReading = readingKey(route, expectedProjectionStamp);
      if (visibleReading === requestedReading && reading !== null) return;
      reading = null;
      readingRefusal = null;
      visibleReading = requestedReading;
      paint(pollDraft);
      const pendingToken = /* @__PURE__ */ Symbol("exact-reading");
      pendingReading = { key: requestedReading, token: pendingToken };
      try {
        const { loaded, postflight } = await withinTimeout(
          (async () => {
            const loaded2 = await loadChangeInspectorReading(
              route,
              expectedProjectionStamp
            );
            const postflight2 = decodeReaderProfile(
              await fetchChangeInspectorJSON("/api/v2/profile")
            );
            return { loaded: loaded2, postflight: postflight2 };
          })(),
          EXACT_READING_TIMEOUT_MS,
          "exact Change reading timed out"
        );
        if (epoch !== requestEpoch || currentRoute().kind === "invalid") return;
        const staged = state.snapshot().generation;
        if (staged === null || formatChangeInspectorRoute(
          currentRoute()
        ) !== requested || !sameProfileGeneration(staged.profile, postflight)) {
          throw new ChangeInspectorGenerationChanged();
        }
        reading = loaded;
        readingRefusal = null;
        paint(pollDraft);
      } catch (error) {
        if (epoch !== requestEpoch) return;
        if ((error instanceof ChangeInspectorGenerationChanged || error instanceof ChangeInspectorPageFailure && (error.code === "stale_projection" || error.code === "moving_journal")) && consumeProjectionRetry(retryBudget)) {
          await loadGeneration(route, retryBudget, pollDraft);
          return;
        }
        reading = null;
        readingRefusal = error instanceof Error ? error.message : String(error);
        paint(pollDraft);
      } finally {
        if (pendingReading?.token === pendingToken) pendingReading = null;
        releaseQueuedPoll();
      }
    }, "loadReading");
    const loadGeneration = /* @__PURE__ */ __name(async (route, retryBudget, pollDraft = null) => {
      const epoch = ++requestEpoch;
      try {
        const profile = decodeReaderProfile(
          await fetchChangeInspectorJSON("/api/v2/profile")
        );
        if (epoch !== requestEpoch) return;
        if (profile.availability !== "ready") {
          visibleRequest = "";
          clearReading();
          state.clearGeneration();
          renderChangeInspectorUnavailable(profile.availability);
          return;
        }
        const query = route.kind === "timeline" || route.kind === "event" ? {} : route.query;
        const activeLens = lensForRoute(route);
        const changesQuery = activeLens === "changes" ? query : firstPageQuery(query);
        const attentionQuery = activeLens === "attention" ? query : firstPageQuery(query);
        const historyRequest = route.kind === "timeline" || route.kind === "event" ? fetchChangeInspectorJSON(
          buildEventHistoryUrl(
            route.kind === "event" ? {
              ...route.historyQuery,
              after: void 0,
              at: route.eventId
            } : route.historyQuery
          )
        ).then(decodeEventHistory) : Promise.resolve(null);
        const [changes, attention, history2] = await Promise.all([
          fetchChangeInspectorJSON(
            buildChangePageUrl("changes", changesQuery)
          ).then(
            (value) => decodeChangePage(value, { lens: "changes", bounded: true })
          ),
          fetchChangeInspectorJSON(
            buildChangePageUrl("attention", attentionQuery)
          ).then(
            (value) => decodeChangePage(value, { lens: "attention", bounded: true })
          ),
          historyRequest
        ]);
        const postflight = decodeReaderProfile(
          await fetchChangeInspectorJSON("/api/v2/profile")
        );
        if (epoch !== requestEpoch) return;
        const staged = stageGeneration(
          profile,
          changes,
          attention,
          postflight,
          history2
        );
        if (route.kind !== "lens" && route.kind !== "timeline" && route.kind !== "event") {
          const requestedReading = readingKey(route, changes.projectionStamp);
          if (visibleReading !== requestedReading) {
            reading = null;
            readingRefusal = null;
          }
        }
        state.publish(staged);
        if (route.kind === "timeline" && history2 !== null) {
          timelineMonitor.observe(route, history2);
        }
        visibleRequest = requestKey(route);
        paint(pollDraft);
        await loadReading(
          route,
          changes.projectionStamp,
          epoch,
          retryBudget,
          pollDraft
        );
      } catch (error) {
        if (epoch !== requestEpoch) return;
        if ((error instanceof ChangeInspectorPageFailure && (error.code === "stale_projection" || error.code === "moving_journal") || error instanceof ChangeInspectorGenerationChanged) && consumeProjectionRetry(retryBudget)) {
          await loadGeneration(route, retryBudget, pollDraft);
          return;
        }
        visibleRequest = "";
        clearReading();
        state.clearGeneration();
        renderChangeInspectorRefusal(error);
      }
    }, "loadGeneration");
    const onRoute = /* @__PURE__ */ __name(async () => {
      filterDisclosure?.close();
      viewDisclosure?.close();
      const capability2 = bootstrapCapability();
      const route = parseChangeInspectorRoute(
        capability2.cleanedHash || "#/timeline"
      );
      requestEpoch += 1;
      state.setRoute(route);
      if (route.kind === "invalid") {
        visibleRequest = "";
        clearReading();
        state.clearGeneration();
        paint();
        return;
      }
      let request;
      try {
        request = requestKey(route);
      } catch (error) {
        state.clearGeneration();
        renderChangeInspectorRefusal(error);
        return;
      }
      if (request === visibleRequest) {
        const generation = state.snapshot().generation;
        if (generation === null) {
          await loadGeneration(route, newProjectionRetryBudget());
        } else {
          await loadReading(
            route,
            generation.changes.projectionStamp,
            requestEpoch,
            newProjectionRetryBudget()
          );
          paint();
        }
      } else {
        visibleRequest = "";
        clearReading();
        state.clearGeneration();
        paint();
        await loadGeneration(route, newProjectionRetryBudget());
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
      await loadGeneration(route, newProjectionRetryBudget());
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
    const toggleTimelineMonitoring = /* @__PURE__ */ __name(() => {
      if (currentRoute().kind !== "timeline") return;
      if (timelineMonitor.toggle() !== null) paint();
    }, "toggleTimelineMonitoring");
    const navigateTimelineBoundary = /* @__PURE__ */ __name(async (boundary, route) => {
      const first = firstTimelineRoute(route);
      if (boundary === "first") {
        navigate(first);
        return first;
      }
      const retryBudget = newProjectionRetryBudget();
      const requestedRoute = formatChangeInspectorRoute(route);
      for (; ; ) {
        const generation = state.snapshot().generation;
        const anchor = generation?.history;
        if (generation === null || anchor === null || anchor === void 0) {
          return null;
        }
        const epoch = ++requestEpoch;
        try {
          const preflight = decodeReaderProfile(
            await fetchChangeInspectorJSON("/api/v2/profile")
          );
          if (epoch !== requestEpoch || currentRoute().kind === "invalid" || formatChangeInspectorRoute(
            currentRoute()
          ) !== requestedRoute) {
            return null;
          }
          if (!sameProfileGeneration(generation.profile, preflight)) {
            throw new ChangeInspectorGenerationChanged();
          }
          const tail = await traverseTimelineTail(
            route,
            anchor,
            async (query) => {
              const page = decodeEventHistory(
                await fetchChangeInspectorJSON(buildEventHistoryUrl(query))
              );
              if (epoch !== requestEpoch) {
                throw new ChangeInspectorGenerationChanged();
              }
              return page;
            }
          );
          const postflight = decodeReaderProfile(
            await fetchChangeInspectorJSON("/api/v2/profile")
          );
          if (epoch !== requestEpoch || !sameProfileGeneration(generation.profile, postflight)) {
            throw new ChangeInspectorGenerationChanged();
          }
          navigate(tail.route);
          return tail.route;
        } catch (error) {
          if (epoch !== requestEpoch) return null;
          if ((error instanceof ChangeInspectorGenerationChanged || error instanceof ChangeInspectorPageFailure && (error.code === "stale_projection" || error.code === "moving_journal")) && consumeProjectionRetry(retryBudget)) {
            await loadGeneration(route, retryBudget);
            if (currentRoute().kind === "invalid" || formatChangeInspectorRoute(
              currentRoute()
            ) !== requestedRoute) {
              return null;
            }
            continue;
          }
          visibleRequest = "";
          clearReading();
          state.clearGeneration();
          renderChangeInspectorRefusal(error);
          return null;
        }
      }
    }, "navigateTimelineBoundary");
    prepareChangeInspectorShell({ navigate, toggleTimelineMonitoring });
    filterDisclosure = createDisclosure({
      container: "#filter-controls",
      trigger: "#filters-toggle",
      panel: "#filters-panel"
    });
    viewDisclosure = createDisclosure({
      container: "#view-controls",
      trigger: "#view-toggle",
      panel: "#view-panel"
    });
    interaction = installChangeInspectorInteraction({
      navigate,
      navigateTimelineBoundary,
      revealTimelineEvent: revealChangeInspectorTimelineEvent,
      toggleTimelineMonitoring,
      parkTimelineMonitoring
    });
    interactionStop = interaction.stop;
    filterInput = document.querySelector("#filter-text");
    filterInputListener = /* @__PURE__ */ __name(() => {
      const route = currentRoute();
      const base = route.kind === "invalid" ? { kind: "timeline", historyQuery: {} } : route;
      if (base.kind === "timeline" || base.kind === "event") {
        navigate({
          kind: "timeline",
          historyQuery: {
            ...base.historyQuery,
            after: void 0,
            at: void 0,
            q: filterInput?.value || void 0
          }
        });
      } else {
        navigate({
          ...base,
          query: {
            ...base.query,
            after: void 0,
            q: filterInput?.value || void 0
          }
        });
      }
    }, "filterInputListener");
    filterInput?.addEventListener("change", filterInputListener);
    await onRoute();
    if (options.poll !== false) {
      let pollRequested = false;
      let pollRunning = false;
      let pollActive = true;
      const drainPoll = /* @__PURE__ */ __name(() => {
        if (!pollActive || pollRunning || !pollRequested) return;
        const route = currentRoute();
        if (route.kind === "invalid") {
          pollRequested = false;
          return;
        }
        const generation = state.snapshot().generation;
        if (route.kind !== "lens" && route.kind !== "timeline" && route.kind !== "event" && generation !== null && pendingReading?.key === readingKey(route, generation.changes.projectionStamp)) {
          return;
        }
        pollRequested = false;
        pollRunning = true;
        const operation = loadGeneration(
          route,
          newProjectionRetryBudget(),
          capturePollFilterDraft()
        );
        void withinTimeout(
          operation,
          POLL_CYCLE_TIMEOUT_MS,
          "Change generation poll timed out"
        ).catch((error) => {
          if (error instanceof ChangeInspectorTimeout) {
            requestEpoch += 1;
          }
        }).finally(() => {
          pollRunning = false;
          drainPoll();
        });
      }, "drainPoll");
      const requestPoll = /* @__PURE__ */ __name(() => {
        pollRequested = true;
        drainPoll();
      }, "requestPoll");
      releaseQueuedPoll = drainPoll;
      pollCoordinatorStop = /* @__PURE__ */ __name(() => {
        pollActive = false;
        pollRequested = false;
      }, "pollCoordinatorStop");
      pollTimer = setInterval(requestPoll, 3e3);
    }
  }
  __name(bootstrapChangeInspector, "bootstrapChangeInspector");

  // src/entry.ts
  void bootstrapChangeInspector();
})();
