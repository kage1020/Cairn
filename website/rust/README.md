# cairn-lang-docs

Every Markdown page of the [Cairn documentation site](https://cairn.kage1020.com/), the specification among them, compiled into the crate. It is what `cairn docs` prints from: the pages a binary carries are the ones of the release it was built from, and reading them needs no network.

The crate's manifest lives in the site's own directory, `website/`, so its package carries the site's page sources as they are rather than a copy that could drift from them. A build script walks `src/content/docs/`, takes each `.md` page's path and front-matter title, and embeds the file; an `.mdx` page is left out, since its components do not read as Markdown.

## Use

`pages()` lists every page, English before Japanese, each directory's index first and its chapters by number, and `page("spec/syntax")` finds one by the path the site serves it at. A `Page` gives its title and Markdown, its headings with the anchor the site gives each, and the terms it defines with `- **Term.**` list items.

`Page::cited` returns what a citation names: a heading's title without its section number, a defined term, or the page's own title, the form `spec/lint "Error vs warning"` takes in the compiler's `--help` and comments. `Page::anchored` returns the section a link's `#fragment` opens.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
