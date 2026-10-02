import assert from "node:assert/strict";
import { describe, test } from "node:test";

import { buildGrammarIndex, type EbnfKind, ruleTarget, tokenizeEbnf } from "../src/ebnf.ts";
import { createMarkdown, type RenderEnv } from "../src/markdown.ts";

const SAMPLE = `(* A trait declaration. *)
trait_decl = "trait", identifier, [ type_params ],
             ( NEWLINE | ':', NEWLINE, INDENT, trait_member, { trait_member }, DEDENT ) ;
trait_member = ? any member ? | "0" ... "9" ;`;

const significant = (code: string): [string, EbnfKind][] =>
  tokenizeEbnf(code)
    .filter((span) => span.kind !== "plain")
    .map((span) => [span.text, span.kind]);

describe("EBNF tokenizer", () => {
  test("classifies every token of a sample rule", () => {
    const spans = significant(SAMPLE);
    assert.deepEqual(spans.slice(0, 12), [
      ["(* A trait declaration. *)", "comment"],
      ["trait_decl", "definition"],
      ["=", "operator"],
      ['"trait"', "terminal"],
      [",", "operator"],
      ["identifier", "reference"],
      [",", "operator"],
      ["[", "operator"],
      ["type_params", "reference"],
      ["]", "operator"],
      [",", "operator"],
      ["(", "operator"],
    ]);
    const kinds = new Map(spans);
    assert.equal(kinds.get("NEWLINE"), "token");
    assert.equal(kinds.get("INDENT"), "token");
    assert.equal(kinds.get("DEDENT"), "token");
    assert.equal(kinds.get("':'"), "terminal");
    assert.equal(kinds.get("|"), "operator");
    assert.equal(kinds.get(";"), "operator");
    assert.equal(kinds.get("trait_member"), "definition");
    assert.equal(kinds.get("? any member ?"), "special");
    assert.equal(kinds.get("..."), "operator");
  });

  test("keeps the source text exactly", () => {
    assert.equal(
      tokenizeEbnf(SAMPLE)
        .map((span) => span.text)
        .join(""),
      SAMPLE,
    );
  });

  test("a name defines a rule only at a rule start, before `=`", () => {
    assert.deepEqual(significant("a = b ;\nc = d, e = f ;"), [
      ["a", "definition"],
      ["=", "operator"],
      ["b", "reference"],
      [";", "operator"],
      ["c", "definition"],
      ["=", "operator"],
      ["d", "reference"],
      [",", "operator"],
      ["e", "reference"],
      ["=", "operator"],
      ["f", "reference"],
      [";", "operator"],
    ]);
    assert.deepEqual(significant("DIGIT (* c *) = '0' ;").slice(0, 2), [
      ["DIGIT", "definition"],
      ["(* c *)", "comment"],
    ]);
  });

  test("quoted terminals hold the other quote and operator characters", () => {
    assert.deepEqual(significant(`q = '"', "'", "(*", "?" ;`), [
      ["q", "definition"],
      ["=", "operator"],
      [`'"'`, "terminal"],
      [",", "operator"],
      [`"'"`, "terminal"],
      [",", "operator"],
      ['"(*"', "terminal"],
      [",", "operator"],
      ['"?"', "terminal"],
      [";", "operator"],
    ]);
  });
});

describe("grammar index", () => {
  const index = buildGrammarIndex(
    [
      { source: "spec/lang/01-lexical.md", blocks: [{ code: "identifier = LETTER ;", line: 3 }] },
      {
        source: "spec/lang/02-grammar.md",
        blocks: [
          { code: "trait_decl = identifier, trait_member ;\ntrait_member = missing ;", line: 10 },
        ],
      },
      {
        source: "spec/lang/09-traits.md",
        blocks: [{ code: "trait_decl = identifier ;", line: 1 }],
      },
      {
        source: "spec/lang/11-chapter.md",
        blocks: [{ code: "uses = trait_decl, NEWLINE ;", line: 5 }],
      },
    ],
    "spec/lang/02-grammar.md",
  );

  test("indexes definitions and reports names no rule defines", () => {
    assert.deepEqual(
      [...index.definitions.keys()],
      ["identifier", "trait_decl", "trait_member", "uses"],
    );
    assert.deepEqual(index.definitions.get("trait_decl"), [
      "spec/lang/02-grammar.md",
      "spec/lang/09-traits.md",
    ]);
    assert.equal(index.definitionCount, 5);
    assert.equal(index.referenceCount, 7);
    assert.equal(index.linkedCount, 4);
    assert.deepEqual(index.unresolved, [
      { name: "missing", source: "spec/lang/02-grammar.md", line: 12 },
    ]);
    assert.deepEqual(index.abstractTokens, ["LETTER", "NEWLINE"]);
  });

  test("links a use to its own page's definition, else the canonical one", () => {
    assert.equal(
      ruleTarget(index, "trait_decl", "spec/lang/09-traits.md"),
      "spec/lang/09-traits.md",
    );
    assert.equal(
      ruleTarget(index, "trait_decl", "spec/lang/11-chapter.md"),
      "spec/lang/02-grammar.md",
    );
    assert.equal(ruleTarget(index, "missing", "spec/lang/02-grammar.md"), undefined);
  });

  test("renders anchors on definitions and links on references", () => {
    const render = (source: string, code: string): string => {
      const env: RenderEnv = {
        source,
        resolveLink: (href, from) => `/site/${from}|${href}`,
        playgroundUrl: () => "",
        headings: [],
        slugCounts: new Map(),
        grammar: index,
      };
      return createMarkdown().render(`\`\`\`ebnf\n${code}\n\`\`\`\n`, env);
    };
    const grammar = render(
      "spec/lang/02-grammar.md",
      "trait_decl = identifier, trait_member ;\ntrait_member = missing ;",
    );
    assert.match(grammar, /<pre class="code ebnf"><code class="language-ebnf">/);
    assert.match(
      grammar,
      /<a class="eb-definition" href="#rule-trait_decl" id="rule-trait_decl">trait_decl<\/a>/,
    );
    assert.match(grammar, /<a class="eb-reference" href="#rule-trait_member">trait_member<\/a>/);
    assert.match(
      grammar,
      /<a class="eb-reference" href="\/site\/spec\/lang\/02-grammar.md\|01-lexical.md#rule-identifier">identifier<\/a>/,
    );
    assert.match(grammar, /<span class="eb-reference">missing<\/span>/);
    assert.match(grammar, /<span class="eb-operator">=<\/span>/);

    const chapter = render("spec/lang/11-chapter.md", "uses = trait_decl, NEWLINE ;");
    assert.match(
      chapter,
      /href="\/site\/spec\/lang\/11-chapter.md\|02-grammar.md#rule-trait_decl">trait_decl</,
    );
    assert.match(chapter, /<span class="eb-token">NEWLINE<\/span>/);

    const restated = render("spec/lang/09-traits.md", "trait_decl = identifier ;");
    assert.match(
      restated,
      /<a class="eb-definition" href="\/site\/spec\/lang\/09-traits.md\|02-grammar.md#rule-trait_decl" id="rule-trait_decl"/,
    );
  });
});
