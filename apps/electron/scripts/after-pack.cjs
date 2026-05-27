// electron-builder `afterPack` hook — runs after the per-platform
// directory is produced but before asar integrity / code-signing.
//
// Embeds our `.ico` directly into the packaged exe using `rcedit`, so the
// final binary carries the correct icon even when later electron-builder
// stages (e.g. winCodeSign cache extraction on Windows without Developer
// Mode) fail and skip their own rcedit invocation.

const path = require("node:path");
const fs = require("node:fs");

/** @type {(ctx: import("electron-builder").AfterPackContext) => Promise<void>} */
module.exports = async function afterPack(ctx) {
  if (ctx.electronPlatformName !== "win32") return;

  const rceditMod = await import("rcedit");
  const rcedit = rceditMod.rcedit ?? rceditMod.default ?? rceditMod;

  const productName = ctx.packager.appInfo.productFilename;
  const exePath = path.join(ctx.appOutDir, `${productName}.exe`);
  const iconPath = path.join(ctx.packager.projectDir, "..", "shared", "icon.ico");

  if (!fs.existsSync(exePath)) {
    console.warn(`[after-pack] exe not found: ${exePath}`);
    return;
  }
  if (!fs.existsSync(iconPath)) {
    console.warn(`[after-pack] icon not found: ${iconPath}`);
    return;
  }

  const version = ctx.packager.appInfo.version;
  const companyName = ctx.packager.appInfo.companyName ?? "";

  await rcedit(exePath, {
    icon: iconPath,
    "version-string": {
      ProductName: productName,
      FileDescription: productName,
      CompanyName: companyName,
      LegalCopyright: ctx.packager.appInfo.copyright ?? "",
      OriginalFilename: `${productName}.exe`,
      InternalName: productName,
    },
    "file-version": version,
    "product-version": version,
  });

  console.log(`[after-pack] embedded icon into ${exePath}`);
};
