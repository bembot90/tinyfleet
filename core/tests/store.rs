//! `Store` as the library reaches it: the opener that reads `[store] adapter`
//! out of the project's own file and opens the adapter it names — by absolute
//! path, or by a name the installed packs carry — and the timeline over the
//! board held in memory.
//!
//! No arm here runs a real store. A store adapter's own behaviour is its
//! pack's to test, in the repository the pack is published from; what this
//! file holds is what fleet does with whichever adapter answers.
//!
//! The TIMELINE arms are the board held in memory's own: what its comments
//! answer, planted as a store's would be, and the append-and-read-back helper
//! over a store that drops its writes. They run no binary.
//!
//! The OPENER arms write their adapters as `#!/bin/sh` stubs.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::{a_delivery, seat_actor, Fixture, A_COMMIT};
use fleet_core::entry::{Body, Ordered};
use fleet_core::item::{recorded, Unrecorded};
use fleet_core::store::{self, ItemId, OrderKind, Store, StoreError};

// ---- the timeline on the board held in memory -------------------------------

fn an_order() -> Body {
    Body::Ordered(Ordered {
        order: OrderKind::Dispatch,
        seat: None,
    })
}

/// Plants no contract verb makes, made on the fake: its comments are read by
/// `read_row`, the one reader of an entry, so a person's text is left out and
/// an entry whose author is no actor refuses the whole read, naming the
/// comment.
#[test]
fn the_fake_leaves_a_persons_comment_out_and_refuses_a_malformed_entry() {
    let board = fleet_core::test_support::Board::new("store-fake-plants");
    let item = board.item("an item a person commented on");

    board
        .store
        .comment(&item, "Alberto Vildosola", "a person's words");
    assert_eq!(
        board
            .store
            .timeline(&ItemId::from(item.as_str()))
            .expect("the timeline reads"),
        Vec::new(),
        "a person's comment is not an entry"
    );

    let comment = board.store.comment(
        &item,
        "Alberto Vildosola",
        r#"{"fleet.entry":1,"kind":"ordered","order":"dispatch"}"#,
    );
    match board.store.timeline(&ItemId::from(item.as_str())) {
        Err(StoreError::Unreadable(why)) => assert!(
            why.contains(&comment) && why.contains("Alberto Vildosola"),
            "the refusal names the comment and its author: {why}"
        ),
        other => panic!("an entry whose author is no actor refuses the read: {other:?}"),
    }
}

/// The fake keeps the contract's refusal word for word: a body that breaks
/// its kind's rules is never written, and an item it does not hold is
/// Refused.
#[test]
fn the_fake_refuses_an_entry_that_does_not_validate_and_an_item_it_does_not_hold() {
    let board = fleet_core::test_support::Board::new("store-fake-refusals");
    let item = board.item("an item a short sha is appended to");
    let by = seat_actor("a-short-seat");

    match board
        .store
        .append(&ItemId::from(item.as_str()), &a_delivery("1111111"), &by)
    {
        Err(StoreError::Unreadable(why)) => assert!(
            why.starts_with(&format!(
                "the delivered entry for {item} does not validate: `commit`"
            )) && why.ends_with(" — nothing was written"),
            "{why}"
        ),
        other => panic!("a delivery naming a 7-character commit is refused: {other:?}"),
    }
    assert_eq!(
        board
            .store
            .timeline(&ItemId::from(item.as_str()))
            .expect("the timeline reads"),
        Vec::new(),
        "and nothing was written"
    );

    match board
        .store
        .append(&ItemId::from("fx-nobody-filed-this"), &an_order(), &by)
    {
        Err(StoreError::Refused(_)) => {}
        other => panic!("an append to an item nobody filed is Refused: {other:?}"),
    }
}

/// A store that drops its writes still answers an id, which is exactly why a
/// verb reads the timeline back rather than trusting it.
#[test]
fn a_deaf_store_answers_an_id_and_keeps_no_entry() {
    let board = fleet_core::test_support::Board::new("store-fake-deaf");
    let item = board.item("an item whose entry is dropped");
    board.store.ignore_writes();

    let id = board
        .store
        .append(
            &ItemId::from(item.as_str()),
            &an_order(),
            &seat_actor("a-deaf-seat"),
        )
        .expect("the append answers");
    assert!(!id.is_empty(), "the store names what it was handed");
    assert_eq!(
        board
            .store
            .timeline(&ItemId::from(item.as_str()))
            .expect("the timeline reads"),
        Vec::new(),
        "and the timeline holds nothing"
    );
}

