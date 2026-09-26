//! `fleet create`, `fleet start`, `fleet stop` — the cli half of the lifecycle
//! family.
//!
//! Everything here is argv, the terminal and what only a process knows: which
//! directory the call was made in, which executable is running, and the three
//! questions `create` asks. The files, the register, the install work and the
//! confirmations are `fleet_controller::lifecycle`'s, the binary's own
//! defaults are `fleet_core::defaults`'s, and the store's pack is installed by
//! `fleet_core::add`, the function `fleet pack add` calls.
//!
//! THERE IS NO `fleet install`. Its work runs on the first `fleet start`, in one
//! function, so a later installer script calls the same one.

use std::path::{Path, PathBuf};

use fleet_controller::adapter::{self, claude_code::DaemonListing};
use fleet_controller::lifecycle::{self, FirstRun, Mode, ProjectAt};
use fleet_controller::run::{self, DaemonCheck};
use fleet_controller::{config, events, platform, policy as controller, sessions};
use fleet_core::item::Stop;
use fleet_core::seat::identity;
use fleet_core::store::{self, AdapterSource, Opening, PackDirs, STORE_TIMEOUT};
use fleet_core::{add, defaults, lock, supported};

use crate::exit::Exit;
use crate::item::{derived_worktrees_dir, resolve_at, resolve_from};
use crate::seat_add::{self, Listed};
use crate::ui::{Prompt, Stream, Tone, Ui};

/// What `create` takes. The mode, agent and store flags are the three
/// questions' answers, for a caller with no terminal to answer them on; the
/// store's has a default, which such a caller takes by leaving it out.
#[derive(clap::Args)]
pub struct CreateArgs {
    /// one fleet.toml at this project's root
    #[arg(long, conflicts_with = "standalone")]
    pub embedded: bool,
    /// declare this project to the fleet this machine runs
    #[arg(long)]
    pub standalone: bool,
    /// the agent the fleet's seats run on
    #[arg(long, value_name = "NAME")]
    pub agent: Option<String>,
    /// the store's pack to install: bd, or none
    #[arg(long, value_name = "NAME")]
    pub store: Option<String>,
    /// a fleet-packs checkout to install it from
    #[arg(long = "packs-from", value_name = "DIR")]
    pub packs_from: Option<PathBuf>,
    /// the embedded fleet's directory, before it has started
    #[arg(long, value_name = "DIR", conflicts_with = "embedded")]
    pub fleet: Option<PathBuf>,
}

/// The answer to the store question that installs nothing.
const NO_STORE: &str = "none";

/// The stores `create` installs a pack for, in the order the question lists
/// them: the first is the default. The name is the one `[store] adapter`
/// takes, and the pack is fleet-packs' `adapters/store/<name>`.
const STORES: [&str; 1] = [store::DEFAULT_ADAPTER];

/// The question's rows, one per store and then [`NO_STORE`], in that order.
const STORE_ROWS: [&str; 2] = [
    "bd — beads, installed from fleet-packs",
    "none — install a store pack later",
];

/// What `start` takes.
#[derive(clap::Args)]
pub struct StartArgs {
    /// run the loop in this process instead of the service
    #[arg(long)]
    pub foreground: bool,
}

// ---- fleet create -----------------------------------------------------------

pub fn create_command(ui: &Ui, args: &CreateArgs) -> Exit {
    match create(ui, args) {
        Ok(exit) => exit,
        Err(stop) => stopped("create", &stop),
    }
}

