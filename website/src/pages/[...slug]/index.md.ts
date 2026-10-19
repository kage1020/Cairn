import type { APIRoute, GetStaticPaths, InferGetStaticPropsType } from "astro";

import { asMarkdown, markdownPages } from "../../llms";

export const getStaticPaths = (async () =>
  (await markdownPages()).map((page) => ({
    params: { slug: page.id },
    props: { page },
  }))) satisfies GetStaticPaths;

type Props = InferGetStaticPropsType<typeof getStaticPaths>;

export const GET: APIRoute<Props> = ({ props }) => new Response(asMarkdown(props.page));
