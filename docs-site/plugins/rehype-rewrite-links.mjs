/**
 * Rehype plugin that rewrites relative .md/.mdx links to absolute, base-path-prefixed URLs.
 *
 * Authors write GitHub-friendly relative links with .md extensions:
 *   [Quickstart](quickstart.md)
 *   [Concepts](../concepts/canisters.md#lifecycle)
 *
 * Astro outputs each page as a directory (project-structure.md → project-structure/index.html).
 * A relative link only resolves correctly when the page's URL carries a trailing slash (so the
 * browser's base is the page's own directory). GitHub Pages used to enforce that by redirecting
 * /x/foo → /x/foo/. Since hosting moved to the IC asset canister (#439) there is no such
 * redirect: it serves both /x/foo and /x/foo/ with HTTP 200. A visitor landing on the
 * no-trailing-slash URL (shared links, external links, typed URLs) then resolves relative links
 * one directory too high, dropping the version segment (e.g. /1.2/) and 404ing.
 *
 * To be immune to that trailing-slash ambiguity, we resolve each link against the SOURCE file's
 * location in the docs tree and emit an absolute URL that includes the deployment base path
 * (e.g. /1.2/). Absolute URLs resolve identically regardless of the current page's trailing
 * slash, and this matches how Starlight's own sidebar/nav links already work.
 *
 * The base path is passed in from astro.config.mjs (the single source of truth, set per-version
 * in CI via PUBLIC_BASE_PATH — e.g. /1.2/, /main/); it defaults to / for local/root builds. This
 * is what makes the fix release-proof: the link prefix always tracks the deployment base, so
 * every future versioned build gets correct links with no code change.
 *
 * Result (base = /1.2/, page = migration/from-dfx.md):
 *   ../tutorial.md                      → /1.2/tutorial/
 *   ../concepts/index.md                → /1.2/concepts/
 *   ../reference/configuration.md       → /1.2/reference/configuration/
 *   ../concepts/canisters.md#lifecycle  → /1.2/concepts/canisters/#lifecycle
 *
 * Only relative links are affected — external URLs, anchors, and already-absolute paths are
 * untouched. A link that would escape the docs root is left as-is.
 *
 * This plugin replaced a fragile sed-based preprocessing step; see #423 for that history.
 *
 * Important: Astro caches rendered content in node_modules/.astro/data-store.json.
 * After changing this plugin, delete that file to force re-rendering.
 *
 * Note: This is a rehype (HTML-level) plugin, not a remark plugin. Starlight overrides
 * Astro's markdown.remarkPlugins, but rehypePlugins are correctly merged. See #423.
 */
import { visit } from "unist-util-visit";
import path from "node:path";
import { fileURLToPath } from "node:url";

// Docs source lives at ../../docs relative to this plugin (docs-site/plugins/),
// mirroring astro-agent-docs.mjs. Used to locate each page within the docs tree.
const DOCS_ROOT = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../../docs",
);

/** Normalize a base path to always start and end with a single slash. */
function normalizeBase(base) {
  let b = base || "/";
  if (!b.startsWith("/")) b = "/" + b;
  if (!b.endsWith("/")) b += "/";
  return b;
}

/**
 * @param {{ base?: string }} [options] - `base` is the deployment base path
 *   (e.g. "/1.2/"), passed from astro.config.mjs so it always matches Astro's own `base`.
 */
export default function rehypeRewriteLinks(options = {}) {
  const basePath = normalizeBase(options.base);

  return (tree, file) => {
    const filePath = file?.path || file?.history?.[0] || "";
    // Directory of the current page relative to the docs root, using forward
    // slashes so link math is identical across platforms. "" for top-level pages.
    const pageDir = filePath
      ? path.relative(DOCS_ROOT, path.dirname(filePath)).split(path.sep).join("/")
      : "";

    visit(tree, "element", (node) => {
      if (node.tagName !== "a") return;

      const href = node.properties?.href;
      if (!href || typeof href !== "string") return;

      // Skip external links and protocol links
      if (/^[a-z][a-z0-9+.-]*:/i.test(href)) return;

      // Skip anchor-only links
      if (href.startsWith("#")) return;

      // Skip already-absolute paths
      if (href.startsWith("/")) return;

      // Split off anchor/query suffix (everything from the first # or ?).
      const suffixIdx = href.search(/[#?]/);
      const linkPath = suffixIdx === -1 ? href : href.slice(0, suffixIdx);
      const suffix = suffixIdx === -1 ? "" : href.slice(suffixIdx);

      // Only rewrite links that target an internal doc file (.md or .mdx).
      if (!/\.mdx?$/.test(linkPath)) return;

      // Resolve the author's relative link against the page's directory in the
      // docs tree, yielding a path relative to the docs root.
      const targetDoc = path.posix.normalize(path.posix.join(pageDir, linkPath));

      // A link that climbs above the docs root can't be made absolute safely —
      // leave it untouched rather than emit a bogus URL.
      if (targetDoc.startsWith("..")) return;

      // Strip the .md/.mdx extension.
      let out = targetDoc.replace(/\.mdx?$/, "");

      // index → directory root: "index" → "", "foo/index" → "foo/".
      out = out.replace(/(^|\/)index$/, "$1");

      // Directory URLs get a trailing slash (skip the empty root case).
      if (out && !out.endsWith("/")) out += "/";

      // Prefix the deployment base path (e.g. /1.2/) so the version segment can
      // never be dropped by trailing-slash-sensitive relative resolution.
      node.properties.href = basePath + out + suffix;
    });
  };
}
