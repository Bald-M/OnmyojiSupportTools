import { Buffer } from "node:buffer";
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
} from "node:fs";
import { delimiter, dirname, join, resolve } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const ARCHITECTURES = Object.freeze({
  x64: Object.freeze({
    target: "x86_64-pc-windows-msvc",
    expectedMachine: 0x8664,
  }),
  arm64: Object.freeze({
    target: "aarch64-pc-windows-msvc",
    expectedMachine: 0xaa64,
  }),
});

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

export function createBuildPlan({ architecture, hostPlatform = process.platform }) {
  const targetConfig = ARCHITECTURES[architecture];
  if (!targetConfig) {
    throw new Error(
      `Unsupported Windows architecture "${architecture}". Expected x64 or arm64.`,
    );
  }

  if (hostPlatform === "win32") {
    return {
      architecture,
      ...targetConfig,
      hostPlatform,
      producesInstaller: true,
      runner: null,
      strategy: "native",
    };
  }

  if (hostPlatform === "darwin") {
    return {
      architecture,
      ...targetConfig,
      hostPlatform,
      producesInstaller: false,
      runner: "cargo-xwin",
      strategy: "cross-compile",
    };
  }

  throw new Error(
    `Windows bundles cannot be built from host platform "${hostPlatform}". Use Windows, macOS, or GitHub Actions.`,
  );
}

function parseArguments(arguments_) {
  let architecture;
  let dryRun = false;

  for (let index = 0; index < arguments_.length; index += 1) {
    const argument = arguments_[index];
    if (argument === "--architecture") {
      architecture = arguments_[index + 1];
      index += 1;
    } else if (argument === "--dry-run") {
      dryRun = true;
    } else {
      throw new Error(`Unknown build argument "${argument}".`);
    }
  }

  if (!architecture) {
    throw new Error("Missing --architecture. Expected x64 or arm64.");
  }

  return { architecture, dryRun };
}

function runChecked(program, arguments_, options = {}) {
  const result = spawnSync(program, arguments_, {
    cwd: repositoryRoot,
    env: options.env ?? process.env,
    stdio: "inherit",
  });

  if (result.error) {
    throw new Error(`Unable to start ${program}: ${result.error.message}`);
  }
  if (result.status !== 0) {
    throw new Error(
      `Command failed with exit code ${result.status}: ${program} ${arguments_.join(" ")}`,
    );
  }
}

function commandSucceeds(program, arguments_, env) {
  const result = spawnSync(program, arguments_, {
    cwd: repositoryRoot,
    env,
    stdio: "ignore",
  });
  return result.status === 0;
}

function findLlvmBin(env) {
  const candidates = (env.PATH ?? "").split(delimiter).filter(Boolean);
  candidates.push("/opt/homebrew/opt/llvm/bin", "/usr/local/opt/llvm/bin");

  const brewResult = spawnSync("brew", ["--prefix", "llvm"], {
    cwd: repositoryRoot,
    env,
    encoding: "utf8",
  });
  if (brewResult.status === 0) {
    candidates.push(join(brewResult.stdout.trim(), "bin"));
  }

  return candidates.find((candidate) => existsSync(join(candidate, "llvm-rc")));
}

function prepareMacEnvironment() {
  const llvmBin = findLlvmBin(process.env);
  const missing = [];

  if (!commandSucceeds("cargo-xwin", ["--version"], process.env)) {
    missing.push("cargo-xwin (`cargo install --locked cargo-xwin`)");
  }
  if (!llvmBin) {
    missing.push("LLVM (`brew install llvm`)");
  }
  if (missing.length > 0) {
    throw new Error(`Missing macOS Windows-build dependencies: ${missing.join(", ")}.`);
  }

  return {
    ...process.env,
    PATH: `${llvmBin}${delimiter}${process.env.PATH ?? ""}`,
  };
}