fn create(ui: &Ui, args: &CreateArgs) -> Result<Exit, Stop> {
    let root = std::env::current_dir()
        .map_err(|e| Stop::could_not_tell(format!("the current directory cannot be read: {e}")))?;
    let machine_dir = platform::machine_dir();

    // REFUSED FIRST, before any question — where the refusal holds whatever the
    // answers are. A person answers two prompts and then meets a refusal the
    // call could have carried from the start otherwise.
    //
    // A DECLARED PROJECT WINS AT ITS OWN LEVEL: the resolver reads a directory
    // carrying `.fleet/project.toml` as a standalone project whatever sits
    // beside the declaration, so a fleet.toml there is not this project's fleet
    // and `--standalone` registers the declaration rather than colliding with
    // its neighbour. That makes this refusal the one mode's and not both, so
    // the embedded arm below carries it instead — after the question, because a
    // mode nobody has answered for yet cannot be refused on.
    let fleet_toml = root.join(lifecycle::FLEET_TOML);
    let declared = root.join(lifecycle::PROJECT_TOML);
    if fleet_toml.is_file() && !declared.is_file() {
        return Err(already_a_fleet(&fleet_toml));
    }
    let dot_fleet = root.join(".fleet");
    if dot_fleet.exists() && !declared.is_file() {
        return Err(Stop::refused(format!(
            "{} is here and carries no {} — a directory that is not this project's own \
             declaration is not one this verb will write into",
            dot_fleet.display(),
            lifecycle::PROJECT_TOML
        )));
    }
    // Every scripted answer is checked before any question is asked, so a
    // caller that gave them meets every refusal at once.
    if let Some(named) = &args.agent {
        known_agent(named)?;
    }
    let scripted_store = args.store.as_deref().map(known_store).transpose()?;
    let packs_repo = packs_repo(&root, args.packs_from.as_deref())?;
    if scripted_store == Some(None) && args.packs_from.is_some() {
        return Err(Stop::usage(format!(
            "--packs-from names where the store's pack comes from, and --store {NO_STORE} \
             installs none — drop one of them"
        )));
    }
    // Relative to the directory the call was made in, the way every other path
    // this verb stores is.
    let named_fleet = args.fleet.as_ref().map(|dir| root.join(dir));
    if args.standalone {
        registered_fleet(&machine_dir, named_fleet.as_deref())?;
    }

    let mode = match (args.embedded, args.standalone) {
        (true, _) => Mode::Embedded,
        (_, true) => Mode::Standalone,
        _ => match ui.select(
            "embedded or standalone?",
            &[
                "embedded — one fleet.toml at this project's root",
                "standalone — declare this project to the fleet this machine runs",
            ],
            "--embedded",
        ) {
            Ok(0) => Mode::Embedded,
            Ok(_) => Mode::Standalone,
            Err(prompt) => return Err(asked(&prompt)),
        },
    };
    if mode == Mode::Standalone {
        registered_fleet(&machine_dir, named_fleet.as_deref())?;
    }

    let agent = match &args.agent {
        Some(named) => known_agent(named)?,
        None => match ui.select("which agent?", &controller::AGENTS, "--agent") {
            Ok(index) => controller::AGENTS[index].to_string(),
            Err(prompt) => return Err(asked(&prompt)),
        },
    };

    // THE STORE IS THE ONE QUESTION WITH A DEFAULT: a call with no terminal and
    // no --store takes the first store rather than being refused, because a
    // fleet whose items have nowhere to live is not what a script meant by
    // leaving the flag out.
    let store: Option<String> = match scripted_store {
        Some(chosen) => chosen,
        None => match ui.select_or("which store?", &STORE_ROWS, 0) {
            Ok(index) => STORES.get(index).map(|name| name.to_string()),
            Err(prompt) => return Err(asked(&prompt)),
        },
    };

    // The header the two writers put at the top of the file says how this call
    // reached its answers, so only a flag this call actually carried is named.
    let mode_flag = match (args.embedded, args.standalone) {
        (true, _) => Some("--embedded"),
        (_, true) => Some("--standalone"),
        _ => None,
    };
    let agent_flag = args.agent.as_ref().map(|named| format!("--agent {named}"));
    let store_flag = args
        .store
        .as_ref()
        .map(|named| format!("--store {}", named.trim()));
    let mut flags: Vec<&str> = Vec::new();
    if let Some(flag) = mode_flag {
        flags.push(flag);
    }
    if let Some(flag) = &agent_flag {
        flags.push(flag);
    }
    if let Some(flag) = &store_flag {
        flags.push(flag);
    }
    let store_asked = store_flag.is_none() && ui.can_ask();
    let written_by = lifecycle::written_by(
        &flags,
        mode_flag.is_none() || agent_flag.is_none() || store_asked,
    );

    // THE STORE'S PACK BEFORE ANY FILE: a standalone declaration reads its
    // item prefix off the store it names, so the adapter has to be installed
    // for that read to reach it, and a fetch that fails — no network, a tag the
    // source does not carry — refuses with no fleet file written.
    //
    // THE DEFAULTS GO FIRST WHERE A PACK IS INSTALLED: a pack is checked
    // against the layers it lands on, and the defaults are the bottom one, so
    // `fleet pack add` refuses a machine without them. Where none is, they go
    // in at the end as they always have, after the creator is listed.
    let mut early_defaults = None;
    match &store {
        Some(name) => {
            early_defaults = Some(defaults_into(&machine_dir)?);
            let installed = install_store(ui, &machine_dir, &packs_repo, name)?;
            say_store(ui, name, &installed);
        }
        None => ui.status(
            Stream::Err,
            Tone::Flat,
            "store:",
            &format!(
                "none installed — `{}` installs one",
                store::pack_line(&packs_repo, STORES[0], supported::PINNED_PACKS)
            ),
            None,
        ),
    }

    let written = match mode {
        Mode::Embedded => {
            if fleet_toml.is_file() {
                return Err(already_a_fleet(&fleet_toml));
            }
            write_new(
                &fleet_toml,
                &lifecycle::embedded_text(&agent, store.as_deref(), &written_by),
            )?;
            fleet_toml.clone()
        }
        // A `.fleet/project.toml` ALREADY THERE is a declared project, not a
        // collision: this verb validates its keys and registers it, and writes
        // nothing over a file somebody wrote. EVERY key, not just the one the
        // register is keyed on — a file this verb did not write is the only one
        // whose keys nobody has checked.
        Mode::Standalone if declared.is_file() => {
            let name = lifecycle::validate_written_declaration(&declared).map_err(Stop::refused)?;
            ui.status(
                Stream::Err,
                Tone::Flat,
                "declared:",
                &format!("{name}, already — its keys read and nothing written over"),
                Some(&declared.display().to_string()),
            );
            declared.clone()
        }
        Mode::Standalone => {
            let name = basename(&root);
            // The prefix is the store's to say: the project's store opened as
            // every verb opens it, and asked. A store that cannot be opened or
            // does not answer names no prefix, and the line is left for the
            // person rather than guessed.
            //
            // The file is not written yet, so the policy read here names no
            // adapter, which opens the default store — the first of STORES,
            // and the only store this verb installs.
            let prefix = store::open(&Opening {
                root: &root,
                policy: &store::project_policy(&root)?,
                source: AdapterSource::Setting,
                search_path: &platform::child_path(&platform::home_dir()),
                timeout: STORE_TIMEOUT,
                packs: Some(PackDirs {
                    packs_dir: &machine_dir.join("packs"),
                    defaults_dir: &machine_dir.join(defaults::DIR),
                }),
            })
            .ok()
            .and_then(|opened| opened.capabilities().ok())
            .and_then(|capabilities| capabilities.item_prefix);
            write_new(
                &declared,
                &lifecycle::project_text(
                    &name,
                    prefix.as_deref(),
                    &root,
                    &derived_worktrees_dir(&root),
                    store.as_deref(),
                    &written_by,
                ),
            )?;
            declared.clone()
        }
    };

    // The fleet's own policy file: the one `[seats]` lives in. In standalone
    // mode it is not the file this call wrote — a `[seats]` table in a
    // project's declaration does nothing.
    let fleet_file = match mode {
        Mode::Embedded => fleet_toml.clone(),
        Mode::Standalone => {
            // The declaration's own name, which is the project's: a file
            // somebody wrote may name it something other than its directory.
            let name = lifecycle::validate_declaration(&declared).map_err(Stop::refused)?;
            // THE SEAT LIST BEFORE THE FIRST START. Where `--fleet` named the
            // fleet and no start has written the record yet, this verb writes
            // the record `start`'s first run writes — the same one writer — so
            // declaring a project to a fleet is one act and not a start either
            // side of it. The rows stay `start`'s: this writes `children`
            // empty, exactly as a first run does, and the next start renders
            // what is registered.
            let seats = machine_dir.join("config.json");
            let fleet_file = match registered_fleet(&machine_dir, named_fleet.as_deref())? {
                MachineFleet::Registered(fleet_file) => fleet_file,
                MachineFleet::Named(fleet_file) => {
                    if lifecycle::write_seat_list(&seats, &fleet_file)
                        .map_err(Stop::could_not_tell)?
                    {
                        ui.status(
                            Stream::Err,
                            Tone::Good,
                            "fleet:",
                            &format!("registered on this machine — {}", fleet_file.display()),
                            Some(&seats.display().to_string()),
                        );
                    }
                    fleet_file
                }
            };
            let fresh =
                lifecycle::register(&machine_dir, &root, &name).map_err(Stop::could_not_tell)?;
            if fresh {
                lifecycle::registered_event(&machine_dir, &root, &name)
                    .map_err(Stop::could_not_tell)?;
            }
            ui.status(
                Stream::Err,
                Tone::Good,
                "registered",
                &format!("{name} at {}", root.display()),
                Some(&machine_dir.join(lifecycle::PROJECTS).display().to_string()),
            );
            fleet_file
        }
    };

    // THE PERSON WHO RAN THIS IS THE FLEET'S FIRST SEAT, a human one, listed
    // through `fleet seat add --human`'s own writer. Nobody is asked for a
    // name. Before the defaults where no store's pack took them first, so a
    // machine directory this call cannot write in is met here, with the
    // sentence that says what is left to do.
    let creator = creator(&machine_dir, &fleet_file)?;
    if mode == Mode::Embedded {
        // The file was written by this call a moment ago, so a listing that
        // fails in it is this call's failure and not the fleet's.
        if let Err(why) = &creator.listed {
            return Err(lists_nobody(why));
        }
    }

    let defaults = match early_defaults {
        Some(defaults) => defaults,
        None => defaults_into(&machine_dir)?,
    };

    // The done message. On stderr, because stdout is a script's.
    ui.status(
        Stream::Err,
        Tone::Good,
        "created",
        &format!("{} fleet", mode.as_str()),
        Some(&written.display().to_string()),
    );
    // THE GUARDS AND TELEMETRY LINES BELONG TO THE FILE THIS CALL WROTE. Only
    // `--embedded` writes a policy file, so only `--embedded` can report what
    // is in one: a standalone project's declaration carries neither table, and
    // the fleet's own file was written by whoever created that fleet.
    if mode == Mode::Embedded {
        ui.status(
            Stream::Err,
            Tone::Flat,
            "guards:",
            "shell-trap on, record on",
            None,
        );
        ui.status(
            Stream::Err,
            Tone::Flat,
            "telemetry:",
            "off — nothing leaves this machine",
            None,
        );
    }
    ui.status(
        Stream::Err,
        Tone::Flat,
        "defaults:",
        &defaults_line(&defaults),
        Some(&defaults.root.display().to_string()),
    );
    say_retired(ui, &defaults);
    say_creator(ui, &creator, &machine_dir);
    ui.status(
        Stream::Err,
        Tone::Good,
        "next:",
        "fleet start — it installs the service on its first run and loads it",
        None,
    );
    Ok(Exit::Done)
}

