//! The store as an adapter executable answers it: `docs/store.md`'s contract
//! spoken to a process, one per call.
//!
//! ONE PROCESS PER CALL. Each trait method runs `<adapter> <verb>` with the
//! verb's request on stdin — its fields, `schema_version` and the project's
//! root, built by [`types::request`] — and reads the answer off stdout through
//! [`types::answer`], which takes the first JSON value and demands
//! `schema_version` 1 exactly. The working directory is inherited and never
//! relied on: the root is in the request.
//!
//! THE EXIT IS READ THROUGH THE CONTRACT'S TABLE, and nowhere else: 0 is the
//! answer, 1 the record's refusal, 2 a request the adapter does not speak, 3
//! could not tell, and any other code or a signal is 3 too. Every refusal
//! carries the last line the adapter wrote on stderr, because that line is
//! for the person reading the refusal.
//!
//! BOUNDED like every store call: [`STORE_TIMEOUT`], then the adapter's whole
//! process group is killed and the call is could not tell — and for a write,
//! a write whose effect cannot be told.
//!
//! SELECTED BY `[store] adapter` naming an executable by absolute path, which
//! [`super::open`] reads out of the project's own file.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::types::{
    self, Answered, Appended, Capabilities, Created, Exported, Listed, OpenHolds, Raised, Refusal,
    RefusalReason, Resolved, Scratched, Shown,
};
use super::{
    first_value, tail, validated, validated_new, writable, Filter, HoldId, Item, ItemId,
    ItemSummary, NewItem, Order, ReadProof, RunRecord, Store, StoreError, Update, Version,
    WithdrawFence, STORE_TIMEOUT,
};
use crate::entry::{self, Body, Entry};
use crate::process::{deadline_cause, run_bounded_fed};
use crate::seat::actor::Actor;

/// The verbs that change the store: a call to one of them that outruns its
/// bound is a write whose effect cannot be told.
const WRITES: [&str; 9] = [
    "create",
    "update",
    "append",
    "order.set",
    "order.withdraw",
    "run.set",
    "hold.raise",
    "hold.clear",
    "close",
];

/// What a write that outran its bound adds to its refusal: the kill cannot
/// say whether the write landed before it. The words are the built-in
/// adapter's.
const UNTOLD: &str = " — the write's effect cannot be told, so the item must be read before \
                      anything is written to it again";

/// The store as an adapter executable, scoped to one project.
pub struct Exec {
    adapter: PathBuf,
    root: PathBuf,
    timeout: Duration,
}

/// Exit 1's answer: the refusal, under its one key.
#[derive(Deserialize)]
struct Refused {
    refused: Refusal,
}

/// `timeline`'s answer, each entry held as the value it came as until it is
/// read into an [`Entry`].
#[derive(Deserialize)]
struct Entries {
    entries: Vec<Value>,
}

impl Exec {
    /// The store the executable at `adapter` keeps for the project at `root`,
    /// bounded by [`STORE_TIMEOUT`].
    pub fn at(adapter: &Path, root: &Path) -> Exec {
        Exec {
            adapter: adapter.to_path_buf(),
            root: root.to_path_buf(),
            timeout: STORE_TIMEOUT,
        }
    }

    /// The same store under another bound.
    pub fn with_timeout(self, timeout: Duration) -> Exec {
        Exec { timeout, ..self }
    }

