import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import fs from "node:fs";

const base = (process.env.SITE_BASE ?? "/nile").replace(/\/$/, "");
const site = process.env.SITE_URL ?? "https://hsemix.github.io";
const side = JSON.parse(fs.readFileSync(new URL("./src/sidebar.json", import.meta.url), "utf8"));

export default defineConfig({
  site,
  base,
  integrations: [
    starlight({
      title: "Nile",
      description: "An open-source, embeddable HTML, CSS and JavaScript UI engine for desktop apps, written in Rust.",
      logo: { src: "./src/assets/logo.svg", alt: "Nile" },
      favicon: "/favicon.svg",
      social: [{ icon: "github", label: "GitHub", href: "https://github.com/hsemix/nile" }],
      customCss: ["./src/styles/nile.css"],
      editLink: { baseUrl: "https://github.com/hsemix/nile/edit/main/" },
      sidebar: [
        { label: "Guide", items: side.guide.map((slug) => ({ slug })) },
        { label: "Reference", items: side.reference.map((slug) => ({ slug })) },
        { label: "Native extensions", items: side.extensions.map((slug) => ({ slug })) },
        { label: "Architecture", collapsed: true, items: side.architecture.map((slug) => ({ slug })) },
      ],
    }),
  ],
});
