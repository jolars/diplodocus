import { readFileSync } from "node:fs";
import { defineConfig } from "vitepress";

const reference = JSON.parse(
  readFileSync(new URL("../reference/sidebar.json", import.meta.url), "utf8"),
);

export default defineConfig({
  title: "Tiny Stats",
  description: "VitePress guides and package documentation prepared by Diplodocus.",
  srcExclude: ["README.md"],
  themeConfig: {
    nav: [
      { text: "Guide", link: "/guide" },
      { text: "Reference", link: "/reference/" },
    ],
    sidebar: [
      { text: "Start here", items: [{ text: "Guide", link: "/guide" }] },
      ...reference,
    ],
    search: { provider: "local" },
    outline: [2, 3],
  },
});
