// Builds the Signal bridge and puts it where Tauri bundles it from:
// src-tauri/binaries/omega-zap-signal-<target triple>. Pass --release for a
// release build.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const release = process.argv.includes("--release");
const crate = join(root, "signal-sidecar");

const triple =
  process.env.TAURI_ENV_TARGET_TRIPLE ??
  /^host: (.+)$/m.exec(execFileSync("rustc", ["-vV"], { encoding: "utf8" }))[1];
const exe = triple.includes("windows") ? ".exe" : "";

execFileSync(
  "cargo",
  ["build", "--target", triple, ...(release ? ["--release"] : [])],
  { cwd: crate, stdio: "inherit" },
);

const out = join(root, "src-tauri", "binaries");
mkdirSync(out, { recursive: true });
copyFileSync(
  join(crate, "target", triple, release ? "release" : "debug", `omega-zap-signal${exe}`),
  join(out, `omega-zap-signal-${triple}${exe}`),
);
