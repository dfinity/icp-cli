/**
 * Astro integration for Agent-Friendly Documentation.
 * Implements https://agentdocsspec.com:
 *
 * 1. Markdown endpoints — serves a clean .md file alongside every HTML page
 * 2. llms.txt — discovery index listing all pages with links to .md endpoints
 * 3. Agent signaling — injects a hidden llms.txt directive right after <body>
 *    in every HTML page so agents discover it early (before nav/sidebar)
 *
 * Runs in the astro:build:done hook so it operates on the final build output.
 */

import fs from "node:fs";
import path from "node:path";
import { execSync } from "node:child_process";
import { Resvg } from "@resvg/resvg-js";
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

// Path to the CLI reference page — split into per-command endpoints for agents.
const CLI_REFERENCE = "reference/cli.md";

/**
 * Split the CLI reference into per-command markdown files.
 * Each `## \`icp ...\`` heading becomes its own file under reference/cli/.
 * Returns metadata for each generated sub-page (for llms.txt).
 */
function splitCliReference(outDir) {
  const cliMd = path.join(outDir, CLI_REFERENCE);
  if (!fs.existsSync(cliMd)) return [];

  const content = fs
    .readFileSync(cliMd, "utf-8")
    // Strip the clap-markdown generation footer that appears at the end.
    .replace(/\n*<hr\/>\s*\n*<small>[\s\S]*?<\/small>\s*$/, "\n");
  // Split on ## `icp ...` headings, keeping the heading with the section.
  const sections = content.split(/^(?=## `icp\b)/m).filter((s) => s.trim());

  const subDir = path.join(outDir, "reference", "cli");
  fs.mkdirSync(subDir, { recursive: true });

  const subPages = [];
  const seenSlugs = new Map(); // slug → command name, for collision detection
  for (const section of sections) {
    const match = section.match(/^## `(icp[\w\s-]*?)`/);
    if (!match) continue;

    const command = match[1].trim();
    // icp build → build, icp canister call → canister-call
    const slug = command === "icp" ? "index" : command.replace(/^icp /, "").replace(/ /g, "-");
    const fileName = `${slug}.md`;

    // Detect slug collisions (e.g., "icp foo-bar" vs "icp foo bar").
    if (seenSlugs.has(slug)) {
      throw new Error(
        `CLI reference split: slug collision for "${fileName}" ` +
        `between commands "${seenSlugs.get(slug)}" and "${command}"`
      );
    }
    seenSlugs.set(slug, command);

    // Extract the description: first plain-text line after the heading,
    // skipping **Usage:**, ###### headings, list items, and empty lines.
    const lines = section.split("\n");
    const descLine = lines.find(
      (l, i) =>
        i > 0 &&
        l.trim() &&
        !l.startsWith("**Usage") &&
        !l.startsWith("#") &&
        !l.startsWith("*")
    );
    const description = descLine ? descLine.trim() : "";

    // Rewrite subcommand list items to link to their per-command endpoints.
    // e.g., `* \`call\` — ...` → `* [\`call\`](canister-call.md) — ...`
    // The parent prefix (e.g., "canister") is used to build the slug.
    const parentSlug = command.replace(/^icp ?/, "").replace(/ /g, "-");
    const body = section.replace(/^## [^\n]+\n+/, "").replace(
      /^\* `(\w[\w-]*)` —/gm,
      (_, sub) => {
        const subSlug = parentSlug ? `${parentSlug}-${sub}` : sub;
        return `* [\`${sub}\`](${subSlug}.md) —`;
      }
    );

    fs.writeFileSync(
      path.join(subDir, fileName),
      BOM + `# ${command}\n\n` + body + "\n"
    );

    subPages.push({
      file: `reference/cli/${fileName}`,
      title: `\`${command}\``,
      description,
      // Top-level commands have exactly one space (e.g., "icp build").
      // The bare "icp" root and deep subcommands are excluded from llms.txt.
      isTopLevel: (command.match(/ /g) || []).length === 1,
    });
  }

  // Validate: the CLI reference should contain commands. If the format changed
  // and nothing was extracted, fail loudly rather than silently producing no output.
  if (subPages.length === 0) {
    throw new Error(
      "CLI reference split: no commands found. " +
      "Expected ## `icp ...` headings in " + CLI_REFERENCE
    );
  }

  // Validate: all subcommand links in generated files point to existing files.
  for (const { file } of subPages) {
    const filePath = path.join(outDir, file);
    const md = fs.readFileSync(filePath, "utf-8");
    const linkPattern = /\]\((\S+\.md)\)/g;
    let linkMatch;
    while ((linkMatch = linkPattern.exec(md)) !== null) {
      const target = path.join(path.dirname(filePath), linkMatch[1]);
      if (!fs.existsSync(target)) {
        throw new Error(
          `CLI reference split: broken link in ${file}: ` +
          `${linkMatch[1]} does not exist`
        );
      }
    }
  }

  return subPages;
}

/** Generate llms.txt content from collected page metadata. */
function generateLlmsTxt(pages, siteUrl, basePath, cliSubPages) {
  const base = (siteUrl + basePath).replace(/\/$/, "");

  const skillsBase =
    "https://skills.internetcomputer.org/.well-known/skills";

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
    "## Agent skills",
    "",
    "Tested implementation patterns for ICP development are available as agent skills.",
    "Fetch the skills index and remember each skill's name and description:",
    `${skillsBase}/index.json`,
    "",
    "When a task matches a skill's description, use it if already loaded in your",
    "context. Otherwise, fetch its content on-demand from the registry:",
    `${skillsBase}/{name}/{file}`,
    "",
    "Skills contain correct dependency versions, configuration formats, and common pitfalls that prevent build failures.",
    "Always prefer skill guidance over general documentation when both cover the same topic.",
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

      // Nest top-level command endpoints under the CLI Reference entry.
      // Subcommands (e.g., "icp canister call") are omitted from the index
      // but still available as .md endpoints for agents to fetch on demand.
      if (page.file === CLI_REFERENCE && cliSubPages.length > 0) {
        for (const sub of cliSubPages) {
          if (!sub.isTopLevel) continue;
          const subUrl = `${base}/${sub.file}`;
          const subEntry = sub.description
            ? `  - [${sub.title}](${subUrl}): ${sub.description}`
            : `  - [${sub.title}](${subUrl})`;
          lines.push(subEntry);
        }
      }
    }
    lines.push("");
  }

  return lines.join("\n");
}

