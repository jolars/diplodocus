import { expect, test } from "@playwright/test";
import { spawnSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createServer, type Server } from "node:http";
import { tmpdir } from "node:os";
import { extname, join } from "node:path";

const endpoint = "/api/ask?docs=fixture&v=1";
const question = "How do I fit a line?";
const delta = "### Fit a line\n\n- Use **`fit()`** first.\n- Read the [guide](/manual/guide.html).";
const answer = `${delta}\n\n\`\`\`rust\nfit(x);\n\`\`\``;
const encode = (part: object | "[DONE]") => `data: ${typeof part === "string" ? part : JSON.stringify(part)}\r\n\r\n`;

let workspace: string;
let server: Server;
let origin: string;
let finishAnswer: () => void;

test.beforeAll(async () => {
  const binary = process.env.DIPLODOCUS_BROWSER_BINARY;
  if (!binary) throw new Error("Run npm run site-test or npm run site-capture.");
  workspace = await mkdtemp(join(tmpdir(), "diplodocus-chat-"));
  await mkdir(join(workspace, "docs"));
  await writeFile(join(workspace, "docs/index.md"), "# Documentation\n");
  await writeFile(join(workspace, "docs/guide.md"), "# Guide\n\n## Fitting a line\n\nCall `fit()` to fit a line.\n");
  const configuration = join(workspace, "diplodocus.yaml");
  await writeFile(configuration, `project:
  name: Documentation
presentation:
  site-url: https://docs.example.test/manual/
  ask-ai:
    endpoint: '${endpoint}'
repositories:
  - { id: docs, path: . }
content:
  - { id: guide, owner: project, repository: docs, path: docs, mount: '', format: gfm }
`);
  const site = join(workspace, "site");
  const build = spawnSync(binary, ["build", "--config", configuration, "--output", site], { encoding: "utf8" });
  expect(build.status, build.error?.message ?? build.stderr).toBe(0);

  const media: Record<string, string> = { ".html": "text/html", ".css": "text/css", ".js": "text/javascript" };
  server = createServer(async (request, response) => {
    const path = new URL(request.url!, origin).pathname;
    if (request.method === "POST" && request.url === endpoint) {
      const body = JSON.parse((await Array.fromAsync(request)).join(""));
      const prompt = body.messages.at(-1).parts[0].text;
      response.writeHead(200, {
        "Content-Type": "text/event-stream; charset=utf-8",
        "x-vercel-ai-ui-message-stream": "v1",
      });
      response.write(encode({ type: "text-start", id: "answer" }));
      if (prompt === question) {
        const first = encode({ type: "text-delta", id: "answer", delta });
        response.write(`: heartbeat\r\n${first.slice(0, -1)}`);
        setImmediate(() => response.write(first.slice(-1)));
        finishAnswer = () => response.end([
          encode({ type: "text-delta", id: "answer", delta: answer.slice(delta.length) }),
          encode({ type: "text-end", id: "answer" }),
          encode({ type: "source-url", sourceId: "guide", url: `${origin}/manual/guide.html#fitting-a-line`, title: "Fitting a line" }),
          encode({ type: "source-url", sourceId: "bad", url: "javascript:alert(1)", title: "Invalid source" }),
          encode({ type: "finish" }),
          encode("[DONE]"),
        ].join(""));
      } else if (prompt === "Stop this answer") {
        response.write(encode({ type: "text-delta", id: "answer", delta: "This answer is still streaming." }));
      } else if (prompt === "Interrupt this answer") {
        response.end(encode({ type: "text-delta", id: "answer", delta: "This stream ended early." }));
      } else {
        response.end([
          encode({ type: "text-delta", id: "answer", delta: "Here is a follow-up answer. <img src=x>\n\n[Unsafe](javascript:alert(1))\n\n![Image](https://example.test/image.png)" }),
          encode({ type: "text-end", id: "answer" }),
          encode({ type: "finish" }),
          encode("[DONE]"),
        ].join(""));
      }
    } else if (path.startsWith("/manual/")) {
      try {
        const file = path.slice("/manual/".length) || "index.html";
        const bytes = await readFile(join(site, file));
        response.writeHead(200, { "Content-Type": media[extname(file)] ?? "application/octet-stream" }).end(bytes);
      } catch {
        response.writeHead(404).end();
      }
    } else {
      response.writeHead(404).end();
    }
  });
  await new Promise<void>(resolve => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("Chat fixture did not start.");
  origin = `http://127.0.0.1:${address.port}`;
});

test.afterAll(async () => {
  if (server) {
    const closed = new Promise<void>(resolve => server.close(() => resolve()));
    server.closeAllConnections();
    await closed;
  }
  if (workspace) await rm(workspace, { recursive: true, force: true });
});

test("Ask AI streams answers and supports follow-ups from a prefixed site", async ({ page }, info) => {
  await page.goto(`${origin}/manual/`);
  await expect(page.getByRole("button", { name: "Search documentation" })).toBeVisible();
  const trigger = page.getByRole("button", { name: "Ask AI", exact: true });
  await trigger.click();
  const chat = page.getByRole("dialog", { name: "Ask AI" });
  const input = chat.getByRole("textbox", { name: "Question about the docs" });
  await expect(input).toBeFocused();
  await input.fill(question);
  const firstRequest = page.waitForRequest(`${origin}${endpoint}`);
  await chat.getByRole("button", { name: "Send question", exact: true }).click();
  const first = (await firstRequest).postDataJSON();
  expect(first.messages).toMatchObject([{ role: "user", parts: [{ type: "text", text: question }] }]);
  const rendered = chat.locator(".ask-ai-answer");
  await expect(rendered.getByRole("heading", { name: "Fit a line" })).toBeVisible();
  await expect(rendered.getByRole("listitem")).toHaveCount(2);
  await expect(rendered.getByRole("link", { name: "guide" })).toHaveAttribute("href", "/manual/guide.html");
  await expect(rendered.locator("strong code")).toHaveText("fit()");
  await expect(chat.getByRole("status")).toHaveText("Writing an answer…");
  finishAnswer();
  await expect(chat.getByRole("status")).toHaveText("Answer ready.");
  await expect(chat.locator("pre code")).toHaveText("fit(x);");
  await expect(chat.locator(".ask-ai-sources").getByRole("link")).toHaveCount(1);

  if (process.env.DIPLODOCUS_BROWSER_CAPTURE) {
    const path = info.outputPath("ask-ai.png");
    await page.screenshot({ path });
    await info.attach("Ask AI", { path, contentType: "image/png" });
  }

  await chat.getByRole("link", { name: "Fitting a line" }).click();
  await expect(page).toHaveURL(`${origin}/manual/guide.html#fitting-a-line`);
  await expect(chat).not.toBeVisible();
  await trigger.click();
  await input.fill("What about noisy input?");
  const followUp = page.waitForRequest(`${origin}${endpoint}`);
  await chat.getByRole("button", { name: "Send question", exact: true }).click();
  expect((await followUp).postDataJSON()).toMatchObject({ id: first.id, messages: [
    { role: "user", parts: [{ text: question }] },
    { role: "assistant", parts: [{ text: answer }] },
    { role: "user", parts: [{ text: "What about noisy input?" }] },
  ] });
  await expect(chat.getByText("Here is a follow-up answer. <img src=x>", { exact: true })).toBeVisible();
  await expect(chat.locator("img")).toHaveCount(0);
  await expect(chat.getByRole("link", { name: "Unsafe" })).toHaveCount(0);
  await chat.getByRole("button", { name: "New chat" }).click();
  await expect(chat.getByRole("region", { name: "Conversation" })).not.toBeVisible();
  await page.keyboard.press("Escape");
  await expect(trigger).toBeFocused();
});

test("Ask AI handles a stopped stream and a response without a completion marker", async ({ page }) => {
  await page.goto(`${origin}/manual/`);
  await page.getByRole("button", { name: "Ask AI", exact: true }).click();
  const chat = page.getByRole("dialog", { name: "Ask AI" });
  const input = chat.getByRole("textbox");
  const ask = chat.getByRole("button", { name: "Send question", exact: true });
  await input.fill("Stop this answer");
  await ask.click();
  await expect(chat.getByText("This answer is still streaming.")).toBeVisible();
  await chat.getByRole("button", { name: "Stop", exact: true }).click();
  await expect(chat.getByRole("status")).toHaveText("Answer stopped.");
  await input.fill("Interrupt this answer");
  const request = page.waitForRequest(`${origin}${endpoint}`);
  await ask.click();
  expect((await request).postDataJSON().messages).toHaveLength(1);
  await expect(chat.getByRole("status")).toHaveText("The answer was interrupted. Try again.");
  await expect(ask).toBeEnabled();
});
