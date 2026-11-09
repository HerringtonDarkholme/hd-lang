type Expr = { tag: "Num"; fields: { n: number } } | { tag: "Add"; fields: { a: Expr; b: Expr } };

const total = (expr: Expr): number =>
  expr.tag === "Num" ? expr.fields.n : total(expr.fields.a) + total(expr.fields.b);

// `eval` sums the tree it is given. `twice` returns a node that holds its
// argument on both paths, so the argument crosses back twice.
export default {
  eval(expr: Expr): number {
    return total(expr);
  },
  twice(expr: Expr): Expr {
    return { tag: "Add", fields: { a: expr, b: expr } };
  },
};
