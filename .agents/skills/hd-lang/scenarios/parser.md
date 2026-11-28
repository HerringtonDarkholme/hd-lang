# A Lexer, A Parser And An Evaluator

**Scenario.** Read a small language, report errors with their position,
and evaluate it. Markdown renderers, query languages, template engines
and config formats all have this shape: a lexer turns bytes into tokens,
a parser turns tokens into a tree, and a walk over the tree does the
work. This one reads s-expressions such as `(+ 1 (* 2 3))`.

## The program

```hd
use std.testing.it_each

## Why a program could not be read or run, with the byte offset.
@error
@derive(Eq, Debug)
pub enum LispError:
    @error("unexpected `$found` at byte $at")
    Unexpected(found: string, at: usize)
    @error("the input ends inside a list")
    Unclosed
    @error("unknown operator `$name`")
    UnknownOp(name: string)
    @error("`$name` needs at least one argument")
    NoArguments(name: string)

## A token, with the byte offset where it starts.
@derive(Eq, Debug)
enum Token:
    Open(at: usize)
    Close(at: usize)
    Int(value: i64, at: usize)
    Word(text: string, at: usize)

## A syntax tree. A list holds its items by value: a `List` already gives
## each element its own storage, so no `box` is needed here.
@derive(Eq, Debug)
enum Expr:
    Int(i64)
    Call(op: string, args: List[Expr])

# The lexer keeps its input as bytes and its position as an index.
struct Lexer:
    bytes: List[u8]
    at: usize

impl Lexer:
    fn peek(&self) -> u8?:
        self.bytes.get(self.at)

    fn word_end(&self) -> usize:
        let end = self.at
        while end < self.bytes.len():
            b := self.bytes[end]
            if b == 32 || b == 10 || b == 40 || b == 41:
                break # space, newline, ( )
            end += 1
        end

    fn tokens(&mut self) -> Result[List[Token], LispError]:
        let mut out: List[Token] = []
        while true:
            match self.peek():
                .None => break
                .Some(32) => self.at += 1 # space
                .Some(10) => self.at += 1 # newline
                .Some(40) =>
                    out.push(.Open(self.at))
                    self.at += 1
                .Some(41) =>
                    out.push(.Close(self.at))
                    self.at += 1
                .Some(_) =>
                    start := self.at
                    self.at = self.word_end()
                    text := string::from_utf8(self.bytes.slice(start, self.at))
                        .expect("ASCII input")
                    match text.parse::[i64]():
                        .Ok(n) => out.push(.Int(n, start))
                        .Err(_) => out.push(.Word(text, start))
        .Ok(out)

# The parser walks the tokens with an index; each rule returns the tree it read.
struct Parser:
    tokens: List[Token]
    at: usize

impl Parser:
    fn expr(&mut self) -> Result[Expr, LispError]:
        token := match self.tokens.get(self.at):
            .Some(token) => token
            .None => return .Err(.Unclosed)
        self.at += 1
        match token:
            .Int(value, _) => .Ok(.Int(value))
            .Word(text, at) => .Err(.Unexpected(text, at))
            .Close(at) => .Err(.Unexpected(")", at))
            .Open(_) => self.call()

    fn call(&mut self) -> Result[Expr, LispError]:
        op := match self.tokens.get(self.at):
            .Some(.Word(text, _)) => text
            .Some(.Open(at)) => return .Err(.Unexpected("(", at))
            .Some(.Close(at)) => return .Err(.Unexpected(")", at))
            .Some(.Int(value, at)) => return .Err(.Unexpected("$value", at))
            .None => return .Err(.Unclosed)
        self.at += 1
        let mut args: List[Expr] = []
        while true:
            match self.tokens.get(self.at):
                .Some(.Close(_)) =>
                    self.at += 1
                    return .Ok(.Call(op, args))
                .None => return .Err(.Unclosed)
                _ => args.push(self.expr()?)

fn parse(src: string) -> Result[Expr, LispError]:
    let mut lexer = Lexer { bytes: src.to_utf8(), at: 0 }
    tokens := lexer.tokens()?
    let mut parser = Parser { tokens: tokens, at: 0 }
    tree := parser.expr()?
    match tokens.get(parser.at):
        .Some(.Open(at)) => .Err(.Unexpected("(", at))
        .Some(.Close(at)) => .Err(.Unexpected(")", at))
        .Some(.Int(value, at)) => .Err(.Unexpected("$value", at))
        .Some(.Word(text, at)) => .Err(.Unexpected(text, at))
        .None => .Ok(tree)

fn eval(e: &Expr) -> Result[i64, LispError]:
    match e:
        .Int(n) => .Ok(n)
        .Call(op, args) =>
            let mut values: List[i64] = []
            for arg in args:
                values.push(eval(&arg)?)
            if values.is_empty():
                return .Err(.NoArguments(op))
            let acc = values[0]
            for v in values.slice(1, values.len()):
                acc = match op:
                    "+" => acc + v
                    "*" => acc * v
                    "-" => acc - v
                    "max" =>
                        if v > acc: v else: acc
                    _ => return .Err(.UnknownOp(op))
            if values.len() == 1 && op == "-": .Ok(-acc) else: .Ok(acc)

pub fn run(src: string) -> Result[i64, LispError]:
    eval(&parse(src)?)

pub fn main() -> void $ Console:
    for src in ["(+ 1 (* 2 3))", "(max 4 9 2)", "(- 5)", "(+ 1 2", "(pow 2 3)"]:
        match run(src):
            .Ok(n) => println("$src = $n")
            .Err(error) => println("$src: $error")

tests:
    it_each(
        "evaluates",
        [
            ("7", 7i64),
            ("(+ 1 2 3)", 6i64),
            ("(* 2 (+ 3 4))", 14i64),
            ("(- 10 3 2)", 5i64),
            ("(- 5)", -5i64),
        ],
        body=fn!(row: (string, i64)): assert_equal(run(row._0), .Ok(row._1), reason=row._0)
    )

    it_each(
        "reports where it failed",
        [
            ("(+ 1", LispError.Unclosed),
            (")", LispError.Unexpected(")", 0)),
            ("(+ 1) 2", LispError.Unexpected("2", 6)),
            ("(1 2)", LispError.Unexpected("1", 1)),
            ("(pow 2 3)", LispError.UnknownOp("pow")),
            ("(+)", LispError.NoArguments("+")),
        ],
        body=fn!(row: (string, LispError)): assert_equal(run(row._0), .Err(row._1), reason=row._0)
    )
```