#[test]
fn recorded_on_a_deaf_store_is_unconfirmed() {
    let board = fleet_core::test_support::Board::new("store-fake-recorded-deaf");
    let item = board.item("an item whose entry is dropped");
    board.store.ignore_writes();

    match recorded(
        &board.store,
        &item,
        &a_delivery(A_COMMIT),
        &seat_actor("a-deaf-seat"),
    ) {
        Err(Unrecorded::Unconfirmed(why)) => assert!(
            why.contains(&item) && why.contains("does not hold") && why.contains("delivered"),
            "the refusal names the item, the kind and what the timeline lacks: {why}"
        ),
        other => panic!("a write the store dropped is unconfirmed: {other:?}"),
    }
}

/// The helper's one success: the id the store answered, read back with the
/// body and the actor written, and the one log line the fake keeps per append.
#[test]
fn recorded_answers_the_id_it_read_back() {
    let board = fleet_core::test_support::Board::new("store-fake-recorded");
    let item = board.item("an item an entry is recorded on");
    let by = seat_actor("a-recording-seat");
    board.forget_writes();

    let id = recorded(&board.store, &item, &an_order(), &by).expect("the entry is recorded");
    let timeline = board
        .store
        .timeline(&ItemId::from(item.as_str()))
        .expect("the timeline reads");
    let read = fleet_core::entry::Timeline(&timeline)
        .entry(&id)
        .expect("the entry is on the timeline");
    assert_eq!(read.body, an_order());
    assert_eq!(read.by, by);
    assert_eq!(board.store.wrote(), [format!("append {item} ordered {by}")]);
}

/// A write refused before it reached the store is not written, which is a
/// different sentence from a write the store took and the read did not show.
#[test]
fn recorded_of_an_entry_that_does_not_validate_is_not_written() {
    let board = fleet_core::test_support::Board::new("store-fake-recorded-invalid");
    let item = board.item("an item a short sha is recorded on");

    match recorded(
        &board.store,
        &item,
        &a_delivery("1111111"),
        &seat_actor("a-short-seat"),
    ) {
        Err(Unrecorded::NotWritten(StoreError::Unreadable(why))) => {
            assert!(why.contains("nothing was written"), "{why}")
        }
        other => panic!("a body that does not validate is not written: {other:?}"),
    }
}

// ---- the opener: `[store] adapter`, read out of the project's own file ------

/// A project's policy as the opener reads it, parsed from `text`.
fn policy_of(text: &str) -> toml::Table {
    text.parse().expect("the fixture policy parses")
}

/// The opener over `policy` for the project at `root`, under `timeout`, on
/// no search path of the caller's — so an adapter carries this process's own
/// `PATH`.
fn opened(
    root: &Path,
    policy: &toml::Table,
    timeout: Duration,
) -> Result<Box<dyn Store>, StoreError> {
    store::open(&store::Opening {
        root,
        policy,
        source: store::AdapterSource::Setting,
        search_path: "",
        timeout,
        packs: None,
    })
}

/// An adapter executable in `dir` that copies its request to `request.json`
/// and then runs `answer`.
fn an_adapter(dir: &Fixture, answer: &str) -> PathBuf {
    let bin = dir.path("adapter");
    std::fs::write(
        &bin,
        format!(
            "#!/bin/sh\ncat > '{dir}/request.json'\n{answer}\n",
            dir = dir.root.display(),
        ),
    )
    .expect("the adapter is written");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
        .expect("the adapter is executable");
    bin
}

