//! The exact source snapshot bundled when this executable was built.
pub const ARCHIVE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/frame-record-source.zip"));

pub fn save_to(path: &std::path::Path) -> std::io::Result<()> {
    std::fs::write(path, ARCHIVE)
}
