import path from "node:path";
import { spawn } from "node:child_process";
import fs from "node:fs/promises";
import { fileURLToPath } from "node:url";

function parseArgs(argv) {
  let workspace = null;
  let out = null;

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--workspace") {
      workspace = argv[i + 1] ?? null;
      i += 1;
    } else if (arg === "--out") {
      out = argv[i + 1] ?? null;
      i += 1;
    }
  }

  if (!workspace || !out) {
    throw new Error(
      "Usage: node pack-renderer-feature-workspace.mjs --workspace <workspace-dir> --out <app.asar>",
    );
  }

  return {
    workspace: path.resolve(workspace),
    out: path.resolve(out),
  };
}

async function readJson(file) {
  return JSON.parse(await fs.readFile(file, "utf8"));
}

function run(command, args, cwd) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd,
      stdio: "inherit",
      shell: true,
    });
    child.on("exit", (code) => {
      if (code === 0) {
        resolve();
      } else {
        reject(new Error(`${command} exited with code ${code}`));
      }
    });
    child.on("error", reject);
  });
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const scriptDir = path.dirname(fileURLToPath(import.meta.url));
  const workspaceMeta = await readJson(path.join(args.workspace, "meta", "workspace.json"));

  await run(
    "node",
    [path.join(scriptDir, "sync-renderer-feature-workspace.mjs"), "--workspace", args.workspace],
    scriptDir,
  );
  await run("npx", ["--yes", "asar", "pack", workspaceMeta.packageRoot, args.out], scriptDir);
}

main().catch((error) => {
  console.error(error instanceof Error ? error.stack : String(error));
  process.exitCode = 1;
});
