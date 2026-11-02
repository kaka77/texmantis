// 构建时自动生成版本戳（每次 vite build 刷新），供标题栏显示，用于辨认构建版本
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";

import { readFileSync } from "node:fs";
const version = JSON.parse(readFileSync("package.json", "utf8")).version;
const d = new Date();
const pad = (n) => String(n).padStart(2, "0");
// 例：20260908-1712
const id = `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}`;

const target = "src/lib/build-stamp.ts";
mkdirSync(dirname(target), { recursive: true });
writeFileSync(target, `// 自动生成，勿手改（scripts/stamp.mjs）\nexport const APP_VERSION = "${version}";\nexport const BUILD_STAMP = "${id}";\n`);
console.log("build stamp:", id);
