// The stream as a test writes and reads it: one stored line per event, the
// shape the binary's own writer stores, so a line a test appends by hand and a
// line `fleet event step` appends sit in one sequence.

/** Who wrote a line, as the stream stores it. */
export interface Actor {
  kind: string;
  id: string;
}

export interface Line {
  seq: number;
  kind: string;
  actor: Actor | null;
  payload: Record<string, unknown>;
}

/** A typed actor's text, `<kind>:<id>`, as the object the stream stores. Text
 * with no kind is a seat argument, as the binary reads one. */
export function typed(text: string): Actor {
  const at = text.indexOf(":");
  return at < 0
    ? { kind: "seat", id: text }
    : { kind: text.slice(0, at), id: text.slice(at + 1) };
}

/** The stream as stored, parsed. */
export async function lines(stream: string): Promise<Line[]> {
  const body = await Deno.readTextFile(stream);
  return body.split("\n").filter((l) => l.trim() !== "").map((l) => {
    const stored = JSON.parse(l);
    return {
      seq: stored.seq,
      kind: stored.type,
      actor: stored.actor ?? null,
      payload: stored.payload,
    };
  });
}

/** One stored line at the sequence after the file's last. */
export async function append(
  stream: string,
  kind: string,
  actor: Actor,
  payload: Record<string, unknown>,
): Promise<number> {
  const all = await lines(stream);
  const seq = (all.at(-1)?.seq ?? 0) + 1;
  const line = JSON.stringify({
    id: `${"0".repeat(24)}${Date.now().toString(16).padStart(8, "0")}-${
      seq.toString(16).padStart(8, "0")
    }`,
    seq,
    ts: "2026-09-18T00:00:00Z",
    type: kind,
    actor,
    payload,
  });
  await Deno.writeTextFile(stream, `${line}\n`, { append: true });
  return seq;
}
