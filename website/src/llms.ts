import { getCollection, type CollectionEntry } from "astro:content";

import { sidebar } from "../sidebar.mjs";

export type Page = CollectionEntry<"docs">;

/** Every Markdown page of the site, in every locale; an MDX page is left out. */
export async function markdownPages(): Promise<Page[]> {
  return getCollection("docs", (page) => page.filePath?.endsWith(".md") ?? false);
}

/** A page as Markdown: its title as a heading over its body, the text `cairn docs` prints. */
export function asMarkdown(page: Page): string {
  return `# ${page.data.title}\n\n${(page.body ?? "").trim()}\n`;
}

/** The URL a page's Markdown is served at. */
export function markdownUrl(page: Page, site: URL): URL {
  return new URL(`/${page.id}/index.md`, site);
}

/** The sidebar's groups with the English Markdown pages each lists, empty groups left out. */
export async function sidebarGroups(): Promise<{ label: string; pages: Page[] }[]> {
  const pages = new Map((await markdownPages()).map((page) => [page.id, page]));
  return sidebar
    .map((group) => ({
      label: group.label,
      pages: group.items.flatMap((item) => pages.get(item.slug) ?? []),
    }))
    .filter((group) => group.pages.length > 0);
}

/** The line `llms.txt` and `llms-full.txt` open with, under the project's name. */
export const summary =
  "Cairn is a description language for Minecraft builds: a `.crn` file declares intent (walls, " +
  "roofs, windows, themes, redstone) and the `cairn` compiler resolves it into structure files " +
  "for the Minecraft edition and version it is asked to build for.";