/// `[store] adapter` naming an executable by absolute path opens the store
/// that executable answers: a `show` is one call to it, carrying the project's
/// root, and the item is the one it answered. The bound the caller hands in is
/// the call's own — an adapter that outruns it is could not tell.
#[test]
fn an_adapter_named_by_absolute_path_is_the_store_opened() {
    let dir = Fixture::new("store-open-exec");
    let root = dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");
    let adapter = an_adapter(
        &dir,
        r#"printf '%s\n' '{"schema_version":1,"item":{"id":"fx-c3d4","title":"an item the adapter holds","status":"open","type":"task","labels":[],"order":{"state":"none"}}}'"#,
    );
    let policy = policy_of(&format!("[store]\nadapter = {:?}\n", adapter.display()));

    let store =
        opened(&root, &policy, store::STORE_TIMEOUT).expect("the adapter is an executable file");
    let item = store.show("c3d4").expect("the adapter answered an item");

    assert_eq!(item.id, ItemId::from("fx-c3d4"));
    assert_eq!(item.title, "an item the adapter holds");
    let request: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path("request.json")).expect("the adapter was called"),
    )
    .expect("the request is one JSON value");
    assert_eq!(
        request["root"],
        serde_json::json!(root.display().to_string()),
        "the request names the root the opener was handed: {request}"
    );

    let slow = Fixture::new("store-open-exec-slow");
    let adapter = an_adapter(&slow, "sleep 5");
    let policy = policy_of(&format!("[store]\nadapter = {:?}\n", adapter.display()));
    let started = Instant::now();
    let refused = opened(&root, &policy, Duration::from_millis(300))
        .expect("the adapter is an executable file")
        .show("c3d4");
    assert!(
        matches!(&refused, Err(StoreError::Unreadable(why)) if why.contains("did not answer within")),
        "an adapter that outruns the opener's bound is could not tell: {refused:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "the bound was the opener's and not the store's own: {:?}",
        started.elapsed()
    );
}

/// A path that is no executable file, and a value that is neither form, are
/// could not tell before anything is run, each naming what the file said.
#[test]
fn an_adapter_that_is_no_executable_or_neither_form_is_could_not_tell() {
    let dir = Fixture::new("store-open-refused");
    dir.file("not-executable", "#!/bin/sh\nexit 0\n");
    for path in [
        dir.path("not-executable"),
        dir.path("absent"),
        dir.root.clone(),
    ] {
        let policy = policy_of(&format!("[store]\nadapter = {:?}\n", path.display()));
        match opened(&dir.root, &policy, store::STORE_TIMEOUT) {
            Err(StoreError::Unreadable(why)) => assert_eq!(
                why,
                format!(
                    "[store] adapter names `{}`, which is not an executable file",
                    path.display()
                )
            ),
            Err(other) => panic!("wanted Unreadable, got {other:?}"),
            Ok(_) => panic!("{} opened as an adapter", path.display()),
        }
    }
    for (value, named) in [("\"bin/adapter\"", "bin/adapter"), ("\"\"", ""), ("3", "3")] {
        let policy = policy_of(&format!("[store]\nadapter = {value}\n"));
        match opened(&dir.root, &policy, store::STORE_TIMEOUT) {
            Err(StoreError::Unreadable(why)) => assert_eq!(
                why,
                format!(
                    "[store] adapter is `{named}` — it is the name of a store adapter an \
                     installed pack carries, or an absolute path to an adapter executable"
                )
            ),
            Err(other) => panic!("wanted Unreadable, got {other:?}"),
            Ok(_) => panic!("{value} opened a store"),
        }
    }
}

/// The same refusals name `--adapter` where the flag carried the setting in,
/// and never the `[store] adapter` the person did not write.
///
/// RED-PROOF: on the base there is no source to open by, and every refusal
/// names `[store] adapter`.
#[test]
fn a_refusal_names_the_flag_that_said_the_adapter() {
    let dir = Fixture::new("store-open-flag");
    let refused = |value: &str| {
        let policy = policy_of(&format!("[store]\nadapter = {value}\n"));
        match store::open(&store::Opening {
            root: &dir.root,
            policy: &policy,
            source: store::AdapterSource::Flag,
            search_path: "",
            timeout: store::STORE_TIMEOUT,
            packs: None,
        }) {
            Err(StoreError::Unreadable(why)) => why,
            Err(other) => panic!("wanted Unreadable, got {other:?}"),
            Ok(_) => panic!("{value} opened a store"),
        }
    };
    let absent = dir.path("absent");
    assert_eq!(
        refused(&format!("{:?}", absent.display())),
        format!(
            "--adapter names `{}`, which is not an executable file",
            absent.display()
        )
    );
    assert_eq!(
        refused("\"bin/adapter\""),
        "--adapter is `bin/adapter` — it is the name of a store adapter an installed pack \
         carries, or an absolute path to an adapter executable"
    );
}

