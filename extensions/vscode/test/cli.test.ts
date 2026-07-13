import { afterEach, describe, expect, it } from "vitest";
import type { ResolvedBinary } from "../src/binary";
import {
  type CaptureOptions,
  captureArgs,
  type ExecFn,
  PointbreakCli,
} from "../src/cli";
import { ATTENTION_JSON, REVISION_LIST_JSON, VERSION_JSON } from "./fixtures";

const binary: ResolvedBinary = { path: "/bin/shore", source: "setting" };

afterEach(() => {
  delete process.env.SHORE_ACTOR_ID;
  delete process.env.SHORE_FORMAT;
});

it("strips actor and format overrides from every spawn env", async () => {
  process.env.SHORE_ACTOR_ID = "actor:agent:leaked";
  process.env.SHORE_FORMAT = "text";
  const seen: NodeJS.ProcessEnv[] = [];
  const exec: ExecFn = async (_file, args, opts) => {
    seen.push(opts.env);
    const stdout = args[0] === "version" ? VERSION_JSON : ATTENTION_JSON;
    return { stdout, stderr: "", exitCode: 0 };
  };
  const cli = new PointbreakCli(binary, exec);

  await cli.attentionList("/repo");

  expect(seen).toHaveLength(2);
  for (const env of seen) {
    expect(env).not.toHaveProperty("SHORE_ACTOR_ID");
    expect(env).not.toHaveProperty("SHORE_FORMAT");
  }
});

it("sanitizes writes through the same spawn seam", async () => {
  process.env.SHORE_ACTOR_ID = "actor:agent:leaked";
  process.env.SHORE_FORMAT = "text";
  const seen: NodeJS.ProcessEnv[] = [];
  const exec: ExecFn = async (_file, args, opts) => {
    seen.push(opts.env);
    return {
      stdout:
        args[0] === "version"
          ? VERSION_JSON
          : JSON.stringify({
              schema: "pointbreak.review-capture",
              version: 1,
              revision: { id: "rev:sha256:1234" },
              diagnostics: [],
            }),
      stderr: "",
      exitCode: 0,
    };
  };
  const cli = new PointbreakCli(binary, exec);

  await cli.capture("/repo", {
    choice: "worktree",
    includeUntracked: false,
    allowEmpty: false,
  });

  expect(seen).toHaveLength(2);
  expect(seen.every((env) => env.SHORE_ACTOR_ID === undefined)).toBe(true);
  expect(seen.every((env) => env.SHORE_FORMAT === undefined)).toBe(true);
});

describe("captureArgs", () => {
  const args = (opts: CaptureOptions) => captureArgs(opts);

  it("maps choices to exact flags", () => {
    expect(
      args({ choice: "worktree", includeUntracked: true, allowEmpty: false }),
    ).toEqual(["capture", "--include-untracked"]);
    expect(
      args({ choice: "staged", includeUntracked: false, allowEmpty: false }),
    ).toEqual(["capture", "--staged"]);
    expect(
      args({ choice: "unstaged", includeUntracked: true, allowEmpty: true }),
    ).toEqual([
      "capture",
      "--unstaged",
      "--include-untracked",
      "--allow-empty",
    ]);
  });

  it("rejects including untracked files with a staged capture", () => {
    expect(() =>
      args({ choice: "staged", includeUntracked: true, allowEmpty: false }),
    ).toThrow(/staged/i);
  });
});

it("rejects a document whose schema does not match the requested command", async () => {
  const exec: ExecFn = async (_file, args) => ({
    stdout:
      args[0] === "version"
        ? VERSION_JSON
        : JSON.stringify({ schema: "wrong.schema", version: 1 }),
    stderr: "",
    exitCode: 0,
  });
  const cli = new PointbreakCli(binary, exec);

  await expect(cli.storeStatus("/repo")).rejects.toThrow(/store-status/);
});

it("decodes the live revision-list entries shape", async () => {
  const exec: ExecFn = async (_file, args) => ({
    stdout: args[0] === "version" ? VERSION_JSON : REVISION_LIST_JSON,
    stderr: "",
    exitCode: 0,
  });
  const cli = new PointbreakCli(binary, exec);

  const document = await cli.revisionList("/repo");

  expect(document.entries[0]).toMatchObject({
    revisionId: "rev:sha256:9442bfeb",
    mergeStatus: "merged",
  });
  expect(document.revisionCount).toBe(1);
});
