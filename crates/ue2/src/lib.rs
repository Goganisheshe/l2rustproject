//! Reader for Unreal Engine 2 packages as shipped with the Lineage II client.

pub mod crypt;
pub mod library;
pub mod mesh;
pub mod package;
pub mod props;
pub mod reader;
pub mod texture;

pub use package::{Export, Import, ObjectRef, Package};
pub use reader::Reader;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("unexpected end of data at {pos} (wanted {want} bytes)")]
    Eof { pos: usize, want: usize },
    #[error("bad package magic {0:#x}")]
    BadMagic(u32),
    #[error("Lineage2Ver{0} encryption is not supported")]
    UnsupportedEncryption(u16),
    #[error("{0}")]
    Format(String),
}
