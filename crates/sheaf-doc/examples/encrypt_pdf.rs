//! Dev tool: write a password-protected copy of a PDF (AES-256, revision 6,
//! which every modern reader including PDFium opens) for testing the password
//! prompt and decrypt-on-save.
//!
//! Usage: cargo run -p sheaf-doc --example encrypt_pdf <in.pdf> <out.pdf> <user-password> [owner-password]

use sheaf_doc::pdf::{Document, Object, Permissions};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output, user, rest @ ..] = args.as_slice() else {
        anyhow::bail!("usage: encrypt_pdf <in.pdf> <out.pdf> <user-password> [owner-password]");
    };
    let owner = rest
        .first()
        .cloned()
        .unwrap_or_else(|| format!("{user}-owner"));
    let mut pdf = Document::load(input)?;
    if !pdf.trailer.has(b"ID") {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let id = Object::hex(format!("{nanos:032x}").into_bytes());
        pdf.trailer.set("ID", vec![id.clone(), id]);
    }
    pdf.encrypt_aes256(user, &owner, Permissions::ALL)?;
    pdf.save(output)?;
    println!("wrote {output} (user password {user:?})");
    Ok(())
}