```output
(+ 1 (* 2 3)) = 7
(max 4 9 2) = 9
(- 5) = -5
(+ 1 2: the input ends inside a list
(pow 2 3): unknown operator `pow`
```

## Decisions

- **Each stage is a struct with an index.** The lexer holds the input as
  bytes (`to_utf8()`) and a byte position; the parser holds the tokens
  and a token position. Their methods take `&mut self`. Index into the
  bytes; don't re-slice the remaining string per token, which copies it
  each time ([performance](performance.md#when-copies-happen)).
- **Compare bytes with numbers.** hd has no byte literal: `40` is `(`,
  `48` to `57` are the digits. Work in `chars()` instead when the input
  isn't ASCII.
- **Tokens and errors carry the byte offset** they start at, so an error
  can say where it happened. The error enum is `@error` with a message
  per variant ([errors](../design/errors.md)).
- **One `match` arm per pattern.** There is no `A | B` pattern: write two
  arms with the same body, as the lexer does for space and newline.
- **Parse first, evaluate second.** `parse` returns the whole tree or the
  first error; `eval` never sees a malformed program.
- **Test with tables.** One `it_each` row per input, for values and for
  errors with their positions ([testing](../design/testing.md)).

## A tree with fixed children

A list of children, as in `Call(op, args: List[Expr])`, needs nothing
special: the list already stores its elements apart. A variant that holds
one or two children of its own type directly needs `*Expr`, made with
`box(...)`, because a type can't contain itself by value:

```hd
enum Expr:
    Num(i64)
    Add(*Expr, *Expr)
    Mul(*Expr, *Expr)

fn eval(e: &Expr) -> i64:
    match e:
        .Num(n) => n
        .Add(l, r) => eval(l) + eval(r)
        .Mul(l, r) => eval(l) * eval(r)

tests:
    it("evaluates a tree"):
        e := Expr.Add(box(Expr.Num(2)), box(Expr.Mul(box(Expr.Num(3)), box(Expr.Num(4)))))
        assert_equal(eval(&e), 14)
```

A pattern binds a copy of a small value (`.Num(n) => n`); `*` is only for
references ([references](../reference/references.md)).
