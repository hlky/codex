import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

function parseArgs(argv) {
  let source = null;
  let out = null;
  let prettify = false;

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--source") {
      source = argv[i + 1] ?? null;
      i += 1;
    } else if (arg === "--out") {
      out = argv[i + 1] ?? null;
      i += 1;
    } else if (arg === "--prettify") {
      prettify = true;
    }
  }

  if (!source || !out) {
    throw new Error(
      "Usage: node unbundle-codex-app.mjs --source <extracted-app-dir> --out <output-dir> [--prettify]",
    );
  }

  return {
    source: path.resolve(source),
    out: path.resolve(out),
    prettify,
  };
}

async function ensureDir(dir) {
  await fs.mkdir(dir, { recursive: true });
}

async function pathExists(target) {
  try {
    await fs.access(target);
    return true;
  } catch {
    return false;
  }
}

async function copyFileIfExists(from, to) {
  if (!(await pathExists(from))) {
    return false;
  }
  await ensureDir(path.dirname(to));
  await fs.copyFile(from, to);
  return true;
}

async function copyDir(from, to) {
  await ensureDir(path.dirname(to));
  await fs.cp(from, to, { recursive: true });
}

async function collectFiles(root, extensions) {
  const out = [];

  async function walk(dir) {
    const entries = await fs.readdir(dir, { withFileTypes: true });
    for (const entry of entries) {
      const fullPath = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        await walk(fullPath);
      } else if (extensions.has(path.extname(entry.name))) {
        out.push(fullPath);
      }
    }
  }

  if (await pathExists(root)) {
    await walk(root);
  }

  return out.sort();
}

function relativePosix(root, fullPath) {
  return path.relative(root, fullPath).split(path.sep).join("/");
}