/// The name as the adapters spell it, or the usage refusal naming what is known.
fn known_agent(named: &str) -> Result<String, Stop> {
    let named = named.trim();
    if controller::AGENTS.contains(&named) {
        return Ok(named.to_string());
    }
    Err(Stop::usage(format!(
        "no adapter answers to `{named}` — this fleet knows {}",
        controller::AGENTS.join(", ")
    )))
}

/// The store `--store` names — `None` for [`NO_STORE`] — or the usage refusal
/// naming the answers there are.
fn known_store(named: &str) -> Result<Option<String>, Stop> {
    let named = named.trim();
    if named == NO_STORE {
        return Ok(None);
    }
    if STORES.contains(&named) {
        return Ok(Some(named.to_string()));
    }
    Err(Stop::usage(format!(
        "no store pack answers to `{named}` — this fleet installs {}, or {NO_STORE}",
        STORES.join(", ")
    )))
}

/// The repository the store's pack is installed from: the source this binary
/// pins, or the checkout `--packs-from` names, resolved against the directory
/// the call was made in so the lock records a source that reads the same from
/// anywhere.
fn packs_repo(root: &Path, from: Option<&Path>) -> Result<String, Stop> {
    let Some(dir) = from else {
        return Ok(supported::PINNED_PACKS_SOURCE.to_string());
    };
    let dir = root.join(dir);
    if !dir.is_dir() {
        return Err(Stop::usage(format!(
            "--packs-from {} is not a directory — name a checkout of fleet-packs",
            dir.display()
        )));
    }
    Ok(dir.canonicalize().unwrap_or(dir).display().to_string())
}

