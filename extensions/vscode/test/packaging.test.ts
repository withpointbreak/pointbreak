import { readFileSync } from "node:fs";
import { expect, it } from "vitest";
import {
  assertExactArchiveFiles,
  assertExactPackageFiles,
  powershellCommand,
  verifyBundledBinary,
} from "../scripts/package-contract.mjs";

const packageScript = readFileSync("scripts/package-local.mjs", "utf8");
const packageContract = readFileSync("scripts/package-contract.mjs", "utf8");

it("excludes the package-local Git ignore file from the VSIX", () => {
  const ignored = readFileSync(".vscodeignore", "utf8").split("\n");

  expect(ignored).toContain(".gitignore");
});

it("excludes debug source maps from the VSIX", () => {
  const ignored = readFileSync(".vscodeignore", "utf8").split("\n");

  expect(ignored).toContain("out/**/*.map");
});

it("excludes development-only packaging scripts from the VSIX", () => {
  const ignored = readFileSync(".vscodeignore", "utf8").split("\n");

  expect(ignored).toContain("scripts/**");
});

it("includes the webview runtime in both exact package allowlists", () => {
  expect(packageContract.match(/"out\/review\.js"/g)).toHaveLength(1);
  expect(packageContract.match(/"out\/review\.css"/g)).toHaveLength(1);
  expect(packageContract).toContain("...runtimeFiles");
  expect(packageScript).toContain("assertExactPackageFiles");
  expect(packageScript).toContain("assertExactArchiveFiles");
});

it("keeps command runtime modules bundled into the existing host artifact", () => {
  expect(packageContract).toContain('"out/extension.js"');
  expect(packageContract).not.toContain("problemsSnapshot.js");
  expect(packageContract).not.toContain("recordProblemsSnapshot.js");
  expect(packageContract).not.toContain("runTaskAndRecordValidation.js");
});

it("binds arguments through a PowerShell scriptblock on Windows", () => {
  expect(powershellCommand("Write-Output $args.Count")).toBe(
    "& { Write-Output $args.Count }",
  );
  expect(packageScript.match(/powershellCommand\(/g)).toHaveLength(2);
});

it("uses Windows command shims for npm and npx packaging subprocesses", () => {
  expect(packageScript).toMatch(
    /process\.platform === "win32" \? `\$\{command\}\.cmd` : command/,
  );
  expect(packageScript).toContain('["/d", "/s", "/c", command, ...args]');
  expect(packageScript).not.toMatch(/run\("np[62x]"/);
});

it("excludes source and local-only build inputs from the VSIX", () => {
  const ignored = readFileSync(".vscodeignore", "utf8").split("\n");

  expect(ignored).toEqual(
    expect.arrayContaining([
      ".gitignore",
      "build.mjs",
      "src/**",
      "test/**",
      "scripts/**",
      "out/**/*.map",
    ]),
  );
});

const expectedFiles = [
  "package.json",
  "README.md",
  "LICENSE",
  "NOTICE",
  "out/extension.js",
  "out/review.js",
  "out/review.css",
  "bin/darwin-arm64/pointbreak",
];

it.each([
  {
    name: "the retired executable",
    files: expectedFiles.map((file) => file.replace(/pointbreak$/, "shore")),
  },
  {
    name: "a second executable",
    files: [...expectedFiles, "bin/darwin-arm64/shore"],
  },
  {
    name: "an extra payload",
    files: [...expectedFiles, "unexpected.txt"],
  },
])("rejects $name in the package allowlist", ({ files }) => {
  expect(() =>
    assertExactPackageFiles(
      files,
      "bin/darwin-arm64/pointbreak",
      "test package",
    ),
  ).toThrow(/unexpected files/i);
});

it("rejects an extra top-level VSIX payload", () => {
  const archiveFiles = [
    "[Content_Types].xml",
    "extension.vsixmanifest",
    ...expectedFiles.map((file) =>
      file === "README.md"
        ? "extension/readme.md"
        : file === "LICENSE"
          ? "extension/LICENSE.txt"
          : `extension/${file}`,
    ),
    "unexpected.txt",
  ];

  expect(() =>
    assertExactArchiveFiles(
      archiveFiles,
      "bin/darwin-arm64/pointbreak",
      "test VSIX",
    ),
  ).toThrow(/unexpected files/i);
});

it("rejects a bundled executable hash mismatch", () => {
  expect(() =>
    verifyBundledBinary(
      {
        sha256: "a".repeat(64),
        versionDocument: '{"schema":"pointbreak.version"}',
      },
      {
        sha256: "b".repeat(64),
        versionDocument: '{"schema":"pointbreak.version"}',
      },
    ),
  ).toThrow(/sha-256/i);
});

it("rejects a bundled executable machine-identity mismatch", () => {
  expect(() =>
    verifyBundledBinary(
      {
        sha256: "a".repeat(64),
        versionDocument:
          '{"schema":"pointbreak.version","version":1,"cliVersion":"0.7.0"}',
      },
      {
        sha256: "a".repeat(64),
        versionDocument:
          '{"schema":"pointbreak.version","version":1,"cliVersion":"0.7.1"}',
      },
    ),
  ).toThrow(/machine identity/i);
});

it("rejects a bundled executable Change reader profile mismatch", () => {
  expect(() =>
    verifyBundledBinary(
      {
        sha256: "a".repeat(64),
        versionDocument: '{"schema":"pointbreak.version"}',
        readerProfileDocument: '{"documents":{"pointbreak.review-change":1}}',
      },
      {
        sha256: "a".repeat(64),
        versionDocument: '{"schema":"pointbreak.version"}',
        readerProfileDocument: '{"documents":{"pointbreak.review-change":0}}',
      },
    ),
  ).toThrow(/reader profile/i);
});
