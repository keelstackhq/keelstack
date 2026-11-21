#!/usr/bin/env node
"use strict";

// npm launcher for keel: runs the binary from the platform package npm installed through
// optionalDependencies, with inherited stdio and the binary's exit code. Launcher environment
// errors (unsupported platform, missing or unrunnable binary) exit 3, keel's environment code.

const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const ENV_ERROR = 3;

/** `process.platform-process.arch` -> platform package (one per release target). */
const PLATFORMS = {
  "linux-x64": "@keelstackhq/keel-linux-x64",
  "linux-arm64": "@keelstackhq/keel-linux-arm64",
  "darwin-x64": "@keelstackhq/keel-darwin-x64",
  "darwin-arm64": "@keelstackhq/keel-darwin-arm64",
  "win32-x64": "@keelstackhq/keel-win32-x64",
};

function packageFor(platform, arch) {
  return PLATFORMS[`${platform}-${arch}`];
}

/** Path of `bin/keel[.exe]` inside the platform package; throws when it is not installed. */
function binaryPath(platform, arch, resolve = require.resolve) {
  const pkg = packageFor(platform, arch);
  const dir = path.dirname(resolve(`${pkg}/package.json`));
  return path.join(dir, "bin", platform === "win32" ? "keel.exe" : "keel");
}

/** True when `--json` appears before a `--` terminator. */
function wantsJson(args) {
  const end = args.indexOf("--");
  return (end < 0 ? args : args.slice(0, end)).includes("--json");
}

/** The subcommand word, skipping flags and the value of `-C` ("" when there is none). */
function commandName(args) {
  for (let i = 0; i < args.length; i++) {
    if (args[i] === "-C") i++;
    else if (args[i] === "--") break;
    else if (!args[i].startsWith("-")) return ["up", "plan", "doctor"].includes(args[i]) ? args[i] : "";
  }
  return "";
}

/** Report a launcher error: stderr text, plus keel's one-object JSON envelope with --json. */
function fail(args, message, fix) {
  process.stderr.write(`keel: ${message}\n`);
  if (wantsJson(args)) {
    process.stdout.write(
      JSON.stringify({
        version: 1,
        command: commandName(args),
        ok: false,
        exit_code: ENV_ERROR,
        status: "error",
        steps: [],
        human_step: null,
        error: { message, fix },
      }) + "\n",
    );
  }
  return ENV_ERROR;
}

/** Run keel with `args`; returns the exit code. */
function run(args, resolve = require.resolve) {
  const { platform, arch } = process;
  const key = `${platform}-${arch}`;
  const pkg = packageFor(platform, arch);
  if (!pkg) {
    return fail(
      args,
      `no binary for ${key}; supported platforms: ${Object.keys(PLATFORMS).join(", ")}`,
      "run keel on a supported platform",
    );
  }
  let bin;
  try {
    bin = binaryPath(platform, arch, resolve);
  } catch {
    return fail(
      args,
      `platform package ${pkg} is not installed; reinstall @keelstackhq/keel ` +
        "without --no-optional (or --omit=optional)",
      "npm install @keelstackhq/keel",
    );
  }
  let result = spawnSync(bin, args, { stdio: "inherit" });
  if (result.error && result.error.code === "EACCES") {
    // Some installers drop the executable bit; restore it once and retry.
    try {
      fs.chmodSync(bin, 0o755);
      result = spawnSync(bin, args, { stdio: "inherit" });
    } catch {
      // Keep the first error.
    }
  }
  if (result.error) {
    return fail(
      args,
      `cannot run ${bin}: ${result.error.message}`,
      "reinstall @keelstackhq/keel",
    );
  }
  if (result.signal) {
    process.kill(process.pid, result.signal);
    return 1;
  }
  return result.status ?? 1;
}

module.exports = { PLATFORMS, packageFor, binaryPath, run, wantsJson, commandName };

if (require.main === module) {
  process.exitCode = run(process.argv.slice(2));
}