function assertPeArchitecture(path, expectedMachine) {
  const bytes = readFileSync(path);
  if (bytes.length < 64 || bytes[0] !== 0x4d || bytes[1] !== 0x5a) {
    throw new Error(`Built executable has an invalid DOS header: ${path}`);
  }

  const peOffset = bytes.readInt32LE(0x3c);
  if (peOffset < 0 || peOffset + 6 > bytes.length) {
    throw new Error(`Built executable has an invalid PE header offset: ${path}`);
  }
  if (!bytes.subarray(peOffset, peOffset + 4).equals(Buffer.from("PE\0\0"))) {
    throw new Error(`Built executable has an invalid PE signature: ${path}`);
  }

  const actualMachine = bytes.readUInt16LE(peOffset + 4);
  if (actualMachine !== expectedMachine) {
    throw new Error(
      `Expected PE machine 0x${expectedMachine.toString(16)}, found 0x${actualMachine.toString(16)}: ${path}`,
    );
  }
}

function packageMacPortableArtifact(plan, version, env) {
  const targetRoot = join(
    repositoryRoot,
    "src-tauri",
    "target",
    plan.target,
    "release",
  );
  const binaryPath = join(targetRoot, "onmyoji-support-tools.exe");
  const artifactDirectory = join(
    repositoryRoot,
    "artifacts",
    `windows-${plan.architecture}`,
  );
  const artifactStem = `OnmyojiSupportTools_${version}_windows_${plan.architecture}`;
  const portableExecutablePath = join(artifactDirectory, `${artifactStem}_portable.exe`);
  const portableZipPath = join(artifactDirectory, `${artifactStem}_portable.zip`);

  if (!existsSync(binaryPath)) {
    throw new Error(`Tauri did not produce the expected executable: ${binaryPath}`);
  }
  assertPeArchitecture(binaryPath, plan.expectedMachine);

  mkdirSync(artifactDirectory, { recursive: true });
  copyFileSync(binaryPath, portableExecutablePath);
  rmSync(portableZipPath, { force: true });
  try {
    runChecked("/usr/bin/zip", ["-j", "-q", portableZipPath, portableExecutablePath], {
      env,
    });
  } finally {
    rmSync(portableExecutablePath, { force: true });
  }

  process.stdout.write(
    `Verified platform=windows architecture=${plan.architecture} target=${plan.target}\n`,
  );
  process.stdout.write("NSIS: skipped on macOS; use a native Windows build or CI\n");
  process.stdout.write(`Portable ZIP: ${portableZipPath}\n`);
}

function buildOnMac(plan) {
  const packageJson = JSON.parse(
    readFileSync(join(repositoryRoot, "package.json"), "utf8"),
  );
  const env = prepareMacEnvironment();

  process.stdout.write(
    `Building platform=windows architecture=${plan.architecture} target=${plan.target} version=${packageJson.version} host=macOS runner=${plan.runner}\n`,
  );
  runChecked("rustup", ["target", "add", plan.target], { env });
  runChecked(
    "pnpm",
    [
      "tauri",
      "build",
      "--runner",
      plan.runner,
      "--target",
      plan.target,
      "--no-bundle",
      "--ci",
    ],
    { env },
  );
  packageMacPortableArtifact(plan, packageJson.version, env);
}

function buildOnWindows(plan) {
  runChecked("powershell", [
    "-NoProfile",
    "-ExecutionPolicy",
    "Bypass",
    "-File",
    join(repositoryRoot, "scripts", "build-windows.ps1"),
    "-Architecture",
    plan.architecture,
  ]);
}

function main() {
  const { architecture, dryRun } = parseArguments(process.argv.slice(2));
  const plan = createBuildPlan({ architecture });

  if (dryRun) {
    process.stdout.write(`${JSON.stringify(plan)}\n`);
    return;
  }

  if (plan.strategy === "native") {
    buildOnWindows(plan);
  } else {
    buildOnMac(plan);
  }
}

const isMainModule = process.argv[1]
  ? fileURLToPath(import.meta.url) === resolve(process.argv[1])
  : false;

if (isMainModule) {
  try {
    main();
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    process.stderr.write(`Windows build failed: ${message}\n`);
    process.exitCode = 1;
  }
}
