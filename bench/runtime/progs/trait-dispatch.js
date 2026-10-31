// Same shape as the hd program: two "steppers" applied alternately
// through one generic call site each.
class Double {
  constructor(k) {
    this.k = k;
  }
  step(value) {
    return value * 2 + this.k;
  }
}
class Add {
  constructor(k) {
    this.k = k;
  }
  step(value) {
    return value + this.k;
  }
}
function apply(stepper, value) {
  return stepper.step(value);
}
function drive(count) {
  const double = new Double(3);
  const add = new Add(5);
  let value = 1;
  for (let i = 0; i < count; i++) {
    value = apply(double, value) % 1000003;
    value = apply(add, value) % 1000003;
  }
  return value;
}
console.log(drive(35000000));
