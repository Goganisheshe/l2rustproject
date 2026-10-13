//! Lineage II file-level encryption ("Lineage2VerXXX" header).
//!
//! Packages (.u/.utx/.usx/.unr) are XORed with a single-byte key. Ver111 always
//! uses 0xAC, Ver121 derives the key from the file name. Instead of re-deriving
//! the formula we recover the key from the known UE package magic (0x9E2A83C1).

use crate::Error;

pub const HEADER_LEN: usize = 28;
pub const PACKAGE_MAGIC: u32 = 0x9E2A_83C1;

/// Encryption scheme detected from the 28-byte UTF-16 header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    None,
    Xor { version: u16, key: u8 },
    /// Ver41x: RSA + zlib, used for .dat/.ini. Not handled here.
    Rsa { version: u16 },
}

pub fn detect(data: &[u8]) -> Scheme {
    if data.len() >= 4 && u32::from_le_bytes(data[0..4].try_into().unwrap()) == PACKAGE_MAGIC {
        return Scheme::None;
    }
    if data.len() < HEADER_LEN + 4 {
        return Scheme::None;
    }
    let header: String = data[..HEADER_LEN]
        .chunks_exact(2)
        .map(|c| char::from(c[0]))
        .collect();
    let Some(ver) = header.strip_prefix("Lineage2Ver").and_then(|v| v.parse::<u16>().ok()) else {
        return Scheme::None;
    };
    match ver {
        111 | 121 => Scheme::Xor { version: ver, key: data[HEADER_LEN] ^ (PACKAGE_MAGIC as u8) },
        _ => Scheme::Rsa { version: ver },
    }
}

/// Returns a plain UE package buffer, decrypting in place if needed.
pub fn decrypt(mut data: Vec<u8>) -> Result<Vec<u8>, Error> {
    match detect(&data) {
        Scheme::None => Ok(data),
        Scheme::Xor { key, .. } => {
            data.drain(..HEADER_LEN);
            for b in &mut data {
                *b ^= key;
            }
            Ok(data)
        }
        Scheme::Rsa { version } => Err(Error::UnsupportedEncryption(version)),
    }
}
