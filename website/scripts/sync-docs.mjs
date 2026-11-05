// Builds src/content/docs from the repository's ../docs folder, which stays the single source: it adds the front matter
// Starlight needs, turns relative links into site links (or GitHub links for files that are not part of the site), and
// writes the sidebar order. Nothing here is committed (see .gitignore); run it through `npm run dev` / `npm run build`.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const docs = path.join(root, "docs");
const out = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "src", "content", "docs");
const base = (process.env.SITE_BASE ?? "/nile").replace(/\/$/, "");
const repo = process.env.REPO_URL ?? "https://github.com/hsemix/nile";
const branch = process.env.REPO_BRANCH ?? "main";

// What goes on the site (relative to docs/). Everything else stays on GitHub and is linked there.
const SECTIONS = ["guide", "reference", "architecture"];
const SINGLES = ["extensions.md"];

const walk = (dir) =>
  fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(path.join(dir, e.name)) : [path.join(dir, e.name)]));

const included = (rel) => SINGLES.includes(rel) || SECTIONS.some((s) => rel.startsWith(s + "/"));
const slugOf = (rel) => rel.replace(/\.md$/, "").replace(/(^|\/)README$/, "$1").replace(/\/$/, "");
const siteUrl = (rel) => `${base}/${slugOf(rel)}${slugOf(rel) ? "/" : ""}`;
const github = (abs) => {
  const rel = path.relative(root, abs).split(path.sep).join("/");
  const kind = fs.existsSync(abs) && fs.statSync(abs).isDirectory() ? "tree" : "blob";
  return `${repo}/${kind}/${branch}/${rel}`;
};

function fixLink(url, fromAbs) {
  if (/^([a-z][a-z0-9+.-]*:|#|\/)/i.test(url)) return url;
  const [target, hash = ""] = url.split("#");
  const abs = path.resolve(path.dirname(fromAbs), decodeURIComponent(target));
  const rel = path.relative(docs, abs).split(path.sep).join("/");
  if (!rel.startsWith("..") && included(rel) && rel.endsWith(".md")) return siteUrl(rel) + (hash ? "#" + hash : "");
  if (!rel.startsWith("..") && /\.(png|jpe?g|gif|svg|webp)$/i.test(rel)) return `${base}/docs-assets/${rel}`;
  if (abs.startsWith(root) && fs.existsSync(abs)) return github(abs) + (hash ? "#" + hash : "");
  return url;
}

function convert(file) {
  const rel = path.relative(docs, file).split(path.sep).join("/");
  const lines = fs.readFileSync(file, "utf8").split("\n");
  let title = path.basename(rel, ".md");
  let fence = false;
  let titled = false;
  const body = [];
  for (const line of lines) {
    if (/^\s*(```|~~~)/.test(line)) fence = !fence;
    if (!fence && !titled && /^# /.test(line)) {
      title = line.slice(2).trim().replace(/`/g, "");
      titled = true;
      continue;
    }
    body.push(fence ? line : line.replace(/(\]\()([^)\s]+)(\))/g, (_, a, u, c) => a + fixLink(u, file) + c));
  }
  // "Guide index · Next: CLI" style navigation lines are replaced by the sidebar.
  const text = body.join("\n").replace(/^\[Guide index\]\([^\n]*\n/m, "").replace(/^\n+/, "");
  const front = `---\ntitle: ${JSON.stringify(title)}\neditUrl: ${JSON.stringify(github(file))}\n---\n\n`;
  const target = path.join(out, slugOf(rel) === "" ? "index.md" : /(^|\/)README\.md$/.test(rel) ? slugOf(rel) + "/index.md" : rel);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, front + text);
  return { rel, title };
}

fs.rmSync(out, { recursive: true, force: true });
fs.mkdirSync(out, { recursive: true });
const files = walk(docs).filter((f) => f.endsWith(".md") && included(path.relative(docs, f).split(path.sep).join("/")));
const pages = files.map(convert);

// Images referenced from the docs are served as-is from public/docs-assets.
const assets = path.resolve(out, "..", "..", "..", "public", "docs-assets");
fs.rmSync(assets, { recursive: true, force: true });
for (const f of walk(docs).filter((f) => /\.(png|jpe?g|gif|svg|webp)$/i.test(f))) {
  const dest = path.join(assets, path.relative(docs, f));
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  fs.copyFileSync(f, dest);
}

// The home page is written by hand.
fs.copyFileSync(path.resolve(out, "..", "..", "..", "home.mdx"), path.join(out, "index.mdx"));

// Sidebar: the guide follows the numbered list in its README; the rest is alphabetical.
const guideReadme = fs.readFileSync(path.join(docs, "guide", "README.md"), "utf8");
const guideOrder = [...guideReadme.matchAll(/^\d+\.\s+\[[^\]]*\]\(([\w-]+)\.md\)/gm)].map((m) => `guide/${m[1]}`).filter((s, i, all) => all.indexOf(s) === i);
const have = new Set(pages.map((p) => slugOf(p.rel)));
const guide = [...guideOrder, ...[...have].filter((s) => s.startsWith("guide/") && s !== "guide" && !guideOrder.includes(s)).sort()].filter((s) => have.has(s));
const section = (name) => [...have].filter((s) => s.startsWith(name + "/") && s !== name).sort();
fs.writeFileSync(
  path.resolve(out, "..", "..", "sidebar.json"),
  JSON.stringify(
    {
      guide: ["guide", ...guide],
      reference: ["reference", ...section("reference")],
      architecture: ["architecture", ...section("architecture")],
      extensions: have.has("extensions") ? ["extensions"] : [],
    },
    null,
    2,
  ),
);
console.log(`synced ${pages.length} pages`);
