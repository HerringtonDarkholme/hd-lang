///|
struct Money {
  cents : Int64
}

///|
impl Show for Money with fn to_string(self) {
  let whole = self.cents / 100
  let fraction = self.cents % 100
  if fraction < 10 {
    "\{whole}.0\{fraction}"
  } else {
    "\{whole}.\{fraction}"
  }
}

///|
struct Line {
  sku : Int32
  quantity : Int32
  price : Money
}

///|
impl Show for Line with fn to_string(self) {
  "item#\{self.sku} x\{self.quantity} @\{self.price}"
}

///|
struct Order {
  id : Int32
  first : Line
  second : Line
  third : Line
}

///|
impl Show for Order with fn to_string(self) {
  "order#\{self.id}[\{self.first};\{self.second};\{self.third}]"
}

///|
fn line(seed : Int32) -> Line {
  {
    sku: seed % 1000,
    quantity: seed % 5 + 1,
    price: { cents: (seed % 10000).to_int64() + 100, },
  }
}

///|
fn main {
  let mut checksum : Int64 = 0
  for id in 0..<330000 {
    let order = Order::{
      id,
      first: line(id * 3),
      second: line(id * 3 + 1),
      third: line(id * 3 + 2),
    }
    let text = "\{order}"
    for character in text.iter() {
      checksum = (checksum * 33 + character.to_int().to_int64()) % 1000000007
    }
  }
  println(checksum)
}
