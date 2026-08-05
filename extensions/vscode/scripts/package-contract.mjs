export const runtimeFiles = [
  "out/extension.js",
  "out/review.js",
  "out/review.css",
];

const listedFiles = ["package.json", "README.md", "LICENSE", "NOTICE"];
const archiveFiles = ["package.json", "readme.md", "LICENSE.txt", "NOTICE"];

export function assertExactPackageFiles(
  actual,
  binary,
  source,
  packageFiles = listedFiles,
) {
  const expected = [...packageFiles, ...runtimeFiles, binary];
  assertExactFiles(actual, expected, source);
}

export function assertExactArchiveFiles(actual, binary, source) {
  const expected = [
    "[Content_Types].xml",
    "extension.vsixmanifest",
    ...archiveFiles.map((file) => `extension/${file}`),
    ...runtimeFiles.map((file) => `extension/${file}`),
    `extension/${binary}`,
  ];
  assertExactFiles(actual, expected, source);
}

function assertExactFiles(actual, expected, source) {
  const sortedActual = [...actual].sort();
  const sortedExpected = [...expected].sort();
  if (JSON.stringify(sortedActual) !== JSON.stringify(sortedExpected)) {
    throw new Error(
      `${source} contained unexpected files.\nExpected: ${sortedExpected.join(", ")}\nActual: ${sortedActual.join(", ")}`,
    );
  }
}

export function verifyBundledBinary(approved, bundled) {
  if (bundled.sha256 !== approved.sha256) {
    throw new Error(
      `Bundled executable SHA-256 mismatch: expected ${approved.sha256}, received ${bundled.sha256}`,
    );
  }
  if (bundled.versionDocument !== approved.versionDocument) {
    throw new Error(
      "Bundled executable machine identity does not match the approved input.",
    );
  }
  if (bundled.readerProfileDocument !== approved.readerProfileDocument) {
    throw new Error(
      "Bundled executable Change reader profile does not match the approved input.",
    );
  }
}

export function powershellCommand(script) {
  return `& { ${script} }`;
}