// ---- a bare name, resolved through the installed packs ----------------------

/// A machine directory's packs and the binary's defaults beneath them, as
/// `fleet start` leaves them, with a pack per call of [`Machine::pack`].
struct Machine {
    dir: Fixture,
    packs_dir: PathBuf,
    defaults_dir: PathBuf,
}

impl Machine {
    fn new(label: &str) -> Machine {
        let dir = Fixture::new(label);
        let defaults_dir = dir.materialize_defaults();
        let packs_dir = dir.path("packs");
        std::fs::create_dir_all(&packs_dir).expect("the packs dir is created");
        Machine {
            dir,
            packs_dir,
            defaults_dir,
        }
    }

    fn packs(&self) -> store::PackDirs<'_> {
        store::PackDirs {
            packs_dir: &self.packs_dir,
            defaults_dir: &self.defaults_dir,
        }
    }

    /// A pack named `name` installed here, its manifest carrying `more` after
    /// the `[pack]` table.
    fn pack(&self, name: &str, more: &str) -> PathBuf {
        self.dir.file(
            &format!("packs/{name}/pack.toml"),
            &format!("[pack]\nname = \"{name}\"\nversion = \"0.1.0\"\nschema = 3\n{more}"),
        );
        self.dir.path(&format!("packs/{name}"))
    }

    /// The store adapter `name` in the pack at `pack`: its `adapter.toml`, and
    /// a `main` that records its request beside itself and answers a show
    /// of an item titled `title`.
    fn adapter(&self, pack: &Path, name: &str, title: &str) -> PathBuf {
        let dir = pack.join(format!("adapters/store/{name}"));
        std::fs::create_dir_all(&dir).expect("the adapter's directory is created");
        std::fs::write(
            dir.join("adapter.toml"),
            format!(
                "[adapter]\nname = \"{name}\"\nkind = \"store\"\nversion = \"0.1.0\"\n\
                 entry = \"main\"\n"
            ),
        )
        .expect("the adapter's manifest is written");
        let entry = dir.join("main");
        std::fs::write(
            &entry,
            format!(
                "#!/bin/sh\ncat > '{request}'\nprintf '%s\\n' \
                 '{{\"schema_version\":1,\"item\":{{\"id\":\"fx-c3d4\",\"title\":\"{title}\",\
                 \"status\":\"open\",\"type\":\"task\",\"labels\":[],\"order\":{{\"state\":\"none\"}}}}}}'\n",
                request = dir.join("request.json").display(),
            ),
        )
        .expect("the adapter's entry is written");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o755))
            .expect("the entry is executable");
        dir
    }
}

/// The opener over `policy` with `packs` to resolve a name through.
fn opened_over(
    root: &Path,
    policy: &toml::Table,
    packs: Option<store::PackDirs>,
) -> Result<Box<dyn Store>, StoreError> {
    opened_on(root, policy, packs, "")
}

/// The same, with `search_path` as the caller's constructed child `PATH`.
fn opened_on(
    root: &Path,
    policy: &toml::Table,
    packs: Option<store::PackDirs>,
    search_path: &str,
) -> Result<Box<dyn Store>, StoreError> {
    store::open(&store::Opening {
        root,
        policy,
        source: store::AdapterSource::Setting,
        search_path,
        timeout: store::STORE_TIMEOUT,
        packs,
    })
}

fn named(name: &str) -> toml::Table {
    policy_of(&format!("[store]\nadapter = {name:?}\n"))
}

