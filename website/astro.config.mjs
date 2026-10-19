// @ts-check
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import starlightLinksValidator from "starlight-links-validator";

import { sidebar } from "./sidebar.mjs";

const githubRepo = "https://github.com/kage1020/Cairn";

export default defineConfig({
  site: "https://cairn.kage1020.com",
  integrations: [
    starlight({
      title: "Cairn",
      description:
        "A description language for Minecraft builds. Declare intent — walls, roofs, windows, redstone — and the compiler resolves the voxels.",
      favicon: "/favicon.svg",
      logo: {
        light: "./src/assets/logo.light.svg",
        dark: "./src/assets/logo.dark.svg",
      },
      social: [{ icon: "github", label: "GitHub", href: githubRepo }],
      editLink: { baseUrl: `${githubRepo}/edit/main/website/src/content/docs/` },
      defaultLocale: "root",
      locales: {
        root: { label: "English", lang: "en" },
        ja: { label: "日本語", lang: "ja" },
      },
      sidebar,
      customCss: ["./src/styles/cairn.css"],
      // `astro build` resolves no link at all: a dead cross-chapter link and
      // a dead `#anchor` both build green without this. The spec chapters
      // link to each other by heading, and `ja/` mirrors `en/` page for page,
      // so a renamed heading breaks links in two trees at once.
      //
      // Left at its defaults, which includes erroring on a relative link.
      // That is not a style rule here: Astro emits an extensionless relative
      // `href` verbatim, and the site canonicalises to a trailing slash, so
      // `](lint)` written in `spec/syntax.md` is served from `/spec/syntax/`
      // and resolves to `/spec/syntax/lint` — a 404. A relative link on this
      // site is a broken link, and the validator skips relative links rather
      // than resolving them, so refusing them is what keeps the rest honest.
      plugins: [starlightLinksValidator()],
    }),
  ],
});
