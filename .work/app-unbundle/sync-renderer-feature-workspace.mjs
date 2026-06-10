import fs from "node:fs/promises";
import path from "node:path";

function parseArgs(argv) {
  let workspace = null;

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--workspace") {
      workspace = argv[i + 1] ?? null;
      i += 1;
    }
  }

  if (!workspace) {
    throw new Error(
      "Usage: node sync-renderer-feature-workspace.mjs --workspace <workspace-dir>",
    );
  }

  return {
    workspace: path.resolve(workspace),
  };
}

async function ensureDir(dir) {
  await fs.mkdir(dir, { recursive: true });
}

async function readJson(file) {
  return JSON.parse(await fs.readFile(file, "utf8"));
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const workspaceMeta = await readJson(path.join(args.workspace, "meta", "workspace.json"));
  const fileMap = await readJson(path.join(args.workspace, "meta", "file-map.json"));

  for (const entry of fileMap) {
    const from = path.join(args.workspace, entry.aliasPath.replace(/\//gu, path.sep));
    const to = path.join(workspaceMeta.packageRoot, entry.packagePath.replace(/\//gu, path.sep));
    await ensureDir(path.dirname(to));
    await fs.copyFile(from, to);
  }

  console.log(`Synced ${fileMap.length} renderer files into ${workspaceMeta.packageRoot}`);
}

main().catch((error) => {
  console.error(error instanceof Error ? error.stack : String(error));
  process.exitCode = 1;
});
