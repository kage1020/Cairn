import { describe, expect, it } from "vitest";

import { type PagesContext, onRequest, prefersMarkdown } from "./negotiation";

const origin = "https://cairn.example";
const browserAccept = "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";

describe("prefersMarkdown", () => {
  it.each([
    ["text/markdown", true],
    ["text/markdown, text/html;q=0.9", true],
    ["text/html, text/markdown", true],
    ["text/markdown;q=0.5", true],
    ["Text/Markdown ; q=1", true],
    [browserAccept, false],
    ["*/*", false],
    ["text/*", false],
    ["text/html;q=0.9, text/markdown;q=0.5", false],
    ["text/markdown;q=0.5, text/*", false],
    ["text/markdown;q=0", false],
    ["", false],
    [null, false],
  ])("Accept %j prefers Markdown: %s", (accept, expected) => {
    expect(prefersMarkdown(accept)).toBe(expected);
  });
});

interface Site {
  /** The static files the deployment serves, by path. */
  assets: Record<string, string>;
  /** What the rest of the pipeline answers, standing in for the page itself. */
  page: Response;
}

function context(path: string, init: RequestInit, site: Site): PagesContext {
  return {
    request: new Request(new URL(path, origin), init),
    next: async () => site.page,
    env: {
      ASSETS: {
        fetch: async (input) => {
          const body = site.assets[new URL(input instanceof Request ? input.url : input).pathname];
          return body === undefined
            ? new Response("<h1>404</h1>", { status: 404, headers: { "Content-Type": "text/html" } })
            : new Response(body, {
                headers: { "Content-Type": "text/plain; charset=utf-8", ETag: '"md"' },
              });
        },
      },
    },
  };
}

function htmlPage(status = 200): Response {
  return new Response("<html>page</html>", {
    status,
    headers: { "Content-Type": "text/html; charset=utf-8", Vary: "accept-encoding" },
  });
}

const markdown = { headers: { Accept: "text/markdown" } };
const html = { headers: { Accept: browserAccept } };
const assets = {
  "/introduction/index.md": "# Introduction\n",
  "/llms.txt": "# Cairn\n",
};

async function snapshot(response: Response) {
  return {
    status: response.status,
    contentType: response.headers.get("Content-Type"),
    vary: response.headers.get("Vary"),
    body: await response.text(),
  };
}

describe("onRequest", () => {
  it("serves a page's index.md as Markdown to a client that prefers it", async () => {
    const response = await onRequest(
      context("/introduction/", markdown, { assets, page: htmlPage() }),
    );
    expect(await snapshot(response)).toEqual({
      status: 200,
      contentType: "text/markdown; charset=utf-8",
      vary: "Accept",
      body: "# Introduction\n",
    });
    expect(response.headers.get("ETag")).toBe('"md"');
  });

  it("serves the page's HTML to a browser and says the response varies on Accept", async () => {
    const response = await onRequest(context("/introduction/", html, { assets, page: htmlPage() }));
    expect(await snapshot(response)).toEqual({
      status: 200,
      contentType: "text/html; charset=utf-8",
      vary: "accept-encoding, Accept",
      body: "<html>page</html>",
    });
  });

  it.each(["/", "/ja/"])("serves llms.txt as the Markdown of the home page %s", async (path) => {
    const response = await onRequest(context(path, markdown, { assets, page: htmlPage() }));
    expect(await snapshot(response)).toEqual({
      status: 200,
      contentType: "text/markdown; charset=utf-8",
      vary: "Accept",
      body: "# Cairn\n",
    });
  });

  it("serves the HTML of a page that has no Markdown, varying on Accept", async () => {
    const response = await onRequest(
      context("/playground/", markdown, { assets, page: htmlPage() }),
    );
    expect(await snapshot(response)).toEqual({
      status: 200,
      contentType: "text/html; charset=utf-8",
      vary: "accept-encoding, Accept",
      body: "<html>page</html>",
    });
  });

  it.each(["/missing/", "/missing"])(
    "answers a missing path %s with a Markdown 404 that links the index",
    async (path) => {
      const response = await onRequest(context(path, markdown, { assets, page: htmlPage(404) }));
      expect(await snapshot(response)).toEqual({
        status: 404,
        contentType: "text/markdown; charset=utf-8",
        vary: "Accept",
        body: [
          "# Not found",
          "",
          `Nothing is served at \`${path}\`.`,
          "",
          `- [llms.txt](${origin}/llms.txt) lists every page of the documentation, each as Markdown.`,
          `- [The sitemap](${origin}/sitemap-index.xml) lists every page as HTML.`,
          "",
        ].join("\n"),
      });
    },
  );

  it("answers a missing path with the HTML 404 page for a browser", async () => {
    const response = await onRequest(context("/missing", html, { assets, page: htmlPage(404) }));
    expect(await snapshot(response)).toEqual({
      status: 404,
      contentType: "text/html; charset=utf-8",
      vary: "accept-encoding, Accept",
      body: "<html>page</html>",
    });
  });

  it("passes a redirect to the trailing-slash URL through untouched", async () => {
    const redirect = new Response(null, { status: 308, headers: { Location: "/introduction/" } });
    const response = await onRequest(context("/introduction", markdown, { assets, page: redirect }));
    expect(response).toBe(redirect);
  });

  it("passes a request other than GET or HEAD through untouched", async () => {
    const page = htmlPage();
    const response = await onRequest(
      context("/introduction/", { ...markdown, method: "POST" }, { assets, page }),
    );
    expect(response).toBe(page);
  });

  it("answers HEAD with the Markdown headers and no body", async () => {
    const response = await onRequest(
      context("/introduction/", { ...markdown, method: "HEAD" }, { assets, page: htmlPage() }),
    );
    expect(await snapshot(response)).toEqual({
      status: 200,
      contentType: "text/markdown; charset=utf-8",
      vary: "Accept",
      body: "",
    });
  });
});