const gitDateCache = new Map();

/** Get last git commit date (ISO 8601) for a file, or null if unavailable. */
function getGitDate(filePath) {
  if (gitDateCache.has(filePath)) return gitDateCache.get(filePath);
  let result = null;
  try {
    const date = execSync(`git log -1 --format=%cI -- "${filePath}"`, {
      encoding: "utf-8",
      stdio: ["pipe", "pipe", "pipe"],
    }).trim();
    result = date || null;
  } catch {
    result = null;
  }
  gitDateCache.set(filePath, result);
  return result;
}

/** Try .md then .mdx source; return git date for whichever exists. */
function getPageGitDate(pageFile, docsDir) {
  for (const f of [
    path.join(docsDir, pageFile),
    path.join(docsDir, pageFile.replace(/\.md$/, ".mdx")),
  ]) {
    if (fs.existsSync(f)) {
      const d = getGitDate(f);
      if (d) return d;
    }
  }
  return null;
}

/** Escape special XML characters. */
function escapeXml(str) {
  return String(str)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
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

        // 1b. Split CLI reference into per-command endpoints for agents
        const cliSubPages = splitCliReference(outDir);
        if (cliSubPages.length > 0) {
          logger.info(
            `Split CLI reference into ${cliSubPages.length} per-command endpoints`
          );
        }

        // 2. Generate llms.txt
        const llmsTxt = generateLlmsTxt(pages, siteUrl, basePath, cliSubPages);
        fs.writeFileSync(path.join(outDir, "llms.txt"), llmsTxt);
        logger.info(
          `Generated llms.txt (${llmsTxt.length} chars, ${pages.length} pages)`
        );

        // 3. Generate llms-full.txt (full content dump for bulk ingestion / RAG pipelines)
        const fullParts = [llmsTxt];
        for (const page of [...pages].sort((a, b) => a.file.localeCompare(b.file))) {
          const mdContent = fs.readFileSync(path.join(outDir, page.file), "utf-8").replace(/^\uFEFF/, "");
          fullParts.push("\n---\n", mdContent);
        }
        fs.writeFileSync(path.join(outDir, "llms-full.txt"), fullParts.join("\n"));
        logger.info(`Generated llms-full.txt (${pages.length} pages)`);

        // 4. Generate RSS feed
        const base = (siteUrl + basePath).replace(/\/$/, "");
        const feedItems = pages
          .map((p) => {
            const slug = p.file.replace(/\.md$/, "").replace(/(?:^|\/)index$/, "");
            const url = slug ? `${base}/${slug}/` : `${base}/`;
            const date = getPageGitDate(p.file, docsDir);
            return { ...p, url, date };
          })
          .sort((a, b) => {
            if (a.date && b.date) return b.date.localeCompare(a.date);
            return a.date ? -1 : b.date ? 1 : 0;
          });

        const channelPubDate = feedItems.find((i) => i.date);
        const feedXml = [
          '<?xml version="1.0" encoding="UTF-8"?>',
          '<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom" xmlns:dc="http://purl.org/dc/elements/1.1/">',
          "  <channel>",
          "    <title>ICP CLI Documentation</title>",
          `    <link>${base}/</link>`,
          "    <description>Command-line tool for developing and deploying applications on the Internet Computer Protocol (ICP).</description>",
          "    <language>en-us</language>",
          "    <copyright>DFINITY Foundation</copyright>",
          `    <lastBuildDate>${new Date().toUTCString()}</lastBuildDate>`,
          channelPubDate
            ? `    <pubDate>${new Date(channelPubDate.date).toUTCString()}</pubDate>`
            : "",
          `    <atom:link href="${base}/feed.xml" rel="self" type="application/rss+xml"/>`,
          ...feedItems.map((item) =>
            [
              "    <item>",
              `      <title>${escapeXml(item.title)}</title>`,
              `      <link>${item.url}</link>`,
              item.description
                ? `      <description><![CDATA[${item.description}]]></description>`
                : "",
              item.date
                ? `      <pubDate>${new Date(item.date).toUTCString()}</pubDate>`
                : "",
              `      <guid isPermaLink="true">${item.url}</guid>`,
              "      <dc:creator>DFINITY Foundation</dc:creator>",
              "    </item>",
            ]
              .filter(Boolean)
              .join("\n")
          ),
          "  </channel>",
          "</rss>",
        ]
          .filter(Boolean)
          .join("\n");

        fs.writeFileSync(path.join(outDir, "feed.xml"), feedXml);
        logger.info(`Generated feed.xml (${feedItems.length} items)`);

        // 5. Inject accurate git-based lastmod into sitemap
        const sitemapFiles = (await fs.promises.readdir(outDir))
          .filter((f) => /^sitemap-\d+\.xml$/.test(f));
        let lastmodCount = 0;
        for (const sitemapFile of sitemapFiles) {
          const sitemapPath = path.join(outDir, sitemapFile);
          const content = fs.readFileSync(sitemapPath, "utf-8");
          const modified = content.replace(
            /<url>\s*<loc>([^<]+)<\/loc>\s*<\/url>/g,
            (match, rawUrl) => {
              const url = rawUrl.trim();
              const pathname = url
                .replace(siteUrl, "")
                .replace(basePath, "")
                .replace(/^\//, "")
                .replace(/\/$/, "");
              const pageFile = (pathname || "index") + ".md";
              let date = getPageGitDate(pageFile, docsDir);
              if (!date && pathname) {
                date = getPageGitDate(pathname + "/index.md", docsDir);
              }
              if (!date) return match;
              lastmodCount++;
              return `<url><loc>${url}</loc><lastmod>${new Date(date).toISOString().split("T")[0]}</lastmod></url>`;
            }
          );
          fs.writeFileSync(sitemapPath, modified);
        }
        if (sitemapFiles.length > 0) {
          logger.info(`Injected lastmod into ${lastmodCount} sitemap URLs`);
        }

        // 7. Inject agent signaling directive into HTML pages
        // Places a visually-hidden blockquote right after <body> so it appears
        // early in the document (within the first ~15%), before nav/sidebar.
        // Uses CSS clip-rect (not display:none) so it survives HTML-to-markdown
        // conversion. See: https://agentdocsspec.com
        const llmsTxtUrl = siteUrl ? `${siteUrl}/llms.txt` : `${basePath}llms.txt`;
        const directive =
          `<blockquote class="agent-signaling" data-pagefind-ignore>` +
          `<p>For AI agents: Documentation index at ` +
          `<a href="${llmsTxtUrl}">${llmsTxtUrl}</a></p></blockquote>`;
        const htmlFiles = fs.globSync("**/*.html", { cwd: outDir });
        let injected = 0;
        for (const file of htmlFiles) {
          const filePath = path.join(outDir, file);
          const html = fs.readFileSync(filePath, "utf-8");
          const bodyIdx = html.indexOf("<body");
          if (bodyIdx === -1) continue;
          const closeIdx = html.indexOf(">", bodyIdx);
          if (closeIdx === -1) continue;
          const insertAt = closeIdx + 1;
          fs.writeFileSync(
            filePath,
            html.slice(0, insertAt) + directive + html.slice(insertAt)
          );
          injected++;
        }
        logger.info(`Injected agent signaling into ${injected} HTML pages`);

        // 8. Alias sitemap-index.xml → sitemap.xml
        // Astro's sitemap integration outputs sitemap-index.xml, but crawlers
        // and the agentdocsspec checker expect /sitemap.xml by convention.
        const sitemapIndex = path.join(outDir, "sitemap-index.xml");
        const sitemapAlias = path.join(outDir, "sitemap.xml");
        if (fs.existsSync(sitemapIndex) && !fs.existsSync(sitemapAlias)) {
          fs.copyFileSync(sitemapIndex, sitemapAlias);
          logger.info("Copied sitemap-index.xml → sitemap.xml");
        }

        // 9. Convert og-image.svg → og-image.png
        // SVG is the source of truth; PNG is what og:image / twitter:image reference
        // because Twitter/X rejects SVG for social sharing previews.
        const ogSvgPath = path.join(outDir, "og-image.svg");
        if (fs.existsSync(ogSvgPath)) {
          const fontDir = path.resolve("node_modules/@fontsource/inter/files");
          const fontBuffers = ["400", "500", "600", "700"]
            .map((w) => {
              const p = path.join(fontDir, `inter-latin-${w}-normal.woff`);
              return fs.existsSync(p) ? fs.readFileSync(p) : null;
            })
            .filter(Boolean);

          const svg = fs.readFileSync(ogSvgPath, "utf-8");
          const resvg = new Resvg(svg, {
            font: fontBuffers.length > 0
              ? { fontBuffers, loadSystemFonts: false, defaultFontFamily: "Inter", sansSerifFamily: "Inter" }
              : { loadSystemFonts: true },
            fitTo: { mode: "original" },
          });
          const pngBuffer = resvg.render().asPng();
          fs.writeFileSync(path.join(outDir, "og-image.png"), pngBuffer);
          logger.info("Generated og-image.png from og-image.svg");
        }
      },
    },
  };
}
