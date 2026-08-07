import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  mkdtempSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  assertExactArchiveFiles,
  assertExactPackageFiles,
  powershellCommand,
  verifyBundledBinary,
} from "./package-contract.mjs";

const scriptRoot = path.dirname(fileURLToPath(import.meta.url));
const extensionRoot = path.resolve(scriptRoot, "..");
const repoRoot = path.resolve(extensionRoot, "../..");
const readerProfileContract = JSON.parse(
  readFileSync(
    path.join(repoRoot, "src/documents/change_reader_profile_v1.json"),
    "utf8",
  ),
);
const targetManifest = JSON.parse(
  readFileSync(path.join(repoRoot, ".github/binary-targets.json"), "utf8"),
);
const hostLabel = `${process.platform}-${process.arch}`;
const knownLabels = new Set(targetManifest.map(({ target }) => target));
const hostTarget = targetManifest.find(({ target }) => target === hostLabel);
if (!hostTarget) {
  throw new Error(
    `Unsupported packaging host ${hostLabel}; expected one of ${[...knownLabels].join(", ")}`,
  );
}

const executable = hostTarget.executable;
const configuredBinary = process.env.POINTBREAK_EXTENSION_BINARY?.trim();
const sourceBinary = configuredBinary
  ? path.resolve(configuredBinary)
  : path.join(repoRoot, "target", "release", executable);
const bundledRelative = `bin/${hostLabel}/${executable}`;
const bundledBinary = path.join(extensionRoot, bundledRelative);

if (!configuredBinary) {
  run("cargo", ["build", "--release", "--bin", "pointbreak"], repoRoot);
} else if (!existsSync(sourceBinary)) {
  throw new Error(
    `Configured Pointbreak executable does not exist: ${sourceBinary}`,
  );
}

const approvedEvidence = binaryEvidence(sourceBinary);
const expectedSha256 = process.env.POINTBREAK_EXTENSION_BINARY_SHA256?.trim();
if (
  expectedSha256 &&
  approvedEvidence.sha256 !== expectedSha256.toLowerCase()
) {
  throw new Error(
    `Approved Pointbreak executable SHA-256 mismatch: expected ${expectedSha256.toLowerCase()}, received ${approvedEvidence.sha256}`,
  );
}
mkdirSync(path.dirname(bundledBinary), { recursive: true });
copyFileSync(sourceBinary, bundledBinary);
const bundledEvidence = binaryEvidence(bundledBinary);
verifyBundledBinary(approvedEvidence, bundledEvidence);

// The extension's version is derived from Git, not the checked-in package.json
// value (automation will own that field once releases start). We describe against
// the extension's own release tags (vscode-v<semver>) and fall back to 0.0.0 while
// none exist yet. vsce stamps this derived version into the packaged VSIX (its
// manifest and bundled package.json, i.e. what the Extensions UI shows) via the
// [version] arg with --no-update-package-json, so the source package.json is never
// modified and the worktree stays clean. Dogfood builds carry the full description
// (0.0.0-g<sha>[-dirty]) so each build is distinguishable in the UI and the filename;
// POINTBREAK_EXTENSION_CLEAN_VERSION=1 yields a publish-shaped, undecorated
// Major.Minor.Patch version instead.
const profile =
  process.env.POINTBREAK_EXTENSION_PROFILE?.trim() ||
  path.basename(path.dirname(sourceBinary)) ||
  "custom";
const identity = extensionIdentity(repoRoot);
const baseVersion = identity.baseVersion;
const cleanVersion = process.env.POINTBREAK_EXTENSION_CLEAN_VERSION === "1";
let versionTag = baseVersion;
if (!cleanVersion) {
  if (identity.source !== "git") {
    versionTag = `${baseVersion}-nogit`;
  } else if (identity.baseTag) {
    // git describe already encodes distance, short commit, and dirtiness.
    versionTag = identity.describe.replace(/^vscode-v/, "");
  } else {
    versionTag = `${baseVersion}-g${identity.shortCommit}${identity.dirty ? "-dirty" : ""}`;
  }
}
const outDir = path.join(repoRoot, "target", "vsix", hostLabel, profile);
const outPath = path.join(outDir, `pointbreak-${hostLabel}-${versionTag}.vsix`);

// Keep exactly one artifact per target/profile, and clear any legacy root VSIX.
mkdirSync(outDir, { recursive: true });
for (const dir of [extensionRoot, outDir]) {
  for (const entry of readdirSync(dir)) {
    if (entry.startsWith("pointbreak-") && entry.endsWith(".vsix")) {
      rmSync(path.join(dir, entry));
    }
  }
}

runNodePackageCommand("npm", ["run", "build"], extensionRoot);
assertListedFiles(bundledRelative);
runNodePackageCommand(
  "npx",
  [
    "--no-install",
    "vsce",
    "package",
    versionTag,
    "--no-update-package-json",
    "--no-git-tag-version",
    "--target",
    hostLabel,
    "-o",
    outPath,
  ],
  extensionRoot,
);

