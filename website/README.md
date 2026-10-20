# Cairn website

The documentation site for [Cairn](https://github.com/kage1020/Cairn). Built with [Astro](https://astro.build/) + [Starlight](https://starlight.astro.build/), in English and 日本語. Deployed to Cloudflare Pages at <https://cairn.kage1020.com/> (project name `cairn`, default URL `https://cairn.pages.dev/`).

This is the **canonical home** for the language specification, tutorial, developer guide, and examples index. There is no separate Markdown source elsewhere in the repository — edit the files under [`src/content/docs/`](src/content/docs/) directly.

## Layout

```
website/
├── astro.config.mjs        # Starlight config: title, locales, plugins
├── sidebar.mjs             # the sidebar (en + ja labels), which llms.txt follows too
├── package.json
├── Cargo.toml              # the cairn-lang-docs crate, which embeds the pages in `cairn`
├── rust/                   # its build script, library and README
├── public/
│   └── favicon.svg
├── src/
│   ├── content.config.ts   # Starlight docs collection
│   ├── llms.ts             # the Markdown pages, as llms.txt and index.md serve them
│   ├── pages/
│   │   ├── llms.txt.ts          # /llms.txt
│   │   ├── llms-full.txt.ts     # /llms-full.txt
│   │   └── [...slug]/index.md.ts  # /<path>/index.md for every Markdown page
│   ├── content/
│   │   └── docs/
│   │       ├── index.mdx        # landing (English)
│   │       ├── introduction.md
│   │       ├── tutorial.md
│   │       ├── development.md
│   │       ├── examples.md
│   │       ├── playground.mdx
│   │       ├── roadmap.md
│   │       ├── spec/
│   │       │   ├── index.md
│   │       │   ├── overview.md
│   │       │   ├── principles.md
│   │       │   └── …            # 16 spec chapters + glossary
│   │       └── ja/
│   │           ├── index.mdx        # landing (日本語)
│   │           ├── introduction.md
│   │           ├── tutorial.md
│   │           ├── examples.md
│   │           ├── playground.mdx
│   │           ├── roadmap.md
│   │           └── spec/
│   │               └── …            # Japanese spec chapters
│   └── styles/
│       └── cairn.css
└── tsconfig.json
```

English is the source of truth for the specification; Japanese pages are secondary copies maintained alongside the English ones (see [CONTRIBUTING.md](https://github.com/kage1020/Cairn/blob/main/CONTRIBUTING.md)). Worked `.crn` source files referenced from the tutorial live at [`../examples/`](../examples/) — those are code samples, not narrative docs, so they stay outside the site.

## Develop

```sh
pnpm install        # once
pnpm dev            # astro dev at http://localhost:4321/
pnpm check          # astro check — types in astro.config.mjs and src/
pnpm build          # astro build → ./dist, and resolves every internal link
pnpm preview        # serves ./dist
```

`pnpm check` and `pnpm build` answer different questions and CI runs both. The build does not type-check anything; the checker does not render a page, so it cannot see a link. Link validation is a build hook, which is why a dead link fails `pnpm build` rather than `pnpm check`.

## Editing content

- Every chapter is a plain `.md` (or `.mdx` for the landing pages and playground placeholder) with Starlight YAML frontmatter. Add `title:` and an optional `description:` and you are done.
- Internal links are **root-absolute** and carry a trailing slash: `[Overview](/spec/overview/)`, `[概要](/ja/spec/overview/)`. Astro does not rewrite a link's href, so whatever is written is what ships, and the site canonicalises to a trailing slash — which makes a relative link a broken link. `](lint)` written in `spec/syntax.md` is served from `/spec/syntax/` and resolves to `/spec/syntax/lint`, a 404. The build refuses a relative link for that reason, and resolves every root-absolute one, and every `#anchor` fragment, against the pages it just rendered.
- A heading's anchor is its slug, so renaming one breaks every link that names it — in the `ja/` tree as well as the `en` one. The slug is not always what the heading looks like: `10.4` becomes `104-`, and a `、` is dropped without leaving a hyphen. The build resolves these too, across pages as well as within one.
- Japanese pages live under `src/content/docs/ja/` mirroring the English layout. The sidebar in `sidebar.mjs` uses `translations: { ja: "…" }` so a single sidebar definition serves both locales.

## Reading the pages elsewhere

Every `.md` page under `src/content/docs/` that is not a draft reaches readers by three more routes, and none of them is a copy to keep in step: each reads the page sources as they are.

- **As Markdown on the site.** `/<path>/index.md` serves a page's title and body, `/llms.txt` indexes the English ones the sidebar lists, in its order, for language models, and `/llms-full.txt` is every English page in one file. The endpoints are in `src/pages/`; an `.mdx` page is left out, since its components do not read as Markdown.
- **Inside `cairn`.** `Cargo.toml` here makes this directory the `cairn-lang-docs` crate, whose build script embeds every page, so `cairn docs spec/syntax` prints the page the installed release was built with. Its package carries the pages themselves, which is why the manifest sits next to them rather than under `crates/`.
- **As citations.** A heading's title without its number, a term a page defines in bold, and a page's own title are what `spec/<chapter> "<title>"` citations quote. `cargo xtask lint spec-citations` resolves them through the same crate, so retitling a heading fails the lint at every citation of it, and `cargo test -p cairn-lang-docs` fails if a page links to a fragment the crate gives no heading. After `pnpm run build`, `cargo xtask lint --site website/dist site-parity`, run from the repository root, checks that the site serves each embedded page and no other, with the same Markdown and the same heading anchors; CI runs it after every build of the site.

## Deploying

The site is hosted on **Cloudflare Pages** (project name `cairn`) with the built-in Git integration. Cloudflare watches `main` on GitHub and rebuilds on every push that changes files under `website/`.

Cloudflare Pages project settings:

| Setting | Value |
|---|---|
| Production branch | `main` |
| Root directory | `website` |
| Build command | `pnpm install --frozen-lockfile && pnpm build` |
| Build output directory | `dist` |
| Node.js version | `24` (also pinned via [`.nvmrc`](.nvmrc)) |
| Compatibility date | `2026-06-14` (in [`wrangler.jsonc`](wrangler.jsonc)) |

Custom domain `cairn.kage1020.com` is wired in the Cloudflare dashboard; the default `cairn.pages.dev` URL also resolves. Deployment is Cloudflare's Git integration, not a workflow. The `Documentation site` job in [`editors-website.yml`](../.github/workflows/editors-website.yml) runs `pnpm check` and `pnpm build` on `website/**` changes, so a type error or a dead link fails a PR instead of reaching the deploy. Cloudflare runs the build too, so a dead link that got past review would fail the deploy rather than ship — but the type check is CI's alone.

To deploy a one-off preview from the CLI:

```sh
pnpm build
pnpm dlx wrangler pages deploy ./dist --project-name=cairn --branch=preview
```

To deploy from a fork, change `site` in `astro.config.mjs`, update `name` in `wrangler.jsonc`, and connect the fork's GitHub repo to a new Cloudflare Pages project.

## License

Apache-2.0 (same as the rest of Cairn).
