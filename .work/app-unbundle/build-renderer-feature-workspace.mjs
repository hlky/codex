import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

function parseArgs(argv) {
  let unbundled = null;
  let pkg = null;
  let out = null;

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--unbundled") {
      unbundled = argv[i + 1] ?? null;
      i += 1;
    } else if (arg === "--package") {
      pkg = argv[i + 1] ?? null;
      i += 1;
    } else if (arg === "--out") {
      out = argv[i + 1] ?? null;
      i += 1;
    }
  }

  if (!unbundled || !pkg || !out) {
    throw new Error(
      "Usage: node build-renderer-feature-workspace.mjs --unbundled <dir> --package <dir> --out <dir>",
    );
  }

  return {
    unbundled: path.resolve(unbundled),
    packageRoot: path.resolve(pkg),
    out: path.resolve(out),
  };
}

async function ensureDir(dir) {
  await fs.mkdir(dir, { recursive: true });
}

async function readJson(file) {
  return JSON.parse(await fs.readFile(file, "utf8"));
}

function splitCamelCase(text) {
  return text.replace(/([a-z0-9])([A-Z])/g, "$1-$2");
}

function stripLastHash(stem) {
  return stem.replace(/-[A-Za-z0-9_]{6,}$/u, "");
}

function tokenizeStem(stem) {
  return splitCamelCase(stem)
    .split(/[-_.]+/u)
    .map((token) => token.toLowerCase())
    .filter(Boolean);
}

function isSharedRuntimeStem(stem, importerCount) {
  return (
    importerCount >= 25 ||
    /^(react|react-dom|jsx-runtime|tslib|chunk|lib|dist|core|src|proxy|request|queryoptions|persisted|statsig|fuse|mime-types|marked|katex|pdf|v4|clsx|sumby|sortby|starthase|startcase)$/u.test(
      stem,
    ) ||
    /^chunk-/u.test(stem) ||
    /^lib-/u.test(stem) ||
    /^dist-/u.test(stem) ||
    /^core/u.test(stem) ||
    /^src-/u.test(stem)
  );
}

function isSharedUiStem(stem, importerCount) {
  return (
    importerCount >= 15 &&
    /(button|tooltip|popover|dropdown|dialog|badge|spinner|checkbox|toggle|banner|icon|chevron|arrow|modal|empty-state)/u.test(
      stem,
    )
  );
}

function isLikelyIconStem(stem, importerCount, size, importCount) {
  const tokens = tokenizeStem(stem);
  return (
    size <= 10000 &&
    importCount === 0 &&
    importerCount >= 1 &&
    tokens.length <= 3 &&
    !tokens.some((token) =>
      new Set([
        "app",
        "browser",
        "context",
        "diff",
        "file",
        "git",
        "host",
        "layout",
        "menu",
        "model",
        "page",
        "query",
        "route",
        "settings",
        "sidebar",
        "state",
        "window",
        "workspace",
      ]).has(token),
    ) &&
    /(^|-)(arrow|archive|badge|banner|branch|bug|building|check|chevron|circle|clock|cloud|code|comment|copy|document|download|drag|edit|expand|external-link|face|file|flask|folder|github-mark|globe|graduation-cap|history|image-square|info|keyboard|laptop|lightning|link|lock|log-out|macbook|minus|moon|more-menu-trigger|notebook|openai-blossom|pencil|phone|play|plus|search|settings|shield|speedometer|spinner|star|sun|terminal|three-dots|trash|warning|window|workspace-root-icon|x|x-circle)(-|$)/u.test(
      stem,
    )
  );
}

function isLikelyThemeStem(stem, size) {
  return (
    size <= 25000 &&
    /(^|-)(absolutely|aurora-x|ayu|catppuccin|dracula|everforest|gruvbox|kanagawa|laserwave|material|monokai|nord|one-dark|poimandres|rose-pine|snazzy|solarized|synthwave|tokyo-night|vercel|vesper|vitesse|xcode|dark|light)(-|$)/u.test(
      stem,
    )
  );
}

function isLikelyDiagramStem(stem) {
  return /(diagram|mindmap|timeline|quadrant|gantt|pie|treemap|venn|sankey|xychart|radar|architecture|class|block|c4|kanban|journey|requirement|gitgraph|flowchart|infodiagram|sequencediagram|statediagram|erdiagram|dagre|cose-bilkent)/iu.test(
    stem,
  );
}