/// A bare name is the store adapter an installed pack carries under
/// `adapters/store/<name>/`: its entry is the store opened, called with the
/// project's root, and the item is the one it answered.
///
/// RED-PROOF: on the base the name is neither form, and the open refuses.
#[test]
fn a_bare_name_opens_the_store_adapter_an_installed_pack_carries() {
    let machine = Machine::new("store-open-named");
    let pack = machine.pack("tracker", "");
    let adapter = machine.adapter(&pack, "x", "an item the adapter in the pack holds");
    let root = machine.dir.path("project");
    std::fs::create_dir_all(&root).expect("the project root is created");

    let store = opened_over(&root, &named("x"), Some(machine.packs()))
        .expect("the name resolves to the pack's adapter");
    let item = store.show("c3d4").expect("the adapter answered an item");

    assert_eq!(item.title, "an item the adapter in the pack holds");
    let request: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(adapter.join("request.json")).expect("the entry was called"),
    )
    .expect("the request is one JSON value");
    assert_eq!(
        request["root"],
        serde_json::json!(root.display().to_string()),
        "{request}"
    );
}

/// A file that names no adapter — no `[store]` table, or one with no
/// `adapter` key — opens [`store::DEFAULT_ADAPTER`] BY NAME, through the
/// installed packs as any name is: the adapter a pack carries under it answers,
/// and with no pack carrying it the open is could not tell, naming the pack
/// that would.
///
/// RED-PROOF: on the base a file naming nothing opened the built-in store
/// without a look at the packs, so the pack's adapter was never called and
/// the refusal never came.
#[test]
fn a_file_naming_no_adapter_opens_the_default_name_through_the_packs() {
    let machine = Machine::new("store-open-default");
    let pack = machine.pack("tracker", "");
    let adapter = machine.adapter(
        &pack,
        store::DEFAULT_ADAPTER,
        "an item the default adapter holds",
    );
    for policy in [policy_of(""), policy_of("[store]\n")] {
        let _ = std::fs::remove_file(adapter.join("request.json"));
        let item = opened_over(&machine.dir.root, &policy, Some(machine.packs()))
            .expect("the default name resolves to the pack's adapter")
            .show("c3d4")
            .expect("the adapter answered an item");
        assert_eq!(
            item.title, "an item the default adapter holds",
            "{policy:?}"
        );
        assert!(
            adapter.join("request.json").is_file(),
            "the pack's entry was called: {policy:?}"
        );
    }

    std::fs::remove_dir_all(&adapter).expect("the adapter is taken out");
    match opened_over(&machine.dir.root, &policy_of(""), Some(machine.packs())) {
        Err(StoreError::Unreadable(why)) => assert_eq!(
            why,
            format!(
                "no store adapter named `{name}` in the installed packs — `fleet pack add \
                 {}//adapters/store/{name} --version {}` installs the one fleet-packs carries",
                fleet_core::supported::PINNED_PACKS_SOURCE,
                fleet_core::supported::PINNED_PACKS,
                name = store::DEFAULT_ADAPTER,
            )
        ),
        Err(other) => panic!("wanted Unreadable, got {other:?}"),
        Ok(_) => panic!("a file naming nothing opened a store no pack carries"),
    }
}

