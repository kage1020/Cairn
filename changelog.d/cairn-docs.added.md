- *(cli)* `cairn docs` prints a page of the documentation site as Markdown from a copy compiled into
  the binary, so the specification a person or an agent reads is the one the installed compiler
  implements, offline. `cairn docs` lists the pages, `cairn docs spec/syntax` prints one, `--toc`
  lists its headings, and a part of a page is named the way the spec is cited
  (`cairn docs spec/lint "Error vs warning"`) or by the `#fragment` of a link to it. Japanese pages
  sit under `ja/`.
- *(docs)* The new `cairn-lang-docs` crate embeds every Markdown page of the site, and `cairn docs`
  reads from it. Its manifest is `website/Cargo.toml`, so it packages the page sources themselves.
- The site serves every Markdown page as Markdown at its URL followed by `index.md`, indexes them
  for language models in `/llms.txt`, and puts every English page in `/llms-full.txt`.
