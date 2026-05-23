const fs = require("node:fs/promises");
const path = require("node:path");

const electronRoot = path.resolve(__dirname, "..");
const source = path.resolve(electronRoot, "../web/dist");
const target = path.resolve(electronRoot, "dist");

async function copyRenderer() {
  await fs.mkdir(target, { recursive: true });
  await fs.rm(path.join(target, "renderer"), { recursive: true, force: true });
  const entries = await fs.readdir(source, { withFileTypes: true });
  await Promise.all(
    entries.map((entry) =>
      fs.rm(path.join(target, entry.name), { recursive: true, force: true }),
    ),
  );
  await Promise.all(
    entries.map((entry) =>
      fs.cp(path.join(source, entry.name), path.join(target, entry.name), {
        recursive: true,
      }),
    ),
  );
}

copyRenderer().catch((error) => {
  console.error("[copy-renderer] failed:", error);
  process.exitCode = 1;
});
