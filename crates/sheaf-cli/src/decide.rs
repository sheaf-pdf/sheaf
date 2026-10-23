//! What the command line answers a save's or an export's asks (M10.5
//! proposal 40): only what its flags say. A choice they leave to ask stops
//! the subcommand before it writes anything, and it exits 3 with the
//! choice as JSON on standard output, so a script never has one made for
//! it silently. `--force` never answers one: it only replaces an output
//! that's there.

use clap::{Args, ValueEnum};
use sheaf_doc::materialize::AskAnswers;

/// The flags that answer what writing the output asks.
#[derive(Args, Clone, Copy)]
pub struct Decisions {
    /// When the output would break or flag the file's signatures: stop and
    /// say so (exit 3), break them, or append anyway where that's offered
    /// (the signed revision stays as it was, and what the changes remove
    /// stays readable in it).
    #[arg(long, value_enum, default_value_t = Signatures::Ask)]
    signatures: Signatures,
    /// When the output would drop a form's XFA part that holds values: stop
    /// and say so (exit 3), or drop it.
    #[arg(long, value_enum, default_value_t = Xfa::Ask)]
    xfa: Xfa,
}

#[derive(Clone, Copy, ValueEnum)]
enum Signatures {
    Ask,
    Break,
    AppendAnyway,
}

#[derive(Clone, Copy, ValueEnum)]
enum Xfa {
    Ask,
    Drop,
}

impl Decisions {
    /// The answers the flags give. Appending anyway agrees to whatever the
    /// offer says stays readable in the signed revision, what the removal
    /// check finds included.
    pub fn answers(&self) -> AskAnswers {
        let anyway = matches!(self.signatures, Signatures::AppendAnyway);
        AskAnswers {
            break_signatures: matches!(self.signatures, Signatures::Break),
            append_anyway: anyway,
            stays_readable: anyway,
            found: anyway,
            drop_xfa_values: matches!(self.xfa, Xfa::Drop),
            ..AskAnswers::default()
        }
    }
}
