#!/usr/bin/env python3
"""Parse every conformance fixture with the EBNF in ``02-grammar.md``.

The script extracts the chapter's EBNF blocks, expands EBNF option/repetition
nodes to BNF, tokenizes source with the chapter-01 layout model, and recognizes
the resulting token stream with an Earley chart.  A small final pass implements
the explicitly context-sensitive syntax restrictions stated beside the grammar
(for example, decorators are top-level and comparisons do not chain).  It is
not a type checker, so typing-invalid fixtures must still parse.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import ast
import re
import sys


@dataclass(frozen=True)
class Diagnostic:
    code: str
    line: int


@dataclass(frozen=True)
class Token:
    kinds: frozenset[str]
    line: int
    text: str


@dataclass(frozen=True)
class ChartItem:
    lhs: str
    rhs: tuple[str, ...]
    dot: int
    origin: int


@dataclass(frozen=True)
class ENode:
    kind: str
    value: object


RESERVED = {
    "Self", "and", "annotate", "as", "break", "continue", "data", "defer", "else",
    "enum", "false", "fn", "for", "if", "impl", "in", "is", "let",
    "match", "mut", "nil", "not", "or", "pass", "pub", "reified",
    "return", "self", "shape", "super", "trait", "true", "type", "use",
    "where", "while",
}
OPEN_TO_CLOSE = {"(": ")", "[": "]", "{": "}"}
CLOSE_TO_OPEN = {value: key for key, value in OPEN_TO_CLOSE.items()}
MULTI_OPERATORS = ("...", ":=", "->", "=>", "::", "==", "!=", "<=", ">=", "<<", ">>", "**")
VALID_ESCAPES = set("0nrt\\\"'$ {}")


class EbnfReader:
    """Read the deliberately small EBNF notation used by chapter 02."""

    def __init__(self, text: str):
        self.tokens = self._lex(text)
        self.index = 0

    @staticmethod
    def _lex(text: str) -> list[tuple[str, str]]:
        result: list[tuple[str, str]] = []
        index = 0
        while index < len(text):
            ch = text[index]
            if ch.isspace():
                index += 1
                continue
            if ch.isalpha() or ch == "_":
                end = index + 1
                while end < len(text) and (text[end].isalnum() or text[end] == "_"):
                    end += 1
                result.append(("name", text[index:end]))
                index = end
                continue
            if ch in "\"'":
                quote = ch
                end = index + 1
                while end < len(text):
                    if text[end] == "\\":
                        end += 2
                    elif text[end] == quote:
                        end += 1
                        break
                    else:
                        end += 1
                if end > len(text) or text[end - 1] != quote:
                    raise ValueError("unterminated EBNF literal")
                literal = text[index:end]
                try:
                    value = ast.literal_eval(literal)
                except (SyntaxError, ValueError):
                    value = literal[1:-1]
                result.append(("literal", value))
                index = end
                continue
            if ch in "=;|,()[]{}":
                result.append((ch, ch))
                index += 1
                continue
            raise ValueError(f"unexpected EBNF character {ch!r}")
        return result

    def peek(self, kind: str | None = None) -> tuple[str, str] | None:
        if self.index >= len(self.tokens):
            return None
        token = self.tokens[self.index]
        return token if kind is None or token[0] == kind else None

    def take(self, kind: str) -> str:
        token = self.peek(kind)
        if token is None:
            actual = self.peek()
            raise ValueError(f"expected EBNF {kind}, got {actual}")
        self.index += 1
        return token[1]

    def productions(self) -> dict[str, ENode]:
        result: dict[str, ENode] = {}
        while self.peek() is not None:
            name = self.take("name")
            self.take("=")
            result[name] = self.alternatives({";"})
            self.take(";")
        return result

    def alternatives(self, end: set[str]) -> ENode:
        choices = [self.sequence(end | {"|"})]
        while self.peek("|"):
            self.take("|")
            choices.append(self.sequence(end | {"|"}))
        return choices[0] if len(choices) == 1 else ENode("alt", tuple(choices))

    def sequence(self, end: set[str]) -> ENode:
        parts = [self.factor()]
        while self.peek(","):
            self.take(",")
            parts.append(self.factor())
        if self.peek() is not None and self.peek()[0] not in end:
            raise ValueError(f"missing EBNF comma before {self.peek()}")
        return parts[0] if len(parts) == 1 else ENode("seq", tuple(parts))

    def factor(self) -> ENode:
        token = self.peek()
        if token is None:
            raise ValueError("unexpected end of EBNF")
        kind, value = token
        if kind == "name":
            self.index += 1
            return ENode("sym", value)
        if kind == "literal":
            self.index += 1
            return ENode("lit", value)
        wrappers = {"(": (")", "group"), "[": ("]", "opt"), "{": ("}", "rep")}
        if kind in wrappers:
            close, node_kind = wrappers[kind]
            self.index += 1
            child = self.alternatives({close})
            self.take(close)
            return ENode(node_kind, child)
        raise ValueError(f"unexpected EBNF token {token}")


class GrammarCompiler:
    def __init__(self):
        self.rules: dict[str, list[tuple[str, ...]]] = {}
        self.counter = 0

    def helper(self, prefix: str) -> str:
        self.counter += 1
        return f"@{prefix}_{self.counter}"

    def compile_node(self, node: ENode) -> list[tuple[str, ...]]:
        if node.kind in {"sym", "lit"}:
            return [(str(node.value),)]
        if node.kind == "alt":
            result: list[tuple[str, ...]] = []
            for child in node.value:  # type: ignore[union-attr]
                result.extend(self.compile_node(child))
            return result
        if node.kind == "seq":
            result = [()]
            for child in node.value:  # type: ignore[union-attr]
                additions = self.compile_node(child)
                result = [left + right for left in result for right in additions]
            return result
        if node.kind == "group":
            return self.compile_node(node.value)  # type: ignore[arg-type]
        if node.kind == "opt":
            name = self.helper("optional")
            self.rules[name] = [()] + self.compile_node(node.value)  # type: ignore[arg-type]
            return [(name,)]
        if node.kind == "rep":
            name = self.helper("repeat")
            self.rules[name] = [()]
            self.rules[name].extend(
                rhs + (name,) for rhs in self.compile_node(node.value)  # type: ignore[arg-type]
            )
            return [(name,)]
        raise AssertionError(node.kind)

    def compile(self, productions: dict[str, ENode]) -> dict[str, tuple[tuple[str, ...], ...]]:
        for name, node in productions.items():
            self.rules[name] = self.compile_node(node)
        # Strings are tokens from the chapter-01 lexer. Their interpolation is
        # lexically validated before this syntactic grammar is entered.
        self.rules["string_expression"] = [("string_literal",)]
        return {
            name: tuple(dict.fromkeys(rhs_list))
            for name, rhs_list in self.rules.items()
        }


def chapter_grammar() -> dict[str, tuple[tuple[str, ...], ...]]:
    chapter = Path(__file__).with_name("02-grammar.md").read_text(encoding="utf-8")
    blocks = re.findall(r"^```ebnf\s*\n(.*?)^```", chapter, re.MULTILINE | re.DOTALL)
    reader = EbnfReader("\n".join(blocks))
    return GrammarCompiler().compile(reader.productions())


GRAMMAR = chapter_grammar()


def token(kinds: str | set[str], line: int, text: str) -> Token:
    values = {kinds} if isinstance(kinds, str) else kinds
    return Token(frozenset(values), line, text)


def scan_string(source: str, start: int, line: int) -> tuple[int, int, str, list[Diagnostic]]:
    diagnostics: list[Diagnostic] = []
    raw = source[start] == "r"
    quote_at = start + 1 if raw else start
    quote = source[quote_at]
    triple = source.startswith(quote * 3, quote_at)
    terminator = quote * (3 if triple else 1)
    index = quote_at + len(terminator)
    start_line = line
    while index < len(source):
        if source.startswith(terminator, index):
            return index + len(terminator), line, quote, diagnostics
        ch = source[index]
        if ch == "\n":
            if not triple:
                diagnostics.append(Diagnostic("unterminated-string", start_line))
                return index, line, quote, diagnostics
            line += 1
            index += 1
            continue
        if ch == "\\" and not raw:
            if index + 1 >= len(source) or source[index + 1] == "\n":
                diagnostics.append(Diagnostic("invalid-escape", line))
                index += 1
                continue
            if source[index + 1] not in VALID_ESCAPES:
                diagnostics.append(Diagnostic("invalid-escape", line))
            index += 2
            continue
        index += 1
    diagnostics.append(Diagnostic("unterminated-string", start_line))
    return index, line, quote, diagnostics


def number_end(source: str, start: int, after_dot: bool) -> tuple[int, bool]:
    rest = source[start:]
    based = re.match(r"0[xX][0-9A-Fa-f_]+|0[bB][01_]+|0[oO][0-7_]+", rest)
    if based:
        return start + based.end(), False
    integer = re.match(r"[0-9][0-9_]*", rest)
    assert integer is not None
    end = start + integer.end()
    floating = False
    if not after_dot and end < len(source) and source[end] == "." and end + 1 < len(source) and source[end + 1].isdigit():
        floating = True
        end += 1
        fraction = re.match(r"[0-9][0-9_]*", source[end:])
        assert fraction is not None
        end += fraction.end()
    if end < len(source) and source[end] in "eE":
        exponent = re.match(r"[eE][+-]?[0-9][0-9_]*", source[end:])
        if exponent:
            floating = True
            end += exponent.end()
    return end, floating


def lex_source(source: str) -> tuple[list[Token], list[Diagnostic]]:
    tokens: list[Token] = []
    diagnostics: list[Diagnostic] = []
    indents = [0]
    delimiters: list[tuple[str, int]] = []
    index = 0
    line = 1
    at_line_start = True
    line_has_token = False
    previous_text = ""
    pending_headers: list[tuple[str, int]] = []
    inline_suites: list[int] = []
    pending_forced_suite: tuple[int, int] | None = None
    forced_indents: list[tuple[int, int]] = []

    while index < len(source):
        if at_line_start:
            start = index
            indent = 0
            while index < len(source) and source[index] in " \t":
                if source[index] == "\t":
                    diagnostics.append(Diagnostic("tab-whitespace", line))
                    indent += 4
                else:
                    indent += 1
                index += 1
            if index >= len(source):
                break
            if source[index] == "#" or source[index] == "\n":
                while index < len(source) and source[index] != "\n":
                    index += 1
                if index < len(source):
                    index += 1
                    line += 1
                at_line_start = True
                continue
            if delimiters:
                if pending_forced_suite is not None:
                    _, header_indent = pending_forced_suite
                    if indent <= header_indent:
                        diagnostics.append(Diagnostic("unexpected-indentation", line))
                    forced_indents.append((indent, len(delimiters)))
                    tokens.append(token("INDENT", line, "<indent>"))
                    pending_forced_suite = None
                elif forced_indents:
                    while forced_indents and indent < forced_indents[-1][0]:
                        forced_indents.pop()
                        tokens.append(token("DEDENT", line, "<dedent>"))
            else:
                if indent > indents[-1]:
                    indents.append(indent)
                    tokens.append(token("INDENT", line, "<indent>"))
                elif indent < indents[-1]:
                    while len(indents) > 1 and indent < indents[-1]:
                        indents.pop()
                        tokens.append(token("DEDENT", line, "<dedent>"))
                    if indent != indents[-1]:
                        diagnostics.append(Diagnostic("invalid-dedent", line))
            at_line_start = False
            if index == start and source[index] == "\ufeff" and index == 0:
                index += 1
                continue

        if index >= len(source):
            break
        ch = source[index]
        if ch == "\ufeff":
            if index != 0:
                diagnostics.append(Diagnostic("unexpected-bom", line))
            index += 1
            continue
        if ch in " \r\t":
            index += 1
            continue
        if ch == "#":
            while index < len(source) and source[index] != "\n":
                index += 1
            continue
        if ch == "\n":
            if delimiters and pending_forced_suite is None and not forced_indents:
                line += 1
                index += 1
                at_line_start = False
                continue
            if inline_suites:
                for _ in inline_suites:
                    tokens.append(token("SUITE_END", line, "<suite-end>"))
                inline_suites.clear()
            elif line_has_token:
                tokens.append(token({"NEWLINE", "SUITE_END"}, line, "<newline>"))
            line += 1
            index += 1
            at_line_start = True
            line_has_token = False
            previous_text = ""
            pending_headers.clear()
            continue

        raw_string = ch == "r" and index + 1 < len(source) and source[index + 1] in "\"'"
        if ch in "\"'" or raw_string:
            end, new_line, quote, found = scan_string(source, index, line)
            diagnostics.extend(found)
            text = source[index:end]
            kind = "char_literal" if quote == "'" and not raw_string else "string_literal"
            tokens.append(token(kind, line, text))
            line = new_line
            index = end
            line_has_token = True
            previous_text = text
            continue

        if ch == "_" and (index + 1 == len(source) or not (source[index + 1].isalnum() or source[index + 1] == "_")):
            tokens.append(token("_", line, ch))
            index += 1
            line_has_token = True
            previous_text = ch
            continue
        if ch == "_" or ch.isalpha():
            end = index + 1
            while end < len(source) and (source[end] == "_" or source[end].isalnum()):
                end += 1
            word = source[index:end]
            depth = len(delimiters)
            if word == "else" and inline_suites and inline_suites[-1] == depth:
                inline_suites.pop()
                tokens.append(token("SUITE_END", line, "<suite-end>"))
            if word in {"true", "false"}:
                kinds = {word, "boolean_literal"}
            elif word == "nil":
                kinds = {word, "nil_literal"}
            elif word in RESERVED:
                kinds = {word}
            else:
                kinds = {word, "identifier"}
            tokens.append(token(kinds, line, word))
            suite_word = word in {"fn", "if", "while", "match", "else", "data", "test", "annotate", "with"}
            if word == "for":
                suite_word = previous_text in {"", ":=", "=", "return", "break", ":", "=>", "(", ","}
            if suite_word:
                pending_headers.append((word, depth))
            index = end
            line_has_token = True
            previous_text = word
            continue
        if ch.isdigit():
            end, floating = number_end(source, index, previous_text == ".")
            text = source[index:end]
            tokens.append(token("float_literal" if floating else "integer_literal", line, text))
            index = end
            line_has_token = True
            previous_text = text
            continue

        operator = next((value for value in MULTI_OPERATORS if source.startswith(value, index)), None)
        text = operator or ch
        if not operator and ch not in "()[]{}.,:+-*/%~!?&|^<>=@$;":
            diagnostics.append(Diagnostic("invalid-token", line))
            index += 1
            continue
        depth = len(delimiters)
        if text in {",", ")", "]", "}"}:
            while inline_suites and inline_suites[-1] >= depth:
                inline_suites.pop()
                tokens.append(token("SUITE_END", line, "<suite-end>"))
            pending_headers = [entry for entry in pending_headers if entry[1] < depth]
        tokens.append(token(text, line, text))
        index += len(text)
        line_has_token = True
        previous_text = text
        if text in OPEN_TO_CLOSE:
            delimiters.append((text, line))
        elif text in CLOSE_TO_OPEN:
            if not delimiters or delimiters[-1][0] != CLOSE_TO_OPEN[text]:
                diagnostics.append(Diagnostic("unmatched-delimiter", line))
            else:
                delimiters.pop()
        if text == "=>":
            pending_headers = [entry for entry in pending_headers if entry[1] != depth]
        elif text == ":":
            match = next(
                (position for position in range(len(pending_headers) - 1, -1, -1) if pending_headers[position][1] == depth),
                None,
            )
            look = index
            while look < len(source) and source[look] in " \t\r":
                look += 1
            if match is not None:
                if look < len(source) and source[look] not in "\n#":
                    inline_suites.append(depth)
                elif delimiters:
                    line_start = source.rfind("\n", 0, index) + 1
                    header_indent = len(source[line_start:]) - len(source[line_start:].lstrip(" "))
                    pending_forced_suite = (depth, header_indent)
                del pending_headers[match:]

    if inline_suites:
        for _ in inline_suites:
            tokens.append(token("SUITE_END", line, "<suite-end>"))
    elif line_has_token:
        tokens.append(token({"NEWLINE", "SUITE_END"}, line, "<newline>"))
    diagnostics.extend(Diagnostic("unclosed-delimiter", at) for _, at in delimiters)
    while forced_indents:
        forced_indents.pop()
        tokens.append(token("DEDENT", line + 1, "<dedent>"))
    while len(indents) > 1:
        indents.pop()
        tokens.append(token("DEDENT", line + 1, "<dedent>"))
    tokens.append(token("EOF", line + 1, "<eof>"))
    return tokens, diagnostics


def earley_accepts(tokens: list[Token]) -> tuple[bool, int]:
    chart: list[set[ChartItem]] = [set() for _ in range(len(tokens) + 1)]
    chart[0].add(ChartItem("@root", ("source_file",), 0, 0))
    nonterminals = set(GRAMMAR)
    farthest = 0

    for position in range(len(chart)):
        agenda = list(chart[position])
        cursor = 0
        while cursor < len(agenda):
            item = agenda[cursor]
            cursor += 1
            if item.dot < len(item.rhs):
                symbol = item.rhs[item.dot]
                if symbol in nonterminals:
                    for rhs in GRAMMAR[symbol]:
                        predicted = ChartItem(symbol, rhs, 0, position)
                        if predicted not in chart[position]:
                            chart[position].add(predicted)
                            agenda.append(predicted)
            else:
                for waiting in tuple(chart[item.origin]):
                    if waiting.dot < len(waiting.rhs) and waiting.rhs[waiting.dot] == item.lhs:
                        completed = ChartItem(waiting.lhs, waiting.rhs, waiting.dot + 1, waiting.origin)
                        if completed not in chart[position]:
                            chart[position].add(completed)
                            agenda.append(completed)

        if position < len(tokens):
            for item in chart[position]:
                if item.dot < len(item.rhs) and item.rhs[item.dot] in tokens[position].kinds:
                    chart[position + 1].add(ChartItem(item.lhs, item.rhs, item.dot + 1, item.origin))
            if chart[position + 1]:
                farthest = position + 1

    accepted = ChartItem("@root", ("source_file",), 1, 0) in chart[len(tokens)]
    return accepted, farthest


def mask_string_literals(source: str) -> str:
    """Hide string contents from contextual regex checks while preserving lines."""

    masked = re.sub(r'"(?:\\.|[^"\\])*"', '""', source)
    return re.sub(r"'(?:\\.|[^'\\])*'", "''", masked)


def line_records(source: str) -> list[tuple[int, int, str, str]]:
    records = []
    for number, original in enumerate(source.splitlines(), 1):
        indent = len(original) - len(original.lstrip(" "))
        clean = re.sub(r"#.*", "", mask_string_literals(original)).strip()
        records.append((number, indent, original, clean))
    return records


def split_top_level(text: str) -> list[str]:
    parts: list[str] = []
    stack: list[str] = []
    start = 0
    for index, ch in enumerate(text):
        if ch in OPEN_TO_CLOSE:
            stack.append(ch)
        elif ch in CLOSE_TO_OPEN and stack and stack[-1] == CLOSE_TO_OPEN[ch]:
            stack.pop()
        elif ch == "," and not stack:
            parts.append(text[start:index].strip())
            start = index + 1
    parts.append(text[start:].strip())
    return parts


def argument_order_diagnostics(source: str) -> list[Diagnostic]:
    diagnostics: list[Diagnostic] = []
    masked = mask_string_literals(source)
    stack: list[tuple[int, int]] = []
    line = 1
    for index, ch in enumerate(masked):
        if ch == "\n":
            line += 1
        elif ch == "(":
            stack.append((index, line))
        elif ch == ")" and stack:
            start, start_line = stack.pop()
            content = masked[start + 1 : index]
            named_seen = False
            for part in split_top_level(content):
                if not part:
                    continue
                is_named = re.match(r"^[^=,:]+=(?!=)", part) is not None
                if is_named:
                    named_seen = True
                elif named_seen and not part.startswith(("fn ", "mut fn ")):
                    line_text = masked.splitlines()[start_line - 1]
                    code = "pattern-order" if "=>" in line_text else "argument-order"
                    diagnostics.append(Diagnostic(code, start_line))
                    break
    return diagnostics


def context_diagnostics(source: str) -> list[Diagnostic]:
    diagnostics: list[Diagnostic] = []
    records = line_records(source)
    context: list[tuple[int, str]] = []
    for line, indent, original, clean in records:
        if not clean:
            continue
        while context and indent <= context[-1][0]:
            context.pop()
        parent = context[-1][1] if context else "module"
        if ";" in clean:
            diagnostics.append(Diagnostic("reserved-semicolon", line))
        if re.match(r"^struct\b", clean):
            diagnostics.append(Diagnostic("old-struct-declaration", line))
        if re.match(r"^import\b", clean):
            diagnostics.append(Diagnostic("old-import-declaration", line))
        if re.match(r"^export\b", clean):
            diagnostics.append(Diagnostic("old-export-declaration", line))
        if re.match(r"^use\b.*\{[^}]*\.[A-Za-z_]", clean):
            diagnostics.append(Diagnostic("direct-variant-use", line))
        if re.search(r"\bfn\s+\w+[^\n]*\([^)]*\bshape\s*:", clean):
            diagnostics.append(Diagnostic("reserved-name", line))
        if re.search(r"\b[\w.]+\s*(?:<=|>=|==|!=|<|>)\s*[\w.]+\s*(?:<=|>=|==|!=|<|>)\s*[\w.]+", clean):
            diagnostics.append(Diagnostic("comparison-chaining", line))
        if re.search(r"\[[^]]*,\s*[^],]+\s*:=", clean):
            diagnostics.append(Diagnostic("multi-binding-needs-parentheses", line))
        if clean == ":" or re.search(r"\[[^]]*\w+\s*:\s*$", clean):
            diagnostics.append(Diagnostic("trailing-block-position", line))
        if clean.startswith("@") and parent not in {"module", "data", "enum"}:
            diagnostics.append(Diagnostic("decorator-not-top-level", line))
        if parent == "data" and re.match(r"^mut\s+[A-Z]\w*\s*$", clean):
            diagnostics.append(Diagnostic("mutable-embedded-field", line))
        if parent == "data" and re.match(r"^mut\s+\w+\s*:", clean):
            diagnostics.append(Diagnostic("mutable-field-modifier", line))
        if parent == "trait" and re.match(r"^pub\s+fn\b", clean):
            diagnostics.append(Diagnostic("trait-method-visibility", line))
        first_word = clean.split(maxsplit=1)[0].removesuffix(":")
        if (
            parent not in {"data", "enum", "trait", "impl"}
            and first_word not in RESERVED
            and re.match(r"^[^\W\d]\w*\s*:\s*[^=]+\s*=", clean, re.UNICODE)
        ):
            diagnostics.append(Diagnostic("missing-let", line))

        kind = None
        if re.match(r"^(?:pub\s+)?data\b", clean):
            kind = "data"
        elif re.match(r"^(?:pub\s+)?enum\b", clean):
            kind = "enum"
        elif re.match(r"^(?:pub\s+)?trait\b", clean):
            kind = "trait"
        elif re.match(r"^(?:pub\s+)?impl\b", clean):
            kind = "impl"
        elif re.match(r"^(?:pub\s+)?fn\b", clean):
            kind = "function"
        elif clean.startswith("test "):
            kind = "test"
        if kind is not None and clean.endswith(":"):
            context.append((indent, kind))

    for index, (line, indent, original, _) in enumerate(records):
        if not original.lstrip().startswith("##"):
            continue
        follower = index + 1
        while follower < len(records) and records[follower][2].lstrip().startswith("##"):
            follower += 1
        if follower >= len(records):
            diagnostics.append(Diagnostic("doc-comment-without-target", line))
            continue
        _, next_indent, _, next_clean = records[follower]
        declaration = re.match(
            r"^(?:pub\s+)?(?:data|enum|trait|impl|type|fn)\b|^@|^\w+\s*(?::|\()",
            next_clean,
        )
        if next_indent != indent or not declaration or ":=" in next_clean:
            diagnostics.append(Diagnostic("doc-comment-without-target", line))
    return diagnostics


def parse_source(source: str) -> list[Diagnostic]:
    tokens, diagnostics = lex_source(source)
    accepted, farthest = earley_accepts(tokens)
    if not accepted:
        at = tokens[min(farthest, len(tokens) - 1)].line
        diagnostics.append(Diagnostic("syntax-error", at))
    diagnostics.extend(argument_order_diagnostics(source))
    diagnostics.extend(context_diagnostics(source))
    unique = {(item.code, item.line): item for item in diagnostics}
    return sorted(unique.values(), key=lambda item: (item.line, item.code))


def parser_self_test() -> list[str]:
    """Guard the grammar parser against collapsing back into a line lint."""

    probes = {
        "x := 1 2 3 4\n": False,
        "+ + + * / % if else while\n": False,
        "fn f(type: i32) -> void: pass\n": False,
        "ok := check(a < b, c > d)\n": True,
        "flags := [x == 1, y != 2]\n": True,
        'fn f() -> void: log("a;b")\n': True,
        'fn f() -> void: log("a < b > c")\n': True,
        "fn f(flag: bool) -> void:\n    let total: i32 = 1\n    if flag: total = 2\n    else: total = 0\n": True,
    }
    failures: list[str] = []
    for source, expected in probes.items():
        accepted = not parse_source(source)
        if accepted != expected:
            failures.append(f"parser probe {source.strip()!r}: expected {expected}, got {accepted}")
    return failures


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: reference_parser.py CASES_TSV CONFORMANCE_DIR", file=sys.stderr)
        return 2
    manifest = Path(sys.argv[1])
    root = Path(sys.argv[2])
    failures = parser_self_test()
    for row in manifest.read_text(encoding="utf-8").splitlines()[1:]:
        path_text, phase, expectation, _ = row.split("\t")
        path = root / path_text
        diagnostics = parse_source(path.read_text(encoding="utf-8"))
        codes = {item.code for item in diagnostics}
        parse_reject = phase == "parse" and expectation.startswith("reject:")
        if not parse_reject and diagnostics:
            rendered = ", ".join(f"{item.code}:{item.line}" for item in diagnostics)
            failures.append(f"{path_text}: expected to parse, got {rendered}")
        elif parse_reject:
            expected = expectation.split(":", 1)[1]
            if expected not in codes:
                rendered = ", ".join(f"{item.code}:{item.line}" for item in diagnostics)
                failures.append(f"{path_text}: expected {expected}, got {rendered or 'accept'}")
    if failures:
        print("reference parser failures:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1
    print("chapter-02 Earley parser passed for every fixture")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
