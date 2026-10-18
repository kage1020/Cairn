import type { APIRoute } from "astro";

import { asMarkdown, sidebarGroups, summary } from "../llms";

export const GET: APIRoute = async () => {
  const pages = (await sidebarGroups()).flatMap((group) => group.pages);
  return new Response(`# Cairn\n\n> ${summary}\n\n${pages.map(asMarkdown).join("\n")}`);
};
