type BaseNode = { type: string; named: boolean };
type ChildNode = { multiple: boolean; required: boolean; types: BaseNode[] };
type NodeInfo =
  | (BaseNode & { subtypes: BaseNode[] })
  | (BaseNode & { fields: { [name: string]: ChildNode }; children: ChildNode[] });

declare const cairn: {
  name: "cairn";
  language: unknown;
  nodeTypeInfo?: NodeInfo[];
};

export = cairn;