/// What became of the store's pack.
enum StorePack {
    /// Fetched, moved in and pinned by this call, with the imports its
    /// checkout carried.
    Added(add::Installed),
    /// A pack of that name was already on this machine — every fleet on it
    /// shares one packs directory — and was left as it stands, with its line
    /// in the lock where it has one.
    Already {
        root: PathBuf,
        pinned: Option<lock::Entry>,
    },
}

/// The store `name`'s pack installed from `repo` at the tag this binary pins,
/// the same work `fleet pack add <repo>//adapters/store/<name> --version <tag>`
/// does: fetched, checked against what is installed, moved in with the
/// imports its checkout carries, and pinned in the machine's lock.
///
/// A pack of that name already installed is left alone, whatever it was
/// installed from: a second fleet on a machine reads the one the first
/// installed, and replacing it is `fleet pack remove` and `fleet pack add`.
fn install_store(ui: &Ui, machine_dir: &Path, repo: &str, name: &str) -> Result<StorePack, Stop> {
    let packs_dir = machine_dir.join("packs");
    let lock_path = machine_dir.join(lock::LOCK);
    let root = packs_dir.join(name);
    if root.exists() {
        let pinned = lock::read(&lock_path).ok().and_then(|lines| {
            lines
                .into_iter()
                .find(|line| line.name.as_deref() == Some(name))
        });
        return Ok(StorePack::Already { root, pinned });
    }

    let source = store::pack_source(repo, name);
    let version = supported::PINNED_PACKS;
    let wait = ui.spinner(&format!("fetching {source} at {version}"));
    let added = add::add(
        &packs_dir,
        &machine_dir.join(defaults::DIR),
        &lock_path,
        &source,
        version,
        &lifecycle::stamp(),
    );
    wait.done();
    added.map(StorePack::Added).map_err(|refusals| {
        let said: Vec<String> = refusals.iter().map(ToString::to_string).collect();
        Stop::refused(format!(
            "the store's pack was not installed, so no fleet file was written: {} — `fleet create \
             --store {NO_STORE}` creates the fleet without one",
            said.join("; ")
        ))
    })
}

/// The lines that say which store pack the fleet reads, and what came with it.
fn say_store(ui: &Ui, name: &str, pack: &StorePack) {
    match pack {
        StorePack::Added(installed) => {
            ui.status(
                Stream::Err,
                Tone::Good,
                "store:",
                &format!(
                    "{} {} at {}, installed and pinned",
                    installed.name, installed.entry.version, installed.entry.commit
                ),
                Some(&installed.root.display().to_string()),
            );
            for import in &installed.imports {
                ui.status(
                    Stream::Err,
                    Tone::Good,
                    "store:",
                    &format!(
                        "{} {} at {}, which {} imports",
                        import.name, import.entry.version, import.entry.commit, installed.name
                    ),
                    Some(&import.root.display().to_string()),
                );
            }
            for missing in &installed.missing {
                ui.status(
                    Stream::Err,
                    Tone::Flat,
                    "store:",
                    &missing.to_string(),
                    None,
                );
            }
        }
        StorePack::Already { root, pinned } => {
            let at = match pinned {
                Some(line) => format!(" at {} from {}", line.version, line.source),
                None => String::new(),
            };
            ui.status(
                Stream::Err,
                Tone::Flat,
                "store:",
                &format!("{name}, already installed on this machine{at} — left as it stands"),
                Some(&root.display().to_string()),
            );
        }
    }
}

/// The one refusal two call sites carry, so the sentence a person meets is the
/// same whether their directory declares a project or not.
fn already_a_fleet(fleet_toml: &Path) -> Stop {
    Stop::refused(format!(
        "{} is already here, so this directory is already a fleet",
        fleet_toml.display()
    ))
}

/// What became of listing the person who ran `create`.
struct Creator {
    /// The listing, or why the fleet's file could not take it.
    listed: Result<Listed, String>,
    /// True where THIS call minted the machine's identity.
    minted: bool,
    /// The fleet's own policy file, the one the listing is in.
    file: PathBuf,
}

/// The person who ran `create`, listed in the fleet's own file.
///
/// THE IDENTITY IS MINTED FIRST AND ON ITS OWN, so the two failures stay apart:
/// an identity this machine cannot read or mint is the call's exit 3 in either
/// mode, and a fleet file that cannot take the listing is the caller's to
/// weigh — the embedded one this call just wrote, or a standalone fleet's that
/// somebody else did.
fn creator(machine_dir: &Path, fleet_file: &Path) -> Result<Creator, Stop> {
    let (_, minted) = identity::identity_or_mint(machine_dir).map_err(lists_nobody)?;
    Ok(Creator {
        listed: seat_add::list_human(fleet_file, machine_dir, None).map_err(|stop| stop.message),
        minted,
        file: fleet_file.to_path_buf(),
    })
}

