#!/usr/bin/env node
/**
 * Apply locale translations to en-US/app.ftl → zh-CN, ja-JP, th-TH
 * Preserves exact line structure (section headers, ordering).
 */
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, "..");

const LOCALES = ["zh-CN", "ja-JP", "th-TH"];

function translateFtl(content, locale, enToLocale) {
  const lines = content.split("\n");
  return lines
    .map((line) => {
      if (line.startsWith("# Futureboard Studio —")) {
        return `# Futureboard Studio — ${locale}`;
      }
      const eq = line.indexOf(" = ");
      if (eq === -1) return line;
      const key = line.slice(0, eq);
      const value = line.slice(eq + 3);
      const translated = enToLocale[value] ?? value;
      return `${key} = ${translated}`;
    })
    .join("\n");
}

const enPath = path.join(root, "packages/shared/locales/en-US/app.ftl");
const enContent = fs.readFileSync(enPath, "utf8");

for (const locale of LOCALES) {
  const mapPath = path.join(__dirname, "translations", `${locale}.json`);
  const enToLocale = JSON.parse(fs.readFileSync(mapPath, "utf8"));
  const content = translateFtl(enContent, locale, enToLocale);
  const outPath = path.join(root, `packages/shared/locales/${locale}/app.ftl`);
  fs.writeFileSync(outPath, content.endsWith("\n") ? content : content + "\n", "utf8");
  const entryCount = enContent.split("\n").filter((l) => l.includes(" = ")).length;
  console.log(`Wrote ${outPath} (${entryCount} keys)`);
}
