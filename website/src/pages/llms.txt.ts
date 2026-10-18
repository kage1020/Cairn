import type { APIRoute } from "astro";

import { markdownUrl, sidebarGroups, summary } from "../llms";

export const GET: APIRoute = async ({ site }) => {
  if (!site) {
    throw new Error("llms.txt links absolute URLs, so `site` must be set in astro.config.mjs");
  }
  const sections = (await sidebarGroups()).map(({ label, pages }) => {
    const links = pages.map((page) => {
      const description = page.data.description ? `: ${page.data.description}` : "";
      return `- [${page.data.title}](${markdownUrl(page, site)})${description}`;
    });
    return `## ${label}\n\n${links.join("\n")}`;
  });
  const full = new URL("/llms-full.txt", site);
  const text = `# Cairn

> ${summary}

The specification is normative: where the compiler and the specification disagree, the compiler is
wrong. Each page below is linked as Markdown, at the page's URL followed by \`index.md\`, and
[llms-full.txt](${full}) holds every one of them in a single file.
Most pages have a Japanese copy at the same path under \`/ja/\`.

An installed \`cairn\` carries these pages for the release it was built from, which makes it the copy
to read when one is at hand, since this site follows the latest release:

- \`cairn docs\` lists the pages, and \`cairn docs spec/syntax\` prints one.
- \`cairn docs spec/lint "Error vs warning"\` prints the part of a page a citation names, and
  \`cairn docs /spec/lint/#113-error-vs-warning\` the part a link names.
- \`cairn check FILE --format json\` says what is wrong with a source, what would be valid, and how
  to fix it.

${sections.join("\n\n")}
`;
  return new Response(text);
};
