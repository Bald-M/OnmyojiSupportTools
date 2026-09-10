import assert from "node:assert/strict";
import test from "node:test";

import { createBuildPlan } from "./build-windows.mjs";

test("uses cargo-xwin for Windows x64 builds started on macOS", () => {
  const plan = createBuildPlan({ architecture: "x64", hostPlatform: "darwin" });

  assert.equal(plan.target, "x86_64-pc-windows-msvc");
  assert.equal(plan.runner, "cargo-xwin");
  assert.equal(plan.strategy, "cross-compile");
  assert.equal(plan.producesInstaller, false);
});

test("maps Windows ARM64 builds to the ARM64 MSVC target on macOS", () => {
  const plan = createBuildPlan({ architecture: "arm64", hostPlatform: "darwin" });

  assert.equal(plan.target, "aarch64-pc-windows-msvc");
  assert.equal(plan.expectedMachine, 0xaa64);
  assert.equal(plan.runner, "cargo-xwin");
});

test("keeps native Windows builds on the PowerShell packager", () => {
  const plan = createBuildPlan({ architecture: "x64", hostPlatform: "win32" });

  assert.equal(plan.strategy, "native");
  assert.equal(plan.runner, null);
  assert.equal(plan.producesInstaller, true);
});

test("rejects unsupported architectures before spawning a build", () => {
  assert.throws(
    () => createBuildPlan({ architecture: "riscv64", hostPlatform: "darwin" }),
    /Unsupported Windows architecture/,
  );
});