/// The exit 3 a fleet with nobody listed in it leaves, naming the verb that
/// finishes it.
fn lists_nobody(why: impl std::fmt::Display) -> Stop {
    Stop::could_not_tell(format!(
        "the fleet was written and lists nobody: {why} — fleet seat add --human finishes it"
    ))
}

/// The seat line, and the identity line where this call minted one.
fn say_creator(ui: &Ui, creator: &Creator, machine_dir: &Path) {
    let file = creator.file.display().to_string();
    match &creator.listed {
        Ok(Listed::Added { seat, .. }) => ui.status(
            Stream::Err,
            Tone::Good,
            "seat:",
            &format!(
                "you — human {}, listed as [seats.{}]",
                seat.machine_name(),
                seat.id
            ),
            Some(&file),
        ),
        Ok(Listed::AlreadyListed { seat }) => ui.status(
            Stream::Err,
            Tone::Flat,
            "seat:",
            &format!("you — human {}, already listed", seat.machine_name()),
            Some(&file),
        ),
        // A LINE AND NOT A REFUSAL: the project is declared and registered, and
        // the fleet's file is somebody else's to put right.
        Err(why) => ui.status(
            Stream::Err,
            Tone::Bad,
            "seat:",
            &format!("not listed — {why}; fleet seat add --human lists you"),
            None,
        ),
    }
    if creator.minted {
        ui.status(
            Stream::Err,
            Tone::Flat,
            "identity:",
            "minted — who acts here when no --by is given",
            Some(&machine_dir.join(identity::IDENTITY).display().to_string()),
        );
    }
}

/// Which fleet a `--standalone` project is being declared to.
enum MachineFleet {
    /// The seat list is there and names this readable policy file.
    Registered(PathBuf),
    /// No seat list yet, and `--fleet` named this readable policy file — the
    /// caller writes the record before it registers the project.
    Named(PathBuf),
}

/// A standalone fleet needs one already on the machine: its file is the one the
/// seat list names, and without it a declared project is declared to nothing.
///
/// A SEAT LIST IS NOT THE ONLY WAY TO NAME ONE. The list is written by a fleet's
/// first `start`, so requiring it here put a start between the two creates for
/// no reason a person could see; `--fleet` names the embedded fleet's own
/// directory instead, and the refusal below is owed only where neither says
/// which fleet this project is being declared to.
///
/// PURE — every arm reads, and the write the `Named` arm licences is the
/// caller's, so the two refusal checks this verb makes before its questions
/// move nothing.
fn registered_fleet(machine_dir: &Path, named: Option<&Path>) -> Result<MachineFleet, Stop> {
    let seats = machine_dir.join("config.json");
    match config::read(&seats) {
        Ok(machine) if machine.fleet_toml.is_file() => {
            Ok(MachineFleet::Registered(machine.fleet_toml))
        }
        Ok(machine) => Err(Stop::refused(format!(
            "{} names {}, which is not a readable file — run `fleet start` where that fleet's \
             own file is",
            seats.display(),
            machine.fleet_toml.display()
        ))),
        // A file that is there and does not read is not an absent one: writing
        // the record over it would replace a machine's own seat rows.
        Err(why) if seats.exists() => Err(Stop::refused(format!(
            "{why} — that file names the fleet this project would be declared to"
        ))),
        Err(_) => match named {
            Some(dir) => {
                let fleet_toml = dir.join(lifecycle::FLEET_TOML);
                if !fleet_toml.is_file() {
                    return Err(Stop::refused(format!(
                        "--fleet {} holds no {} — name the directory of an embedded fleet, the \
                         one `fleet create --embedded` wrote that file in",
                        dir.display(),
                        lifecycle::FLEET_TOML
                    )));
                }
                // Resolved, the way `create` resolves the project root it
                // stores: the record is read back by every later verb, and two
                // spellings of one directory are two fleets to a reader that
                // compares them.
                Ok(MachineFleet::Named(
                    fleet_toml.canonicalize().unwrap_or(fleet_toml),
                ))
            }
            None => Err(Stop::refused(format!(
                "no fleet is registered on this machine — {} is not there; name an embedded \
                 fleet's directory with --fleet, run `fleet start` in one first, or create this \
                 one with --embedded",
                seats.display()
            ))),
        },
    }
}

/// The binary's defaults materialized into a machine directory: the ONE call
/// `create` and `start` share, so which of them ran does not decide which set
/// the machine holds. What it does about a copy already standing there —
/// refresh it, leave it, or refuse it — is [`fleet_core::defaults`]'s.
fn defaults_into(machine_dir: &Path) -> Result<defaults::Installed, Stop> {
    defaults::install(
        &machine_dir.join(defaults::DIR),
        &machine_dir.join("packs"),
        &machine_dir.join(fleet_core::lock::LOCK),
        env!("CARGO_PKG_VERSION"),
        &lifecycle::stamp(),
    )
    .map_err(|refusal| Stop::refused(refusal.to_string()))
}

