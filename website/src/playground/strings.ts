/** A count's text, by whether the count is one. */
export interface Plural {
  one: string;
  other: string;
}

/** The playground's interface text in one locale. */
export interface Strings {
  editor: string;
  editorHint: string;
  example: string;
  edition: string;
  target: string;
  latest: string;
  loading: string;
  compiling: string;
  built: string;
  refused: string;
  unavailable: string;
  unavailableLink: string;
  crashed: string;
  findings: string;
  noFindings: string;
  errors: Plural;
  warnings: Plural;
  and: string;
  result: string;
  noResult: string;
  structure: string;
  layers: string;
  rotate: string;
  palette: string;
  cubes: string;
  ir: string;
  downloads: string;
  noDownloads: string;
}

export type Lang = "en" | "ja";

export const strings: Record<Lang, Strings> = {
  en: {
    editor: "Cairn source",
    editorHint: "Tab indents. To leave the editor with the keyboard, press Esc, then Tab.",
    example: "Example",
    edition: "Edition",
    target: "Target",
    latest: "latest ({version})",
    loading: "Loading the compiler…",
    compiling: "Compiling…",
    built: "Built with cairn {version}.",
    refused: "Not built: the findings say why.",
    unavailable:
      "This copy of the site was built without the compiler module, so nothing on this page compiles. Building it takes Rust and wasm-pack:",
    unavailableLink: "see the developer guide.",
    crashed:
      "The compiler stopped on this source and has been restarted. That is a bug in Cairn: please report it with the source.",
    findings: "Findings",
    noFindings: "No findings.",
    errors: { one: "{n} error", other: "{n} errors" },
    warnings: { one: "{n} warning", other: "{n} warnings" },
    and: ", ",
    result: "Result",
    noResult: "The view appears once no finding is an error.",
    structure: "Structure",
    layers: "Layers",
    rotate: "Rotate",
    palette: "Palette",
    cubes: "Every block is drawn as a full cube; the palette lists its states.",
    ir: "Block-array IR (JSON)",
    downloads: "Structure files",
    noDownloads: "A build with no error writes them here.",
  },
  ja: {
    editor: "Cairn のソース",
    editorHint:
      "Tab でインデントします。キーボードでエディタを離れるには、Esc を押してから Tab を押します。",
    example: "サンプル",
    edition: "エディション",
    target: "ターゲット",
    latest: "latest ({version})",
    loading: "コンパイラを読み込んでいます…",
    compiling: "コンパイルしています…",
    built: "cairn {version} でビルドしました。",
    refused: "ビルドしていません。理由は指摘のとおりです。",
    unavailable:
      "このサイトはコンパイラのモジュールなしでビルドされているため、このページでは何もコンパイルできません。モジュールのビルドには Rust と wasm-pack が必要です。",
    unavailableLink: "開発者ガイドを参照してください。",
    crashed:
      "このソースでコンパイラが停止したため、再起動しました。Cairn の不具合なので、ソースを添えて報告してください。",
    findings: "指摘",
    noFindings: "指摘はありません。",
    errors: { one: "エラー {n} 件", other: "エラー {n} 件" },
    warnings: { one: "警告 {n} 件", other: "警告 {n} 件" },
    and: "、",
    result: "結果",
    noResult: "エラーの指摘がなくなると、ここに表示します。",
    structure: "構造物",
    layers: "表示する層",
    rotate: "回転",
    palette: "パレット",
    cubes: "ブロックはすべて立方体で描きます。状態はパレットに示します。",
    ir: "Block-array IR (JSON)",
    downloads: "構造物ファイル",
    noDownloads: "エラーのないビルドで、ここに書き出します。",
  },
};

/** `template` with each `{name}` replaced by `values[name]`. */
export function fill(template: string, values: Record<string, string | number>): string {
  return template.replace(/\{(\w+)\}/g, (_, name: string) => String(values[name] ?? ""));
}

/** `plural`'s text for `n`, with the count put in. */
export function count(plural: Plural, n: number): string {
  return fill(n === 1 ? plural.one : plural.other, { n });
}