function inferFeatureGroup(entry) {
  const stem = entry.strippedStem;
  const importerCount = entry.importerCount;
  const size = entry.size;
  const importCount = entry.imports.length + entry.dynamicImports.length;

  if (isSharedRuntimeStem(stem, importerCount)) {
    return "shared/runtime";
  }
  if (isSharedUiStem(stem, importerCount)) {
    return "shared/ui";
  }
  if (isLikelyIconStem(stem, importerCount, size, importCount)) {
    return "shared/icons";
  }
  if (isLikelyThemeStem(stem, size)) {
    return "shared/highlight-themes";
  }
  if (isLikelyDiagramStem(stem)) {
    return "features/artifacts-diagrams";
  }

  const rules = [
    ["features/app-server", /^app-server|^mcp($|-)|^process-manager-target$|^use-app-server-/u],
    ["features/app-core", /^app($|-)|^app-main$|^app-scope$|^app-intl|^app-preloader$|^app-prefetch|^use-app-/u],
    ["features/environment", /(^|-)environment(s)?(-|$)|^use-environment$|^worktree-environment/u],
    ["features/worktree", /(^|-)worktree(-|$)|pending-worktree/u],
    ["features/workspace", /^workspace|^open-workspace-file$|^file-tree|^local-active-workspace-root/u],
    ["features/browser", /^browser|^web-search|^browser-use/u],
    ["features/composer", /^composer|above-composer|^prompt-|mention|autocomplete|slash-command/u],
    ["features/thread", /^thread|conversation|^chat$|^chats$|message|side-panel|pinned-thread/u],
    ["features/git-review", /^git|^pull-request|^review|^diff|^github|^gh-|branch/u],
    ["features/settings", /^settings|settings-page|appearance-settings|font-settings|usage-settings|model-settings|agent-settings|hooks-settings/u],
    ["features/plugins-skills", /^plugin|^plugins|^skill|^skills|^app-connect|^apps($|-)|connected-apps|connector/u],
    ["features/automation", /^automation|^automations|heartbeat-automation/u],
    ["features/avatar-overlay", /^avatar|^codex-avatar/u],
    ["features/codex-product", /^codex($|-)|^get-codex-/u],
    ["features/home-onboarding", /^home|^homepage|^onboarding|conversation-starter|workspace-onboarding|referral/u],
    ["features/hotkey-window", /^hotkey-window|^use-hotkey-window/u],
    ["features/plan-management", /^plan($|-)|^is-plan-event-enabled$/u],
    ["features/artifacts-office", /^popcorn|document-panel|presentation-panel|workbook-panel/iu],
    ["features/artifacts", /^artifact|^markdown|^code-snippet|^image-preview|^pdf|^workbook|diagram|mermaid|shiki/u],
    ["features/shell-tools", /^terminal|^xterm|^workspace-file|^workspace-directory|^open-target|^open-file/u],
    ["features/models-controls", /^model|^models|^reasoning|^service-tier|^rate-limit|^permissions-mode|^personality|collaboration-mode/u],
    ["features/app-shell", /^app-shell|^window-|^loading-page$|^homepage-logo$|^initial-route/u],
    ["features/appgen-appshots", /^appgen|^appshot/u],
  ];

  for (const [group, pattern] of rules) {
    if (pattern.test(stem)) {
      return group;
    }
  }

  if (/^use-/u.test(stem)) {
    return "shared/hooks";
  }

  const tokens = tokenizeStem(stem);
  const firstMeaningful =
    tokens.find(
      (token) =>
        !new Set([
          "use",
          "local",
          "remote",
          "open",
          "build",
          "get",
          "set",
          "check",
          "with",
          "from",
        ]).has(token),
    ) ?? tokens[0] ?? "misc";

  return `features/misc-${firstMeaningful.replace(/[^a-z0-9]+/gu, "-")}`;
}

function makeFriendlyStem(fileName) {
  const ext = path.extname(fileName);
  const stem = path.basename(fileName, ext);
  return {
    ext,
    strippedStem: stripLastHash(stem),
  };
}

function packagePathForRendererPath(rendererPath) {
  if (rendererPath === "renderer/index.html") {
    return "webview/index.html";
  }
  return rendererPath.replace(/^renderer\//u, "webview/");
}

async function writeGroupReadmes(outRoot, groups) {
  for (const [groupName, entries] of groups.entries()) {
    const groupDir = path.join(outRoot, "renderer", groupName);
    const lines = [
      `# ${groupName}`,
      "",
      `Files in this inferred feature group: ${entries.length}`,
      "",
      ...entries.map(
        (entry) =>
          `- \`${path.basename(entry.aliasPath)}\` -> \`${entry.packagePath}\``,
      ),
      "",
    ];
    await fs.writeFile(path.join(groupDir, "README.md"), lines.join("\n"), "utf8");
  }
}

function buildWorkspaceReadme({ outRoot, packageRoot, unbundledRoot, groups }) {
  const sortedGroups = [...groups.entries()].sort((a, b) => b[1].length - a[1].length);
  return `# Codex Renderer Feature Workspace

Package source: \`${packageRoot}\`

Readable source base: \`${unbundledRoot}\`

Workspace root: \`${outRoot}\`

This workspace is a second-pass feature view over the renderer bundle.

How it works:

- Files under \`renderer/features/*\` and \`renderer/shared/*\` are editable copies with cleaner names.
- \`meta/file-map.json\` maps each editable file back to the real bundled file under \`${packageRoot}\`.
- After editing, run:
  - \`node ${path.join(__dirname, "sync-renderer-feature-workspace.mjs")} --workspace "${outRoot}"\`
- To repack into a usable \`app.asar\`, run:
  - \`node ${path.join(__dirname, "pack-renderer-feature-workspace.mjs")} --workspace "${outRoot}" --out "<path-to-app.asar>"\`

Notes:

- The import graph is not rewritten. The real bundle layout remains under the package root.
- The feature view is for editing and orientation; sync copies changes back into the canonical package paths before packing.

Top groups:

${sortedGroups
  .slice(0, 20)
  .map(([groupName, entries]) => `- \`${groupName}\`: ${entries.length} files`)
  .join("\n")}