/// What became of the bundled core pack a previous binary installed, where one
/// stood. A second line rather than a clause on the first, because it is a
/// REMOVAL a person would want to have been told about, and the machine it
/// happens on is every machine that ran that binary.
fn retired_line(defaults: &defaults::Installed) -> Option<(Tone, String, String)> {
    match defaults.retired.as_ref()? {
        defaults::Retired::Removed(root) => Some((
            Tone::Flat,
            "retired the bundled core pack — its templates are the defaults now".to_string(),
            root.display().to_string(),
        )),
        defaults::Retired::LeftStanding { root, why } => Some((
            Tone::Bad,
            format!(
                "the bundled core pack is not this binary's to remove — {why}; it layers \
                 ABOVE the defaults until you take it out"
            ),
            root.display().to_string(),
        )),
    }
}

/// The retirement line beside the defaults line, on the one stream both use.
fn say_retired(ui: &Ui, defaults: &defaults::Installed) {
    if let Some((tone, said, root)) = retired_line(defaults) {
        ui.status(Stream::Err, tone, "defaults:", &said, Some(&root));
    }
}

/// The one line that says which of the three happened, because a replacement a
/// person cannot see is the same to them as no replacement at all.
fn defaults_line(defaults: &defaults::Installed) -> String {
    let version = &defaults.entry.version;
    match defaults.landed {
        defaults::Landed::Fresh => format!("installed {version}"),
        defaults::Landed::Refreshed => {
            format!("refreshed {version} — the copy that stood here held another set")
        }
        defaults::Landed::Already => format!("already at {version}"),
    }
}

/// Write a file that is not there. The two refusals above have already read the
/// two paths this verb writes, so a file that appeared in between is a race with
/// a second `create` and is refused rather than overwritten.
fn write_new(path: &Path, text: &str) -> Result<(), Stop> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| Stop::could_not_tell(format!("{}: {e}", dir.display())))?;
    }
    if path.exists() {
        return Err(Stop::refused(format!(
            "{} appeared while this verb was running and was not written over",
            path.display()
        )));
    }
    std::fs::write(path, text).map_err(|e| Stop::could_not_tell(format!("{}: {e}", path.display())))
}

// ---- fleet start ------------------------------------------------------------

pub fn start_command(ui: &Ui, args: &StartArgs) -> Exit {
    match start(ui, args) {
        Ok(exit) => exit,
        Err(stop) => stopped("start", &stop),
    }
}

fn start(ui: &Ui, args: &StartArgs) -> Result<Exit, Stop> {
    let fleet = Fleet::resolve()?;
    let service = service(&fleet.machine_dir)?;

    // THE FIVE REFUSALS, before the first-run work and before anything is
    // loaded.
    if let Some(pid) = service.running().map_err(Stop::could_not_tell)? {
        let tick = lifecycle::last_tick(&fleet.machine_dir)
            .unwrap_or_else(|| "no projection has been published".to_string());
        return Err(Stop::refused(format!(
            "the controller is already running as pid {pid}; its last tick was {tick}"
        )));
    }
    let home = platform::home_dir();
    let child_path = platform::child_path(&home);
    // The agent, opened the one way the controller will open it: an agent that
    // can issue no effect is a controller that would start no seat, so it is
    // refused here, before anything is loaded.
    let opened = adapter::open(&adapter::Opening {
        home: &home,
        plugin_dir: None,
        permissions: None,
    })
    .map_err(Stop::could_not_tell)?;
    if let Some(why) = &opened.effects_off {
        return Err(Stop::could_not_tell(format!(
            "{why} — nothing was loaded; the search path is {child_path}"
        )));
    }
    let capabilities = opened
        .agent
        .capabilities()
        .map_err(|why| Stop::could_not_tell(format!("{why} — nothing was loaded")))?;
    // The host every seat's session runs on, resolved as the controller
    // resolves it: a controller with none starts no seat at all.
    if let Err(why) = fleet_controller::host::TmuxHost::resolve(&child_path) {
        return Err(Stop::could_not_tell(format!(
            "{why} — nothing was loaded; the search path is {child_path}"
        )));
    }
    daemon_hosted(ui, &fleet.machine_dir, opened.daemon.as_deref())?;

    let policy = controller::load(&fleet.fleet_toml).map_err(Stop::could_not_tell)?;
    let report = lifecycle::first_run(&FirstRun {
        machine_dir: &fleet.machine_dir,
        fleet_toml: &fleet.fleet_toml,
        project: fleet
            .project
            .as_ref()
            .map(|(name, worktrees_dir)| ProjectAt {
                name,
                worktrees_dir,
            }),
        policy: &policy,
        agent: &capabilities,
        service: &service,
    })
    .map_err(Stop::could_not_tell)?;
    for line in &report.lines {
        ui.status(Stream::Err, Tone::Flat, "first run:", line, None);
    }
    if !report.changed {
        ui.status(
            Stream::Err,
            Tone::Flat,
            "first run:",
            "every step was already done, so this start repeated none of it",
            None,
        );
    }

    // THE DEFAULTS THIS BINARY CARRIES, on every start and not only on the one
    // `create` that made the fleet: `create` refuses a root that already holds
    // a fleet.toml, so a machine whose binary was rebuilt has no other call
    // that would bring the set it ships.
    //
    // A refusal here is a LINE AND NEVER A STOP. Start has its own five
    // refusals and this is not a sixth: a machine whose defaults this binary
    // will not write over still gets its controller.
    match defaults_into(&fleet.machine_dir) {
        Ok(defaults) => {
            ui.status(
                Stream::Err,
                Tone::Flat,
                "defaults:",
                &defaults_line(&defaults),
                Some(&defaults.root.display().to_string()),
            );
            say_retired(ui, &defaults);
        }
        Err(stop) => ui.status(
            Stream::Err,
            Tone::Flat,
            "defaults:",
            &format!("left as it stands — {}", stop.message),
            None,
        ),
    }

    if args.foreground {
        ui.status(
            Stream::Err,
            Tone::Good,
            "running",
            "in this process — nothing was loaded",
            Some(&fleet.machine_dir.display().to_string()),
        );
        // THE SERVICE'S LOOP, not a thinner one: `crate::observe_loop` is the
        // single wiring both ways of running the controller come through, so a
        // waiting run advances here exactly as it does under the service.
        let status = crate::observe_loop(false);
        return Exit::from_status(status)
            .map_err(|e| Stop::could_not_tell(format!("the loop answered {status}: {e}")));
    }

    // The line the confirmation has to be above, read BEFORE the load: an old
    // `controller.started` from a previous run is what a confirmation must not
    // accept.
    let head = lifecycle::stream_head(&fleet.machine_dir);
    let wait = ui.spinner("loading the service");
    let loaded = service.load();
    wait.done();
    loaded.map_err(Stop::could_not_tell)?;

    let wait = ui.spinner("waiting for the controller's first line");
    let confirmed = lifecycle::confirm(
        &fleet.machine_dir,
        events::CONTROLLER_STARTED,
        head,
        lifecycle::CONFIRM_TIMEOUT,
    );
    wait.done();
    match confirmed {
        Ok(seq) => {
            ui.status(
                Stream::Err,
                Tone::Good,
                "started",
                &format!("{} — controller.started at sequence {seq}", service.label()),
                Some(&fleet.machine_dir.display().to_string()),
            );
            Ok(Exit::Done)
        }
        Err(not) => Err(Stop::could_not_tell(not.sentence())),
    }
}