    /// One call: the verb's response body, and the whole of stdout it was read
    /// off.
    ///
    /// A refusal names the item by the request's own `id` — `hold` for a
    /// clear — as the caller typed it, because that is the text it refuses.
    fn call<T: DeserializeOwned>(
        &self,
        verb: &str,
        fields: Map<String, Value>,
    ) -> Result<(T, String), StoreError> {
        let adapter = self.adapter.display();
        let named = format!("{adapter} {verb}");
        let subject = fields
            .get("id")
            .or_else(|| fields.get("hold"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let request = types::request(fields, &self.root).to_string().into_bytes();
        let mut cmd = Command::new(&self.adapter);
        cmd.arg(verb);
        let out = run_bounded_fed(cmd, request, self.timeout).map_err(|why| {
            if why != deadline_cause(self.timeout) {
                return StoreError::Unreadable(format!(
                    "{adapter} could not be run ({why}) — nothing was written"
                ));
            }
            let mut refusal = format!("{named} {why}");
            if WRITES.contains(&verb) {
                refusal.push_str(UNTOLD);
            }
            StoreError::Unreadable(refusal)
        })?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let said = tail(&out);
        let read = match out.status.code() {
            Some(0) => types::answer::<T>(&stdout).map_err(|why| {
                StoreError::Unreadable(format!("{named} answered no readable response: {why}"))
            }),
            Some(1) => Err(match types::answer::<Refused>(&stdout) {
                Ok(Refused { refused }) => on_the_record(refused, subject),
                Err(_) => StoreError::Unreadable(format!(
                    "{named} refused with no readable refusal: {said}"
                )),
            }),
            Some(2) => Err(StoreError::Unreadable(format!(
                "{named} refused the request as usage (exit 2) — this fleet and the adapter do \
                 not speak the same store contract: {said}"
            ))),
            Some(3) => {
                let error = first_value(&stdout)
                    .and_then(|answer| answer.get("error")?.as_str().map(str::to_string));
                Err(StoreError::Unreadable(format!(
                    "{named} could not tell: {}",
                    error.unwrap_or_else(|| said.clone())
                )))
            }
            Some(code) => Err(StoreError::Unreadable(format!(
                "{named} exited {code}, which is not a row of the store contract's exit table: \
                 {said}"
            ))),
            None => Err(StoreError::Unreadable(format!(
                "{named} was ended by a signal: {said}"
            ))),
        };
        read.map(|body| (body, stdout))
            .map_err(|refused| carrying_stderr(refused, &out))
    }
}

/// Exit 1's refusal as the record's answer, by its reason. `subject` is the
/// text the request named the item by; a verb that names none is refused in
/// the adapter's own words. `moved` is a fence the item did not meet, and its
/// message is the adapter's, which names what holds the item.
fn on_the_record(refused: Refusal, subject: Option<String>) -> StoreError {
    let Refusal {
        reason,
        message,
        candidates,
    } = refused;
    let subject = subject.unwrap_or_else(|| message.clone());
    match reason {
        RefusalReason::Missing => StoreError::Refused(format!("{subject} is not in the store")),
        RefusalReason::Ambiguous => {
            let named = if candidates.is_empty() {
                message
            } else {
                candidates
                    .iter()
                    .map(ItemId::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            StoreError::Refused(format!(
                "{subject} matches more than one item — {named} — and more of the id says which \
                 one this is"
            ))
        }
        RefusalReason::Already => StoreError::Refused(message),
        RefusalReason::Moved => StoreError::Moved(message),
    }
}

/// The refusal with what the adapter said last on stderr beside it, where it
/// said anything there and the refusal does not already carry it.
fn carrying_stderr(refused: StoreError, out: &Output) -> StoreError {
    if String::from_utf8_lossy(&out.stderr).trim().is_empty() {
        return refused;
    }
    let said = tail(out);
    let carried = |text: String| {
        if text.contains(&said) {
            text
        } else {
            format!("{text} (the adapter said: {said})")
        }
    };
    match refused {
        StoreError::Refused(text) => StoreError::Refused(carried(text)),
        StoreError::Unreadable(text) => StoreError::Unreadable(carried(text)),
        moved => moved,
    }
}

/// A `json!` object as the fields of a request.
fn fields(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(fields) => fields,
        _ => Map::new(),
    }
}

impl Store for Exec {
    fn show(&self, item: &str) -> Result<Item, StoreError> {
        let (Shown { item: read }, raw) = self.call("show", fields(json!({ "id": item })))?;
        Ok(Item {
            proof: ReadProof::of(raw),
            ..read
        })
    }

    fn resolve(&self, id: &str) -> Result<ItemId, StoreError> {
        let (Resolved { id }, _) = self.call("resolve", fields(json!({ "id": id })))?;
        Ok(id)
    }

    fn list(&self, filter: &Filter) -> Result<Vec<ItemSummary>, StoreError> {
        let (Listed { items }, _) = self.call("list", fields(json!({ "filter": filter })))?;
        Ok(items)
    }

    fn create(&self, item: &NewItem, by: &Actor) -> Result<ItemId, StoreError> {
        validated_new(item)?;
        let (Created { id }, _) = self.call("create", fields(json!({ "item": item, "by": by })))?;
        Ok(id)
    }

    /// A change no store takes is refused here and the adapter is never run:
    /// a status other than `open` is usage on this side of the call too.
    fn update(&self, id: &ItemId, change: &Update, by: &Actor) -> Result<(), StoreError> {
        writable(change)?;
        let mut request = fields(json!({ "id": id, "by": by }));
        request.extend(fields(json!(change)));
        self.call::<Answered>("update", request).map(|_| ())
    }

    fn order_set(&self, id: &ItemId, order: &Order, by: &Actor) -> Result<(), StoreError> {
        self.call::<Answered>(
            "order.set",
            fields(json!({ "id": id, "by": by, "order": order })),
        )
        .map(|_| ())
    }

    /// The fence's fields beside the id, each only where it is set: the plain
    /// withdrawal is the request it always was.
    fn order_withdraw(
        &self,
        id: &ItemId,
        fence: &WithdrawFence,
        by: &Actor,
    ) -> Result<(), StoreError> {
        let mut request = fields(json!({ "id": id, "by": by }));
        request.extend(fields(json!(fence)));
        self.call::<Answered>("order.withdraw", request).map(|_| ())
    }

    fn run_set(&self, id: &ItemId, run: &RunRecord, by: &Actor) -> Result<(), StoreError> {
        self.call::<Answered>("run.set", fields(json!({ "id": id, "by": by, "run": run })))
            .map(|_| ())
    }

    /// The contract's reopen is an `update` whose status is `open`, under the
    /// actor the text types as: text that is no typed actor is Unreadable,
    /// with nothing asked.
    fn reopen(&self, item: &str, by: &str) -> Result<(), StoreError> {
        let actor = match Actor::typed(by) {
            Some(Ok(actor)) => actor,
            Some(Err(why)) => {
                return Err(StoreError::Unreadable(format!(
                    "{why} — nothing was written"
                )))
            }
            None => {
                return Err(StoreError::Unreadable(format!(
                    "`{by}` is not a typed actor, and a reopen of {item} is written under one \
                     — nothing was written"
                )))
            }
        };
        let reopened = Update {
            status: Some(super::Status::Open),
            ..Update::default()
        };
        self.update(&ItemId::from(item), &reopened, &actor)
    }

    fn hold_raise(&self, id: &ItemId, reason: &str, by: &Actor) -> Result<HoldId, StoreError> {
        let (Raised { hold }, _) = self.call(
            "hold.raise",
            fields(json!({ "id": id, "by": by, "reason": reason })),
        )?;
        Ok(hold)
    }

    fn hold_clear(&self, hold: &HoldId, by: &Actor) -> Result<(), StoreError> {
        self.call::<Answered>("hold.clear", fields(json!({ "hold": hold, "by": by })))
            .map(|_| ())
    }

    fn holds_open(&self) -> Result<Vec<HoldId>, StoreError> {
        let (OpenHolds { holds }, _) = self.call("holds.open", Map::new())?;
        Ok(holds)
    }

    fn close(&self, id: &ItemId, reason: &str, by: &Actor) -> Result<(), StoreError> {
        self.call::<Answered>(
            "close",
            fields(json!({ "id": id, "by": by, "reason": reason })),
        )
        .map(|_| ())
    }

    fn append(&self, item: &ItemId, body: &Body, by: &Actor) -> Result<String, StoreError> {
        validated(item, body)?;
        let entry: Value = serde_json::from_str(&entry::encode(body)).map_err(|why| {
            StoreError::Unreadable(format!(
                "the {} entry for {item} did not encode: {why} — nothing was written",
                body.kind()
            ))
        })?;
        let (Appended { entry }, _) = self.call(
            "append",
            fields(json!({ "id": item, "by": by, "entry": entry })),
        )?;
        Ok(entry)
    }

    /// Each entry as `fleet item show --json` prints one ([`entry::to_json`]):
    /// the body's fields and `kind` beside the store's `id` and `at` and the
    /// actor as its `<kind>:<id>` string. The body is read through the same
    /// reader the built-in adapter's comments go through, so an entry that
    /// does not read — an actor in any other shape among them — refuses the
    /// whole timeline, naming it.
    fn timeline(&self, item: &ItemId) -> Result<Vec<Entry>, StoreError> {
        let (Entries { entries }, _) = self.call("timeline", fields(json!({ "id": item })))?;
        entries
            .into_iter()
            .enumerate()
            .map(|(at, row)| {
                entry_of(item, row).map_err(|why| {
                    StoreError::Unreadable(format!(
                        "{} timeline answered an entry for {item}, row {at}, that does not \
                         read: {why}",
                        self.adapter.display()
                    ))
                })
            })
            .collect()
    }

    /// An export the adapter declares is held to the contract's rules for
    /// one, because a landing commits what it names.
    fn capabilities(&self) -> Result<Capabilities, StoreError> {
        let (declared, _) = self.call::<Capabilities>("capabilities", Map::new())?;
        if let Some(export) = &declared.export {
            export.validate().map_err(|why| {
                StoreError::Unreadable(format!(
                    "{} capabilities answered no readable response: {why}",
                    self.adapter.display()
                ))
            })?;
        }
        Ok(declared)
    }

    fn version(&self) -> Result<Version, StoreError> {
        self.call("version", Map::new()).map(|(version, _)| version)
    }

    /// Asked only of an adapter whose capabilities declare an export, and
    /// `into` goes out absolute, because the adapter's working directory is
    /// never relied on.
    fn export(&self, into: &Path) -> Result<PathBuf, StoreError> {
        if self.capabilities()?.export.is_none() {
            return Err(StoreError::Unreadable(String::from(
                "the adapter declares no export",
            )));
        }
        let into = std::path::absolute(into).unwrap_or_else(|_| into.to_path_buf());
        let (Exported { file }, _) = self.call(
            "export",
            fields(json!({ "into": into.display().to_string() })),
        )?;
        Ok(PathBuf::from(file))
    }

    /// Asked only of an adapter whose capabilities declare a scratch store,
    /// and `into` goes out absolute, as the export's does.
    fn scratch(&self, into: &Path) -> Result<PathBuf, StoreError> {
        if !self.capabilities()?.scratch {
            return Err(StoreError::Unreadable(String::from(
                "the adapter declares no scratch",
            )));
        }
        let into = std::path::absolute(into).unwrap_or_else(|_| into.to_path_buf());
        let (Scratched { root }, _) = self.call(
            "scratch",
            fields(json!({ "into": into.display().to_string() })),
        )?;
        Ok(PathBuf::from(root))
    }
}

/// One timeline entry read into an [`Entry`]: `id`, `at` and `by` taken off
/// the object as text, and the rest read as the entry's text is, with the
/// entry key stamped in where the adapter left it out. `by` is the actor's
/// one string and is read as a comment's author is, so a `by` in any other
/// shape is no actor.
fn entry_of(item: &ItemId, row: Value) -> Result<Entry, String> {
    let Value::Object(mut object) = row else {
        return Err(String::from("it is not an object"));
    };
    let text_of = |object: &mut Map<String, Value>, key: &str| match object.remove(key) {
        Some(Value::String(text)) => Ok(text),
        Some(other) => Err(format!("its {key} is {other}, which is not text")),
        None => Err(format!("it carries no {key}")),
    };
    let id = text_of(&mut object, "id")?;
    let at = text_of(&mut object, "at")?;
    let by = text_of(&mut object, "by")?;
    object
        .entry(entry::KEY)
        .or_insert_with(|| Value::from(entry::VERSION));
    let text = Value::Object(object).to_string();
    entry::read_row(item, &id, &by, &text, &at)?
        .ok_or_else(|| format!("{item}'s entry {id} is not fleet's"))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    use super::*;
    use crate::process::DRAIN_GRACE;

    const SEAT: &str = "0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718";

    /// The item `docs/store.md` prints, as a `show` answer.
    const SHOWN: &str = r#"{"schema_version":1,"item":{"id":"fx-a1b2","title":"Teach the parser the new stamp","status":"in_progress","type":"task","labels":["fleet"],"assignee":"0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718","order":{"state":"ordered","order":{"kind":"dispatch","by":"seat:0199a3c4-5e6f-7a8b-9c0d-1e2f3a4b5c6d","seat":"0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718","at":"2026-09-23T10:00:00Z"}},"blockers":["fx-c3d4"],"run":null}}"#;

    /// An adapter written as a `#!/bin/sh` stub in a directory of its own,
    /// which is also the project root it is handed. The stub writes its pid to
    /// `pid`, appends its verb to `argv`, copies its stdin to `request.json`,
    /// and then runs `answer`.
    struct Stub {
        dir: PathBuf,
        bin: PathBuf,
    }

    impl Stub {
        fn new(label: &str, answer: &str) -> Stub {
            static N: AtomicUsize = AtomicUsize::new(0);
            let n = N.fetch_add(1, Ordering::SeqCst);
            let dir =
                std::env::temp_dir().join(format!("fleet-exec-{label}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("the stub's directory is made");
            let bin = dir.join("adapter");
            std::fs::write(
                &bin,
                format!(
                    "#!/bin/sh\n\
                     echo $$ > '{dir}/pid'\n\
                     printf '%s\\n' \"$1\" >> '{dir}/argv'\n\
                     cat > '{dir}/request.json'\n\
                     {answer}\n",
                    dir = dir.display(),
                ),
            )
            .expect("the stub is written");
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
                .expect("the stub is executable");
            Stub { dir, bin }
        }

        fn exec(&self) -> Exec {
            Exec::at(&self.bin, &self.dir)
        }

        /// The last request the stub was handed, as JSON.
        fn request(&self) -> Value {
            let text = std::fs::read_to_string(self.dir.join("request.json"))
                .expect("the stub recorded its request");
            serde_json::from_str(&text).expect("the request is one JSON value")
        }

        /// Every verb the stub was called with, in order.
        fn verbs(&self) -> Vec<String> {
            std::fs::read_to_string(self.dir.join("argv"))
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect()
        }
    }

    impl Drop for Stub {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// A stub body that prints `json` and exits `code`.
    fn answers(json: &str, code: i32) -> String {
        format!("cat <<'JSON'\n{json}\nJSON\nexit {code}")
    }

    fn id() -> ItemId {
        ItemId::from("fx-a1b2")
    }

    fn by() -> Actor {
        Actor::typed("routine:nightly")
            .expect("typed")
            .expect("a routine")
    }

    fn unreadable(result: Result<impl std::fmt::Debug, StoreError>) -> String {
        match result {
            Err(StoreError::Unreadable(why)) => why,
            other => panic!("wanted Unreadable, got {other:?}"),
        }
    }

    fn refused(result: Result<impl std::fmt::Debug, StoreError>) -> String {
        match result {
            Err(StoreError::Refused(why)) => why,
            other => panic!("wanted Refused, got {other:?}"),
        }
    }

    /// Arm 1. One call is one `<adapter> show` process, carrying the request
    /// on stdin — the id beside the envelope — and the answer reads as the
    /// item, with the whole of stdout as its proof.
    #[test]
    fn a_show_is_one_process_carrying_the_request_and_reading_the_item() {
        let stub = Stub::new("show", &answers(SHOWN, 0));
        let item = stub.exec().show("a1b2").expect("the stub answered an item");

        assert_eq!(
            stub.verbs(),
            ["show"],
            "argv[1] is the verb, and one call ran"
        );
        let request = stub.request();
        assert_eq!(request["schema_version"], 1);
        assert_eq!(request["root"], stub.dir.display().to_string());
        assert_eq!(request["id"], "a1b2");

        assert_eq!(item.id, "fx-a1b2");
        assert_eq!(item.title, "Teach the parser the new stamp");
        assert_eq!(item.status, "in_progress");
        assert_eq!(item.item_type, "task");
        assert_eq!(item.labels, ["fleet"]);
        assert_eq!(
            item.assignee.map(|seat| seat.to_string()).as_deref(),
            Some(SEAT)
        );
        assert!(matches!(item.order, types::OrderState::Ordered(_)));
        assert_eq!(item.blockers, [ItemId::from("fx-c3d4")]);
        assert_eq!(item.run, None);
        assert!(
            item.proof.carries("Teach the parser the new stamp"),
            "the proof is the raw answer"
        );
    }

    /// Arm 2. Exit 1 is the record's refusal, read by its reason: an
    /// ambiguous id names every candidate, and a missing one is not in the
    /// store.
    #[test]
    fn an_exit_1_is_refused_by_its_reason() {
        let stub = Stub::new(
            "ambiguous",
            &answers(
                r#"{"schema_version":1,"refused":{"reason":"ambiguous","message":"a1 matches more than one item","candidates":["fx-a1b2","fx-a1c9"]}}"#,
                1,
            ),
        );
        let why = refused(stub.exec().resolve("a1"));
        assert_eq!(
            why,
            "a1 matches more than one item — fx-a1b2, fx-a1c9 — and more of the id says which \
             one this is"
        );

        let stub = Stub::new(
            "missing",
            &answers(
                r#"{"schema_version":1,"refused":{"reason":"missing","message":"nothing is zz"}}"#,
                1,
            ),
        );
        assert_eq!(refused(stub.exec().show("zz")), "zz is not in the store");

        let stub = Stub::new(
            "already",
            &answers(
                r#"{"schema_version":1,"refused":{"reason":"already","message":"fx-a1b2 is already closed"}}"#,
                1,
            ),
        );
        assert_eq!(
            refused(stub.exec().close(&id(), "done", &by())),
            "fx-a1b2 is already closed"
        );

        let stub = Stub::new("garbled", "echo 'not a refusal'; exit 1");
        let why = unreadable(stub.exec().show("a1b2"));
        assert!(
            why.ends_with("show refused with no readable refusal: not a refusal"),
            "{why}"
        );
    }

    /// Arm 3. The table's other rows, and a code that is none of them.
    #[test]
    fn exits_2_3_and_one_off_the_table_are_unreadable_by_their_row() {
        let stub = Stub::new("usage", "exit 2");
        let why = unreadable(stub.exec().version());
        assert!(
            why.contains("refused the request as usage (exit 2)"),
            "{why}"
        );
        assert!(
            why.contains("this fleet and the adapter do not speak the same store contract"),
            "{why}"
        );

        let stub = Stub::new(
            "untold",
            &answers(
                r#"{"schema_version":1,"error":"the database is locked"}"#,
                3,
            ),
        );
        let why = unreadable(stub.exec().holds_open());
        assert!(
            why.ends_with("holds.open could not tell: the database is locked"),
            "{why}"
        );

        let stub = Stub::new("seven", "exit 7");
        let why = unreadable(stub.exec().version());
        assert!(
            why.contains("version exited 7, which is not a row of the store contract's exit table"),
            "{why}"
        );

        let stub = Stub::new("signal", "kill -9 $$");
        let why = unreadable(stub.exec().version());
        assert!(why.contains("version was ended by a signal"), "{why}");
    }

    /// Arm 4. An answer at no `schema_version`, or at one this fleet does not
    /// speak, is could not tell whatever its fields say.
    ///
    /// RED-PROOF: with the exit-0 branch decoding the first value straight
    /// into the body and skipping the version, both answers read as the id.
    #[test]
    fn an_answer_at_no_schema_version_or_another_is_unreadable() {
        for (label, answer) in [
            ("unversioned", r#"{"id":"fx-a1b2"}"#),
            ("version-2", r#"{"schema_version":2,"id":"fx-a1b2"}"#),
        ] {
            let stub = Stub::new(label, &answers(answer, 0));
            let why = unreadable(stub.exec().resolve("a1b2"));
            assert!(
                why.contains("resolve answered no readable response"),
                "{label}: {why}"
            );
        }
    }

    /// Arm 5. A call that outruns its bound is killed with its group inside
    /// the bound and a grace, and is could not tell; the same call as a write
    /// says its effect cannot be told.
    ///
    /// The stub is RUN ONCE BEFORE THE CALL, answering at once: the first
    /// exec of a script just written can take longer than the whole bound on
    /// macOS, and a stub killed before it wrote its pid proves nothing about
    /// the kill.
    #[test]
    fn a_call_that_outruns_its_bound_is_killed_and_unreadable() {
        let bound = Duration::from_secs(1);
        for (verb, suffix) in [("show", ""), ("update", UNTOLD)] {
            let stub = Stub::new(verb, "[ \"$1\" = warm ] && exit 0\nsleep 5");
            let warmed = Command::new(&stub.bin)
                .arg("warm")
                .stdin(std::process::Stdio::null())
                .status()
                .expect("the stub runs");
            assert!(warmed.success(), "{verb}: the warm-up answered");
            let _ = std::fs::remove_file(stub.dir.join("pid"));

            let exec = stub.exec().with_timeout(bound);
            let started = Instant::now();
            let result = match verb {
                "show" => exec.show("a1b2").map(|_| ()),
                _ => exec.update(&id(), &Update::title(String::from("t")), &by()),
            };
            let took = started.elapsed();
            let why = unreadable(result);
            assert!(
                took < bound + DRAIN_GRACE,
                "{verb}: the call answered within its bound and a grace: {took:?}"
            );
            assert_eq!(
                why,
                format!(
                    "{} {verb} did not answer within 1s{suffix}",
                    stub.bin.display()
                )
            );
            let pid =
                std::fs::read_to_string(stub.dir.join("pid")).expect("the stub wrote its pid");
            let alive = Command::new("/bin/sh")
                .arg("-c")
                .arg(format!("kill -0 {} 2>/dev/null", pid.trim()))
                .status()
                .expect("kill -0 ran")
                .success();
            assert!(!alive, "{verb}: the stub is gone");
        }
    }

    /// Arm 6. What the adapter said last on stderr is carried into the
    /// refusal — as the reason where it is the tail, and beside the reason
    /// where the answer named one of its own.
    #[test]
    fn the_adapters_stderr_is_carried_into_the_refusal() {
        let stub = Stub::new("stderr-tail", "echo boom >&2; exit 3");
        let why = unreadable(stub.exec().version());
        assert!(why.ends_with("version could not tell: boom"), "{why}");

        let stub = Stub::new(
            "stderr-error",
            &format!(
                "echo boom >&2\n{}",
                answers(
                    r#"{"schema_version":1,"error":"the database is locked"}"#,
                    3
                )
            ),
        );
        let why = unreadable(stub.exec().version());
        assert!(
            why.ends_with("could not tell: the database is locked (the adapter said: boom)"),
            "{why}"
        );

        let stub = Stub::new(
            "stderr-missing",
            &format!(
                "echo boom >&2\n{}",
                answers(
                    r#"{"schema_version":1,"refused":{"reason":"missing","message":"no"}}"#,
                    1
                )
            ),
        );
        assert_eq!(
            refused(stub.exec().show("zz")),
            "zz is not in the store (the adapter said: boom)"
        );
    }

    /// Each write carries its verb's fields, `by` as the actor's text — the
    /// close's too.
    #[test]
    fn a_write_carries_its_verbs_fields() {
        let stub = Stub::new("update", &answers(r#"{"schema_version":1}"#, 0));
        stub.exec()
            .update(&id(), &Update::unassigned(), &by())
            .expect("the update was taken");
        let request = stub.request();
        assert_eq!(request["id"], "fx-a1b2");
        assert_eq!(request["by"], "routine:nightly");
        assert_eq!(
            request["assignee"],
            Value::Null,
            "null hands the item to nobody"
        );
        assert!(
            request.get("title").is_none(),
            "a title left out is not sent"
        );

        let stub = Stub::new("close", &answers(r#"{"schema_version":1}"#, 0));
        let seat = crate::seat::identity::SeatId::parse(SEAT).expect("a seat id");
        stub.exec()
            .close(&id(), "landed", &Actor::seat(seat))
            .expect("the close was taken");
        let request = stub.request();
        assert_eq!(
            request["by"],
            format!("seat:{SEAT}"),
            "the close's by is the actor's text, as every write's is"
        );
        assert_eq!(request["reason"], "landed");

        let stub = Stub::new("unchanged", &answers(r#"{"schema_version":1}"#, 0));
        let why = unreadable(stub.exec().update(&id(), &Update::default(), &by()));
        assert!(why.starts_with("an update names no title"), "{why}");
        assert!(stub.verbs().is_empty(), "nothing was run");

        let stub = Stub::new(
            "priority",
            &answers(r#"{"schema_version":1,"id":"fx-n"}"#, 0),
        );
        let item = NewItem {
            title: String::from("t"),
            item_type: String::from("task"),
            priority: Some(5),
            ..NewItem::default()
        };
        assert_eq!(
            unreadable(stub.exec().create(&item, &by())),
            "the item `t` does not validate: priority is 5; the range is 0 to 4 — nothing was written"
        );
        assert!(stub.verbs().is_empty(), "nothing was sent");
    }

    /// The fences go out as the verb's own fields — `null` for nobody — and
    /// the plain withdrawal carries none of them.
    #[test]
    fn a_fenced_write_carries_its_fence_and_a_plain_one_none() {
        let seat = crate::seat::identity::SeatId::parse(SEAT).expect("a seat id");
        let stub = Stub::new("fenced-update", &answers(r#"{"schema_version":1}"#, 0));
        let reopened = Update {
            status: Some(types::Status::Open),
            ..Update::fenced(None)
        };
        stub.exec()
            .update(&id(), &reopened, &by())
            .expect("the update was taken");
        let request = stub.request();
        assert_eq!(request["if_assignee"], Value::Null, "{request}");
        assert!(request
            .as_object()
            .expect("an object")
            .contains_key("if_assignee"));
        assert_eq!(request["status"], "open");
        assert!(request.get("assignee").is_none(), "{request}");

        let stub = Stub::new("plain-withdraw", &answers(r#"{"schema_version":1}"#, 0));
        stub.exec()
            .order_withdraw(&id(), &WithdrawFence::default(), &by())
            .expect("the withdrawal was taken");
        assert_eq!(
            stub.request(),
            json!({
                "id": "fx-a1b2",
                "by": "routine:nightly",
                "schema_version": 1,
                "root": stub.dir.display().to_string(),
            })
        );

        let stub = Stub::new("fenced-withdraw", &answers(r#"{"schema_version":1}"#, 0));
        let fence = WithdrawFence {
            if_assignee: Some(Some(seat)),
            if_status: Some(types::Status::InProgress),
            reopen: true,
        };
        stub.exec()
            .order_withdraw(&id(), &fence, &by())
            .expect("the withdrawal was taken");
        let request = stub.request();
        assert_eq!(request["if_assignee"], SEAT);
        assert_eq!(request["if_status"], "in_progress");
        assert_eq!(request["reopen"], true);
    }

    /// A status other than `open` is usage, answered before the adapter is
    /// run — the exit-2 row of the caller's own table.
    #[test]
    fn a_status_other_than_open_is_usage_and_runs_nothing() {
        let stub = Stub::new("closing", &answers(r#"{"schema_version":1}"#, 0));
        let closing = Update {
            status: Some(types::Status::Closed),
            ..Update::default()
        };
        match stub.exec().update(&id(), &closing, &by()) {
            Err(StoreError::Usage(why)) => assert_eq!(
                why,
                "an update sets a status only to open, and this one names `closed` — nothing \
                 was written"
            ),
            other => panic!("wanted Usage, got {other:?}"),
        }
        assert!(stub.verbs().is_empty(), "nothing was run");
    }

    /// Exit 1 with `moved` is the fence the item did not meet, in the
    /// adapter's words, which name what holds it.
    #[test]
    fn an_exit_1_moved_is_moved_in_the_adapters_words() {
        let stub = Stub::new(
            "moved",
            &answers(
                r#"{"schema_version":1,"refused":{"reason":"moved","message":"fx-a1b2 is held by 0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718"}}"#,
                1,
            ),
        );
        let answer = stub.exec().update(
            &id(),
            &Update {
                assignee: Some(None),
                ..Update::fenced(None)
            },
            &by(),
        );
        assert_eq!(
            answer,
            Err(StoreError::Moved(format!("fx-a1b2 is held by {SEAT}")))
        );
    }

    /// The reopen is the contract's `update` with status `open`.
    #[test]
    fn a_reopen_is_an_update_to_open() {
        let stub = Stub::new("reopen", &answers(r#"{"schema_version":1}"#, 0));
        stub.exec()
            .reopen("fx-a1b2", "routine:nightly")
            .expect("the reopen was taken");
        assert_eq!(stub.verbs(), ["update"]);
        let request = stub.request();
        assert_eq!(request["status"], "open");
        assert_eq!(request["by"], "routine:nightly");
    }

    /// The description an adapter answers is the item's.
    #[test]
    fn a_show_answers_the_description_the_adapter_sent() {
        let shown = SHOWN.replacen(
            r#""status""#,
            r#""description":"The stamp gains a seconds field.","status""#,
            1,
        );
        let stub = Stub::new("described", &answers(&shown, 0));
        let item = stub.exec().show("a1b2").expect("the stub answered an item");
        assert_eq!(item.description, "The stamp gains a seconds field.");
    }

    /// A timeline's entries read in the shape `fleet item show --json` prints
    /// them, and one that does not read refuses the whole timeline.
    #[test]
    fn a_timeline_reads_its_entries_and_refuses_one_that_does_not_read() {
        let ordered = Entry {
            id: String::from("e-1"),
            at: String::from("2026-09-23T10:00:00Z"),
            by: by(),
            body: Body::Ordered(entry::Ordered {
                order: crate::store::OrderKind::Dispatch,
                seat: Some(crate::seat::identity::SeatId::parse(SEAT).expect("a seat id")),
            }),
        };
        let mut second = entry::to_json(&ordered);
        second["id"] = Value::from("e-2");
        let answer = json!({
            "schema_version": 1,
            "entries": [entry::to_json(&ordered), second],
        });
        let stub = Stub::new("timeline", &answers(&answer.to_string(), 0));
        let entries = stub.exec().timeline(&id()).expect("the entries read");
        let second = Entry {
            id: String::from("e-2"),
            ..ordered.clone()
        };
        assert_eq!(entries, [ordered, second], "in the order they came");
        assert_eq!(stub.request()["id"], "fx-a1b2");

        let stub = Stub::new(
            "timeline-bad",
            &answers(
                r#"{"schema_version":1,"entries":[{"kind":"held","id":"e-1","at":"2026-09-23T10:00:00Z"}]}"#,
                0,
            ),
        );
        let why = unreadable(stub.exec().timeline(&id()));
        assert!(
            why.contains("row 0, that does not read: it carries no by"),
            "{why}"
        );
    }

    /// An entry's `by` is the actor's one string, `<kind>:<id>`: the same
    /// actor as a `{"kind", "id"}` object is no actor, and refuses the whole
    /// timeline, naming the row.
    #[test]
    fn a_timeline_entry_whose_by_is_not_the_actors_string_refuses_the_timeline() {
        let good = json!({
            "kind": "ordered", "order": "dispatch",
            "id": "e-1", "at": "2026-09-23T10:00:00Z", "by": "routine:nightly",
        });
        let mut object = good.clone();
        object["id"] = Value::from("e-2");
        object["by"] = json!({ "kind": "routine", "id": "nightly" });
        let answer = json!({ "schema_version": 1, "entries": [good, object] });
        let stub = Stub::new("timeline-object-by", &answers(&answer.to_string(), 0));
        let why = unreadable(stub.exec().timeline(&id()));
        assert!(
            why.contains("row 1, that does not read: its by is"),
            "{why}"
        );
        assert!(why.contains("which is not text"), "{why}");
    }

    /// An export is asked only of an adapter that declares one.
    #[test]
    fn an_export_is_asked_only_where_it_is_declared() {
        let stub = Stub::new(
            "no-export",
            &answers(r#"{"schema_version":1,"export":null}"#, 0),
        );
        let why = unreadable(stub.exec().export(&stub.dir));
        assert_eq!(why, "the adapter declares no export");
        assert_eq!(stub.verbs(), ["capabilities"], "export was never run");

        let stub = Stub::new(
            "export",
            r#"case "$1" in
capabilities) echo '{"schema_version":1,"export":{"file":"store/export.jsonl","dir":"store/"}}' ;;
export) echo '{"schema_version":1,"file":"/work/store/export.jsonl"}' ;;
esac"#,
        );
        let written = stub
            .exec()
            .export(&stub.dir)
            .expect("the export was written");
        assert_eq!(written, PathBuf::from("/work/store/export.jsonl"));
        assert_eq!(stub.verbs(), ["capabilities", "export"]);
        assert_eq!(stub.request()["into"], stub.dir.display().to_string());
    }

    /// A scratch store is asked only of an adapter that declares one, and
    /// its root is the one the adapter answered.
    ///
    /// RED-PROOF: with no `scratch` of its own, `Exec` takes the trait's
    /// default, which runs nothing and answers "this store declares no
    /// scratch" — for the adapter that declares one too.
    #[test]
    fn a_scratch_is_asked_only_where_it_is_declared() {
        let stub = Stub::new(
            "no-scratch",
            &answers(r#"{"schema_version":1,"scratch":false}"#, 0),
        );
        let why = unreadable(stub.exec().scratch(&stub.dir.join("store")));
        assert_eq!(why, "the adapter declares no scratch");
        assert_eq!(stub.verbs(), ["capabilities"], "scratch was never run");

        let stub = Stub::new(
            "scratch",
            r#"case "$1" in
capabilities) echo '{"schema_version":1,"scratch":true}' ;;
scratch) echo '{"schema_version":1,"root":"/tmp/fleet-scratch/store"}' ;;
esac"#,
        );
        let into = stub.dir.join("store");
        let root = stub.exec().scratch(&into).expect("the scratch was made");
        assert_eq!(root, PathBuf::from("/tmp/fleet-scratch/store"));
        assert_eq!(stub.verbs(), ["capabilities", "scratch"]);
        assert_eq!(stub.request()["into"], into.display().to_string());
    }
}
