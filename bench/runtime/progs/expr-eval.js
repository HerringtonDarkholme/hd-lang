// The same recursive-descent grammar and enum-shaped syntax tree as expr-eval.hd.
class Parser {
  constructor(text) {
    this.chars = [...text];
    this.at = 0;
  }
  whitespace() {
    while (this.at < this.chars.length && this.chars[this.at] === " ") {
      this.at++;
    }
  }
  atom() {
    this.whitespace();
    if (this.at === this.chars.length) {
      throw new Error("expected a value");
    }
    const character = this.chars[this.at];
    if (character === "(") {
      this.at++;
      const value = this.expression();
      this.whitespace();
      if (this.at === this.chars.length || this.chars[this.at] !== ")") {
        throw new Error("expected a closing parenthesis");
      }
      this.at++;
      return value;
    }
    if (character === "-") {
      this.at++;
      return { kind: "binary", op: "-", left: { kind: "number", value: 0 }, right: this.atom() };
    }
    if (character < "0" || character > "9") {
      throw new Error("expected a number");
    }
    let value = 0;
    while (
      this.at < this.chars.length &&
      this.chars[this.at] >= "0" &&
      this.chars[this.at] <= "9"
    ) {
      value = value * 10 + this.chars[this.at].charCodeAt(0) - 48;
      this.at++;
    }
    return { kind: "number", value };
  }
  term() {
    let left = this.atom();
    for (;;) {
      this.whitespace();
      const op = this.chars[this.at];
      if (op !== "*" && op !== "/") {
        break;
      }
      this.at++;
      left = { kind: "binary", op, left, right: this.atom() };
    }
    return left;
  }
  expression() {
    let left = this.term();
    for (;;) {
      this.whitespace();
      const op = this.chars[this.at];
      if (op !== "+" && op !== "-") {
        break;
      }
      this.at++;
      left = { kind: "binary", op, left, right: this.term() };
    }
    return left;
  }
}
function parse(text) {
  const parser = new Parser(text);
  const root = parser.expression();
  parser.whitespace();
  if (parser.at !== parser.chars.length) {
    throw new Error("unexpected trailing input");
  }
  return root;
}
function evaluate(node) {
  if (node.kind === "number") {
    return node.value;
  }
  const leftValue = evaluate(node.left),
    rightValue = evaluate(node.right);
  switch (node.op) {
    case "+":
      return leftValue + rightValue;
    case "-":
      return leftValue - rightValue;
    case "*":
      return leftValue * rightValue;
    case "/":
      if (rightValue === 0) {
        throw new Error("division by zero");
      }
      return Math.trunc(leftValue / rightValue);
    default:
      throw new Error("unknown operator");
  }
}
const inputs = [
  "(12 + 7) * (31 - 4) / 3",
  "91 - (8 * 7) + 6 * (5 + 4)",
  "-(17 + 9) * 3 + 400 / 8",
  "3 * (2 + (11 - 4) * 5) - 18",
  "100 / (3 - 3)",
  "7 + (2 * 9",
];
let total = 0,
  errors = 0;
for (let round = 0; round < 252000; round++) {
  let tree;
  let text;
  if (round % 1000 === 0) {
    text = inputs[5];
  } else if (round % 1000 === 1) {
    text = inputs[4];
  } else {
    text = inputs[round % 4];
  }
  try {
    tree = parse(text);
  } catch {
    errors++;
    continue;
  }
  for (let i = 0; i < 16; i++) {
    try {
      total += evaluate(tree);
    } catch {
      errors++;
    }
  }
}
console.log(`${total} ${errors}`);
