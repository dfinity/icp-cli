/**
 * Astro integration for Agent-Friendly Documentation.
 * Implements https://agentdocsspec.com:
 *
 * 1. Markdown endpoints — serves a clean .md file alongside every HTML page
 * 2. llms.txt — discovery index listing all pages with links to .md endpoints
 *
 * Runs in the astro:build:done hook so it operates on the final build output.
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import matter from "gray-matter";

// Files excluded from the docs site (must match content.config.ts glob exclusions).
const EXCLUDED = ["schemas/**", "**/README.md", "VERSIONED_DOCS.md"];

function isExcluded(file) {
  if (file === "VERSIONED_DOCS.md" || file.endsWith("/README.md") || file === "README.md") {
    return true;
  }
  if (file.startsWith("schemas/")) return true;
  return false;
}

/**
 * Sidebar section definitions — mirrors the sidebar in astro.config.mjs.
 * Each entry maps a directory prefix to a section label for llms.txt grouping.
 * Order matters: entries are checked in order, longest prefix match wins.
 */
const SECTIONS = [
  { dir: "guides", label: "Guides" },
  { dir: "concepts", label: "Concepts" },
  { dir: "reference", label: "Reference" },
  { dir: "migration", label: "Other" },
];

// UTF-8 BOM — ensures browsers interpret .md files correctly even without
// a charset=utf-8 in the Content-Type header.
const BOM = "\uFEFF";

/** Strip YAML frontmatter and HTML comments, prepend title heading. */
function cleanMarkdown(raw) {
  const { data, content } = matter(raw);
  const body = content.replace(/<!--[\s\S]*?-->/g, "").trim();
  const title = data.title ? `# ${data.title}\n\n` : "";
  return BOM + title + body + "\n";
}

/** Find the best matching section for a file path (longest prefix wins). */
function findSection(filePath) {
  let best = null;
  for (const section of SECTIONS) {
    if (
      filePath.startsWith(section.dir + "/") &&
      (!best || section.dir.length > best.dir.length)
    ) {
      best = section;
    }
  }
  return best;
}

/** Generate llms.txt content from collected page metadata. */
function generateLlmsTxt(pages, siteUrl, basePath) {
  const base = (siteUrl + basePath).replace(/\/$/, "");

  // Extract version from base path (e.g., "/0.2/" → "0.2", "/main/" → "main")
  const version = basePath.replace(/^\/|\/$/g, "") || null;
  const title = version
    ? `# ICP CLI Documentation (v${version})`
    : "# ICP CLI Documentation";

  const lines = [
    title,
    "",
    "> Command-line tool for developing and deploying applications on the Internet Computer Protocol (ICP).",
    "",
    "Agent skills for IC development: https://skills.internetcomputer.org/.well-known/skills/index.json",
    "",
  ];

  // Root index page
  const rootIndex = pages.find((p) => p.file === "index.md");
  if (rootIndex) {
    lines.push(
      `- [${rootIndex.title}](${base}/index.md): ${rootIndex.description}`
    );
    lines.push("");
  }

  // Ungrouped top-level pages (not index, not in any section directory)
  const topLevel = pages.filter(
    (p) => p.file !== "index.md" && !findSection(p.file)
  );
  if (topLevel.length > 0) {
    lines.push("## Start Here");
    lines.push("");
    for (const page of topLevel) {
      const url = `${base}/${page.file}`;
      const entry = page.description
        ? `- [${page.title}](${url}): ${page.description}`
        : `- [${page.title}](${url})`;
      lines.push(entry);
    }
    lines.push("");
  }

  // Group pages by section
  const grouped = new Map();
  for (const section of SECTIONS) {
    grouped.set(section.label, []);
  }

  for (const page of pages) {
    if (page.file === "index.md") continue;
    const section = findSection(page.file);
    if (section) {
      grouped.get(section.label).push(page);
    }
  }

  // Emit sections
  for (const [label, sectionPages] of grouped) {
    if (sectionPages.length === 0) continue;

    sectionPages.sort((a, b) => a.order - b.order);

    lines.push(`## ${label}`);
    lines.push("");
    for (const page of sectionPages) {
      const url = `${base}/${page.file}`;
      const entry = page.description
        ? `- [${page.title}](${url}): ${page.description}`
        : `- [${page.title}](${url})`;
      lines.push(entry);
    }
    lines.push("");
  }

  return lines.join("\n");
}

export default function agentDocs() {
  let siteUrl = "";
  let basePath = "/";

  return {
    name: "agent-docs",
    hooks: {
      "astro:config:done": ({ config }) => {
        siteUrl = (config.site || "").replace(/\/$/, "");
        basePath = config.base || "/";
        if (!basePath.endsWith("/")) basePath += "/";
      },
      "astro:build:done": async ({ dir, logger }) => {
        const outDir = fileURLToPath(dir);
        // Docs source lives at ../docs relative to the docs-site directory.
        const docsDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../docs");

        const files = fs
          .globSync("**/*.md", { cwd: docsDir })
          .filter((f) => !isExcluded(f));
        const pages = [];

        // 1. Generate markdown endpoints
        for (const file of files) {
          if (isExcluded(file)) continue;

          const raw = fs.readFileSync(path.join(docsDir, file), "utf-8");
          const { data: frontmatter } = matter(raw);

          // Write cleaned .md to build output
          const outFile = path.join(outDir, file);
          fs.mkdirSync(path.dirname(outFile), { recursive: true });
          fs.writeFileSync(outFile, cleanMarkdown(raw));

          pages.push({
            file,
            title: frontmatter.title || path.basename(file, ".md"),
            description: frontmatter.description || "",
            order: frontmatter.sidebar?.order ?? 999,
          });
        }

        logger.info(`Generated ${pages.length} markdown endpoints`);

        // 2. Generate llms.txt
        const llmsTxt = generateLlmsTxt(pages, siteUrl, basePath);
        fs.writeFileSync(path.join(outDir, "llms.txt"), llmsTxt);
        logger.info(
          `Generated llms.txt (${llmsTxt.length} chars, ${pages.length} pages)`
        );
      },
    },
  };
}
