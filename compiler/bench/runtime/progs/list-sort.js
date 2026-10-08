function fill(n) {
  const items = [];
  let state = 12345;
  for (let i = 0; i < n; i++) {
    state = (state * 31 + 7) % 100003;
    items.push(state);
  }
  return items;
}
// Insertion sort, the same algorithm as the hd program.
function sort(items) {
  for (let i = 1; i < items.length; i++) {
    const key = items[i];
    let j = i;
    while (j > 0 && items[j - 1] > key) {
      items[j] = items[j - 1];
      j--;
    }
    items[j] = key;
  }
}
function checksum(items) {
  let total = 0;
  for (const value of items) total = (total + value) % 1000003;
  return total;
}
const items = fill(30000);
sort(items);
console.log(checksum(items));
