import { Buffer } from "node:buffer";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const defaultRepositoryRoot = resolve(scriptDirectory, "..");
const distribution = JSON.parse(
  readFileSync(join(defaultRepositoryRoot, "src-tauri", "adb-distribution.json"), "utf8"),
);

export const BUNDLED_ADB = Object.freeze({
  ...distribution,
  files: Object.freeze(distribution.files.map((file) => Object.freeze(file))),
});

export function assertBundledAdbResources(directory) {
  for (const file of BUNDLED_ADB.files) {
    const path = join(directory, file.name);
    if (!existsSync(path)) {
      throw new Error(`Bundled ADB resource is missing: ${file.name}`);
    }

    const actualSha256 = createHash("sha256").update(readFileSync(path)).digest("hex");
    if (actualSha256 !== file.sha256) {
      throw new Error(
        `Bundled ADB checksum mismatch for ${file.name}: expected ${file.sha256}, found ${actualSha256}`,
      );
    }
  }
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function runChecked(program, arguments_) {
  const result = spawnSync(program, arguments_, { stdio: "inherit" });
  if (result.error) {
    throw new Error(`Unable to start ${program}: ${result.error.message}`);
  }
  if (result.status !== 0) {
    throw new Error(
      `Command failed with exit code ${result.status}: ${program} ${arguments_.join(" ")}`,
    );
  }
}

async function ensureArchive(archivePath) {
  if (existsSync(archivePath)) {
    const cached = readFileSync(archivePath);
    if (sha256(cached) === BUNDLED_ADB.archiveSha256) {
      return;
    }
  }

  const response = await globalThis.fetch(BUNDLED_ADB.packageUrl, {
    signal: globalThis.AbortSignal.timeout(30_000),
  });
  if (!response.ok) {
    throw new Error(
      `Unable to download bundled ADB package: HTTP ${response.status} ${response.statusText}`,
    );
  }

  const archive = Buffer.from(await response.arrayBuffer());
  const actualSha256 = sha256(archive);
  if (actualSha256 !== BUNDLED_ADB.archiveSha256) {
    throw new Error(
      `Bundled ADB archive checksum mismatch: expected ${BUNDLED_ADB.archiveSha256}, found ${actualSha256}`,
    );
  }

  mkdirSync(dirname(archivePath), { recursive: true });
  writeFileSync(archivePath, archive);
}

export async function prepareBundledAdb(repositoryRoot) {
  const generatedRoot = join(repositoryRoot, "src-tauri", "generated");
  const resourceDirectory = join(generatedRoot, "adb");
  const archivePath = join(
    generatedRoot,
    "cache",
    `platform-tools_r${BUNDLED_ADB.version}-win.zip`,
  );
  const extractionDirectory = mkdtempSync(join(tmpdir(), "onmyoji-adb-"));

  try {
    await ensureArchive(archivePath);
    runChecked("tar", ["-xf", archivePath, "-C", extractionDirectory]);
    rmSync(resourceDirectory, { recursive: true, force: true });
    mkdirSync(resourceDirectory, { recursive: true });

    const packageDirectory = join(extractionDirectory, "platform-tools");
    for (const file of BUNDLED_ADB.files) {
      copyFileSync(join(packageDirectory, file.name), join(resourceDirectory, file.name));
    }

    assertBundledAdbResources(resourceDirectory);
    writeFileSync(
      join(resourceDirectory, "PROVENANCE.json"),
      `${JSON.stringify(
        {
          component: "Android Debug Bridge",
          license: "Apache-2.0 with bundled third-party notices",
          ...BUNDLED_ADB,
        },
        null,
        2,
      )}\n`,
    );
  } finally {
    rmSync(extractionDirectory, { recursive: true, force: true });
  }

  return resourceDirectory;
}

const isMainModule = process.argv[1]
  ? fileURLToPath(import.meta.url) === resolve(process.argv[1])
  : false;

if (isMainModule) {
  prepareBundledAdb(defaultRepositoryRoot)
    .then((resourceDirectory) => {
      process.stdout.write(
        `Prepared ADB ${BUNDLED_ADB.version} resources: ${resourceDirectory}\n`,
      );
    })
    .catch((error) => {
      const message = error instanceof Error ? error.message : String(error);
      process.stderr.write(`Bundled ADB preparation failed: ${message}\n`);
      process.exitCode = 1;
    });
}