/// The upgrade refusal, at the terminal: the controller makes the same check
/// before its first poll (ruling 10: the upgrade adopts nothing), and a person
/// who meets it here reads it where they typed, not in the service's log.
///
/// It reads what the controller will: the seat list, the session table's
/// recorded directories, and the listing under each. A seat list or a table
/// that will not read names no seat and no directory here — the controller
/// says why when it reads them — and a listing that cannot be read is one
/// line, and the start goes on. An agent that has no daemon to host a seat's
/// session reads nothing.
fn daemon_hosted(
    ui: &Ui,
    machine_dir: &Path,
    daemon: Option<&dyn DaemonListing>,
) -> Result<(), Stop> {
    let Some(daemon) = daemon else {
        return Ok(());
    };
    let seats = config::read(&machine_dir.join("config.json"))
        .map(|machine| machine.seats)
        .unwrap_or_default();
    let (table, _) = sessions::read(&sessions::path_in(machine_dir));
    match run::daemon_check(&seats, &table.unwrap_or_default(), daemon) {
        DaemonCheck::Clear => Ok(()),
        DaemonCheck::Hosted(lines) => Err(Stop::refused(format!(
            "{}\nnothing was loaded",
            lines.join("\n")
        ))),
        DaemonCheck::Unreadable(cause) => {
            ui.status(
                Stream::Err,
                Tone::Flat,
                "agent:",
                &run::daemon_unreadable_line(&cause),
                None,
            );
            Ok(())
        }
    }
}

// ---- fleet stop -------------------------------------------------------------

pub fn stop_command(ui: &Ui) -> Exit {
    match stop(ui) {
        Ok(exit) => exit,
        Err(stop) => stopped("stop", &stop),
    }
}

fn stop(ui: &Ui) -> Result<Exit, Stop> {
    // NO FLEET IS RESOLVED HERE. A stop unloads a service and reads one event
    // back, and both of those live under the machine directory: resolving the
    // policy file first would refuse from a directory that resolves to no
    // project while the service is loaded, leaving a controller running and
    // `controller.stopped` never read.
    let machine_dir = platform::machine_dir();
    let service = service(&machine_dir)?;

    // THE QUERY FIRST: an unload of a service nobody loaded is a refusal on the
    // record and not a no-op.
    if service.running().map_err(Stop::could_not_tell)?.is_none() {
        return Err(Stop::refused(format!(
            "no service is loaded under {} — there is nothing to stop",
            service.label()
        )));
    }

    let head = lifecycle::stream_head(&machine_dir);
    let wait = ui.spinner("unloading the service");
    let unloaded = service.unload();
    wait.done();
    unloaded.map_err(Stop::could_not_tell)?;

    let wait = ui.spinner("waiting for the controller's last line");
    let confirmed = lifecycle::confirm(
        &machine_dir,
        events::CONTROLLER_STOPPED,
        head,
        lifecycle::CONFIRM_TIMEOUT,
    );
    wait.done();
    match confirmed {
        Ok(seq) => {
            ui.status(
                Stream::Err,
                Tone::Good,
                "stopped",
                &format!("{} — controller.stopped at sequence {seq}", service.label()),
                None,
            );
            // Said, because it is the one thing a person expects a stop to have
            // done and it has not: a stopped controller leaves its seats where
            // they are.
            ui.status(
                Stream::Err,
                Tone::Flat,
                "seats:",
                "untouched — their sessions are still running",
                None,
            );
            Ok(Exit::Done)
        }
        Err(not) => Err(Stop::could_not_tell(not.sentence())),
    }
}

