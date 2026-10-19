- *(cli)* `cairn docs` prints a page of the documentation site as Markdown from a copy compiled into
  the binary, so the specification a person or an agent reads is the one the installed compiler
  implements, offline. `cairn docs` lists the pages, `cairn docs spec/syntax` prints one, `--toc`
  lists its headings, and a part of a page is named the way the spec is cited
  (`cairn docs spec/lint "Error vs warning"`) or by the `#fragment` of a link to it. A link or URL
  of the site works as written, the `index.md` form `llms.txt` links included, and Japanese pages
  sit under `ja/`.
- *(website)* The site serves every Markdown page as Markdown at its URL followed by `index.md`,
  indexes the English ones for language models in `/llms.txt`, and puts them all in
  `/llms-full.txt`. The same pages are the new `cairn-lang-docs` crate, which embeds them for
  `cairn docs`, and CI checks after each build of the site that the two serve the same pages.
