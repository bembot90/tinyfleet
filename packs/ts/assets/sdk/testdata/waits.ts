// A workflow the suite runs as a PROCESS, through the wrapper: three steps,
// the second of which waits until a file appears in the run directory, and a
// pinned input that makes the third throw. What the suite reads off it is the
// exit code and stdout's last line, which is the whole of the contract between
// the wrapper and the run that started it.
import { Waiting, workflow } from "../mod.ts";

await workflow(async (run) => {
  console.log("a line before the last one");
  await run.step("count", () => 1);
  await run.step("hold", async () => {
    try {
      await Deno.stat(`${run.env.runDir}/answered`);
      return "answered";
    } catch {
      throw new Waiting({ until: "answered" });
    }
  });
  const boom = await run.input("boom");
  if (boom !== null) throw new Error(`boom: ${boom}`);
});
