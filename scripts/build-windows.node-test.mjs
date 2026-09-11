import assert from "node:assert/strict";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  assertBundledAdbResources,
  BUNDLED_ADB,
} from "./prepare-bundled-adb.mjs";
import {
  createBuildPlan,
  installerFileName,
  replaceDistributionDirectory,
} from "./build-windows.mjs";

test("pins an auditable open-source ADB package for Windows installers", () => {
  assert.equal(BUNDLED_ADB.version, "37.0.1");
  assert.equal(
    BUNDLED_ADB.archiveSha256,
    "45f4d63113e895ebde0c90f194099a4676b6ac653bd28d54314a9e022bbc1a99",
  );
  assert.equal(
    BUNDLED_ADB.sourceUrl,
    "https://android.googlesource.com/platform/packages/modules/adb/",
  );
  assert.deepEqual(
    BUNDLED_ADB.files.map(({ name }) => name),
    ["adb.exe", "AdbWinApi.dll", "AdbWinUsbApi.dll", "NOTICE.txt", "source.properties"],
  );
});

test("rejects a tampered bundled ADB resource before packaging", () => {
  const directory = mkdtempSync(join(tmpdir(), "onmyoji-adb-test-"));
  try {
    mkdirSync(directory, { recursive: true });
    for (const { name } of BUNDLED_ADB.files) {
      writeFileSync(join(directory, name), "tampered");
    }

    assert.throws(
      () => assertBundledAdbResources(directory),
      /Bundled ADB checksum mismatch for adb\.exe/,
    );
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("uses cargo-xwin for Windows x64 builds started on macOS", () => {
  const plan = createBuildPlan({ architecture: "x64", hostPlatform: "darwin" });

  assert.equal(plan.target, "x86_64-pc-windows-msvc");
  assert.equal(plan.runner, "cargo-xwin");
  assert.equal(plan.strategy, "cross-compile");
  assert.equal(plan.producesInstaller, true);
  assert.equal(plan.producesPortableArtifact, false);
  assert.equal(plan.bundledAdbVersion, "37.0.1");
});

test("keeps native Windows builds on the PowerShell packager", () => {
  const plan = createBuildPlan({ architecture: "x64", hostPlatform: "win32" });

  assert.equal(plan.strategy, "native");
  assert.equal(plan.runner, null);
  assert.equal(plan.producesInstaller, true);
  assert.equal(plan.producesPortableArtifact, false);
  assert.equal(plan.bundledAdbVersion, "37.0.1");
});

test("uses a single versioned x64 installer filename", () => {
  assert.equal(
    installerFileName("0.1.0"),
    "OnmyojiSupportTools_0.1.0_windows_x64_nsis-setup.exe",
  );
});

test("replaces dist contents with only the installer", () => {
  const directory = mkdtempSync(join(tmpdir(), "onmyoji-dist-test-"));
  try {
    const source = join(directory, "generated-installer.exe");
    const outputDirectory = join(directory, "dist");
    const destination = join(outputDirectory, installerFileName("0.1.0"));
    mkdirSync(outputDirectory);
    writeFileSync(source, "installer");
    writeFileSync(join(outputDirectory, "stale-portable.zip"), "stale");
    writeFileSync(join(outputDirectory, "stale-installer.exe"), "stale");

    replaceDistributionDirectory(source, destination, outputDirectory);

    assert.deepEqual(readdirSync(outputDirectory), [installerFileName("0.1.0")]);
    assert.equal(readFileSync(destination, "utf8"), "installer");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("rejects the removed Windows ARM64 build target", () => {
  assert.throws(
    () => createBuildPlan({ architecture: "arm64", hostPlatform: "darwin" }),
    /Unsupported Windows architecture/,
  );
});

test("rejects unsupported architectures before spawning a build", () => {
  assert.throws(
    () => createBuildPlan({ architecture: "riscv64", hostPlatform: "darwin" }),
    /Unsupported Windows architecture/,
  );
});