/// A named adapter runs on the caller's search path and not this process's
/// own `PATH`, with the directory of the runtime its pack imports put in
/// front where that path misses it — found where the runtime's installer puts
/// it, `$<NAME>_INSTALL/bin` — so an entry that execs its runtime finds it
/// under a service, whose own `PATH` holds neither. A pack declaring no
/// runtime runs on the search path as it was handed, and a caller handing
/// none leaves the adapter this process's own.
///
/// RED-PROOF: on the base every adapter ran on this process's own `PATH`, so
/// the first two readings were that and not the search path.
#[test]
fn a_named_adapter_runs_on_the_search_path_with_its_runtime_in_front() {
    let machine = Machine::new("store-open-path");
    let runtime = format!("fxrt{}", std::process::id());
    let installed = machine.dir.path("runtime-home");
    std::fs::create_dir_all(installed.join("bin")).expect("the installer's bin is made");
    std::fs::write(installed.join("bin").join(&runtime), "#!/bin/sh\nexit 0\n")
        .expect("the runtime is written");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        installed.join("bin").join(&runtime),
        std::fs::Permissions::from_mode(0o755),
    )
    .expect("the runtime is executable");
    // The variable is this arm's own: its name carries this process's id, so
    // no other arm reads it.
    std::env::set_var(format!("{}_INSTALL", runtime.to_uppercase()), &installed);

    machine.pack(
        "rt",
        &format!(
            "\n[runtime]\nname = \"{runtime}\"\nversion = \"1.0.0\"\n\
             bundle = \"{runtime} bundle {{entry}} {{bundle}}\"\n\
             run = \"{runtime} run {{bundle}}\"\n"
        ),
    );
    let importing = machine.pack(
        "tracker",
        "\n[imports.rt]\nsource = \"../rt\"\nversion = \"0.1.0\"\n",
    );
    // A pack of no runtime on a machine of its own: only the top layer may
    // import (fleet-6oc), so it cannot sit beside the importer.
    let bare = Machine::new("store-open-path-bare");
    let plain = bare.pack("plain", "");
    let seen = |machine: &Machine, pack: &Path, name: &str, search_path: &str| {
        let dir = machine.adapter(pack, name, "an item");
        std::fs::write(
            dir.join("main"),
            format!(
                "#!/bin/sh\nprintf '%s' \"$PATH\" > '{seen}'\ncat > /dev/null\nprintf '%s\\n' \
                 '{{\"schema_version\":1,\"item\":{{\"id\":\"fx-c3d4\",\"title\":\"t\",\
                 \"status\":\"open\",\"type\":\"task\",\"labels\":[],\"order\":{{\"state\":\"none\"}}}}}}'\n",
                seen = dir.join("path").display(),
            ),
        )
        .expect("the entry is rewritten to record its PATH");
        opened_on(
            &machine.dir.root,
            &named(name),
            Some(machine.packs()),
            search_path,
        )
        .expect("the name resolves")
        .show("c3d4")
        .expect("the adapter answered");
        std::fs::read_to_string(dir.join("path")).expect("the entry recorded its PATH")
    };

    let search = "/usr/bin:/bin";
    assert_eq!(
        seen(&machine, &importing, "x", search),
        format!("{}:{search}", installed.join("bin").display()),
        "the imported runtime's directory goes in front of the search path"
    );
    assert_eq!(
        seen(&bare, &plain, "y", search),
        search,
        "a pack with no runtime runs on the search path as handed"
    );
    assert_eq!(
        seen(&bare, &plain, "y", ""),
        std::env::var("PATH").unwrap_or_default(),
        "no search path is this process's own"
    );
}

/// Two packs carrying one adapter name: the higher layer's is the store
/// opened, and the lower one's is reached only once the higher carries none.
#[test]
fn a_higher_layer_carrying_the_name_shadows_a_lower_one() {
    let machine = Machine::new("store-open-shadowed");
    // `top` imports `base`, which puts it above whatever the names sort to.
    let base = machine.pack("base", "");
    let top = machine.pack(
        "top",
        "\n[imports.base]\nsource = \"../base\"\nversion = \"0.1.0\"\n",
    );
    machine.adapter(&base, "x", "the lower layer");
    let over = machine.adapter(&top, "x", "the higher layer");

    let title = || {
        opened_over(&machine.dir.root, &named("x"), Some(machine.packs()))
            .expect("the name resolves")
            .show("c3d4")
            .expect("the adapter answered")
            .title
    };
    assert_eq!(title(), "the higher layer");

    // The control: the lower layer's adapter is one the opener reaches, so
    // the answer above is precedence and not the only adapter there.
    std::fs::remove_dir_all(&over).expect("the higher adapter is taken out");
    assert_eq!(title(), "the lower layer");
}