if (!existsSync(outPath)) {
  throw new Error(`vsce did not produce the expected VSIX: ${outPath}`);
}
const artifact = outPath;
assertArchiveFiles(artifact, bundledRelative);
const archivedEvidence = {
  sha256: archiveBinarySha256(artifact, bundledRelative),
  versionDocument: bundledEvidence.versionDocument,
  readerProfileDocument: bundledEvidence.readerProfileDocument,
};
verifyBundledBinary(approvedEvidence, archivedEvidence);
console.log(
  JSON.stringify(
    {
      vsix: artifact,
      target: hostLabel,
      profile,
      extensionVersion: baseVersion,
      versionTag,
      build: {
        source: identity.source,
        commit: identity.commit,
        shortCommit: identity.shortCommit,
        dirty: identity.dirty,
        baseTag: identity.baseTag,
        describe: identity.describe,
      },
      bundledBinary: {
        path: bundledRelative,
        sha256: archivedEvidence.sha256,
        version: JSON.parse(archivedEvidence.versionDocument),
        readerProfile: JSON.parse(archivedEvidence.readerProfileDocument),
      },
    },
    null,
    2,
  ),
);

function assertListedFiles(binary) {
  const result = runNodePackageCommand(
    "npx",
    ["--no-install", "vsce", "ls"],
    extensionRoot,
    true,
  );
  assertExactPackageFiles(
    result.stdout.split(/\r?\n/).filter(Boolean),
    binary,
    "vsce ls",
  );
}

function runNodePackageCommand(command, args, cwd, capture = false) {
  if (process.platform !== "win32") return run(command, args, cwd, capture);

  const scripts = {
    npm: "npm-cli.js",
    npx: "npx-cli.js",
  };
  const script = scripts[command];
  if (!script) throw new Error(`Unsupported Node package command: ${command}`);
  const scriptPath = path.join(
    path.dirname(process.execPath),
    "node_modules",
    "npm",
    "bin",
    script,
  );
  if (!existsSync(scriptPath)) {
    throw new Error(`Node package command entry point does not exist: ${scriptPath}`);
  }
  return run(process.execPath, [scriptPath, ...args], cwd, capture);
}

function assertArchiveFiles(artifact, binary) {
  const files = archiveEntries(artifact)
    .split(/\r?\n/)
    .filter((entry) => entry && !entry.endsWith("/"));
  assertExactArchiveFiles(files, binary, "VSIX archive");
}

function binaryEvidence(binary) {
  const versionDocument = run(
    binary,
    ["version", "--format", "json"],
    repoRoot,
    true,
  ).stdout.trim();
  let version;
  try {
    version = JSON.parse(versionDocument);
  } catch {
    throw new Error(
      `Pointbreak executable returned invalid version JSON: ${binary}`,
    );
  }
  if (
    version.schema !== "pointbreak.version" ||
    version.version !== 1 ||
    typeof version.cliVersion !== "string" ||
    typeof version.documents !== "object" ||
    version.documents === null
  ) {
    throw new Error(
      `Pointbreak executable returned an incompatible machine identity: ${binary}`,
    );
  }
  const readerProfileDocument = profileEvidence(binary);
  return { sha256: sha256File(binary), versionDocument, readerProfileDocument };
}

