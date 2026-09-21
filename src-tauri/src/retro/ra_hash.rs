//! RetroAchievements-compatible ROM hashing for cartridge-based platforms —
//! ported byte-for-byte from RetroAchievements' own open-source `rcheevos`
//! library (`src/rhash/hash_rom.c` and `src/rhash/hash.c` at
//! github.com/RetroAchievements/rcheevos), so the hash Wervik computes for a
//! ROM matches what RetroArch/gopher64/etc. compute for that same file. See
//! `retroachievements::resolve_game_id`, which uses [`hash`] to identify the
//! exact RA game/hash-set a ROM belongs to instead of fuzzy title matching —
//! title matching can't tell apart RA entries that share a title but not a
//! hash (different regions/revisions, the N64 Shindou re-release, common ROM
//! hacks), which is exactly what a real client's own hash-based identity
//! never confuses.
//!
//! Disc-based platforms, Arcade (`rcheevos` hashes it by filename, not
//! content — not meaningful for Wervik's own ROM files), and Atari 5200
//! (`rcheevos` has no hashing support for it at all) aren't covered here;
//! [`hash`] returns `None` for them so the caller falls back to title
//! matching.

use std::io::Read;
use std::path::Path;

use md5::{Digest, Md5};

use super::platform::Platform;

/// RA's own cap on how much of a file it will hash — irrelevant for real
/// cartridge dumps (the largest, N64, tops out well under this), kept only
/// for parity with `rcheevos`' `MAX_BUFFER_SIZE`.
const MAX_HASH_BYTES: u64 = 64 * 1024 * 1024;

/// Computes the RetroAchievements-compatible hash for `path`, assumed to be
/// a ROM for `platform`. `None` means `platform` isn't one `rcheevos` can
/// hash at all (see the module doc) — the caller should fall back to title
/// matching. `Some(Err(_))` is a real I/O error reading the file, or (for
/// Nintendo 64 only) a file whose first byte doesn't match any known ROM
/// format.
pub fn hash(path: &Path, platform: Platform) -> Option<Result<String, String>> {
    match platform {
        Platform::GameBoy
        | Platform::GameBoyColor
        | Platform::GameBoyAdvance
        | Platform::MasterSystem
        | Platform::Genesis
        | Platform::GameGear
        | Platform::Sega32X
        | Platform::AtariJaguar
        | Platform::WonderSwan
        | Platform::NeoGeoPocket
        | Platform::ColecoVision => Some(hash_whole_file(path)),
        Platform::Nes => Some(hash_with_header(path, nes_header_size)),
        Platform::Snes => Some(hash_with_header(path, snes_header_size)),
        Platform::Atari7800 => Some(hash_with_header(path, atari7800_header_size)),
        Platform::AtariLynx => Some(hash_with_header(path, atari_lynx_header_size)),
        Platform::PcEngine => Some(hash_with_header(path, pc_engine_header_size)),
        Platform::Nintendo64 => Some(hash_n64(path)),
        Platform::PlayStation
        | Platform::Atari5200
        | Platform::Arcade
        | Platform::NeoGeoCd
        | Platform::PlayStation2
        | Platform::WiiU
        | Platform::GameCube
        | Platform::Wii => None,
    }
}

fn read_capped(path: &Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut buffer = Vec::new();
    file.take(MAX_HASH_BYTES)
        .read_to_end(&mut buffer)
        .map_err(|error| error.to_string())?;
    Ok(buffer)
}

