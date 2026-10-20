//! Byte helpers sheaf-doc and sheaf-render share: sheaf's checksum and
//! reading a file into one buffer.
//!
//! Its charter: helpers over bytes, none of the document's types, and no
//! sheaf dependencies, so either crate uses it without the other. Types go
//! where they're used: the edit vocabulary in sheaf-edit, `PageIndex` in
//! sheaf-render. Never published.

/// sheaf's quick checksum: xxh3-128 of `bytes`. Fast, and stable across
/// builds and platforms, so what's named by it (cache files, comments
/// without `/NM`) keeps its name. Not a security boundary.
pub fn checksum(bytes: &[u8]) -> u128 {
    twox_hash::XxHash3_128::oneshot(bytes)
}

/// [`checksum`] as 32 lowercase hex digits.
pub fn checksum_hex(bytes: &[u8]) -> String {
    format!("{:032x}", checksum(bytes))
}

/// [`checksum`] of bytes given a part at a time: of them all joined, so
/// far. A copy goes on from where it was, so what's kept of a file can be
/// extended by what's appended to it.
#[derive(Clone)]
pub struct Checksum(
    twox_hash::xxhash3_128::RawHasher<&'static [u8; twox_hash::xxhash3_128::DEFAULT_SECRET_LENGTH]>,
);

impl Checksum {
    pub fn new() -> Checksum {
        Checksum(twox_hash::xxhash3_128::RawHasher::new(
            twox_hash::xxhash3_128::SecretBuffer::default(),
        ))
    }

    pub fn write(&mut self, bytes: &[u8]) {
        self.0.write(bytes);
    }

    /// The checksum of everything written.
    pub fn finish(&self) -> u128 {
        self.0.finish_128()
    }
}

impl Default for Checksum {
    fn default() -> Checksum {
        Checksum::new()
    }
}

/// The file at `path`, read into one shared buffer: what the store
/// interns and the renderer reads, with no second copy.
pub fn read_file(path: &std::path::Path) -> std::io::Result<std::sync::Arc<[u8]>> {
    Ok(read_file_and_metadata(path)?.0)
}

/// [`read_file`], with the metadata of the file it read, taken from the
/// open file before reading: what a save checks the file against.
pub fn read_file_and_metadata(
    path: &std::path::Path,
) -> std::io::Result<(std::sync::Arc<[u8]>, std::fs::Metadata)> {
    use std::io::Read as _;
    let mut file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    let len = usize::try_from(metadata.len()).map_err(std::io::Error::other)?;
    // SAFETY: zeroed bytes are bytes.
    let mut bytes = unsafe { std::sync::Arc::<[u8]>::new_zeroed_slice(len).assume_init() };
    let buffer = std::sync::Arc::get_mut(&mut bytes).expect("nothing else holds a new buffer");
    file.read_exact(buffer)?;
    Ok((bytes, metadata))
}

#[cfg(test)]
mod checksum_tests {
    #[test]
    fn checksum_is_xxh3_128() {
        // XXH3_128bits of nothing, from the reference implementation.
        assert_eq!(super::checksum(b""), 0x99aa06d3014798d86001c324468d497f);
        assert_eq!(super::checksum_hex(b"").len(), 32);
        assert_ne!(super::checksum(b"a"), super::checksum(b"b"));
    }

    /// Bytes given in parts, of every size around the hash's buffers and
    /// cut anywhere, sum as they do whole, and a copy goes on alone.
    #[test]
    fn a_checksum_in_parts_is_the_checksum_whole() {
        let bytes: Vec<u8> = (0..5000u32).map(|i| (i * 31 % 251) as u8).collect();
        for len in [0, 1, 16, 17, 128, 129, 240, 241, 256, 1024, 1025, 5000] {
            let bytes = &bytes[..len];
            for cut in [0, 1.min(len), len / 3, len / 2, len.saturating_sub(1), len] {
                let mut sum = super::Checksum::new();
                sum.write(&bytes[..cut]);
                let kept = sum.clone();
                sum.write(&bytes[cut..]);
                assert_eq!(sum.finish(), super::checksum(bytes), "{len} cut at {cut}");
                assert_eq!(kept.finish(), super::checksum(&bytes[..cut]), "{len}");
            }
        }
    }
}
