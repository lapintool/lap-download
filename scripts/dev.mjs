import { createServer } from "node:net";
import { spawn } from "node:child_process";

const BASE_PORT = 3415;
const MAX_TRIES = 40;

function isPortFree(port) {
  return new Promise((resolve) => {
    const server = createServer();
    server.unref();
    server.once("error", () => resolve(false));
    server.once("listening", () => {
      server.close(() => resolve(true));
    });
    server.listen(port, "127.0.0.1");
  });
}

async function findFreePort(start) {
  for (let port = start; port < start + MAX_TRIES; port += 1) {
    if (await isPortFree(port)) {
      return port;
    }
  }
  throw new Error(`no free port in ${start}..${start + MAX_TRIES - 1}`);
}

const port = await findFreePort(BASE_PORT);
const devUrl = `http://localhost:${port}`;

process.env.LAPDW_DEV_PORT = String(port);
process.env.TAURI_CONFIG = JSON.stringify({
  build: { devUrl },
});

console.log(`[lapdw] dev server port ${port}`);

const isWin = process.platform === "win32";
const bin = isWin ? "tauri.cmd" : "tauri";
const extraArgs = process.argv.slice(2);

function quoteForCmd(arg) {
  if (/^[a-zA-Z0-9_\-./:=+,]+$/.test(arg)) {
    return arg;
  }
  return `"${arg.replace(/"/g, '\\"')}"`;
}

const child = isWin
  ? spawn([bin, "dev", ...extraArgs.map(quoteForCmd)].join(" "), {
      stdio: "inherit",
      shell: true,
      env: process.env,
    })
  : spawn(bin, ["dev", ...extraArgs], {
      stdio: "inherit",
      env: process.env,
    });

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => {
    if (!child.killed) {
      child.kill(signal);
    }
  });
}

child.on("close", (code) => {
  process.exit(code ?? 1);
});
