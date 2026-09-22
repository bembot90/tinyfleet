//! The one place this binary decides how it looks.
//!
//! Four surfaces, one rule each, and every verb reaches a terminal through one
//! of them rather than picking a look of its own:
//!
//! * **status lines** — a verb, a subject and an outcome, styled through
//!   `console`. The palette is read once, at startup, and is plain whenever
//!   stdout is not a terminal, `NO_COLOR` is set, or `TERM` is `dumb`; plain,
//!   the three parts join as `verb subject — outcome`.
//! * **progress** — a bar for a bounded count and a spinner for an unbounded
//!   wait, through `indicatif`, both drawn on stderr so stdout stays a
//!   script's, and both silent until the wait has ALREADY lasted
//!   [`THRESHOLD`]. A spinner on a verb that takes a second is noise; a bar on
//!   a three-minute wait is information. `land` takes the bounded one, one step
//!   per gate row, its message carrying the suite log's line count as it grows.
//! * **prompts** — select, confirm and input through `dialoguer`, refused with
//!   the usage status and a sentence naming the flag that answers the question
//!   whenever stdin is not a terminal: a prompt never blocks a script.
//! * **tables** — none. The listing verbs, when they land, print their columns
//!   with the standard library and no table crate enters for them.
//!
//! The frames that follow wire in here rather than inventing: `fly` takes a
//! multi-bar, one [`Wait`] per item in flight with the flight's own line above.
//!
//! `create` calls [`Ui::select`] twice — embedded-or-standalone, then the agent
//! — with `--embedded`, `--standalone` and `--agent` as the non-terminal
//! answers. [`Ui::confirm`], [`Ui::input`] and [`Wait::is_showing`] are still
//! held by this module's own arms and by no verb: each carries an
//! `#[allow(dead_code)]` until the frame that calls it lands.

use crate::exit::Exit;
use console::Style;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// How long a wait must already have lasted before any progress is drawn.
pub const THRESHOLD: Duration = Duration::from_secs(2);

/// Which stream a line is a line of: stdout is the verb's answer and stderr is
/// everything a script does not parse.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stream {
    Out,
    Err,
}

/// What a line means, which is the only thing that decides its colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    /// something was done
    Good,
    /// something was refused
    Bad,
    /// a fact the verb is reporting
    Flat,
}

/// The terminal this process was started on, read once.
pub struct Ui {
    color: bool,
    stderr_is_terminal: bool,
    stdin_is_terminal: bool,
}

impl Ui {
    /// The three readings, taken once at startup: a per-call reading would let
    /// one verb's output change shape halfway down a page.
    pub fn from_env() -> Ui {
        let color = std::io::stdout().is_terminal()
            && std::env::var_os("NO_COLOR").is_none()
            && !matches!(std::env::var("TERM").as_deref(), Ok("dumb"));
        Ui::new(
            color,
            std::io::stderr().is_terminal(),
            std::io::stdin().is_terminal(),
        )
    }

    pub fn new(color: bool, stderr_is_terminal: bool, stdin_is_terminal: bool) -> Ui {
        Ui {
            color,
            stderr_is_terminal,
            stdin_is_terminal,
        }
    }

    // ---- status lines -------------------------------------------------------

    /// The one function that prints a styled line.
    pub fn status(
        &self,
        stream: Stream,
        tone: Tone,
        verb: &str,
        subject: &str,
        outcome: Option<&str>,
    ) {
        let line = self.line(tone, verb, subject, outcome);
        match stream {
            Stream::Out => println!("{line}"),
            Stream::Err => eprintln!("{line}"),
        }
    }

    /// The rendering [`Ui::status`] prints, as a string. Plain, it is byte for
    /// byte the text each of these lines carried before this module existed.
    pub fn line(&self, tone: Tone, verb: &str, subject: &str, outcome: Option<&str>) -> String {
        let mut rendered = self.tone_style(tone).apply_to(verb).to_string();
        if !subject.is_empty() {
            rendered.push(' ');
            rendered.push_str(subject);
        }
        if let Some(outcome) = outcome {
            rendered.push_str(" — ");
            rendered.push_str(&self.dim().apply_to(outcome).to_string());
        }
        rendered
    }

