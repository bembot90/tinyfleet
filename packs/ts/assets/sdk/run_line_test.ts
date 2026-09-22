// The manifest's own run line, rendered the way core renders it and executed
// the way core executes it — `sh -c` in the run directory, the environment
// cleared to PATH and the FLEET_ variables — over a scratch bundle that echoes
// every FLEET_ variable the SDK reads. --allow-env takes variable NAMES: a
// grant spelled as a prefix names one variable that is never set, and the
// wrapper's first read throws NotCapable before any step runs.

import { assert, assertEquals } from "jsr:@std/assert@1";
import { bin } from "./testdata/rig.ts";

const here = import.meta.dirname!;
const manifest = `${here}/../../pack.toml`;

/** The six names core hands the child and the SDK reads back. */
const NAMES = [
  "FLEET_DIR",
  "FLEET_RUN_ID",
  "FLEET_STREAM",
  "FLEET_STREAM_SEQ",
  "FLEET_RUN_DIR",
  "FLEET_BIN",
];

/** The [runtime] table's run string, as the manifest stores it. */
async function runLine(): Promise<string> {
  const text = await Deno.readTextFile(manifest);
  const runtime = text.slice(text.indexOf("[runtime]"));
  const m = runtime.match(/^run\s*=\s*"([^"]*)"\s*$/m);
  if (!m) throw new Error(`${manifest} holds no [runtime] run line`);
  return m[1];
}

Deno.test("the run line — rendered with a scratch bundle and executed as core executes it, every FLEET_ variable the SDK reads comes back", async () => {
  const runDir = await Deno.makeTempDir({ prefix: "fleet-run-line-" });
  const bundle = `${runDir}/bundle.js`;
  await Deno.writeTextFile(
    bundle,
    `console.log(JSON.stringify(Object.fromEntries(${
      JSON.stringify(NAMES)
    }.map((n) => [n, Deno.env.get(n)]))));\n`,
  );
  const line = (await runLine())
    .replaceAll("{fleet}", await bin)
    .replaceAll("{run_dir}", runDir)
    .replaceAll("{bundle}", bundle);
  assert(!line.includes("{"), `a placeholder is left standing in: ${line}`);

  const values = Object.fromEntries(
    NAMES.map((n, i) => [n, `${n.toLowerCase()}-${Deno.pid}-${i}`]),
  );
  const child = await new Deno.Command("sh", {
    args: ["-c", line],
    cwd: runDir,
    clearEnv: true,
    env: { PATH: Deno.env.get("PATH") ?? "", ...values },
    stdout: "piped",
    stderr: "piped",
  }).output();
  const stdout = new TextDecoder().decode(child.stdout);
  const stderr = new TextDecoder().decode(child.stderr);
  assertEquals(child.code, 0, `\`${line}\` exited ${child.code}:\n${stderr}`);
  assertEquals(JSON.parse(stdout), values);
  await Deno.remove(runDir, { recursive: true });
});
