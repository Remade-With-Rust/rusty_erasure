// Headless verification of the wasm demo (M6 gate): instantiate the built
// module under Node's WebAssembly, run the roundtrip grid and a bench call,
// and exit nonzero on any failure. Usage: node tools/check_demo.mjs
import { readFileSync } from "node:fs";

const wasm = readFileSync(
  new URL("../target/wasm32-unknown-unknown/release/rusty_erasure_demo.wasm", import.meta.url));
const { instance } = await WebAssembly.instantiate(wasm, {});
const e = instance.exports;

const name = new TextDecoder().decode(new Uint8Array(
  e.memory.buffer, e.demo_kernel_name_ptr(), e.demo_kernel_name_len()));
console.log(`kernel set: ${name}`);

let failed = 0;
for (const [k, p, len, drop] of [[4, 2, 4096, 2], [10, 4, 65536, 4], [10, 4, 1048576, 3], [16, 4, 96, 4]]) {
  const rc = e.demo_roundtrip(k, p, len, drop);
  console.log(`roundtrip k=${k} p=${p} len=${len} drop=${drop}: ${rc === 0 ? "PASS" : `FAIL rc=${rc}`}`);
  if (rc !== 0) failed++;
}
const t0 = performance.now();
const chk = e.demo_bench(10, 4, 65536, 200);
const mbps = (10 * 65536 * 200) / ((performance.now() - t0) / 1e3) / 1e6;
console.log(`bench (10+4, 64 KiB x200): ${mbps.toFixed(0)} MB/s source-basis, checksum=${chk}`);
if (chk < 0) failed++;
process.exit(failed === 0 ? 0 : 1);
