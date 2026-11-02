export function main() {
  const text = "alpha,beta,gamma,delta,epsilon,zeta,eta,theta";
  let total = 0;
  for (let i = 0; i < 500_000; i++) {
    total += text.split(",").length;
  }
  if (total !== 4_000_000) {
    throw new Error("bad split total");
  }
}
