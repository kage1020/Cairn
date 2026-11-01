# cairn-lang-docs

Every Markdown page of the [Cairn documentation site](https://cairn.kage1020.com/), the specification among them, compiled into the crate. It is what `cairn docs` prints from: the pages a binary carries are the ones of the release it was built from, and reading them needs no network.

The crate's manifest lives in the site's own directory, `website/`, so its package carries the site's page sources as they are rather than a copy that could drift from them. A build script walks `src/content/docs/`, takes each `.md` page's path and front-matter title, and embeds the file; an `.mdx` page is left out, since its components do not read as Markdown, and so is a draft, as Starlight leaves it out of a production build.

## Use

`pages()` lists every page, English before Japanese, each directory's index first and its chapters by number, and `page("spec/syntax")` finds one by the path the site serves it at, or by its file (`spec/index`, `spec/lint.md`). A `Page` gives its title and Markdown, the Markdown the site serves at its `index.md`, and its headings with the anchor the site gives each.

`Page::cited` takes the title a citation quotes, in the form `spec/lint "Error vs warning"` takes in the compiler's `--help` and comments, and returns each part of the page with that title: the section of a heading whose title without its number matches, a term a `- **Term.**` list item defines, or the page itself. More than one part makes the citation ambiguous. `Page::anchored` returns the section a link's `#fragment` opens.

`lines` and `heading` read any Markdown the way the pages are read: which lines fenced code holds, and which line is an ATX heading and at what level.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