// ---- what only a process knows ----------------------------------------------

/// The fleet a lifecycle verb acts on.
struct Fleet {
    /// The policy file in force: this project's own where the fleet is embedded,
    /// and the one the machine's seat list names where it is not.
    fleet_toml: PathBuf,
    machine_dir: PathBuf,
    /// The project this directory resolves to, and where its worktrees go.
    /// `None` outside every project — a standalone fleet started from anywhere
    /// else — and then no seat row is rendered, because a row's worktrees map
    /// has no project to key on and a guessed one puts a seat in the wrong
    /// checkout.
    project: Option<(String, PathBuf)>,
}

impl Fleet {
    /// EMBEDDED FIRST, then standalone, then neither — the same order the item
    /// verbs and the guards resolve in.
    ///
    /// THE REFUSAL IS A CONJUNCTION. A directory that resolves to no project is
    /// not by itself a fleetless one: a standalone fleet's policy is the file
    /// the machine's seat list names, so the refusal is owed only where there
    /// is no policy file above the cwd AND no seat list naming one.
    fn resolve() -> Result<Fleet, Stop> {
        let machine_dir = platform::machine_dir();
        let here = resolve_at(None).ok();
        let embedded = here
            .as_ref()
            .map(|here| here.project.root.join(lifecycle::FLEET_TOML))
            .filter(|path| path.is_file());
        let fleet_toml = match embedded {
            Some(path) => path,
            None => match config::read(&machine_dir.join("config.json")) {
                Ok(machine) if machine.fleet_toml.is_file() => machine.fleet_toml,
                _ => return Err(no_fleet(&machine_dir)),
            },
        };
        // THE REGISTERED PROJECT THE FLEET SITS INSIDE WINS OVER THE CWD's own,
        // because a seat row's worktree is a property of the seat and the
        // project and never of the directory a person typed the start in.
        let project = match host_project(&machine_dir, &fleet_toml)? {
            Some(host) => Some(host),
            None => match here {
                Some(here) => Some((here.project.name.clone(), here.worktrees_dir()?)),
                None => None,
            },
        };
        Ok(Fleet {
            fleet_toml,
            machine_dir,
            project,
        })
    }
}

/// The registered project whose root CONTAINS the fleet's own directory, where
/// there is one — an embedded fleet inside a project the machine has registered
/// is that project's fleet, not a project of its own.
///
/// The name and the worktrees directory are read from the containing project's
/// DECLARATION rather than from the register, which carries no worktrees key; a
/// registered root with no declaration beside it is skipped, because there is
/// nothing there to key a row's worktrees on.
///
/// The DEEPEST containing root wins: a project registered inside another one is
/// the nearer statement about this directory. A root equal to the fleet's own
/// directory is not a containing one — that is the standalone case the cwd
/// already answers.
///
/// A registered root is matched by its spelling as the register holds it, so a
/// root reached through a symlink the register does not spell is one this reader
/// will not match.
fn host_project(machine_dir: &Path, fleet_toml: &Path) -> Result<Option<(String, PathBuf)>, Stop> {
    let Some(dir) = fleet_toml.parent() else {
        return Ok(None);
    };
    let registered = lifecycle::registered(machine_dir).map_err(Stop::could_not_tell)?;
    let containing = registered
        .iter()
        .map(|project| PathBuf::from(&project.root))
        .filter(|root| dir.starts_with(root) && dir != root)
        .filter(|root| root.join(lifecycle::PROJECT_TOML).is_file())
        .max_by_key(|root| root.components().count());
    let Some(root) = containing else {
        return Ok(None);
    };
    let there = resolve_from(&root, machine_dir.to_path_buf(), None)?;
    Ok(Some((there.project.name.clone(), there.worktrees_dir()?)))
}

fn no_fleet(machine_dir: &Path) -> Stop {
    Stop::refused(format!(
        "no {} above this directory and no fleet named by {} — `fleet create` writes one",
        lifecycle::FLEET_TOML,
        machine_dir.join("config.json").display()
    ))
}

/// The service manager, with the seam read here: the binary is what only a
/// process knows, exactly as the agent binary is.
fn service(machine_dir: &Path) -> Result<platform::Service, Stop> {
    platform::Service::resolve(
        &platform::home_dir(),
        machine_dir,
        &exe()?,
        // The machine directory reaches the service ONLY when this process was
        // pointed at one: a service that baked in the default would carry a
        // variable the caller never set.
        std::env::var("FLEET_DIR")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(PathBuf::from)
            .as_deref(),
        std::env::var(platform::SERVICE_BIN_ENV).ok().as_deref(),
    )
    .map_err(Stop::could_not_tell)
}

/// This executable's own path. The service runs THIS binary, so a service file
/// naming a binary nobody resolved is one that loads and fails.
fn exe() -> Result<PathBuf, Stop> {
    std::env::current_exe()
        .map_err(|e| Stop::could_not_tell(format!("this executable's own path: {e}")))
}

fn basename(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.display().to_string())
}

/// A question that was not answered: the usage refusal naming the flag, or the
/// terminal that went away.
fn asked(prompt: &Prompt) -> Stop {
    Stop {
        code: prompt.exit().code(),
        message: prompt.sentence(),
    }
}

fn stopped(verb: &str, stop: &Stop) -> Exit {
    eprintln!("fleet {verb}: {}", stop.message);
    Exit::from_status(stop.code).unwrap_or(Exit::CouldNotTell)
}
