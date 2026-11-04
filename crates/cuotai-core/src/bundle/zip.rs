//! The small ZIP subset a backup uses: deflate or stored entries, no
//! ZIP64, no encryption, UTF-8 names, every timestamp fixed at
//! 1980-01-01 so the same files give the same bytes. The reader accepts
//! only that subset and bounds what it inflates.

use std::io::{Read, Write};

/// Largest file a backup may hold once inflated.
pub(crate) const MAX_ENTRY: u64 = 32 << 20;
/// Largest total a backup may inflate to.
const MAX_TOTAL: u64 = 128 << 20;
/// Most entries a backup may hold.
const MAX_ENTRIES: usize = 4096;

const LOCAL: u32 = 0x0403_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const END: u32 = 0x0605_4b50;
/// DOS date of 1980-01-01 (time 00:00:00 is 0).
const DOS_DATE: u16 = (1 << 5) | 1;
const UTF8_NAMES: u16 = 1 << 11;

/// The archive is not one this reader accepts.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Invalid;

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = flate2::Crc::new();
    crc.update(bytes);
    crc.sum()
}

/// `entries` (name, bytes) as a deflated archive, in the given order.
pub(crate) fn write(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).expect("in memory");
        let deflated = encoder.finish().expect("in memory");
        let crc = crc32(data);
        let offset = out.len() as u32;
        let name = name.as_bytes();
        let header = |sig: u32, buf: &mut Vec<u8>, central: bool| {
            buf.extend_from_slice(&sig.to_le_bytes());
            if central {
                buf.extend_from_slice(&20u16.to_le_bytes()); // made by
            }
            buf.extend_from_slice(&20u16.to_le_bytes()); // needed
            buf.extend_from_slice(&UTF8_NAMES.to_le_bytes());
            buf.extend_from_slice(&8u16.to_le_bytes()); // deflate
            buf.extend_from_slice(&0u16.to_le_bytes()); // time
            buf.extend_from_slice(&DOS_DATE.to_le_bytes());
            buf.extend_from_slice(&crc.to_le_bytes());
            buf.extend_from_slice(&(deflated.len() as u32).to_le_bytes());
            buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
            buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes()); // extra
            if central {
                buf.extend_from_slice(&0u16.to_le_bytes()); // comment
                buf.extend_from_slice(&0u16.to_le_bytes()); // disk
                buf.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
                buf.extend_from_slice(&0u32.to_le_bytes()); // external attributes
                buf.extend_from_slice(&offset.to_le_bytes());
            }
            buf.extend_from_slice(name);
        };
        header(LOCAL, &mut out, false);
        out.extend_from_slice(&deflated);
        header(CENTRAL, &mut central, true);
    }
    let central_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&END.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&central_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16, Invalid> {
    bytes
        .get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or(Invalid)
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, Invalid> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(Invalid)
}

/// The entries of `bytes`, inflated and checked against their CRC, in
/// central-directory order. Names are returned as written; the caller
/// decides which are allowed.
pub(crate) fn read(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, Invalid> {
    if bytes.len() < 22 {
        return Err(Invalid);
    }
    let lowest = bytes.len().saturating_sub(22 + usize::from(u16::MAX));
    let end = (lowest..=bytes.len() - 22)
        .rev()
        .find(|&at| u32_at(bytes, at) == Ok(END))
        .ok_or(Invalid)?;
    let count = usize::from(u16_at(bytes, end + 10)?);
    let central_size = u32_at(bytes, end + 12)? as usize;
    let mut at = u32_at(bytes, end + 16)? as usize;
    if count > MAX_ENTRIES || at.checked_add(central_size).is_none_or(|stop| stop > end) {
        return Err(Invalid);
    }
    let mut total = 0u64;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        if u32_at(bytes, at)? != CENTRAL {
            return Err(Invalid);
        }
        let flags = u16_at(bytes, at + 8)?;
        let method = u16_at(bytes, at + 10)?;
        let crc = u32_at(bytes, at + 16)?;
        let compressed = u64::from(u32_at(bytes, at + 20)?);
        let size = u64::from(u32_at(bytes, at + 24)?);
        let name_len = usize::from(u16_at(bytes, at + 28)?);
        let extra_len = usize::from(u16_at(bytes, at + 30)?);
        let comment_len = usize::from(u16_at(bytes, at + 32)?);
        let local = u32_at(bytes, at + 42)? as usize;
        let name = bytes.get(at + 46..at + 46 + name_len).ok_or(Invalid)?;
        let name = std::str::from_utf8(name).map_err(|_| Invalid)?.to_owned();
        at += 46 + name_len + extra_len + comment_len;
        // Encrypted or data-descriptor entries are not ours.
        if flags & 0b1001 != 0 || size > MAX_ENTRY {
            return Err(Invalid);
        }
        total += size;
        if total > MAX_TOTAL {
            return Err(Invalid);
        }
        if u32_at(bytes, local)? != LOCAL {
            return Err(Invalid);
        }
        let start = local
            + 30
            + usize::from(u16_at(bytes, local + 26)?)
            + usize::from(u16_at(bytes, local + 28)?);
        let stop = start.checked_add(compressed as usize).ok_or(Invalid)?;
        let raw = bytes.get(start..stop).ok_or(Invalid)?;
        let data = match method {
            0 => raw.to_vec(),
            8 => {
                let mut data = Vec::new();
                flate2::read::DeflateDecoder::new(raw)
                    .take(size + 1)
                    .read_to_end(&mut data)
                    .map_err(|_| Invalid)?;
                data
            }
            _ => return Err(Invalid),
        };
        if data.len() as u64 != size || crc32(&data) != crc {
            return Err(Invalid);
        }
        entries.push((name, data));
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_archive_round_trips_and_is_deterministic() {
        let entries = vec![
            ("manifest.json".to_owned(), b"{}".to_vec()),
            (
                "config/cuotai.toml".to_owned(),
                b"accounts = []\n".repeat(50),
            ),
            ("user/locales/é.json".to_owned(), Vec::new()),
        ];
        let archive = write(&entries);
        assert_eq!(write(&entries), archive, "same files, same bytes");
        assert_eq!(read(&archive).unwrap(), entries);
    }

    #[test]
    fn damaged_or_foreign_archives_are_refused() {
        let archive = write(&[("a".to_owned(), b"hello".to_vec())]);
        assert_eq!(read(b"not a zip"), Err(Invalid));
        let mut flipped = archive.clone();
        let data_at = 30 + 1;
        flipped[data_at] ^= 0xff;
        assert!(read(&flipped).is_err());
        assert!(read(&archive[..archive.len() - 1]).is_err());
    }

    #[test]
    fn an_entry_claiming_more_than_the_limit_is_refused() {
        let mut archive = write(&[("a".to_owned(), b"hello".to_vec())]);
        // The central directory's uncompressed size of the only entry.
        let central = archive.len() - 22 - (46 + 1);
        archive[central + 24..central + 28].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(read(&archive), Err(Invalid));
    }
}