/// A name no installed pack carries is could not tell, naming the install
/// that would carry it; a caller with no packs behind it resolves no name at
/// all; and a layering that does not resolve says why.
#[test]
fn a_name_resolved_nowhere_is_could_not_tell_naming_how_to_install_one() {
    let machine = Machine::new("store-open-nowhere");
    let pack = machine.pack("tracker", "");
    machine.adapter(&pack, "x", "not this one");

    let refusal = |packs| match opened_over(&machine.dir.root, &named("y"), packs) {
        Err(StoreError::Unreadable(why)) => why,
        Err(other) => panic!("wanted Unreadable, got {other:?}"),
        Ok(_) => panic!("`y` opened a store"),
    };
    // The line names the fleet-packs source and tag this binary pins.
    assert_eq!(
        refusal(Some(machine.packs())),
        format!(
            "no store adapter named `y` in the installed packs — `fleet pack add \
             {}//adapters/store/y --version {}` installs the one fleet-packs carries",
            fleet_core::supported::PINNED_PACKS_SOURCE,
            fleet_core::supported::PINNED_PACKS
        )
    );
    assert_eq!(
        refusal(None),
        "no store adapter named `y` resolves: no packs are installed here to carry one"
    );
    let absent = machine.dir.path("absent-defaults");
    let why = refusal(Some(store::PackDirs {
        packs_dir: &machine.packs_dir,
        defaults_dir: &absent,
    }));
    assert!(
        why.starts_with("no store adapter named `y` resolves: the pack layers do not resolve: ")
            && why.contains("`fleet start` writes them"),
        "{why}"
    );
}

/// An adapter the format refuses is never run: an entry that lost its
/// executable bit is could not tell with the fix, and so is a manifest filed
/// under a kind it does not name.
#[test]
fn an_adapter_the_format_refuses_is_could_not_tell_and_never_run() {
    let machine = Machine::new("store-open-defective");
    let pack = machine.pack("tracker", "");
    let adapter = machine.adapter(&pack, "x", "never read");
    let entry = adapter.join("main");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o644))
        .expect("the entry loses its executable bit");

    let refusal = || match opened_over(&machine.dir.root, &named("x"), Some(machine.packs())) {
        Err(StoreError::Unreadable(why)) => why,
        Err(other) => panic!("wanted Unreadable, got {other:?}"),
        Ok(_) => panic!("a defective adapter opened"),
    };
    assert_eq!(
        refusal(),
        format!(
            "the store adapter `x` cannot be opened: the adapter entry `{e}` is not \
             executable — `chmod +x {e}` makes it one",
            e = entry.display()
        )
    );

    std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o755))
        .expect("the bit is put back");
    std::fs::write(
        adapter.join("adapter.toml"),
        "[adapter]\nname = \"x\"\nkind = \"agent\"\nversion = \"0.1.0\"\nentry = \"main\"\n",
    )
    .expect("the manifest is rewritten");
    assert_eq!(
        refusal(),
        "the store adapter `x` cannot be opened: `adapters/store/x/adapter.toml` says kind \
         `agent`, and it is filed under `adapters/store`"
    );
    assert!(
        !adapter.join("request.json").exists(),
        "the entry was never run"
    );
}

/// The project's own file: a declared project's `.fleet/project.toml` first,
/// else an embedded fleet's `fleet.toml`, else nothing — and a file that does
/// not parse is could not tell, naming it.
#[test]
fn the_projects_own_file_is_its_declaration_else_its_fleet_file() {
    let dir = Fixture::new("store-project-policy");
    assert_eq!(
        store::project_policy(&dir.root).expect("no file is an empty policy"),
        toml::Table::new()
    );

    dir.file("fleet.toml", "[store]\nadapter = \"/embedded\"\n");
    assert_eq!(
        store::project_policy(&dir.root).expect("the fleet file reads"),
        policy_of("[store]\nadapter = \"/embedded\"\n")
    );

    dir.file(".fleet/project.toml", "[store]\nadapter = \"/declared\"\n");
    assert_eq!(
        store::project_policy(&dir.root).expect("the declaration reads"),
        policy_of("[store]\nadapter = \"/declared\"\n"),
        "the declaration is the project's own statement, and wins"
    );

    dir.file(".fleet/project.toml", "[store\n");
    match store::project_policy(&dir.root) {
        Err(StoreError::Unreadable(why)) => assert!(
            why.starts_with(&format!(
                "{} does not parse as TOML: ",
                dir.path(".fleet/project.toml").display()
            )),
            "{why}"
        ),
        other => panic!("wanted Unreadable, got {other:?}"),
    }
}
