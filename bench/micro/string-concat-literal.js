function mix(total, message, name) {
  let next = (total * 31 + message.length) % 1000003;
  if (message.endsWith(name)) {
    next += 1;
  }
  return next;
}

export function main() {
  const names = ["ada", "grace", "edsger", "barbara", "donald", "frances"];
  let total = 0;
  for (let i = 0; i < 5000000; i++) {
    const name = names[i % names.length];
    total = mix(total, "the account owner is named " + name, name);
    total = mix(total, `${name} has no pending invoices`, name);
    total = mix(total, `access was denied for the user ${name}`, name);
  }
  if (total !== 378395) {
    throw new Error("bad checksum");
  }
}
