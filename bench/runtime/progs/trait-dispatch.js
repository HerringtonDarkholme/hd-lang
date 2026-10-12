// Same shape as the hd program: two "steppers" applied alternately
// through one generic call site each.
class Double {
  constructor(k) { this.k = k; }
  step(x) { return x * 2 + this.k; }
}
class Add {
  constructor(k) { this.k = k; }
  step(x) { return x + this.k; }
}
function apply(s, x) { return s.step(x); }
function drive(n) {
  const double = new Double(3);
  const add = new Add(5);
  let x = 1;
  for (let i = 0; i < n; i++) {
    x = apply(double, x) % 1000003;
    x = apply(add, x) % 1000003;
  }
  return x;
}
console.log(drive(80000000));