function parseImports(sourceText) {
  const imports = new Set();
  const dynamicImports = new Set();
  const sourceMap = sourceText.match(/\/\/# sourceMappingURL=(.+)$/m)?.[1] ?? null;

  for (const match of sourceText.matchAll(
    /(?:import|export)\s+(?:[^"'`]*?\s+from\s+)?["'`]([^"'`]+)["'`]/g,
  )) {
    imports.add(match[1]);
  }

  for (const match of sourceText.matchAll(/import\(\s*["'`]([^"'`]+)["'`]\s*\)/g)) {
    dynamicImports.add(match[1]);
  }

  const mapDepsMatch = sourceText.match(
    /m\.f\|\|\(m\.f=\[(?<deps>[\s\S]*?)\]\)/,
  );

  if (mapDepsMatch?.groups?.deps) {
    const depMatcher = /["'`]([^"'`]+)["'`]/g;
    for (const match of mapDepsMatch.groups.deps.matchAll(depMatcher)) {
      dynamicImports.add(match[1]);
    }
  }

  return {
    imports: [...imports].sort(),
    dynamicImports: [...dynamicImports].sort(),
    sourceMap,
  };
}

function classifyChunk(relPath) {
  if (relPath.startsWith("renderer/assets/")) {
    return "renderer-chunk";
  }
  if (relPath === "renderer/index.html") {
    return "renderer-entry";
  }
  if (relPath.startsWith("electron/")) {
    return "electron-chunk";
  }
  if (relPath.startsWith("apps/")) {
    return "app-icon";
  }
  return "asset";
}

async function buildManifest(outDir) {
  const fileTypes = new Set([".js", ".mjs", ".cjs", ".css", ".html", ".json"]);
  const files = await collectFiles(outDir, fileTypes);
  const manifest = [];

  for (const file of files) {
    const relPath = relativePosix(outDir, file);
    const text = await fs.readFile(file, "utf8");
    const stats = await fs.stat(file);
    const parsed = parseImports(text);

    manifest.push({
      path: relPath,
      kind: classifyChunk(relPath),
      size: stats.size,
      imports: parsed.imports,
      dynamicImports: parsed.dynamicImports,
      sourceMap: parsed.sourceMap,
    });
  }

  return manifest;
}

function buildReadme({ sourceDir, outDir, manifest }) {
  const electronFiles = manifest.filter((item) => item.kind === "electron-chunk").length;
  const rendererChunks = manifest.filter((item) => item.kind === "renderer-chunk").length;
  const sourceMapRefs = manifest.filter((item) => item.sourceMap).length;

  return `# Codex App Best-Effort Unbundled

Source: \`${sourceDir}\`

Output: \`${outDir}\`

This is a best-effort pseudo-source tree reconstructed from the extracted \`app.asar\`.

What you get:

- \`electron/\`: Electron main/preload/worker chunks from \`.vite/build/\`
- \`renderer/\`: the webview app entrypoint and named Vite chunks from \`webview/\`
- \`apps/\`: app icon/image assets from \`webview/apps/\`
- \`meta/chunk-graph.json\`: import graph and sourcemap comment inventory
- \`meta/package.json\`: bundled app package manifest

Limits:

- This is not original TypeScript/TSX source.
- Original module boundaries inside each emitted chunk are not recoverable without source maps.
- Identifier names, inlined helpers, and tree-shaken structure are whatever survived production bundling.
- Many files still contain \`sourceMappingURL\` comments, but the actual \`.map\` files are not shipped.

Inventory:

- Electron chunks: ${electronFiles}
- Renderer chunks: ${rendererChunks}
- Files with sourcemap comments: ${sourceMapRefs}
`;
}

async function runPrettier(outDir) {
  const patterns = [
    path.join(outDir, "electron", "**", "*.{js,mjs,cjs,json}"),
    path.join(outDir, "renderer", "**", "*.{js,mjs,cjs,css,html,json}"),
    path.join(outDir, "meta", "*.{json,md}"),
  ];

  await new Promise((resolve, reject) => {
    const child = spawn(
      "npx",
      ["--yes", "prettier", "--write", ...patterns],
      {
        cwd: __dirname,
        stdio: "inherit",
        shell: true,
      },
    );

    child.on("exit", (code) => {
      if (code === 0) {
        resolve();
      } else {
        reject(new Error(`prettier exited with code ${code}`));
      }
    });
    child.on("error", reject);
  });
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const outDir = args.out;
  const sourceDir = args.source;

  const sourcePackageJson = path.join(sourceDir, "package.json");
  const sourceElectronDir = path.join(sourceDir, ".vite", "build");
  const sourceRendererDir = path.join(sourceDir, "webview");
  const sourceAppsDir = path.join(sourceRendererDir, "apps");

  await fs.rm(outDir, { force: true, recursive: true });
  await ensureDir(outDir);

  await copyFileIfExists(sourcePackageJson, path.join(outDir, "meta", "package.json"));
  await copyDir(sourceElectronDir, path.join(outDir, "electron"));

  await copyFileIfExists(
    path.join(sourceRendererDir, "index.html"),
    path.join(outDir, "renderer", "index.html"),
  );
  await copyDir(path.join(sourceRendererDir, "assets"), path.join(outDir, "renderer", "assets"));

  if (await pathExists(sourceAppsDir)) {
    await copyDir(sourceAppsDir, path.join(outDir, "apps"));
  }

  const manifest = await buildManifest(outDir);
  await ensureDir(path.join(outDir, "meta"));
  await fs.writeFile(
    path.join(outDir, "meta", "chunk-graph.json"),
    JSON.stringify(manifest, null, 2),
    "utf8",
  );
  await fs.writeFile(
    path.join(outDir, "README.md"),
    buildReadme({ sourceDir, outDir, manifest }),
    "utf8",
  );

  if (args.prettify) {
    await runPrettier(outDir);
  }
}

main().catch((error) => {
  console.error(error instanceof Error ? error.stack : String(error));
  process.exitCode = 1;
});
