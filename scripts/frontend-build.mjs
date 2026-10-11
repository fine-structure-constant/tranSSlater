import { spawnSync } from "node:child_process";
for (const entry of ["node_modules/typescript/bin/tsc", "node_modules/vite/bin/vite.js"]) {
  const args = entry.includes("vite") ? [entry, "build"] : [entry];
  const result = spawnSync(process.execPath, args, { stdio: "inherit" });
  if (result.error) { console.error(result.error.message); process.exit(1); }
  if (result.status !== 0) process.exit(result.status ?? 1);
}
