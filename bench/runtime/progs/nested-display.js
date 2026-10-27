// Same nested records, interpolation and rolling text checksum as hd.
class Money {
  constructor(cents) {
    this.cents = cents;
  }
  toString() {
    const whole = Math.trunc(this.cents / 100);
    const fraction = this.cents % 100;
    return fraction < 10 ? `${whole}.0${fraction}` : `${whole}.${fraction}`;
  }
}

class Line {
  constructor(sku, quantity, price) {
    this.sku = sku;
    this.quantity = quantity;
    this.price = price;
  }
  toString() {
    return `item#${this.sku} x${this.quantity} @${this.price}`;
  }
}

class Order {
  constructor(id, first, second, third) {
    this.id = id;
    this.first = first;
    this.second = second;
    this.third = third;
  }
  toString() {
    return `order#${this.id}[${this.first};${this.second};${this.third}]`;
  }
}

function line(seed) {
  return new Line(seed % 1000, (seed % 5) + 1, new Money((seed % 10_000) + 100));
}

let checksum = 0;
for (let id = 0; id < 330_000; id++) {
  const order = new Order(id, line(id * 3), line(id * 3 + 1), line(id * 3 + 2));
  const text = `${order}`;
  for (let i = 0; i < text.length; i++) {
    checksum = (checksum * 33 + text.charCodeAt(i)) % 1_000_000_007;
  }
}
console.log(checksum);
