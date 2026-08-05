import { expect, it } from "vitest";
import type { ResolvedBinary } from "../src/binary";
import {
  CHANGE_READER_DOCUMENTS,
  validateReaderProfile,
} from "../src/changeProtocol";
import {
  type BuildIdentityV1,
  type ExecFn,
  PointbreakCli,
  REQUIRED_DOCUMENTS,
  verifyHandshake,
} from "../src/cli";
import { VERSION_DOC, VERSION_JSON } from "./fixtures";

const binary: ResolvedBinary = {
  path: "/bin/arbitrarily-named-review-cli",
  source: "setting",
};

it("pins the exact extension document handshake", () => {
  expect(REQUIRED_DOCUMENTS).toEqual({
    "pointbreak.version": 1,
    "pointbreak.attention-list": 1,
    "pointbreak.identity-whoami": 1,
    "pointbreak.review-assessment-add": 1,
    "pointbreak.review-assessment-show": 1,
    "pointbreak.review-revision-list": 1,
    "pointbreak.review-revision": 2,
    "pointbreak.review-capture": 1,
    "pointbreak.review-input-request-respond": 1,
    "pointbreak.review-observation-add": 1,
    "pointbreak.review-snapshot": 1,
    "pointbreak.review-validation-add": 1,
    "pointbreak.inspect-freshness": 1,
    "pointbreak.inspect-startup": 1,
    "pointbreak.store-status": 1,
  });
});

it("pins the separate Change-capable reader profile registry", () => {
  expect(CHANGE_READER_DOCUMENTS).toEqual({
    "pointbreak.inspect-reader-profile": 1,
    "pointbreak.review-change-list": 1,
    "pointbreak.inspect-changes-page": 1,
    "pointbreak.review-change": 1,
    "pointbreak.review-change-revision": 1,
    "pointbreak.review-revision": 3,
    "pointbreak.review-revision-resource": 1,
    "pointbreak.review-association-comparison": 1,
    "pointbreak.review-revision-interdiff": 1,
    "pointbreak.attention-list": 2,
    "pointbreak.inspect-attention": 2,
    "pointbreak.reader-upgrade-required": 1,
    "pointbreak.store-migration-required": 1,
    "pointbreak.store-migration-in-progress": 1,
  });
});

it("rejects additive Change documents until the client registry is updated", () => {
  expect(() =>
    validateReaderProfile({
      schema: "pointbreak.inspect-reader-profile",
      version: 1,
      availability: "ready",
      minimumReaderProfile: "review_change_revision_v1",
      authorityCursor: { eventCount: 1 },
      documents: {
        ...CHANGE_READER_DOCUMENTS,
        "pointbreak.future-change-document": 1,
      },
    }),
  ).toThrow(/unrecognized document contract/i);
});

it("rejects an array-shaped authority cursor before semantic reads", () => {
  expect(() =>
    validateReaderProfile({
      schema: "pointbreak.inspect-reader-profile",
      version: 1,
      availability: "ready",
      minimumReaderProfile: "review_change_revision_v1",
      authorityCursor: [],
      documents: CHANGE_READER_DOCUMENTS,
    }),
  ).toThrow(/incompatible Change reader document/i);
});

it("fails closed when a required document version mismatches", () => {
  const doc = {
    ...VERSION_DOC,
    documents: {
      ...VERSION_DOC.documents,
      "pointbreak.attention-list": 2,
    },
  };

  const result = verifyHandshake(doc);

  expect(result.ok).toBe(false);
  expect(result.ok === false && result.reason).toMatch(/attention-list/);
});

it("accepts a capable registry without treating product version as store authority", () => {
  expect(verifyHandshake(VERSION_DOC)).toEqual({
    ok: true,
    cliVersion: "0.7.0",
  });
  expect(verifyHandshake({ ...VERSION_DOC, cliVersion: "9.0.0" })).toEqual({
    ok: true,
    cliVersion: "9.0.0",
  });
});

it("tolerates additive build identity and unrelated soft-shell fields", () => {
  const build: BuildIdentityV1 = {
    source: "git",
    commit: "d2bc01650076314897bb8c30ba57623640c0d257",
    describe: "v0.7.0-34-gd2bc016",
    dirty: false,
  };

  expect(
    verifyHandshake({ ...VERSION_DOC, build, futureSoftShellField: true }),
  ).toEqual({ ok: true, cliVersion: "0.7.0" });
});

it("still fails closed on an unknown version envelope with additive fields", () => {
  const result = verifyHandshake({
    ...VERSION_DOC,
    version: 2,
    futureSoftShellField: true,
  } as unknown as typeof VERSION_DOC);

  expect(result.ok).toBe(false);
  expect(result.ok === false && result.reason).toMatch(/version 1/);
});

it("executes an arbitrary configured path only through the exact handshake", async () => {
  const invocations: Array<{ file: string; args: string[] }> = [];
  const exec: ExecFn = async (file, args) => {
    invocations.push({ file, args });
    return { stdout: VERSION_JSON, stderr: "", exitCode: 0 };
  };
  const cli = new PointbreakCli(binary, exec);

  await expect(cli.version("/repo")).resolves.toEqual(VERSION_DOC);
  expect(invocations).toEqual([{ file: binary.path, args: ["version"] }]);
});

it("fails closed when the document map omits a required member", () => {
  const documents = { ...VERSION_DOC.documents };
  delete documents["pointbreak.store-status"];
  const result = verifyHandshake({
    ...VERSION_DOC,
    documents,
  });

  expect(result.ok).toBe(false);
  expect(result.ok === false && result.reason).toMatch(/store-status|missing/i);
});

it("fails closed when the version document body is malformed", () => {
  const result = verifyHandshake({
    schema: "pointbreak.version",
    version: 1,
    diagnostics: [],
  } as unknown as typeof VERSION_DOC);

  expect(result.ok).toBe(false);
});

it("fails closed with Pointbreak-only wording when version is unavailable", async () => {
  const exec: ExecFn = async () => ({
    stdout: "",
    stderr: "unknown subcommand 'version'",
    exitCode: 2,
  });
  const cli = new PointbreakCli(binary, exec);

  await expect(cli.version("/repo")).rejects.toThrow(
    /pointbreak version failed/i,
  );
});