function profileEvidence(binary) {
  const root = mkdtempSync(path.join(tmpdir(), "pointbreak-vsix-profile-"));
  const repo = path.join(root, "repo");
  const home = path.join(root, "home");
  mkdirSync(repo);
  try {
    run("git", ["init", "-q"], repo);
    const result = spawnSync(
      binary,
      ["change", "profile", "--repo", repo, "--format", "json"],
      {
        cwd: repo,
        env: { ...process.env, POINTBREAK_HOME: home },
        encoding: "utf8",
        maxBuffer: 16 * 1024 * 1024,
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
    if (result.error) throw result.error;
    if (result.status !== 0) {
      throw new Error(
        `Pointbreak executable could not advertise its Change reader profile: ${result.stderr.trim()}`,
      );
    }
    const document = result.stdout.trim();
    let profile;
    try {
      profile = JSON.parse(document);
    } catch {
      throw new Error(
        `Pointbreak executable returned invalid Change reader profile JSON: ${binary}`,
      );
    }
    if (
      profile.schema !== "pointbreak.inspect-reader-profile" ||
      profile.version !== 1 ||
      !["migration_required", "migration_in_progress", "ready"].includes(
        profile.availability,
      ) ||
      !sameVersionMap(profile.documents, readerProfileContract.documents)
    ) {
      throw new Error(
        `Pointbreak executable returned an incompatible Change reader profile: ${binary}`,
      );
    }
    return document;
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function sameVersionMap(actual, expected) {
  if (!actual || typeof actual !== "object") return false;
  const actualEntries = Object.entries(actual).sort(([left], [right]) =>
    left.localeCompare(right),
  );
  const expectedEntries = Object.entries(expected).sort(([left], [right]) =>
    left.localeCompare(right),
  );
  return JSON.stringify(actualEntries) === JSON.stringify(expectedEntries);
}

function sha256File(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}

function archiveBinarySha256(artifact, binary) {
  const entry = `extension/${binary}`;
  if (process.platform !== "win32") {
    const result = run(
      "unzip",
      ["-p", artifact, entry],
      extensionRoot,
      true,
      null,
    );
    return createHash("sha256").update(result.stdout).digest("hex");
  }
  const hashScript = powershellCommand(
    [
      "Add-Type -AssemblyName System.IO.Compression.FileSystem",
      "$archive = [System.IO.Compression.ZipFile]::OpenRead($args[0])",
      "$entry = $archive.GetEntry($args[1])",
      'if ($null -eq $entry) { throw "missing archive entry $($args[1])" }',
      "$stream = $entry.Open()",
      "$sha = [System.Security.Cryptography.SHA256]::Create()",
      'try { [BitConverter]::ToString($sha.ComputeHash($stream)).Replace("-", "").ToLowerInvariant() } finally { $sha.Dispose(); $stream.Dispose(); $archive.Dispose() }',
    ].join("; "),
  );
  return run(
    "powershell.exe",
    ["-NoProfile", "-NonInteractive", "-Command", hashScript, artifact, entry],
    extensionRoot,
    true,
  ).stdout.trim();
}

function archiveEntries(artifact) {
  if (process.platform !== "win32") {
    return run("unzip", ["-Z1", artifact], extensionRoot, true).stdout;
  }
  const listScript = powershellCommand(
    [
      "Add-Type -AssemblyName System.IO.Compression.FileSystem",
      "$archive = [System.IO.Compression.ZipFile]::OpenRead($args[0])",
      "try { $archive.Entries | ForEach-Object FullName } finally { $archive.Dispose() }",
    ].join("; "),
  );
  return run(
    "powershell.exe",
    ["-NoProfile", "-NonInteractive", "-Command", listScript, artifact],
    extensionRoot,
    true,
  ).stdout;
}

function run(command, args, cwd, capture = false, encoding = "utf8") {
  const result = spawnSync(command, args, {
    cwd,
    encoding,
    // Captured stdout has to hold a whole binary when archiveBinarySha256 pipes
    // `unzip -p` of the bundled executable. An unstripped debug build exceeds the
    // former 64 MiB ceiling, so double it. Revisit if release binaries approach this.
    maxBuffer: 128 * 1024 * 1024,
    stdio: capture ? ["ignore", "pipe", "pipe"] : "inherit",
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    const stderr = Buffer.isBuffer(result.stderr)
      ? result.stderr.toString("utf8").trim()
      : result.stderr?.trim();
    throw new Error(
      `${command} ${args.join(" ")} failed with exit code ${result.status}${stderr ? `: ${stderr}` : ""}`,
    );
  }
  return result;
}

// Snapshot the extension's Git build identity with the same rules build.rs uses for
// the CLI -- `git describe --tags --always --dirty` at Git's default abbreviation,
// tracked-file-only dirtiness, and the full 40-char commit -- except scoped to the
// extension's own release tags (--match vscode-v*) rather than the CLI's v* tags.
// baseVersion is the nearest such tag's semver, falling back to 0.0.0 before the
// first release. Returns source "package" when the tree is not a Git repo.
function extensionIdentity(cwd) {
  const commit = tryGit(cwd, ["rev-parse", "--verify", "HEAD^{commit}"]);
  if (!commit || !/^[0-9a-f]{40}$/.test(commit)) {
    return {
      source: "package",
      commit: null,
      shortCommit: null,
      dirty: false,
      baseTag: null,
      baseVersion: "0.0.0",
      describe: null,
    };
  }
  // Git's default abbreviation, like build.rs (no explicit --abbrev); the fallback
  // width only applies if rev-parse --short is somehow unavailable.
  const shortCommit =
    tryGit(cwd, ["rev-parse", "--short", "HEAD"]) ?? commit.slice(0, 7);
  const status = tryGit(cwd, [
    "status",
    "--porcelain=v1",
    "--untracked-files=no",
  ]);
  const dirty = status !== null && status.length > 0;
  // --abbrev=0 yields the bare nearest tag (null when none match yet); the second
  // describe keeps --always so it degrades to the short commit before any release.
  const baseTag = tryGit(cwd, [
    "describe",
    "--tags",
    "--match",
    "vscode-v*",
    "--abbrev=0",
  ]);
  const baseVersion = baseTag ? baseTag.replace(/^vscode-v/, "") : "0.0.0";
  const describe = tryGit(cwd, [
    "describe",
    "--tags",
    "--match",
    "vscode-v*",
    "--always",
    "--dirty",
  ]);
  return {
    source: "git",
    commit,
    shortCommit,
    dirty,
    baseTag,
    baseVersion,
    describe,
  };
}

function tryGit(cwd, args) {
  const result = spawnSync("git", args, {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
  if (result.error || result.status !== 0) {
    return null;
  }
  return result.stdout.trim();
}
