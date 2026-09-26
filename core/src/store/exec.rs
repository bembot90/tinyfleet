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
//! THE CALL ITSELF IS EVERY ADAPTER'S: the spawn, the bound and its group
//! kill, the row an exit is and the line the adapter said last are
//! [`crate::adapter::exec`]'s, which the agent contract's caller shares. What
//! is the store's here is the words: each row becomes a refusal naming the
//! store contract, a refusal on the record is read by its reason, and a write
//! that outran its bound says its effect cannot be told.
//!
//! BOUNDED like every store call: [`STORE_TIMEOUT`], then the adapter's whole
//! process group is killed and the call is could not tell — and for a write,
//! a write whose effect cannot be told.
//!
//! SELECTED BY `[store] adapter` naming an executable by absolute path, or by
//! a name an installed pack carries, which [`super::open`] reads out of the
//! project's own file.
//!
//! ITS `PATH` IS THE OPENER'S TO GIVE: a named adapter runs on the caller's
//! constructed search path ([`Exec::on_path`]), and one given none carries
//! this process's own.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::types::{
    self, Answered, Appended, Capabilities, Created, Exported, Listed, OpenHolds, Raised, Refusal,
    RefusalReason, Resolved, Scratched, Shown,
};
use super::{
    validated, validated_new, writable, Filter, HoldId, Item, ItemId, ItemSummary, NewItem, Order,
    ReadProof, RunRecord, Store, StoreError, Update, Version, WithdrawFence, STORE_TIMEOUT,
};
use crate::adapter::exec::{self, Exited, Ran, Unrun};
use crate::entry::{self, Body, Entry};
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
/// say whether the write landed before it.
const UNTOLD: &str = " — the write's effect cannot be told, so the item must be read before \
                      anything is written to it again";

