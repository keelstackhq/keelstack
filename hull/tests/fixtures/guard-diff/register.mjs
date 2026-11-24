// Registers hooks.mjs (guard-differential.test.mjs passes this file to `--import`).
import { register } from "node:module";

register(new URL("./hooks.mjs", import.meta.url));
