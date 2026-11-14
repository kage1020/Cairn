- *(website)* A request for a page whose `Accept` header prefers `text/markdown` over `text/html`
  now gets the page's Markdown at the page's own URL, with `Vary: Accept`, so an agent no longer
  needs to know the `index.md` convention. The home page answers with `/llms.txt`, and a missing
  path with a Markdown 404 that links it. `/llms.txt` also says when Cairn is the right tool and
  how to drive the compiler, and the home page describes Cairn as JSON-LD and gives link previews
  an image.
