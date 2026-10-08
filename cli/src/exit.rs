//! The one exit vocabulary every command shares.
//!
//! A script reading `$?` learns the same thing from every verb, so the table
//! lives in one type and `main` is its only reader. A subcommand returns the
//! row it means; an `Err` is could-not-tell and nothing else, because a verb
//! that cannot say what happened is a third answer rather than a failure.

use anyhow::{bail, Result};

/// The exit table, one variant per row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Exit {
    /// done, and the write (if any) read back
    Done = 0,
    /// refused on the record: the thing named is absent, held, or already so
    Refused = 1,
    /// usage: a missing or unknown argument, with the usage line printed
    Usage = 2,
    /// could not tell: an instrument the answer needs was unreadable
    CouldNotTell = 3,
    /// the seat has no live session
    NoSession = 4,
    /// no collector is consuming the stream
    NoCollector = 5,
    /// the row is transient where a named seat was required
    Transient = 6,
}

impl Exit {
    pub fn code(self) -> u8 {
        self as u8
    }

    /// The row's own name, in snake case: the `code` the JSON envelope's
    /// refusal carries (`envelope.rs`). A caller reading a document branches on
    /// the name where a caller reading `$?` branches on the number, and both
    /// read this one table.
    pub fn class(self) -> &'static str {
        match self {
            Exit::Done => "done",
            Exit::Refused => "refused",
            Exit::Usage => "usage",
            Exit::CouldNotTell => "could_not_tell",
            Exit::NoSession => "no_session",
            Exit::NoCollector => "no_collector",
            Exit::Transient => "transient",
        }
    }

    /// A status a library handed back, read into the table.
    ///
    /// The controller and core answer in `u8`, which is a wider set than the
    /// table: a status outside it is a defect in the caller's own mapping and
    /// not a row, so it is an error here rather than a guessed row.
    pub fn from_status(status: u8) -> Result<Exit> {
        Ok(match status {
            0 => Exit::Done,
            1 => Exit::Refused,
            2 => Exit::Usage,
            3 => Exit::CouldNotTell,
            4 => Exit::NoSession,
            5 => Exit::NoCollector,
            6 => Exit::Transient,
            other => bail!("exit {other} is not a row of the exit table"),
        })
    }

    /// A library states a stop's exit as a number from the same table this
    /// enum holds. A number outside it is that library's own defect and reads
    /// as could-not-tell here rather than as a status this binary invented.
    pub fn of(status: u8) -> Exit {
        Exit::from_status(status).unwrap_or(Exit::CouldNotTell)
    }
}

/// A refusal said once: the human's line on stderr always, the envelope on
/// stdout when the caller asked for the document, and the exit table's own row
/// either way.
pub fn refuse(verb: &str, exit: Exit, why: &str, json: bool) -> Exit {
    eprintln!("fleet {verb}: {why}");
    if json {
        println!("{}", crate::envelope::refusal(verb, exit, why));
    }
    exit
}

/// A stop's refusal: the person's line on stderr, the document on stdout
/// where one was asked for, and the row on `$?`.
///
/// THE ROW IS READ ONCE AND USED FOR BOTH, so the `code` a caller branches on
/// and the number it reads from `$?` cannot come apart — including on a status
/// outside the exit table, which lands on could-not-tell in both places.
pub fn refuse_stop(verb: &str, stop: &fleet_core::item::Stop, json: bool) -> Exit {
    refuse(verb, Exit::of(stop.code), &stop.message, json)
}

/// A line on stderr under the verb's name, and the row it answers.
pub fn say(verb: &str, exit: Exit, why: impl std::fmt::Display) -> Exit {
    eprintln!("fleet {verb}: {why}");
    exit
}

#[cfg(test)]
mod tests {
    use super::Exit;

    /// One arm per row, both ways, so a renumbering has to be deliberate.
    #[test]
    fn every_row_maps_to_the_tables_number_and_back() {
        for (exit, code) in [
            (Exit::Done, 0),
            (Exit::Refused, 1),
            (Exit::Usage, 2),
            (Exit::CouldNotTell, 3),
            (Exit::NoSession, 4),
            (Exit::NoCollector, 5),
            (Exit::Transient, 6),
        ] {
            assert_eq!(exit.code(), code, "{exit:?} is {code}");
            assert_eq!(
                Exit::from_status(code).expect("the status is a row"),
                exit,
                "status {code} reads back as {exit:?}"
            );
        }
    }

    /// The control the arm above needs: a status the table does not hold is
    /// refused rather than rounded into the nearest row.
    #[test]
    fn a_status_outside_the_table_is_not_a_row() {
        for status in [7u8, 42, 255] {
            let read = Exit::from_status(status);
            assert!(read.is_err(), "status {status} is not a row: {read:?}");
        }
    }

    /// A library's status read into the table: a row reads as itself, and a
    /// number outside the table reads as could-not-tell rather than an error.
    #[test]
    fn a_librarys_status_reads_as_its_row_or_could_not_tell() {
        for status in 0u8..=6 {
            assert_eq!(
                Exit::of(status),
                Exit::from_status(status).expect("the status is a row"),
                "status {status} reads as its own row"
            );
        }
        for status in [7u8, 42, 255] {
            assert_eq!(Exit::of(status), Exit::CouldNotTell, "status {status}");
        }
    }

    /// An `Err` is could-not-tell, so no error path can end in a success
    /// status. `main` maps every `Err` to this one number.
    #[test]
    fn an_error_never_carries_a_zero_status() {
        assert_eq!(Exit::CouldNotTell.code(), 3);
        assert_ne!(Exit::CouldNotTell.code(), Exit::Done.code());
    }
}
