// @ts-check
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

const githubRepo = "https://github.com/kage1020/Cairn";

/** @type {import('@astrojs/starlight/schema').StarlightUserConfig['sidebar']} */
const sidebar = [
  {
    label: "Start here",
    translations: { ja: "はじめに" },
    items: [
      {
        label: "Introduction",
        translations: { ja: "イントロダクション" },
        slug: "introduction",
      },
      {
        label: "Tutorial",
        translations: { ja: "チュートリアル" },
        slug: "tutorial",
      },
      {
        label: "Examples",
        translations: { ja: "サンプル" },
        slug: "examples",
      },
      {
        label: "Roadmap",
        translations: { ja: "ロードマップ" },
        slug: "roadmap",
      },
    ],
  },
  {
    label: "Specification",
    translations: { ja: "仕様書" },
    items: [
      { label: "Overview", translations: { ja: "目次" }, slug: "spec" },
      {
        label: "1. Purpose and Scope",
        translations: { ja: "1. 目的とスコープ" },
        slug: "spec/overview",
      },
      {
        label: "2. Design Principles",
        translations: { ja: "2. 設計原則" },
        slug: "spec/principles",
      },
      {
        label: "3. Architecture",
        translations: { ja: "3. アーキテクチャ" },
        slug: "spec/architecture",
      },
      {
        label: "4. Compilation Model",
        translations: { ja: "4. コンパイルモデル" },
        slug: "spec/compilation",
      },
      {
        label: "5. Syntax",
        translations: { ja: "5. 構文" },
        slug: "spec/syntax",
      },
      {
        label: "6. Blockstate Model",
        translations: { ja: "6. ブロックステート" },
        slug: "spec/blockstate",
      },
      {
        label: "7. Materials and Themes",
        translations: { ja: "7. マテリアルとテーマ" },
        slug: "spec/materials-themes",
      },
      {
        label: "8. Entities",
        translations: { ja: "8. エンティティ" },
        slug: "spec/entities",
      },
      {
        label: "9. Components, Editing, Sites",
        translations: { ja: "9. コンポーネント・編集・サイト" },
        slug: "spec/components-editing-sites",
      },
      {
        label: "10. Versioning and Editions",
        translations: { ja: "10. バージョンとエディション" },
        slug: "spec/versioning-editions",
      },
      { label: "11. Lint", translations: { ja: "11. Lint" }, slug: "spec/lint" },
      {
        label: "12. Ecosystem Interop",
        translations: { ja: "12. エコシステム連携" },
        slug: "spec/ecosystem-interop",
      },
      {
        label: "13. Evaluation Framework",
        translations: { ja: "13. 評価フレームワーク" },
        slug: "spec/evaluation",
      },
      {
        label: "14. Redstone",
        translations: { ja: "14. レッドストーン" },
        slug: "spec/redstone",
      },
      {
        label: "15. Open Issues",
        translations: { ja: "15. 未決事項" },
        slug: "spec/open-issues",
      },
      {
        label: "Compatibility Tiers",
        translations: { ja: "互換性ティア" },
        slug: "spec/compatibility",
      },
      {
        label: "Glossary",
        translations: { ja: "用語集" },
        slug: "spec/glossary",
      },
    ],
  },
  {
    label: "Implementation",
    translations: { ja: "実装" },
    items: [
      {
        label: "Developer Guide",
        translations: { ja: "開発者ガイド" },
        slug: "development",
      },
    ],
  },
  {
    label: "Try it",
    translations: { ja: "試す" },
    items: [
      {
        label: "Playground",
        translations: { ja: "プレイグラウンド" },
        slug: "playground",
      },
    ],
  },
];

export default defineConfig({
  site: "https://cairn.kage1020.com",
  integrations: [
    starlight({
      title: "Cairn",
      description:
        "A description language for Minecraft builds. Declare intent — walls, roofs, windows, redstone — and the compiler resolves the voxels.",
      favicon: "/favicon.svg",
      social: [{ icon: "github", label: "GitHub", href: githubRepo }],
      editLink: { baseUrl: `${githubRepo}/edit/main/website/src/content/docs/` },
      defaultLocale: "root",
      locales: {
        root: { label: "English", lang: "en" },
        ja: { label: "日本語", lang: "ja" },
      },
      sidebar,
      customCss: ["./src/styles/cairn.css"],
    }),
  ],
});
