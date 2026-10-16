import { mkdir, writeFile } from "node:fs/promises";

// Pin the generated distribution of GitHub's official VS Code themes.
const revision = "8f1828ebccc57930cf0adde9b35ae929584fe191";
const upstream = `https://raw.githubusercontent.com/shikijs/textmate-grammars-themes/${revision}`;
const github = "https://raw.githubusercontent.com/primer/github-vscode-theme/cd78e5e4e7bcf132a6f428ae0f32264bb1b729cf";
const directory = new URL("../src/rendering/assets/highlighting/", import.meta.url);

async function download(path, source = upstream) {
  const response = await fetch(`${source}/${path}`);
  if (!response.ok) throw new Error(`${response.status}: ${path}`);
  return response.text();
}

function selector(scope) {
  return scope.trim().split(/\s+/).map(part => part.split(".").map(atom => {
    if (!/^[a-zA-Z0-9_-]+$/.test(atom)) throw new Error(`Unsupported scope: ${scope}`);
    return `.syntax-${atom}`;
  }).join("")).join(" ");
}

function rule(selectors, settings) {
  const declarations = [];
  if (settings.foreground) declarations.push(`color: ${settings.foreground.toLowerCase()};`);
  if (settings.background) declarations.push(`background-color: ${settings.background.toLowerCase()};`);
  const styles = settings.fontStyle?.split(/\s+/) ?? [];
  if (styles.includes("bold")) declarations.push("font-weight: bold;");
  if (styles.includes("italic")) declarations.push("font-style: italic;");
  const decorations = [];
  if (styles.includes("underline")) decorations.push("underline");
  if (styles.includes("strikethrough")) decorations.push("line-through");
  if (decorations.length) declarations.push(`text-decoration: ${decorations.join(" ")};`);
  return `${selectors.join(",\n")} {\n${declarations.map(line => `  ${line}`).join("\n")}\n}\n`;
}

await mkdir(directory, { recursive: true });
for (const mode of ["light", "dark"]) {
  const path = `packages/tm-themes/themes/github-${mode}-default.json`;
  const theme = JSON.parse(await download(path));
  let css = `/*\n * ${theme.displayName}\n * Source: ${upstream}/${path}\n * MIT license; see LICENSE. Regenerate with node scripts/update-syntax-themes.mjs.\n */\n\n`;
  css += rule([".syntax-code"], {
    foreground: theme.colors["editor.foreground"],
    background: theme.colors["editor.background"],
  });
  for (const token of theme.tokenColors) {
    // Editor-only control-character markers have no visible text in HTML.
    if (token.settings.content) continue;
    const scopes = Array.isArray(token.scope) ? token.scope : [token.scope];
    css += rule(scopes.flatMap(scope => scope.split(",").map(selector)), token.settings);
  }
  await writeFile(new URL(`github-${mode}.css`, directory), css);
}

await writeFile(new URL("LICENSE", directory),
  `GitHub themes (${github}):\n\n${await download("LICENSE", github)}\n` +
  `TextMate distribution (${upstream}):\n\n${await download("packages/tm-themes/LICENSE")}`,
);
