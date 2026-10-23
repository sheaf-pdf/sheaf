//! `sheaf form`: a PDF's form. So far, `check`: what in the form other
//! viewers would trip on, or a save would carry where it shouldn't.

use std::path::PathBuf;

use anyhow::Context;
use clap::{Args, Subcommand};
use sheaf_ops::session::Session;

#[derive(Args)]
#[command(about = "Check a PDF's form")]
pub struct FormArgs {
    #[command(subcommand)]
    what: What,
}

#[derive(Subcommand)]
enum What {
    /// The form's fields and widgets, its XFA copy, usage rights and
    /// signatures, and the problems found in them.
    Check(CheckArgs),
}

#[derive(Args)]
struct CheckArgs {
    /// The PDF to check.
    input: PathBuf,
    #[arg(long)]
    password: Option<String>,
}

pub fn run(args: FormArgs, quiet: bool) -> anyhow::Result<()> {
    match args.what {
        What::Check(a) => {
            let session = crate::progress::run("opening", quiet, |p| {
                Session::open(&a.input, a.password.as_deref(), p)
            })?;
            // The file as the session read it: its last whole save's, when
            // a crash cut one short.
            let bytes = session.doc.primary().bytes();
            let report = sheaf_doc::forms::check(bytes, a.password.as_deref())
                .with_context(|| format!("checking {}", a.input.display()))?;
            print!("{report}");
            Ok(())
        }
    }
}
