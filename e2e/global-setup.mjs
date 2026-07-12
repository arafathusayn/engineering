// Fresh coverage collection per run.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export default function globalSetup() {
  const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), ".coverage");
  fs.rmSync(dir, { recursive: true, force: true });
}
