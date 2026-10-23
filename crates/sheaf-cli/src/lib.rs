//! sheaf's command line. Each subcommand parses its flags into an op's
//! parameters (`sheaf_ops`), runs it on a document opened with no window
//! (`sheaf_ops::session::Session`) with a progress bar on stderr, and
//! answers what writing its output asks only as its flags say
//! ([`decide`]); Ctrl-C cancels the op cleanly.
//!
//! Two binaries run it: `sheaf-cli`, which has no GUI at all, and the app
//! itself, which hands off here when its first argument is a subcommand
//! (`sheaf export …`).

use clap::{Parser, Subcommand};
use serde_json::Value;
use sheaf_ops::session::NeedsDecision;
use std::ffi::{OsStr, OsString};

mod blanks;
mod combine;
mod comments;
mod decide;
mod edit;
mod export;
mod form;
mod images;
mod inspect;
mod names;
mod progress;
mod scan;
mod split;

/// The subcommands, for the app to recognize its hand-off.
pub const SUBCOMMANDS: &[&str] = &[
    "combine",
    "comments",
    "edit",
    "export",
    "form",
    "images-to-pdf",
    "inspect",
    "remove-blanks",
    "scan",
    "split",
];

/// Whether `arg` (the first argument after the program name) names a
/// subcommand. A file of that name in the current directory still opens
/// in the app as `./export`.
pub fn is_subcommand(arg: &OsStr) -> bool {
    arg.to_str().is_some_and(|a| SUBCOMMANDS.contains(&a))
}

#[derive(Parser)]
#[command(version, about = "sheaf: PDF tools", propagate_version = true)]
struct Cli {
    /// No progress bar.
    #[arg(short, long, global = true)]
    quiet: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Combine(combine::CombineArgs),
    Comments(comments::CommentsArgs),
    Edit(edit::EditArgs),
    Export(export::ExportArgs),
    Form(form::FormArgs),
    ImagesToPdf(images::ImagesArgs),
    Inspect(inspect::InspectArgs),
    RemoveBlanks(blanks::BlanksArgs),
    Scan(scan::ScanArgs),
    Split(split::SplitArgs),
}

/// Exit status when writing the output asks what the flags don't answer
/// (M10.5 proposal 40): the ask is on standard output as JSON.
const NEEDS_DECISION: u8 = 3;

/// Exit status when an op was cancelled (as for SIGINT).
const CANCELLED: u8 = 130;

/// Parse `args` (program name first) and run the subcommand. Returns the
/// exit status: 0, 1 on failure, 2 for bad arguments, 3 when a decision is
/// needed (the ask printed as JSON on standard output), 130 if cancelled.
pub fn run<I, T>(args: I) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            return if e.use_stderr() { 2 } else { 0 };
        }
    };
    let result = match cli.command {
        Command::Combine(args) => combine::run(args, cli.quiet),
        Command::Comments(args) => comments::run(args, cli.quiet),
        Command::Edit(args) => edit::run(args, cli.quiet),
        Command::Export(args) => export::run(args, cli.quiet),
        Command::Form(args) => form::run(args, cli.quiet),
        Command::ImagesToPdf(args) => images::run(args, cli.quiet),
        Command::Inspect(args) => inspect::run(args, cli.quiet),
        Command::RemoveBlanks(args) => blanks::run(args, cli.quiet),
        Command::Scan(args) => scan::run(args, cli.quiet),
        Command::Split(args) => split::run(args, cli.quiet),
    };
    match result {
        Ok(()) => 0,
        Err(e) if sheaf_ops::was_cancelled(&e) => {
            eprintln!("cancelled");
            CANCELLED
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            match e.chain().find_map(|c| c.downcast_ref::<NeedsDecision>()) {
                Some(needed) => {
                    print_json(needed.json(), false);
                    NEEDS_DECISION
                }
                None => 1,
            }
        }
    }
}

/// `value` on standard output, pretty or on one line, its objects' keys
/// sorted: the same whatever the binary was built with, as serde_json
/// keeps keys in the order they were added only when a crate in the build
/// turns on its `preserve_order` (GPUI does, in the app).
fn print_json(value: Value, pretty: bool) {
    let value = sorted(value);
    let text = match pretty {
        true => serde_json::to_string_pretty(&value),
        false => serde_json::to_string(&value),
    };
    println!("{}", text.expect("a JSON value writes"));
}

/// `value` with each object's keys in order.
fn sorted(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(entries.into_iter().map(|(k, v)| (k, sorted(v))).collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    /// The flags hang together, by clap's own check, and the app hands off
    /// every subcommand there is.
    #[test]
    fn every_subcommand_is_handed_off() {
        let cli = super::Cli::command();
        cli.clone().debug_assert();
        let mut names: Vec<&str> = (cli.get_subcommands().map(|c| c.get_name()))
            .filter(|&n| n != "help")
            .collect();
        names.sort_unstable();
        assert_eq!(names, super::SUBCOMMANDS);
    }
}