    fn tone_style(&self, tone: Tone) -> Style {
        let style = match tone {
            Tone::Good => Style::new().green().bold(),
            Tone::Bad => Style::new().red().bold(),
            Tone::Flat => Style::new().cyan(),
        };
        style.force_styling(self.color)
    }

    fn dim(&self) -> Style {
        Style::new().dim().force_styling(self.color)
    }

    // ---- progress -----------------------------------------------------------

    /// An unbounded wait: a clone, a network call, a suite whose length is not
    /// known until it ends.
    pub fn spinner(&self, message: &str) -> Wait {
        Wait::start(
            ProgressBar::new_spinner().with_message(message.to_string()),
            THRESHOLD,
            self.stderr_is_terminal,
        )
    }

    /// A bounded count: `len` items, each one a [`Wait::inc`].
    ///
    /// The template is set because the default one for a bar is the bar and its
    /// counts alone: a [`Wait::say`] on a bar drawn with the default style is
    /// carried and never shown, which is a message nobody can read.
    pub fn bar(&self, len: u64, message: &str) -> Wait {
        let bar = ProgressBar::new(len).with_message(message.to_string());
        let bar = match ProgressStyle::with_template("{bar:24} {pos}/{len} {msg}") {
            Ok(style) => bar.with_style(style),
            Err(_) => bar,
        };
        Wait::start(bar, THRESHOLD, self.stderr_is_terminal)
    }

    // ---- prompts ------------------------------------------------------------

    /// One of `options`, by index.
    ///
    /// The first row is selected before a key is pressed, because dialoguer
    /// accepts Enter only while something is selected: with no default, Enter
    /// redraws the list and an arrow is the first keystroke every question
    /// costs.
    pub fn select(&self, question: &str, options: &[&str], flag: &str) -> Result<usize, Prompt> {
        self.askable(question, flag)?;
        dialoguer::Select::new()
            .with_prompt(question)
            .items(options)
            .default(0)
            .interact()
            .map_err(|e| Prompt::Failed(e.to_string()))
    }

    /// Yes or no.
    #[allow(dead_code)]
    pub fn confirm(&self, question: &str, flag: &str) -> Result<bool, Prompt> {
        self.askable(question, flag)?;
        dialoguer::Confirm::new()
            .with_prompt(question)
            .interact()
            .map_err(|e| Prompt::Failed(e.to_string()))
    }

    /// A line of text.
    #[allow(dead_code)]
    pub fn input(&self, question: &str, flag: &str) -> Result<String, Prompt> {
        self.askable(question, flag)?;
        dialoguer::Input::<String>::new()
            .with_prompt(question)
            .interact_text()
            .map_err(|e| Prompt::Failed(e.to_string()))
    }

    /// The gate every prompt passes first: a question put to a pipe is a usage
    /// error naming the flag, never a wait for an answer that cannot come.
    fn askable(&self, question: &str, flag: &str) -> Result<(), Prompt> {
        if self.stdin_is_terminal {
            return Ok(());
        }
        Err(Prompt::NotATerminal {
            question: question.to_string(),
            flag: flag.to_string(),
        })
    }
}

/// Why a question was not answered.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Prompt {
    /// stdin is not a terminal, and the flag named answers this question
    NotATerminal { question: String, flag: String },
    /// the terminal was there and the interaction failed
    Failed(String),
}

#[allow(dead_code)]
impl Prompt {
    /// A refused question is the caller's usage error; a failed interaction is
    /// an instrument that could not be read.
    pub fn exit(&self) -> Exit {
        match self {
            Prompt::NotATerminal { .. } => Exit::Usage,
            Prompt::Failed(_) => Exit::CouldNotTell,
        }
    }

