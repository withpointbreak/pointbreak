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
  var MAX_LIVE_CHANGE_ROWS = 150;
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
    const relationClaims = detail.relationClaims;
    const diagnostics = detail.diagnostics;
    if (detail.schema !== "pointbreak.review-change" || detail.version !== 1 || !nonEmptyString(stamp) || !isChangeSummary(summary, stamp) || !Array.isArray(relationClaims) || !relationClaims.every(isRelationClaim) || !isStringArray(diagnostics)) {
      throw new Error("invalid Change detail DTO");
    }
    return {
      schema: "pointbreak.review-change",
      version: 1,
      summary,
      relationClaims,
      diagnostics,
      projectionStamp: stamp
    };
  }
  __name(decodeChangeDetail, "decodeChangeDetail");
  function decodeChangeRevisionDetail(value) {
    const detail = object(value, "Change Revision detail");
    const revision = detail.revision;
    const factPresentations = detail.factPresentations;
    const factContentPresentations = detail.factContentPresentations;
    const associations = detail.associations;
    const diagnostics = detail.diagnostics;
    const revisionCurrency = detail.revisionCurrency;
    const relationClassification = detail.relationClassification;
    const availability = detail.availability;
    if (detail.schema !== "pointbreak.review-change-revision" || detail.version !== 1 || !nonEmptyString(detail.changeId) || !isRevisionRef(revision) || typeof revisionCurrency !== "string" || !REVISION_CURRENCY_VALUES.has(revisionCurrency) || relationClassification !== "current" && relationClassification !== "superseded" || typeof availability !== "string" || !CONTENT_AVAILABILITY_VALUES.has(availability) || !Array.isArray(factPresentations) || !factPresentations.every(isFactPresentation) || factContentPresentations !== void 0 && !isFactContentPresentations(factContentPresentations) || !Array.isArray(associations) || !associations.every(isAssociation) || !isStringArray(diagnostics) || !nonEmptyString(detail.projectionStamp)) {
      throw new Error("invalid Change Revision detail DTO");
    }
    return {
      schema: "pointbreak.review-change-revision",
      version: 1,
      changeId: detail.changeId,
      revision,
      revisionCurrency,
      relationClassification,
      availability,
      factPresentations,
      factContentPresentations,
      associations,
      diagnostics,
      projectionStamp: detail.projectionStamp
    };
  }
  __name(decodeChangeRevisionDetail, "decodeChangeRevisionDetail");
  function decodeRevisionResource(value) {
    const document2 = object(value, "Revision resource");
    const resource = document2.resource;
    const diagnostics = document2.diagnostics;
    const availability = document2.availability;
    const capturedDocumentHash = document2.capturedDocumentHash;
    if (document2.schema !== "pointbreak.review-revision-resource" || document2.version !== 1 || !isRecord(resource) || !isRevisionRef(resource.revision) || !nonEmptyString(resource.objectId) || !isOneOf(availability, CONTENT_AVAILABILITY_VALUES) || capturedDocumentHash !== void 0 && !nonEmptyString(capturedDocumentHash) || availability === "available" !== (capturedDocumentHash !== void 0 && document2.capturedDocument !== void 0) || !isStringArray(diagnostics)) {
      throw new Error("invalid Revision resource DTO");
    }
    return {
      schema: "pointbreak.review-revision-resource",
      version: 1,
      resource: { revision: resource.revision, objectId: resource.objectId },
      availability,
      capturedDocumentHash,
      capturedDocument: document2.capturedDocument,
      diagnostics
    };
  }
  __name(decodeRevisionResource, "decodeRevisionResource");
  function decodeRevisionInterdiff(value) {
    const document2 = object(value, "Revision interdiff");
    const interdiff = document2.interdiff;
    const diagnostics = document2.diagnostics;
    const availability = document2.availability;
    if (document2.schema !== "pointbreak.review-revision-interdiff" || document2.version !== 1 || !isRecord(interdiff) || !isRevisionRef(interdiff.from) || !isRevisionRef(interdiff.to) || !isOneOf(availability, INTERDIFF_AVAILABILITY_VALUES) || !isStringArray(diagnostics) || availability === "available" !== (document2.comparison !== void 0)) {
      throw new Error("invalid Revision interdiff DTO");
    }
    return {
      schema: "pointbreak.review-revision-interdiff",
      version: 1,
      interdiff: { from: interdiff.from, to: interdiff.to },
      availability,
      comparison: document2.comparison,
      diagnostics
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
  function isClaimSupport(value) {
    return isRecord(value) && nonEmptyString(value.actorId) && nonEmptyString(value.eventId);
  }
  __name(isClaimSupport, "isClaimSupport");
  function isRelationClaim(value) {
    return isRecord(value) && nonEmptyString(value.claimId) && typeof value.active === "boolean" && isRevisionRef(value.successor) && isRevisionRef(value.predecessor) && Array.isArray(value.supports) && value.supports.every(isClaimSupport) && Array.isArray(value.withdrawals) && value.withdrawals.every(isClaimSupport);
  }
  __name(isRelationClaim, "isRelationClaim");
  function isFactPresentation(value) {
    return isRecord(value) && nonEmptyString(value.factId) && nonEmptyString(value.family) && isRevisionRef(value.originRevision) && isOneOf(value.revisionCurrency, REVISION_CURRENCY_VALUES) && isOneOf(value.familyState, FACT_FAMILY_STATE_VALUES) && isOneOf(value.availability, CONTENT_AVAILABILITY_VALUES);
  }
  __name(isFactPresentation, "isFactPresentation");
  function isFactContentPresentations(value) {
    return isRecord(value) && Object.values(value).every(
      (presentation) => isRecord(presentation) && (presentation.contentType === "text/plain" || presentation.contentType === "text/markdown") && (presentation.bodyContentState === "present" || presentation.bodyContentState === "suppressed_present" || presentation.bodyContentState === "physically_removed") && isRecord(presentation.content)
    );
  }
  __name(isFactContentPresentations, "isFactContentPresentations");
  function isAssociation(value) {
    return isRecord(value) && isOneOf(value.state, ASSOCIATION_STATE_VALUES) && isOneOf(value.proofAvailability, ASSOCIATION_PROOF_VALUES) && isRecord(value.comparison) && isRevisionRef(value.comparison.revision) && nonEmptyString(value.comparison.commitOid);
  }
  __name(isAssociation, "isAssociation");
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
  var ChangePageFailure = class extends RequestFailure {
    constructor(code, status) {
      super("protocol", status);
      this.code = code;
      this.name = "ChangePageFailure";
    }
    code;
    static {
      __name(this, "ChangePageFailure");
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
  function changePageFailure(data, status) {
    if (typeof data !== "object" || data === null || data.schema !== "pointbreak.inspect-change-page-error" || data.version !== 1) {
      return null;
    }
    const code = data.code;
    if (code !== "invalid_query" && code !== "stale_projection") return null;
    if (code === "invalid_query" && status !== 400 || code === "stale_projection" && status !== 409) {
      return null;
    }
    markRequestFailure("protocol");
    return new ChangePageFailure(code, status);
  }
  __name(changePageFailure, "changePageFailure");
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
    let data;
    try {
      data = JSON.parse(text);
    } catch {
      throw failure("protocol", response.status);
    }
    if (!response.ok) {
      const typed = changePageFailure(data, response.status);
      if (typed !== null) throw typed;
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
  var pollTimer = null;
  var readerEpoch = 0;
  var detailSelectionEpoch = 0;
  var connectionControlsInitialized = false;
  var visibleGeneration = null;
  async function bootstrapChangeReader(options = {}) {
    stopChangeReader();
    const bootstrapEpoch = readerEpoch;
    let loadingGeneration = false;
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
      const profile = decodeReaderProfile(await fetchJSON("/api/v2/profile"));
      if (bootstrapEpoch !== readerEpoch) return;
      prepareChangeShell();
      if (profile.availability !== "ready") {
        renderUnavailable(profile.availability);
        return;
      }
      loadingGeneration = true;
      const publishedEpoch = await loadGeneration(profile);
      if (options.poll !== false && publishedEpoch !== null && publishedEpoch === readerEpoch) {
        pollTimer = setInterval(() => {
          void refresh();
        }, 3e3);
      }
    } catch (error) {
      if (!loadingGeneration && bootstrapEpoch !== readerEpoch) return;
      renderRefusal(error);
    }
  }
  __name(bootstrapChangeReader, "bootstrapChangeReader");
  function stopChangeReader() {
    readerEpoch += 1;
    detailSelectionEpoch += 1;
    visibleGeneration = null;
    if (pollTimer !== null) {
      clearInterval(pollTimer);
      pollTimer = null;
    }
  }
  __name(stopChangeReader, "stopChangeReader");
  async function refresh(force = false) {
    const requestedEpoch = readerEpoch;
    const current = visibleGeneration;
    let loadingGeneration = false;
    try {
      const profile = decodeReaderProfile(await fetchJSON("/api/v2/profile"));
      if (requestedEpoch !== readerEpoch || current !== visibleGeneration) {
        return;
      }
      if (profile.availability !== "ready") {
        renderUnavailable(profile.availability);
        stopChangeReader();
        return;
      }
      if (!force && current !== null && sameProfileGeneration(current.profile, profile)) {
        return;
      }
      loadingGeneration = true;
      await loadGeneration(profile);
    } catch (error) {
      if (!loadingGeneration && (requestedEpoch !== readerEpoch || current !== visibleGeneration)) {
        return;
      }
      renderRefusal(error);
    }
  }
  __name(refresh, "refresh");
  async function loadGeneration(profile, restarted = false) {
    const requestedEpoch = ++readerEpoch;
    detailSelectionEpoch += 1;
    try {
      const [changes, attention] = await Promise.all([
        fetchJSON(buildChangePageUrl("changes")).then(
          (page) => decodeChangePage(page, { lens: "changes", bounded: true })
        ),
        fetchJSON(buildChangePageUrl("attention")).then(
          (page) => decodeChangePage(page, { lens: "attention", bounded: true })
        )
      ]);
      const postflight = decodeReaderProfile(await fetchJSON("/api/v2/profile"));
      if (requestedEpoch !== readerEpoch) return null;
      requireCoherentGeneration(changes, attention);
      if (!sameProfileGeneration(profile, postflight)) {
        throw new Error("Change generation changed during staging");
      }
      renderGeneration(profile, changes, attention);
      return requestedEpoch;
    } catch (error) {
      if (requestedEpoch !== readerEpoch) return null;
      const stalePage = error instanceof ChangePageFailure && error.code === "stale_projection";
      const changedDuringStaging = error instanceof Error && error.message === "Change generation changed during staging";
      if (!restarted && (stalePage || changedDuringStaging)) {
        renderRestart(error);
        let retryProfile;
        try {
          retryProfile = decodeReaderProfile(await fetchJSON("/api/v2/profile"));
        } catch (retryError) {
          if (requestedEpoch !== readerEpoch) return null;
          throw retryError;
        }
        if (requestedEpoch !== readerEpoch) return null;
        if (retryProfile.availability !== "ready") {
          renderUnavailable(retryProfile.availability);
          return null;
        }
        return loadGeneration(retryProfile, true);
      }
      throw error;
    }
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
    const list = document.createElement("section");
    list.className = "units";
    const heading2 = document.createElement("h1");
    heading2.textContent = `Changes · ${page.changes.length}`;
    list.append(heading2);
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
      const presentation = page.presentations?.[change.changeId];
      for (const revision of change.currentRevisionRefs) {
        const select = document.createElement("button");
        select.type = "button";
        select.className = "ghost mono";
        select.dataset.revisionId = revision.revisionId;
        select.textContent = currentRevisionLabel(revision, presentation);
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
      list.append(card);
    }
    if (page.changes.length === 0) list.append(message("No Changes."));
    if (page.next !== null) {
      const loadMore = document.createElement("button");
      loadMore.type = "button";
      loadMore.className = "ghost";
      loadMore.textContent = "Load more Changes";
      loadMore.addEventListener("click", () => {
        void loadMoreChanges();
      });
      list.append(loadMore);
    }
    master.replaceChildren(list);
    setText(
      "#stat-events",
      `${profile.authorityCursor.eventCount ?? "—"} events`
    );
    setText("#stat-units", `${page.changes.length} Changes`);
    setText("#stat-threads", `${attention.changes.length} need attention`);
    setText("#stat-hash", page.projectionStamp);
    detailSelectionEpoch += 1;
    visibleGeneration = { profile, changes: page, attention };
  }
  __name(renderGeneration, "renderGeneration");
  async function loadMoreChanges() {
    const current = visibleGeneration;
    if (!current?.changes.next) return;
    const requestedEpoch = readerEpoch;
    try {
      const next = decodeChangePage(
        await fetchJSON(
          buildChangePageUrl("changes", { after: current.changes.next })
        ),
        { lens: "changes", bounded: true }
      );
      if (!isLiveGeneration(requestedEpoch, current)) return;
      const postflight = decodeReaderProfile(await fetchJSON("/api/v2/profile"));
      if (!isLiveGeneration(requestedEpoch, current)) return;
      if (next.projectionStamp !== current.changes.projectionStamp || !sameProfileGeneration(current.profile, postflight)) {
        throw new Error(
          "Change page changed during paging; restarting from first page"
        );
      }
      const merged = mergeChangePages(current.changes, next);
      if (!isLiveGeneration(requestedEpoch, current)) return;
      renderGeneration(current.profile, merged, current.attention);
    } catch (error) {
      if (!isLiveGeneration(requestedEpoch, current)) return;
      if (error instanceof ChangePageFailure && error.code === "stale_projection" || error instanceof Error && error.message === "Change page changed during paging; restarting from first page") {
        renderRestart(error);
        await refresh(true);
        return;
      }
      renderRefusal(error);
    }
  }
  __name(loadMoreChanges, "loadMoreChanges");
  function isLiveGeneration(requestedEpoch, expected) {
    return requestedEpoch === readerEpoch && visibleGeneration === expected;
  }
  __name(isLiveGeneration, "isLiveGeneration");
  function beginDetailRequest(change) {
    const visible = visibleGeneration;
    if (visible === null || !visible.changes.changes.includes(change))
      return null;
    return {
      readerEpoch,
      selectionEpoch: ++detailSelectionEpoch,
      projectionStamp: change.projectionStamp,
      visible
    };
  }
  __name(beginDetailRequest, "beginDetailRequest");
  function isLiveDetailRequest(request) {
    return request.readerEpoch === readerEpoch && request.selectionEpoch === detailSelectionEpoch && request.visible === visibleGeneration && request.visible.changes.projectionStamp === request.projectionStamp;
  }
  __name(isLiveDetailRequest, "isLiveDetailRequest");
  async function detailPostflight(request) {
    const profile = decodeReaderProfile(await fetchJSON("/api/v2/profile"));
    if (!isLiveDetailRequest(request)) return false;
    if (!sameProfileGeneration(request.visible.profile, profile)) {
      throw new Error("Change detail generation changed during staging");
    }
    return true;
  }
  __name(detailPostflight, "detailPostflight");
  function mergeChangePages(current, next) {
    const lastCurrent = current.changes.at(-1)?.changeId;
    const firstNext = next.changes[0]?.changeId;
    if (lastCurrent !== void 0 && firstNext !== void 0 && firstNext <= lastCurrent) {
      throw new Error("Change continuation did not advance in server order");
    }
    const seen = new Set(current.changes.map((change) => change.changeId));
    if (next.changes.some((change) => seen.has(change.changeId))) {
      throw new Error("Change continuation repeated an emitted Change ID");
    }
    const changes = [...current.changes, ...next.changes].slice(
      -MAX_LIVE_CHANGE_ROWS
    );
    const visibleIds = new Set(changes.map((change) => change.changeId));
    const presentations = current.presentations !== void 0 && next.presentations !== void 0 ? Object.fromEntries(
      Object.entries({
        ...current.presentations,
        ...next.presentations
      }).filter(([changeId]) => visibleIds.has(changeId))
    ) : void 0;
    return {
      ...next,
      changes,
      presentations
    };
  }
  __name(mergeChangePages, "mergeChangePages");
  function currentRevisionLabel(revision, presentation) {
    const entry = presentation?.currentRevisions.find(
      (candidate) => sameRevision(candidate.revision, revision)
    );
    if (entry?.summarySource === "revision_proposal_summary") {
      return `Current Revision — proposal summary: ${entry.revisionProposalSummary ?? "absent"} · ${revision.revisionId}`;
    }
    return `Current Revision — summary absent · ${revision.revisionId}`;
  }
  __name(currentRevisionLabel, "currentRevisionLabel");
  async function loadChangeDetail(change) {
    const request = beginDetailRequest(change);
    if (request === null) return;
    try {
      const detail = decodeChangeDetail(
        await fetchJSON(`/api/v2/changes/${encodeURIComponent(change.changeId)}`)
      );
      if (!isLiveDetailRequest(request)) return;
      if (detail.summary.changeId !== change.changeId || detail.projectionStamp !== change.projectionStamp) {
        throw new Error("Change detail generation is stale; refresh and retry");
      }
      if (!await detailPostflight(request)) return;
      const content = [
        heading(change.changeId),
        line(`topology: ${words(detail.summary.topology)}`),
        line(`lifecycle: ${words(detail.summary.lifecycle)}`)
      ];
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
      content.push(relations);
      publishDetail(request, content);
    } catch (error) {
      if (!isLiveDetailRequest(request)) return;
      renderRefusal(error);
    }
  }
  __name(loadChangeDetail, "loadChangeDetail");
  async function loadRevisionDetail(change, revision) {
    const request = beginDetailRequest(change);
    if (request === null) return;
    try {
      const params = new URLSearchParams({
        artifactHash: revision.objectArtifactContentHash
      });
      const detail = decodeChangeRevisionDetail(
        await fetchJSON(
          `/api/v2/changes/${encodeURIComponent(change.changeId)}/revisions/${encodeURIComponent(revision.revisionId)}?${params}`
        )
      );
      if (!isLiveDetailRequest(request)) return;
      if (detail.changeId !== change.changeId || !sameRevision(detail.revision, revision) || detail.projectionStamp !== change.projectionStamp || detail.associations.some(
        (association) => !sameRevision(association.comparison.revision, revision)
      )) {
        throw new Error("Revision detail generation is stale; refresh and retry");
      }
      if (!await detailPostflight(request)) return;
      const content = [
        heading(revision.revisionId),
        line(`currency: ${words(detail.revisionCurrency)}`),
        line(`relation: ${words(detail.relationClassification)}`),
        line(`captured resource: ${words(detail.availability)}`)
      ];
      const facts = document.createElement("section");
      facts.append(heading("Facts", 3));
      for (const fact of detail.factPresentations) {
        facts.append(
          line(
            `${fact.family}: ${fact.factId} · origin ${fact.originRevision.revisionId} · ${words(fact.revisionCurrency)} · ${words(fact.familyState)} · ${words(fact.availability)}`
          )
        );
      }
      if (detail.factPresentations.length === 0) {
        facts.append(message("No facts."));
      }
      content.push(facts);
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
      content.push(associations);
      const resource = document.createElement("button");
      resource.type = "button";
      resource.className = "ghost";
      resource.textContent = "Open exact captured resource";
      resource.addEventListener("click", () => {
        void loadRevisionResource(change, revision);
      });
      content.push(resource);
      publishDetail(request, content);
    } catch (error) {
      if (!isLiveDetailRequest(request)) return;
      renderRefusal(error);
    }
  }
  __name(loadRevisionDetail, "loadRevisionDetail");
  async function loadRevisionResource(change, revision) {
    const request = beginDetailRequest(change);
    if (request === null) return;
    try {
      const params = new URLSearchParams({
        artifactHash: revision.objectArtifactContentHash
      });
      const resource = decodeRevisionResource(
        await fetchJSON(
          `/api/v2/changes/${encodeURIComponent(change.changeId)}/revisions/${encodeURIComponent(revision.revisionId)}/resource?${params}`
        )
      );
      if (!isLiveDetailRequest(request)) return;
      if (!sameRevision(resource.resource.revision, revision)) {
        throw new Error(
          "captured resource identity does not match its exact route"
        );
      }
      if (!await detailPostflight(request)) return;
      const content = [
        heading(`Captured resource · ${revision.revisionId}`),
        line(`availability: ${words(resource.availability)}`)
      ];
      if (resource.capturedDocumentHash) {
        content.push(line(`document hash: ${resource.capturedDocumentHash}`));
      }
      if (resource.capturedDocument !== void 0) {
        const captured = document.createElement("pre");
        captured.textContent = JSON.stringify(resource.capturedDocument, null, 2);
        content.push(captured);
      }
      for (const diagnostic of resource.diagnostics) {
        content.push(line(diagnostic));
      }
      publishDetail(request, content);
    } catch (error) {
      if (!isLiveDetailRequest(request)) return;
      renderRefusal(error);
    }
  }
  __name(loadRevisionResource, "loadRevisionResource");
  async function loadInterdiff(change, from, to) {
    const request = beginDetailRequest(change);
    if (request === null) return;
    try {
      const params = new URLSearchParams({
        fromArtifactHash: from.objectArtifactContentHash,
        toArtifactHash: to.objectArtifactContentHash
      });
      const interdiff = decodeRevisionInterdiff(
        await fetchJSON(
          `/api/v2/changes/${encodeURIComponent(change.changeId)}/interdiff/${encodeURIComponent(from.revisionId)}/${encodeURIComponent(to.revisionId)}?${params}`
        )
      );
      if (!isLiveDetailRequest(request)) return;
      if (!sameRevision(interdiff.interdiff.from, from) || !sameRevision(interdiff.interdiff.to, to)) {
        throw new Error(
          "Revision interdiff identity does not match its exact route"
        );
      }
      if (!await detailPostflight(request)) return;
      const content = [
        heading(`Revision interdiff · ${from.revisionId} → ${to.revisionId}`),
        line(`availability: ${words(interdiff.availability)}`)
      ];
      if (interdiff.comparison !== void 0) {
        const comparison = document.createElement("pre");
        comparison.textContent = JSON.stringify(interdiff.comparison, null, 2);
        content.push(comparison);
      }
      for (const diagnostic of interdiff.diagnostics) {
        content.push(line(diagnostic));
      }
      publishDetail(request, content);
    } catch (error) {
      if (!isLiveDetailRequest(request)) return;
      renderRefusal(error);
    }
  }
  __name(loadInterdiff, "loadInterdiff");
  function publishDetail(request, content) {
    if (!isLiveDetailRequest(request)) return;
    const body = document.querySelector("#detail-body");
    if (!body) throw new Error("Inspector detail container is absent");
    body.replaceChildren(...content);
  }
  __name(publishDetail, "publishDetail");
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
  function renderRestart(error) {
    const text = error instanceof ChangePageFailure && error.code === "stale_projection" ? "Change page became stale; restarting from the first page." : "Change generation changed while loading; restarting from the first page.";
    const banner = document.querySelector("#error");
    if (banner) {
      banner.textContent = text;
      banner.classList.remove("hidden");
    }
  }
  __name(renderRestart, "renderRestart");
  function clearSemanticPresentation() {
    readerEpoch += 1;
    detailSelectionEpoch += 1;
    visibleGeneration = null;
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