/// The store as an adapter executable, scoped to one project.
pub struct Exec {
    adapter: PathBuf,
    root: PathBuf,
    timeout: Duration,
    /// The `PATH` every call runs under, or `None` for this process's own.
    path: Option<String>,
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
            path: None,
        }
    }

    /// The same store under another bound.
    pub fn with_timeout(self, timeout: Duration) -> Exec {
        Exec { timeout, ..self }
    }

    /// The same store with every call run under `path` as its `PATH`, in
    /// place of this process's own: the search path an entry resolves its
    /// runtime and its store's binary on, which under a service is the
    /// constructed one and never the manager's.
    pub fn on_path(self, path: String) -> Exec {
        Exec {
            path: Some(path),
            ..self
        }
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
        let request = types::request(fields, &self.root);
        let ran = exec::run(
            &self.adapter,
            verb,
            &request,
            self.timeout,
            self.path.as_deref(),
        )
        .map_err(|unrun| match unrun {
            Unrun::CouldNotRun(why) => StoreError::Unreadable(format!(
                "{adapter} could not be run ({why}) — nothing was written"
            )),
            Unrun::Deadline(why) => {
                let mut refusal = format!("{named} {why}");
                if WRITES.contains(&verb) {
                    refusal.push_str(UNTOLD);
                }
                StoreError::Unreadable(refusal)
            }
        })?;
        let said = &ran.said;
        let read = match &ran.exited {
            Exited::Answered(stdout) => types::answer::<T>(stdout)
                .map(|body| (body, stdout.clone()))
                .map_err(|why| {
                    StoreError::Unreadable(format!("{named} answered no readable response: {why}"))
                }),
            Exited::Refused(stdout) => Err(match types::answer::<Refused>(stdout) {
                Ok(Refused { refused }) => on_the_record(refused, subject),
                Err(_) => StoreError::Unreadable(format!(
                    "{named} refused with no readable refusal: {said}"
                )),
            }),
            Exited::Usage => Err(StoreError::Unreadable(format!(
                "{named} refused the request as usage (exit 2) — this fleet and the adapter do \
                 not speak the same store contract: {said}"
            ))),
            Exited::CouldNotTell(error) => Err(StoreError::Unreadable(format!(
                "{named} could not tell: {}",
                error.as_deref().unwrap_or(said)
            ))),
            Exited::OffTable(code) => Err(StoreError::Unreadable(format!(
                "{named} exited {code}, which is not a row of the store contract's exit table: \
                 {said}"
            ))),
            Exited::Signalled => Err(StoreError::Unreadable(format!(
                "{named} was ended by a signal: {said}"
            ))),
        };
        read.map_err(|refused| carrying_stderr(refused, &ran))
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
/// said anything there and the refusal does not already carry it
/// ([`exec::carrying`]). A fence the item did not meet is left in the
/// adapter's own words, which already name what holds it.
fn carrying_stderr(refused: StoreError, ran: &Ran) -> StoreError {
    match refused {
        StoreError::Refused(text) => StoreError::Refused(exec::carrying(text, ran)),
        StoreError::Unreadable(text) => StoreError::Unreadable(exec::carrying(text, ran)),
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
    /// actor as its `<kind>:<id>` string. The body is read through
    /// [`entry::read_row`], the one reader of an entry, so an entry that does
    /// not read — an actor in any other shape among them — refuses the whole
    /// timeline, naming it.
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

    /// What the adapter declares is held to the contract's rules for it,
    /// because a landing commits the export it names, the guards police the
    /// command it names, and a routine's item is held to its items.
    fn capabilities(&self) -> Result<Capabilities, StoreError> {
        let (declared, _) = self.call::<Capabilities>("capabilities", Map::new())?;
        declared.validate().map_err(|why| {
            StoreError::Unreadable(format!(
                "{} capabilities answered no readable response: {why}",
                self.adapter.display()
            ))
        })?;
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
    use std::process::Command;
    use std::time::Instant;

    use super::*;
    use crate::adapter::exec::tests::{answers, Stub};
    use crate::process::DRAIN_GRACE;

    const SEAT: &str = "0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718";

    /// The item `docs/store.md` prints, as a `show` answer.
    const SHOWN: &str = r#"{"schema_version":1,"item":{"id":"fx-a1b2","title":"Teach the parser the new stamp","status":"in_progress","type":"task","labels":["fleet"],"assignee":"0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718","order":{"state":"ordered","order":{"kind":"dispatch","by":"seat:0199a3c4-5e6f-7a8b-9c0d-1e2f3a4b5c6d","seat":"0199a3c4-7d8e-7f90-a1b2-c3d4e5f60718","at":"2026-09-23T10:00:00Z"}},"blockers":["fx-c3d4"],"run":null,"foreign":["sprint"]}}"#;

    /// The store over the stub, scoped to the stub's own directory as the
    /// project root: the one method the store adds to every adapter's rig.
    impl Stub {
        fn exec(&self) -> Exec {
            Exec::at(&self.bin, &self.dir)
        }
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

    /// A call runs on the `PATH` the store was given, in place of this
    /// process's own — the search path an entry resolves its runtime and its
    /// store's binary on — and on this process's own where it was given none.
    ///
    /// RED-PROOF: with the `PATH` never set on the command, the first reading
    /// is this process's own `PATH` and not the one given.
    #[test]
    fn a_call_runs_on_the_path_it_was_given_and_on_this_processs_own_without_one() {
        let seen = |exec: Exec, stub: &Stub| {
            exec.show("a1b2").expect("the stub answered an item");
            std::fs::read_to_string(stub.dir.join("path")).expect("the stub recorded its PATH")
        };
        // `${0%/*}` is the stub's own directory, read without a program: the
        // `PATH` under test need hold none but the `cat` the stub runs.
        let body = format!(
            "printf '%s' \"$PATH\" > \"${{0%/*}}/path\"\n{}",
            answers(SHOWN, 0)
        );
        let given = "/usr/bin:/bin:/fleet-exec-given-path";

        let stub = Stub::new("path-given", &body);
        assert_eq!(seen(stub.exec().on_path(given.to_string()), &stub), given);

        let stub = Stub::new("path-own", &body);
        assert_eq!(
            seen(stub.exec(), &stub),
            std::env::var("PATH").unwrap_or_default(),
            "a store given no path carries this process's own"
        );
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
        assert_eq!(item.foreign, ["sprint"]);
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

    /// A listing's rows carry the store's other keys as the adapter names
    /// them, and a row naming none reads as naming none: an adapter that
    /// leaves `foreign` out still answers.
    #[test]
    fn a_listing_reads_each_rows_foreign_keys_and_none_where_it_names_none() {
        let stub = Stub::new(
            "list",
            &answers(
                r#"{"schema_version":1,"items":[{"id":"fx-c3d4","title":"Name the stamp's fields","status":"open","type":"task","labels":["fleet"],"order":{"state":"none"},"foreign":["sprint"]},{"id":"fx-e5f6","title":"t","status":"open","type":"task"}]}"#,
                0,
            ),
        );
        let rows = stub
            .exec()
            .list(&Filter::Ready)
            .expect("the stub answered rows");
        assert_eq!(stub.verbs(), ["list"]);
        assert_eq!(stub.request()["filter"], "ready");
        let foreign: Vec<&[String]> = rows.iter().map(|row| row.foreign.as_slice()).collect();
        assert_eq!(foreign, [&[String::from("sprint")][..], &[][..]]);
    }

    /// The command and the items an adapter declares are read as it answers
    /// them, and a declaration that breaks the contract's rules is no reading
    /// at all.
    #[test]
    fn a_declared_cli_and_items_are_read_and_a_broken_one_is_refused() {
        let stub = Stub::new(
            "declared",
            &answers(
                r#"{"schema_version":1,"cli":"tk","items":{"types":["story"],"priority":{"min":1,"max":3}}}"#,
                0,
            ),
        );
        let declared = stub.exec().capabilities().expect("the declaration reads");
        assert_eq!(declared.cli.as_deref(), Some("tk"));
        assert_eq!(declared.items.types, ["story"]);
        assert_eq!(declared.items.priority.min, 1);
        assert_eq!(declared.items.priority.max, 3);

        let stub = Stub::new(
            "upside-down",
            &answers(
                r#"{"schema_version":1,"items":{"types":["story"],"priority":{"min":3,"max":1}}}"#,
                0,
            ),
        );
        let why = unreadable(stub.exec().capabilities());
        assert!(
            why.ends_with(
                "capabilities answered no readable response: items priority min 3 is above max 1"
            ),
            "{why}"
        );
    }

    /// A listing is one `list` process, and its rows read the holder and the
    /// run's record where the adapter answers them — and nobody and no record
    /// where it leaves the keys out, as an adapter answering no more than a
    /// row's id, title, status, type, labels and order does.
    #[test]
    fn a_listing_row_reads_its_holder_and_run_and_their_absence_as_none() {
        let stub = Stub::new(
            "list-held",
            &answers(
                &format!(
                    r#"{{"schema_version":1,"items":[{{"id":"fx-a1b2","title":"t","status":"in_progress","type":"task","labels":[],"assignee":"{SEAT}","order":{{"state":"none"}},"run":{{"hash":"h1","workflow":"build","pack":"ts","entry":"workflows/build.ts","started_at":"2026-09-23T10:00:00Z"}}}},{{"id":"fx-c3d4","title":"t","status":"open","type":"task","labels":["fleet"],"order":{{"state":"none"}}}}]}}"#
                ),
                0,
            ),
        );
        let rows = stub
            .exec()
            .list(&Filter::Ready)
            .expect("the stub answered a listing");
        assert_eq!(stub.verbs(), ["list"], "one call, and no show after it");
        assert_eq!(stub.request()["filter"], "ready");

        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(
            rows[0].assignee.map(|seat| seat.to_string()).as_deref(),
            Some(SEAT)
        );
        assert_eq!(
            rows[0].run.as_ref().map(|run| run.hash.as_str()),
            Some("h1")
        );
        assert_eq!((rows[1].assignee, rows[1].run.clone()), (None, None));
    }

    /// One call of the verb through `Exec`, with every field it can send set.
    type Call = fn(&Exec, &Path) -> Result<(), StoreError>;

    /// Every verb the schema document names, called with every field it can
    /// send set: the request carries exactly the properties the verb's
    /// request schema names, and passes it; and the answer the stub gave
    /// passes the verb's response schema and is one `Exec` reads.
    ///
    /// RED-PROOF: a field this file sends that the schema does not name — or
    /// one the schema names that this file never sends — fails here, naming
    /// the verb.
    #[test]
    fn each_verb_sends_exactly_the_fields_its_schema_names() {
        let document = super::super::schema::document();
        let seat = || crate::seat::identity::SeatId::parse(SEAT).expect("a seat id");
        let entry = entry::to_json(&Entry {
            id: String::from("e-17"),
            at: String::from("2026-09-23T10:00:00Z"),
            by: by(),
            body: Body::Ordered(entry::Ordered {
                order: crate::store::OrderKind::Dispatch,
                seat: Some(seat()),
            }),
        });
        let summary = json!({
            "id": "fx-c3d4", "title": "Name the stamp's fields", "status": "open",
            "type": "task", "labels": ["fleet"], "assignee": SEAT,
            "order": {"state": "none"}, "run": null, "foreign": ["sprint"],
        });
        let done = json!({"schema_version": 1});
        let answers = [
            (
                "version",
                json!({"schema_version": 1, "name": "stub", "version": "0.1.0"}),
            ),
            (
                "capabilities",
                json!({
                    "schema_version": 1,
                    "export": {"file": "store/export.jsonl", "dir": "store/"},
                    "scratch": true, "item_prefix": "fx", "cli": "stub",
                    "items": {"types": ["task"], "priority": {"min": 0, "max": 4}},
                }),
            ),
            ("resolve", json!({"schema_version": 1, "id": "fx-a1b2"})),
            ("show", serde_json::from_str(SHOWN).expect("SHOWN is JSON")),
            ("list", json!({"schema_version": 1, "items": [summary]})),
            ("timeline", json!({"schema_version": 1, "entries": [entry]})),
            ("create", json!({"schema_version": 1, "id": "fx-a1b2"})),
            ("update", done.clone()),
            ("append", json!({"schema_version": 1, "entry": "e-17"})),
            ("order.set", done.clone()),
            ("order.withdraw", done.clone()),
            ("run.set", done.clone()),
            ("hold.raise", json!({"schema_version": 1, "hold": "fx-h9"})),
            ("hold.clear", done.clone()),
            (
                "holds.open",
                json!({"schema_version": 1, "holds": ["fx-h9"]}),
            ),
            ("close", done),
            (
                "export",
                json!({"schema_version": 1, "file": "/work/store/export.jsonl"}),
            ),
            (
                "scratch",
                json!({"schema_version": 1, "root": "/tmp/store"}),
            ),
        ];
        let arms: Vec<String> = answers
            .iter()
            .map(|(verb, answer)| format!("{verb})\ncat <<'JSON'\n{answer}\nJSON\n;;"))
            .collect();
        let stub = Stub::new(
            "schema",
            &format!("case \"$1\" in\n{}\nesac", arms.join("\n")),
        );

        let calls: [(&str, Call); 18] = [
            ("version", |exec, _| exec.version().map(drop)),
            ("capabilities", |exec, _| exec.capabilities().map(drop)),
            ("resolve", |exec, _| exec.resolve("a1b2").map(drop)),
            ("show", |exec, _| exec.show("a1b2").map(drop)),
            ("list", |exec, _| {
                exec.list(&Filter::Label(String::from("fleet"))).map(drop)
            }),
            ("timeline", |exec, _| exec.timeline(&id()).map(drop)),
            ("create", |exec, _| {
                let item = NewItem {
                    title: String::from("t"),
                    description: String::from("d"),
                    item_type: String::from("task"),
                    labels: vec![String::from("fleet")],
                    priority: Some(2),
                };
                exec.create(&item, &by()).map(drop)
            }),
            ("update", |exec, _| {
                let seat = crate::seat::identity::SeatId::parse(SEAT).expect("a seat id");
                let change = Update {
                    title: Some(String::from("t")),
                    assignee: Some(Some(seat)),
                    if_assignee: Some(None),
                    status: Some(types::Status::Open),
                };
                exec.update(&id(), &change, &by())
            }),
            ("append", |exec, _| {
                let body = Body::Ordered(entry::Ordered {
                    order: crate::store::OrderKind::Review,
                    seat: None,
                });
                exec.append(&id(), &body, &by()).map(drop)
            }),
            ("order.set", |exec, _| {
                let seat = crate::seat::identity::SeatId::parse(SEAT).expect("a seat id");
                let order = Order {
                    kind: crate::store::OrderKind::Dispatch,
                    by: by(),
                    seat: Some(seat),
                    at: crate::store::Stamp::parse("2026-09-23T10:00:00Z").expect("a stamp"),
                };
                exec.order_set(&id(), &order, &by())
            }),
            ("order.withdraw", |exec, _| {
                let seat = crate::seat::identity::SeatId::parse(SEAT).expect("a seat id");
                let fence = WithdrawFence {
                    if_assignee: Some(Some(seat)),
                    if_status: Some(types::Status::InProgress),
                    reopen: true,
                };
                exec.order_withdraw(&id(), &fence, &by())
            }),
            ("run.set", |exec, _| {
                let run = RunRecord {
                    hash: String::from("abc"),
                    workflow: String::from("build"),
                    pack: String::from("ts"),
                    entry: String::from("workflows/build.ts"),
                    started_at: crate::store::Stamp::parse("2026-09-23T10:00:00Z")
                        .expect("a stamp"),
                };
                exec.run_set(&id(), &run, &by())
            }),
            ("hold.raise", |exec, _| {
                exec.hold_raise(&id(), "ask", &by()).map(drop)
            }),
            ("hold.clear", |exec, _| {
                exec.hold_clear(&HoldId::from("fx-h9"), &by())
            }),
            ("holds.open", |exec, _| exec.holds_open().map(drop)),
            ("close", |exec, _| exec.close(&id(), "landed", &by())),
            ("export", |exec, dir| exec.export(dir).map(drop)),
            ("scratch", |exec, dir| {
                exec.scratch(&dir.join("store")).map(drop)
            }),
        ];

        let exec = stub.exec();
        let mut called = Vec::new();
        for ((verb, call), (answered, answer)) in calls.iter().zip(&answers) {
            assert_eq!(verb, answered, "the calls and the answers are in one order");
            call(&exec, &stub.dir).unwrap_or_else(|why| panic!("{verb} did not read: {why}"));
            let request = stub.request();
            let sent: Vec<&String> = request.as_object().expect("an object").keys().collect();
            let schema = &document["verbs"][verb]["request"]["properties"];
            let named: Vec<&String> = schema.as_object().expect("properties").keys().collect();
            assert_eq!(
                sent, named,
                "{verb}: the fields Exec sends are not the ones its request schema names"
            );
            for (pointer, instance) in [("request", &request), ("response", answer)] {
                let at = format!("/verbs/{verb}/{pointer}");
                if let Err(why) = super::super::schema::check(&document, &at, instance) {
                    panic!("{instance} does not pass {at}: {why}");
                }
            }
            called.push(*verb);
        }
        let verbs: Vec<&str> = document["verbs"]
            .as_object()
            .expect("verbs")
            .keys()
            .map(String::as_str)
            .collect();
        called.sort_unstable();
        assert_eq!(called, verbs, "every verb the document names is called");
    }
}