    pub fn sentence(&self) -> String {
        match self {
            Prompt::NotATerminal { question, flag } => {
                format!("fleet: {question} — stdin is not a terminal; answer it with {flag}")
            }
            Prompt::Failed(why) => format!("fleet: the question could not be asked: {why}"),
        }
    }
}

/// A wait that draws itself only once it has already lasted the threshold.
pub struct Wait {
    bar: ProgressBar,
    #[allow(dead_code)]
    showing: Arc<AtomicBool>,
}

impl Wait {
    fn start(bar: ProgressBar, threshold: Duration, draw: bool) -> Wait {
        bar.set_draw_target(ProgressDrawTarget::hidden());
        let showing = Arc::new(AtomicBool::new(false));
        if draw {
            bar.enable_steady_tick(Duration::from_millis(120));
            let bar = bar.clone();
            let showing = Arc::clone(&showing);
            // A timer and not a poll: nothing here asks the wait how long it has
            // lasted, so a verb that ends first pays only for the thread.
            std::thread::spawn(move || {
                std::thread::sleep(threshold);
                if !bar.is_finished() {
                    bar.set_draw_target(ProgressDrawTarget::stderr());
                    showing.store(true, Ordering::SeqCst);
                }
            });
        }
        Wait { bar, showing }
    }

    /// Whether this wait has been drawn — the threshold has passed and stderr
    /// is a terminal.
    #[allow(dead_code)]
    pub fn is_showing(&self) -> bool {
        self.showing.load(Ordering::SeqCst)
    }

    /// One item of a bounded count done.
    pub fn inc(&self, n: u64) {
        self.bar.inc(n);
    }

    /// What the wait says about itself while it lasts. It reaches the terminal
    /// only once the wait is drawn, so a message set under the threshold is
    /// carried and never printed.
    pub fn say(&self, text: &str) {
        self.bar.set_message(text.to_string());
    }

