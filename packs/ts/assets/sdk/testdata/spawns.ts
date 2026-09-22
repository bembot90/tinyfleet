// A workflow the suite runs as a PROCESS from the run directory, the way a
// run's child starts: one spawn, whose exit is what the suite reads.
import { workflow } from "../mod.ts";

await workflow(async (run) => {
  await run.spawn({ role: "builder", item: "it-1" });
});
