import koffi from "koffi";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "../..");

function dylibPath() {
  const base = path.join(repoRoot, "target", "release");
  if (process.platform === "win32") {
    return path.join(base, "flowy_router.dll");
  }
  if (process.platform === "darwin") {
    return path.join(base, "libflowy_router.dylib");
  }
  return path.join(base, "libflowy_router.so");
}

const lib = koffi.load(dylibPath());

const FLOWY_OK = 0;
const flowy_router_version = lib.func("const char *flowy_router_version()");
const flowy_router_start = lib.func(
  "int32 flowy_router_start(const char *config_path, _Out_ char *error_out, size_t error_out_len)",
);
const flowy_router_stop = lib.func(
  "int32 flowy_router_stop(_Out_ char *error_out, size_t error_out_len)",
);
const flowy_router_is_running = lib.func("int32 flowy_router_is_running()");
const flowy_router_gateway_url = lib.func(
  "int32 flowy_router_gateway_url(_Out_ char *url_out, size_t url_out_len)",
);

function readError(buf) {
  const nul = buf.indexOf(0);
  return Buffer.from(buf.subarray(0, nul >= 0 ? nul : buf.length)).toString("utf8");
}

function readUrl(buf) {
  return readError(buf);
}

const errorBuf = Buffer.alloc(512);
const urlBuf = Buffer.alloc(256);

console.log("flowy_router version:", flowy_router_version());

const configPath = process.argv[2] ?? null;
const startCode = flowy_router_start(configPath, errorBuf, errorBuf.length);
if (startCode !== FLOWY_OK) {
  console.error("start failed:", startCode, readError(errorBuf));
  process.exit(1);
}

const urlLen = flowy_router_gateway_url(urlBuf, urlBuf.length);
if (urlLen < 0) {
  console.error("gateway_url failed:", urlLen, readError(urlBuf));
  process.exit(1);
}

console.log("gateway running:", Boolean(flowy_router_is_running()));
console.log("gateway url:", readUrl(urlBuf));

process.on("SIGINT", () => {
  const stopCode = flowy_router_stop(errorBuf, errorBuf.length);
  if (stopCode !== FLOWY_OK) {
    console.error("stop failed:", stopCode, readError(errorBuf));
    process.exit(1);
  }
  console.log("gateway stopped");
  process.exit(0);
});

console.log("Press Ctrl+C to stop the embedded gateway.");