    /// The wait is over: whatever was drawn is taken back off the terminal, so
    /// a verb's own last line is the last thing on the page.
    pub fn done(self) {
        self.bar.finish_and_clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{Prompt, Tone, Ui, Wait};
    use crate::exit::Exit;
    use indicatif::ProgressBar;
    use std::time::{Duration, Instant};

    fn plain() -> Ui {
        Ui::new(false, false, false)
    }

    /// The three call-site shapes this binary prints today, each asserted as
    /// the exact text the verb printed before this module existed.
    #[test]
    fn plain_lines_are_the_text_the_verbs_printed_before() {
        let ui = plain();
        assert_eq!(
            ui.line(
                Tone::Good,
                "added",
                "neighborly v1 at c0ffee",
                Some("/packs/neighborly")
            ),
            "added neighborly v1 at c0ffee — /packs/neighborly"
        );
        assert_eq!(
            ui.line(Tone::Good, "pinned", "in /packs.lock", None),
            "pinned in /packs.lock"
        );
        assert_eq!(
            ui.line(
                Tone::Good,
                "removed",
                "neighborly v1",
                Some("/packs/neighborly")
            ),
            "removed neighborly v1 — /packs/neighborly"
        );
        assert_eq!(
            ui.line(
                Tone::Good,
                "dropped",
                "git@example:packs//neighborly v1 from /packs.lock",
                None
            ),
            "dropped git@example:packs//neighborly v1 from /packs.lock"
        );
        assert_eq!(
            ui.line(Tone::Good, "pack", "tiny 0.1.0", Some("schema 2")),
            "pack tiny 0.1.0 — schema 2"
        );
        assert_eq!(
            ui.line(Tone::Flat, "slot", "assets: 3 entries", None),
            "slot assets: 3 entries"
        );
        assert_eq!(
            ui.line(Tone::Bad, "tiny:", "unknown top-level name `bin`", None),
            "tiny: unknown top-level name `bin`"
        );
        assert_eq!(
            ui.line(
                Tone::Bad,
                "shell-trap",
                "bare-id: not configured",
                Some("item.prefix")
            ),
            "shell-trap bare-id: not configured — item.prefix"
        );
    }

    /// A verb with nothing after it renders alone: the layering refusals and
    /// `pack add`'s own prefix line pass their whole text as the verb.
    #[test]
    fn an_empty_subject_adds_no_separator() {
        assert_eq!(
            plain().line(Tone::Bad, "the agent name is in both", "", None),
            "the agent name is in both"
        );
    }

    /// The control for the arm above: with the palette on, the same call
    /// carries escapes — so a plain assertion is a statement about the palette
    /// and not about a module that never styles anything.
    #[test]
    fn the_palette_on_carries_escapes_the_plain_one_does_not() {
        let styled = Ui::new(true, true, true).line(Tone::Good, "added", "a pack", Some("here"));
        assert!(styled.contains('\u{1b}'), "styled: {styled:?}");
        assert!(
            styled.contains("added"),
            "the text survives the styling: {styled:?}"
        );
        let plain = plain().line(Tone::Good, "added", "a pack", Some("here"));
        assert!(!plain.contains('\u{1b}'), "plain: {plain:?}");
    }

    /// Each of the three prompt surfaces refuses a pipe with the usage status
    /// and a sentence naming its own flag.
    #[test]
    fn every_prompt_refuses_a_pipe_and_names_the_flag_that_answers_it() {
        let ui = plain();
        let refusals = [
            ui.select(
                "embedded or standalone?",
                &["embedded", "standalone"],
                "--embedded",
            )
            .expect_err("stdin is not a terminal"),
            ui.confirm("overwrite it?", "--force")
                .expect_err("stdin is not a terminal"),
            ui.input("which agent?", "--agent")
                .expect_err("stdin is not a terminal"),
        ];
        for (refusal, flag) in refusals.iter().zip(["--embedded", "--force", "--agent"]) {
            assert_eq!(refusal.exit(), Exit::Usage, "{refusal:?}");
            assert_eq!(refusal.exit().code(), 2, "{refusal:?}");
            assert!(
                refusal.sentence().contains(flag),
                "the sentence names the flag that answers it: {}",
                refusal.sentence()
            );
            assert!(
                refusal.sentence().contains("not a terminal"),
                "{}",
                refusal.sentence()
            );
        }
    }

    /// The control the arm above needs: the sentence names the flag it was
    /// given rather than a constant, and a failed interaction is a different
    /// row of the exit table.
    #[test]
    fn a_failed_interaction_is_could_not_tell_and_not_usage() {
        let failed = Prompt::Failed(String::from("the terminal went away"));
        assert_eq!(failed.exit(), Exit::CouldNotTell);
        assert_eq!(failed.exit().code(), 3);
        assert!(!failed.sentence().contains("--"), "{}", failed.sentence());
    }

    /// The threshold is the whole of the progress policy: nothing is drawn
    /// until the wait has already lasted it, and then it is.
    #[test]
    fn a_wait_draws_itself_only_once_the_threshold_has_passed() {
        let wait = Wait::start(ProgressBar::new_spinner(), Duration::from_millis(50), true);
        assert!(!wait.is_showing(), "nothing is drawn at the start");

        let deadline = Instant::now() + Duration::from_secs(5);
        while !wait.is_showing() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            wait.is_showing(),
            "the wait outlasted its threshold and is drawn"
        );
        wait.done();
    }

    /// Two controls for the arm above, each removing one half of the rule: a
    /// wait shorter than its threshold draws nothing, and neither does one on a
    /// stderr that is not a terminal, however long it lasts.
    #[test]
    fn a_short_wait_and_a_piped_stderr_each_draw_nothing() {
        let short = Wait::start(ProgressBar::new_spinner(), Duration::from_secs(30), true);
        std::thread::sleep(Duration::from_millis(200));
        assert!(!short.is_showing(), "the threshold has not passed");
        short.done();

        let piped = Wait::start(ProgressBar::new_spinner(), Duration::from_millis(50), false);
        std::thread::sleep(Duration::from_millis(300));
        assert!(!piped.is_showing(), "stderr is not a terminal");
        piped.done();
    }
}
