// A workflow the suite runs as a PROCESS: one land, whose refusal is what the
// suite reads off the exit code and stdout's last line.
import { workflow } from "../mod.ts";

await workflow(async (run) => {
  await run.land("it-1", "abc1234");
});
