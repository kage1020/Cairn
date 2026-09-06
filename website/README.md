# Cairn website

The documentation site for [Cairn](https://github.com/kage1020/Cairn). Built with [Astro](https://astro.build/) + [Starlight](https://starlight.astro.build/), in English and 日本語. Deployed to Cloudflare Pages at <https://cairn.kage1020.com/> (project name `cairn`, default URL `https://cairn.pages.dev/`).

This is the **canonical home** for the language specification, tutorial, developer guide, and examples index. There is no separate Markdown source elsewhere in the repository — edit the files under [`src/content/docs/`](src/content/docs/) directly.

## Layout

```
website/
├── astro.config.mjs        # Starlight config: title, sidebar (en + ja labels), locales
├── package.json
├── public/
│   └── favicon.svg
├── src/
│   ├── content.config.ts   # Starlight docs collection
│   ├── content/
│   │   └── docs/
│   │       ├── index.mdx        # landing (English)
│   │       ├── introduction.md
│   │       ├── tutorial.md
│   │       ├── development.md
│   │       ├── examples.md
│   │       ├── playground.mdx
│   │       ├── spec/
│   │       │   ├── index.md
│   │       │   ├── overview.md
│   │       │   ├── principles.md
│   │       │   └── …            # 14 spec chapters + glossary
│   │       └── ja/
│   │           ├── index.mdx        # landing (日本語)
│   │           ├── introduction.md
│   │           ├── tutorial.md
│   │           ├── examples.md
│   │           ├── playground.mdx
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
pnpm dev            # astro dev at http://localhost:4321/Cairn/
pnpm build          # astro build → ./dist
pnpm preview        # serves ./dist
```

## Editing content

- Every chapter is a plain `.md` (or `.mdx` for the landing pages and playground placeholder) with Starlight YAML frontmatter. Add `title:` and an optional `description:` and you are done.
- Internal cross-chapter links use extensionless URLs (`[overview](./overview)`) — Astro's content layer rewrites them at build time.
- Japanese pages live under `src/content/docs/ja/` mirroring the English layout. The sidebar in `astro.config.mjs` uses `translations: { ja: "…" }` so a single sidebar definition serves both locales.

## Deploying

The site is hosted on **Cloudflare Pages** (project name `cairn`) with the built-in Git integration. Cloudflare watches `main` on GitHub and rebuilds on every push that changes files under `website/`.

Cloudflare Pages project settings:

| Setting | Value |
|---|---|
| Production branch | `main` |
| Root directory | `website` |
| Build command | `pnpm install --frozen-lockfile && pnpm build` |
| Build output directory | `dist` |
| Node.js version | `22` (also pinned via [`.nvmrc`](.nvmrc) and [`package.json#engines`](package.json)) |
| Compatibility date | `2026-06-14` (in [`wrangler.jsonc`](wrangler.jsonc)) |

Custom domain `cairn.kage1020.com` is wired in the Cloudflare dashboard; the default `cairn.pages.dev` URL also resolves. There is no GitHub Actions workflow — deletion is intentional, as Cloudflare's Git integration owns the build.

To deploy a one-off preview from the CLI:

```sh
pnpm build
pnpm dlx wrangler pages deploy ./dist --project-name=cairn --branch=preview
```

To deploy from a fork, change `site` in `astro.config.mjs`, update `name` in `wrangler.jsonc`, and connect the fork's GitHub repo to a new Cloudflare Pages project.

## License

Apache-2.0 (same as the rest of Cairn).
