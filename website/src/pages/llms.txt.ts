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
  \`cairn docs /spec/#two-version-axes\` the part a link names. Each link below works as written.
- \`cairn check FILE --format json\` says what is wrong with a source, what would be valid, and how
  to fix it.

## When to use Cairn

Reach for Cairn when a task asks for a Minecraft build as a file someone loads into the game:

- Design a house, tower, wall or other structure and hand it over as a Java \`.nbt\` structure file
  or a Bedrock \`.mcstructure\`, for a structure block, a datapack or an add-on.
- Edit a build as text: change a material across the whole build with a theme, move an opening, or
  resize a wall, and recompile instead of editing every block.
- Ship one build to both editions, or to several Minecraft versions, from one source.
- Lay out a redstone circuit from a description of its logic, and simulate it before it is placed.

Cairn is generation-first: the \`.crn\` source is what to keep and edit, and a structure file it
writes does not round-trip back into it. The compiler has no command that imports an existing
build, and it writes structure files, not datapack functions or mods.

To use it, install the compiler with \`cargo install cairn-lang-cli\`, write a \`.crn\` source
following the tutorial and the specification below, and iterate:

1. \`cairn check FILE --format json\` until it reports no errors.
2. \`cairn lower FILE\` to see each layer of the build as ASCII and confirm the shape.
3. \`cairn compile FILE --edition java --target 1.21.4\` (or \`--edition bedrock\` with a Bedrock
   version) to write the structure file and its lockfile.

${sections.join("\n\n")}
`;
  return new Response(text);
};