`;
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const outRoot = args.out;
  const chunkGraphPath = path.join(args.unbundled, "meta", "chunk-graph.json");
  const chunkGraph = await readJson(chunkGraphPath);

  await fs.rm(outRoot, { force: true, recursive: true });
  await ensureDir(path.join(outRoot, "renderer"));
  await ensureDir(path.join(outRoot, "meta"));

  const rendererEntries = chunkGraph.filter((entry) => entry.path.startsWith("renderer/"));
  const reverseCounts = new Map();

  for (const entry of rendererEntries) {
    for (const imported of [...entry.imports, ...entry.dynamicImports]) {
      if (!imported.startsWith("./")) {
        continue;
      }
      const target = path.posix.normalize(
        path.posix.join(path.posix.dirname(entry.path), imported),
      );
      reverseCounts.set(target, (reverseCounts.get(target) ?? 0) + 1);
    }
  }

  const usedAliasPaths = new Set();
  const workspaceEntries = [];
  const groupMap = new Map();

  for (const entry of rendererEntries) {
    const sourcePath = path.join(args.unbundled, entry.path.replace(/\//gu, path.sep));
    const fileName = path.basename(entry.path);
    const { ext, strippedStem } = makeFriendlyStem(fileName);
    const importerCount = reverseCounts.get(entry.path) ?? 0;
    const groupedEntry = {
      strippedStem,
      importerCount,
      size: entry.size,
      imports: entry.imports,
      dynamicImports: entry.dynamicImports,
    };
    const groupName =
      entry.path === "renderer/index.html"
        ? "features/root"
        : inferFeatureGroup(groupedEntry);

    let aliasFileName = entry.path === "renderer/index.html" ? "index.html" : `${strippedStem}${ext}`;
    let aliasPath = path.posix.join("renderer", groupName, aliasFileName);

    if (usedAliasPaths.has(aliasPath)) {
      const originalStem = path.basename(fileName, ext);
      aliasFileName = `${strippedStem}~${originalStem.slice(strippedStem.length + 1)}${ext}`;
      aliasPath = path.posix.join("renderer", groupName, aliasFileName);
    }
    usedAliasPaths.add(aliasPath);

    const outPath = path.join(outRoot, aliasPath.replace(/\//gu, path.sep));
    await ensureDir(path.dirname(outPath));
    await fs.copyFile(sourcePath, outPath);

    const workspaceEntry = {
      groupName,
      aliasPath,
      unbundledPath: entry.path,
      packagePath: packagePathForRendererPath(entry.path),
      originalFileName: fileName,
      friendlyName: aliasFileName,
      importerCount,
      imports: entry.imports,
      dynamicImports: entry.dynamicImports,
      sourceMap: entry.sourceMap,
      size: entry.size,
    };
    workspaceEntries.push(workspaceEntry);

    const list = groupMap.get(groupName) ?? [];
    list.push(workspaceEntry);
    groupMap.set(groupName, list);
  }

  const aliasByOriginal = new Map(workspaceEntries.map((entry) => [entry.unbundledPath, entry.aliasPath]));
  const enrichedEntries = workspaceEntries.map((entry) => {
    const importedAliases = [...entry.imports, ...entry.dynamicImports]
      .filter((imported) => imported.startsWith("./"))
      .map((imported) => {
        const target = path.posix.normalize(
          path.posix.join(path.posix.dirname(entry.unbundledPath), imported),
        );
        return {
          importPath: imported,
          targetUnbundledPath: target,
          targetAliasPath: aliasByOriginal.get(target) ?? null,
        };
      });

    return {
      ...entry,
      importedAliases,
    };
  });

  await fs.writeFile(
    path.join(outRoot, "meta", "workspace.json"),
    JSON.stringify(
      {
        packageRoot: args.packageRoot,
        unbundledRoot: args.unbundled,
        createdAt: new Date().toISOString(),
      },
      null,
      2,
    ),
    "utf8",
  );
  await fs.writeFile(
    path.join(outRoot, "meta", "file-map.json"),
    JSON.stringify(enrichedEntries, null, 2),
    "utf8",
  );

  await writeGroupReadmes(outRoot, groupMap);
  await fs.writeFile(
    path.join(outRoot, "README.md"),
    buildWorkspaceReadme({
      outRoot,
      packageRoot: args.packageRoot,
      unbundledRoot: args.unbundled,
      groups: groupMap,
    }),
    "utf8",
  );
}

main().catch((error) => {
  console.error(error instanceof Error ? error.stack : String(error));
  process.exitCode = 1;
});
