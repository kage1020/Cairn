/** The part of a Cloudflare Pages Functions context the middleware reads. */
export interface PagesContext {
  request: Request;
  next(): Promise<Response>;
  env: { ASSETS: { fetch(input: Request | string): Promise<Response> } };
}

/** The pages whose Markdown is `/llms.txt`, since a splash page has no `index.md` of its own. */
const homePages = new Set(["/", "/ja/"]);

/** Whether an `Accept` header ranks `text/markdown` at least as high as `text/html`. */
export function prefersMarkdown(accept: string | null): boolean {
  const ranges = (accept ?? "").split(",").flatMap((range) => {
    const [type = "", ...params] = range.split(";").map((part) => part.trim().toLowerCase());
    if (type === "") return [];
    const q = params.find((param) => param.startsWith("q="))?.slice(2);
    const quality = q === undefined ? 1 : Number(q);
    return [{ type, quality: Number.isNaN(quality) ? 1 : quality }];
  });
  const quality = (types: string[]) => {
    for (const type of types) {
      const range = ranges.find((candidate) => candidate.type === type);
      if (range) return range.quality;
    }
    return 0;
  };
  // A wildcard never selects Markdown: `*/*` is what a client that never heard of it sends.
  const markdown = quality(["text/markdown"]);
  return markdown > 0 && markdown >= quality(["text/html", "text/*", "*/*"]);
}

function varyOnAccept(response: Response): Response {
  const varied = new Response(response.body, response);
  const vary = varied.headers.get("Vary");
  if (!vary?.split(",").some((field) => field.trim().toLowerCase() === "accept")) {
    varied.headers.append("Vary", "Accept");
  }
  return varied;
}

function asMarkdown(response: Response, method: string): Response {
  const markdown = varyOnAccept(response);
  markdown.headers.set("Content-Type", "text/markdown; charset=utf-8");
  return method === "HEAD" ? new Response(null, markdown) : markdown;
}

function notFound(url: URL, method: string): Response {
  const body = [
    "# Not found",
    "",
    `Nothing is served at \`${url.pathname}\`.`,
    "",
    `- [llms.txt](${url.origin}/llms.txt) lists every page of the documentation, each as Markdown.`,
    `- [The sitemap](${url.origin}/sitemap-index.xml) lists every page as HTML.`,
    "",
  ].join("\n");
  return asMarkdown(
    new Response(body, { status: 404, headers: { "Cache-Control": "no-store" } }),
    method,
  );
}

/** Answers a request for a page with its Markdown when the client prefers it, and with the HTML otherwise. */
export async function onRequest({ request, next, env }: PagesContext): Promise<Response> {
  const { method } = request;
  if (method !== "GET" && method !== "HEAD") return next();
  const url = new URL(request.url);
  const markdown = prefersMarkdown(request.headers.get("Accept"));
  const isPage = url.pathname.endsWith("/");

  if (markdown && isPage) {
    const source = homePages.has(url.pathname) ? "/llms.txt" : `${url.pathname}index.md`;
    const variant = await env.ASSETS.fetch(new URL(source, url).toString());
    if (variant.ok) return asMarkdown(variant, method);
  }
  const response = await next();
  if (response.status === 404) return markdown ? notFound(url, method) : varyOnAccept(response);
  return isPage ? varyOnAccept(response) : response;
}
