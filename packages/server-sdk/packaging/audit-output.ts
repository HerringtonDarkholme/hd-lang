import { auditCloudflareOutput, auditVercelNodeOutput } from "./output.ts";

const [platform, directory, ...extra] = process.argv.slice(2);
try {
  if (!directory || extra.length || !["cloudflare", "vercel-node"].includes(platform)) {
    throw new Error("usage: node audit-output.ts <cloudflare|vercel-node> <output-directory>");
  }
  const audit = platform === "cloudflare" ? auditCloudflareOutput : auditVercelNodeOutput;
  const result = await audit(directory);
  console.log(`SDK output: audited ${result.generations} generations in ${result.directory}`);
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
