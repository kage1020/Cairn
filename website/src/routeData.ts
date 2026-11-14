import { defineRouteMiddleware } from "@astrojs/starlight/route-data";

/** The ids of the home pages, one per locale. */
const homePages = new Set(["", "ja"]);

const repository = "https://github.com/kage1020/Cairn";

/** Adds a JSON-LD description of Cairn to the head of each home page. */
export const onRequest = defineRouteMiddleware((context) => {
  const route = context.locals.starlightRoute;
  if (!homePages.has(route.id) || !context.site) return;
  const home = new URL(context.url.pathname, context.site);
  const software = {
    "@context": "https://schema.org",
    "@type": "SoftwareApplication",
    name: "Cairn",
    description: route.entry.data.description,
    url: home.href,
    inLanguage: route.lang,
    applicationCategory: "DeveloperApplication",
    applicationSubCategory: "Compiler",
    operatingSystem: "Windows, macOS, Linux",
    keywords: [
      "Minecraft",
      "description language",
      "compiler",
      "structure files",
      "NBT",
      "redstone",
      "Java",
      "Bedrock",
    ],
    license: "https://www.apache.org/licenses/LICENSE-2.0",
    isAccessibleForFree: true,
    installUrl: "https://crates.io/crates/cairn-lang-cli",
    softwareHelp: { "@type": "CreativeWork", url: new URL("introduction/", home).href },
    author: { "@type": "Person", name: "kage1020", url: "https://github.com/kage1020" },
    sameAs: [repository, "https://crates.io/crates/cairn-lang-cli"],
  };
  route.head.push({
    tag: "script",
    attrs: { type: "application/ld+json" },
    // A `</script>` inside a string would end the element early.
    content: JSON.stringify(software).replaceAll("<", "\\u003c"),
  });
});
