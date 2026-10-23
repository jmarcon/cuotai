import { dirname } from "node:path";
import { fileURLToPath } from "node:url";
export const here = dirname(fileURLToPath(import.meta.url));
