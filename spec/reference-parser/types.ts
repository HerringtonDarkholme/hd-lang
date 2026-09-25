export interface Diagnostic {
  readonly code: string;
  readonly line: number;
}

export interface GrammarToken {
  readonly kinds: ReadonlySet<string>;
  readonly line: number;
  readonly text: string;
}

export interface ChartItem {
  readonly dot: number;
  readonly lhs: string;
  readonly origin: number;
  readonly rhs: readonly string[];
}

export interface EbnfToken {
  readonly kind: string;
  readonly value: string;
}

export interface EbnfNode {
  readonly kind:
    | "alternative"
    | "group"
    | "literal"
    | "optional"
    | "repeat"
    | "sequence"
    | "symbol";
  readonly children?: readonly EbnfNode[];
  readonly value?: string;
}

export type Grammar = ReadonlyMap<string, readonly (readonly string[])[]>;

export interface LexResult {
  readonly diagnostics: Diagnostic[];
  readonly tokens: GrammarToken[];
}

export interface ParseResult {
  readonly accepted: boolean;
  readonly farthest: number;
}
