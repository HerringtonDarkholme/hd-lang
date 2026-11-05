// The same inventory transactions and record-spread updates as inventory.hd.
function receive(item, depot, delta) {
  item.quantity += delta;
  item.depots.set(depot, (item.depots.get(depot) ?? 0) + delta);
  item.batches[item.revision % item.batches.length] += delta;
}
const items = [];
for (let sku = 0; sku < 256; sku++) {
  items.push({
    sku,
    quantity: 1000,
    batches: [0, 0, 0, 0],
    depots: new Map([
      ["east", 250],
      ["west", 250],
      ["north", 250],
      ["south", 250],
    ]),
    revision: 0,
  });
}
const names = ["east", "west", "north", "south"];
for (let step = 0; step < 3200000; step++) {
  const index = (step * 73) % items.length;
  const previous = items[index];
  items[index] = { ...previous, revision: previous.revision + 1 };
  receive(items[index], names[Math.floor(step / 11) % names.length], (step % 9) - 4);
}
let quantity = 0,
  depotTotal = 0,
  lotTotal = 0,
  revisions = 0;
for (const item of items) {
  quantity += item.quantity;
  revisions += item.revision;
  for (const name of names) {
    depotTotal += item.depots.get(name);
  }
  for (const amount of item.batches) {
    lotTotal += amount;
  }
}
console.log(`${quantity} ${depotTotal} ${lotTotal} ${revisions}`);