fn md5_hex(bytes: &[u8]) -> String {
    let digest = Md5::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hash_whole_file(path: &Path) -> Result<String, String> {
    Ok(md5_hex(&read_capped(path)?))
}

fn hash_with_header(path: &Path, header_size: fn(&[u8]) -> usize) -> Result<String, String> {
    let bytes = read_capped(path)?;
    let skip = header_size(&bytes).min(bytes.len());
    Ok(md5_hex(&bytes[skip..]))
}

fn nes_header_size(bytes: &[u8]) -> usize {
    if bytes.len() > 16 && &bytes[0..4] == b"NES\x1a" {
        16
    } else {
        0
    }
}

fn snes_header_size(bytes: &[u8]) -> usize {
    let size = bytes.len();
    let calc_size = (size / 0x2000) * 0x2000;
    if size - calc_size == 512 {
        512
    } else {
        0
    }
}

fn atari7800_header_size(bytes: &[u8]) -> usize {
    if bytes.len() > 128 && &bytes[1..10] == b"ATARI7800" {
        128
    } else {
        0
    }
}

fn atari_lynx_header_size(bytes: &[u8]) -> usize {
    if bytes.len() > 64 && &bytes[0..5] == b"LYNX\0" {
        64
    } else {
        0
    }
}

fn pc_engine_header_size(bytes: &[u8]) -> usize {
    if bytes.len() & 512 != 0 {
        512
    } else {
        0
    }
}

/// N64 ROM dumps come in four byte orderings, told apart by the first byte —
/// hashed after normalizing to big-endian (z64) order, same as every real RA
/// client.
fn hash_n64(path: &Path) -> Result<String, String> {
    let mut bytes = read_capped(path)?;
    match bytes.first() {
        Some(0x80) => {} // z64 (big endian, native) - no swap
        Some(0x37) => byteswap16(&mut bytes), // v64 (byteswapped)
        Some(0x40) => byteswap32(&mut bytes), // n64 (little endian)
        Some(0xE8 | 0x22) => {} // ndd - no swap
        _ => return Err("Not a recognized Nintendo 64 ROM".to_string()),
    }
    Ok(md5_hex(&bytes))
}

/// Swaps each pair of adjacent bytes: `[b0, b1, b2, b3] -> [b1, b0, b3, b2]`.
fn byteswap16(bytes: &mut [u8]) {
    for pair in bytes.chunks_exact_mut(2) {
        pair.swap(0, 1);
    }
}

/// Reverses each 4-byte word: `[b0, b1, b2, b3] -> [b3, b2, b1, b0]`.
fn byteswap32(bytes: &mut [u8]) {
    for word in bytes.chunks_exact_mut(4) {
        word.reverse();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("wervik-ra-hash-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn md5_hex_matches_known_test_vectors() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn strips_a_real_nes_header() {
        let mut with_header = b"NES\x1a".to_vec();
        with_header.extend(std::iter::repeat_n(0u8, 12)); // rest of the 16-byte header
        with_header.extend_from_slice(b"game data");
        assert_eq!(nes_header_size(&with_header), 16);

        let without_header = b"not a header, plain PRG data".to_vec();
        assert_eq!(nes_header_size(&without_header), 0);
    }

    #[test]
    fn strips_a_512_byte_snes_header() {
        // 512 bytes of header + a size that's an exact multiple of 0x2000.
        let bytes = vec![0u8; 512 + 0x2000];
        assert_eq!(snes_header_size(&bytes), 512);

        // Already an exact multiple of 0x2000 - no header.
        let bytes = vec![0u8; 0x2000];
        assert_eq!(snes_header_size(&bytes), 0);
    }

    #[test]
    fn strips_a_real_atari7800_header() {
        let mut bytes = vec![0u8; 128];
        bytes[1..10].copy_from_slice(b"ATARI7800");
        bytes.extend_from_slice(b"game data");
        assert_eq!(atari7800_header_size(&bytes), 128);
        assert_eq!(atari7800_header_size(&vec![0u8; 200]), 0);
    }

    #[test]
    fn strips_a_real_lynx_header() {
        let mut bytes = vec![0u8; 64];
        bytes[0..5].copy_from_slice(b"LYNX\0");
        bytes.extend_from_slice(b"game data");
        assert_eq!(atari_lynx_header_size(&bytes), 64);
        assert_eq!(atari_lynx_header_size(&vec![0u8; 100]), 0);
    }

    #[test]
    fn byteswap16_swaps_adjacent_byte_pairs() {
        let mut bytes = vec![0u8, 1, 2, 3];
        byteswap16(&mut bytes);
        assert_eq!(bytes, vec![1, 0, 3, 2]);
    }

    #[test]
    fn byteswap32_reverses_each_word() {
        let mut bytes = vec![0u8, 1, 2, 3];
        byteswap32(&mut bytes);
        assert_eq!(bytes, vec![3, 2, 1, 0]);
    }

    #[test]
    fn n64_formats_normalize_to_the_same_hash() {
        let dir = temp_dir("n64");

        // A z64 (big-endian) "ROM": header 0x80 marker plus arbitrary data.
        let z64: Vec<u8> = vec![0x80, 0x37, 0x12, 0x40, 0xAA, 0xBB, 0xCC, 0xDD];
        let z64_path = dir.join("game.z64");
        std::fs::write(&z64_path, &z64).unwrap();

        // The same ROM in v64 (byteswapped) order.
        let mut v64 = z64.clone();
        byteswap16(&mut v64);
        v64[0] = 0x37;
        let v64_path = dir.join("game.v64");
        std::fs::write(&v64_path, &v64).unwrap();

        // The same ROM in n64 (little-endian) order.
        let mut n64 = z64.clone();
        byteswap32(&mut n64);
        n64[0] = 0x40;
        let n64_path = dir.join("game.n64");
        std::fs::write(&n64_path, &n64).unwrap();

        let z64_hash = hash(&z64_path, Platform::Nintendo64).unwrap().unwrap();
        let v64_hash = hash(&v64_path, Platform::Nintendo64).unwrap().unwrap();
        let n64_hash = hash(&n64_path, Platform::Nintendo64).unwrap().unwrap();
        assert_eq!(z64_hash, v64_hash);
        assert_eq!(z64_hash, n64_hash);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn n64_rejects_an_unrecognized_first_byte() {
        let dir = temp_dir("n64-bad");
        let path = dir.join("not-a-rom.bin");
        std::fs::write(&path, [0x00, 0x01, 0x02, 0x03]).unwrap();
        assert!(hash(&path, Platform::Nintendo64).unwrap().is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn returns_none_for_disc_arcade_and_atari5200() {
        let path = Path::new("/does/not/matter");
        assert!(hash(path, Platform::PlayStation).is_none());
        assert!(hash(path, Platform::Arcade).is_none());
        assert!(hash(path, Platform::Atari5200).is_none());
        assert!(hash(path, Platform::PlayStation2).is_none());
    }

    #[test]
    fn hashes_a_whole_file_platform_end_to_end() {
        let dir = temp_dir("whole-file");
        let path = dir.join("game.gba");
        let data = b"some gba rom bytes";
        std::fs::write(&path, data).unwrap();
        assert_eq!(
            hash(&path, Platform::GameBoyAdvance).unwrap().unwrap(),
            md5_hex(data)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
