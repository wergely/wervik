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
//!
//! Every strategy below streams the file through MD5 in fixed-size chunks
//! (mirroring `rc_hash`'s own 65536-byte buffer) rather than reading the
//! whole ROM into memory first — a N64 ROM alone can be up to 64MB, and a
//! sweep hashes a whole library of them back to back.
//!
//! A `.zip`-wrapped ROM (the common distribution format for, e.g., a
//! No-Intro N64 set) is extracted to a throwaway temp directory first, the
//! same way `discovery::extract_rom_from_zip` already does for a native
//! emulator launch — hashing the zip's own bytes would just hash the
//! archive, not the ROM inside it, and RA has no matching hash for that.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use md5::{Digest, Md5};

use super::discovery::{extract_rom_from_zip, zip_rom_entry_fingerprint};
use super::platform::Platform;

/// RA's own cap on how much of a file it will hash — irrelevant for real
/// cartridge dumps (the largest, N64, tops out well under this), kept only
/// for parity with `rcheevos`' `MAX_BUFFER_SIZE`.
const MAX_HASH_BYTES: u64 = 64 * 1024 * 1024;

/// Bumped whenever a hashing rule in this module changes (a fixed header
/// size, a byteswap rule, which platforms are covered at all...) so
/// `RomHashCache` can tell a cached hash was computed under old rules and
/// needs recomputing, without having to reason about which specific cached
/// entries a given code change could have affected.
pub const RA_HASH_SCHEMA_VERSION: u32 = 1;

fn is_zip(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
}

/// Whether `platform` is one [`hash`] can compute a hash for at all — lets a
/// caller skip fingerprinting a ROM entirely for a platform [`hash`] would
/// just return `None` for anyway (see its own doc comment).
pub fn is_hashable(platform: Platform) -> bool {
    strategy_for(platform).is_some()
}

/// A cheap-to-compute identity for a ROM file, used to key `RomHashCache`
/// without ever having to re-extract/re-hash to find out whether a cached
/// hash is still valid.
///
/// For a loose ROM file, this is just its own (size, modified time). For a
/// `.zip`, it's a two-tier fingerprint: the zip's own (size, modified time)
/// — cheap enough that the common case (file untouched since last time)
/// costs only a `stat()` — plus the *chosen entry's* name/CRC32/uncompressed
/// size, read straight from the zip's central directory without
/// decompressing anything (see `discovery::zip_rom_entry_fingerprint`), so a
/// zip that gets rewritten (different outer size/modified time — a
/// different packing tool, a re-download, ...) but still contains
/// byte-identical ROM data still hits the cache instead of re-extracting
/// for nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RomFingerprint {
    pub outer_size: u64,
    pub outer_modified_at: i64,
    pub entry_name: Option<String>,
    pub entry_crc32: Option<u32>,
    pub entry_size: Option<u64>,
}

pub fn fingerprint(path: &Path) -> Result<RomFingerprint, String> {
    let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
    let outer_size = metadata.len();
    let outer_modified_at = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);

    if !is_zip(path) {
        return Ok(RomFingerprint {
            outer_size,
            outer_modified_at,
            entry_name: None,
            entry_crc32: None,
            entry_size: None,
        });
    }

    let (entry_name, entry_crc32, entry_size) = zip_rom_entry_fingerprint(path)?;
    Ok(RomFingerprint {
        outer_size,
        outer_modified_at,
        entry_name: Some(entry_name),
        entry_crc32: Some(entry_crc32),
        entry_size: Some(entry_size),
    })
}

/// How much of the start of a file `peek_header` reads to sniff a
/// content-based header signature (the largest need is Atari7800's 10-byte
/// "ATARI7800" tag starting at offset 1).
const HEADER_PEEK_LEN: usize = 16;

enum Strategy {
    WholeFile,
    Header(fn(&[u8], u64) -> u64),
    Nintendo64,
}

fn strategy_for(platform: Platform) -> Option<Strategy> {
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
        | Platform::ColecoVision => Some(Strategy::WholeFile),
        Platform::Nes => Some(Strategy::Header(nes_header_size)),
        Platform::Snes => Some(Strategy::Header(snes_header_size)),
        Platform::Atari7800 => Some(Strategy::Header(atari7800_header_size)),
        Platform::AtariLynx => Some(Strategy::Header(atari_lynx_header_size)),
        Platform::PcEngine => Some(Strategy::Header(pc_engine_header_size)),
        Platform::Nintendo64 => Some(Strategy::Nintendo64),
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

/// Computes the RetroAchievements-compatible hash for `path`, assumed to be
/// a ROM for `platform`. `None` means `platform` isn't one `rcheevos` can
/// hash at all (see the module doc) — the caller should fall back to title
/// matching. `Some(Err(_))` is a real I/O error reading (or extracting) the
/// file, or (for Nintendo 64 only) a file whose first byte doesn't match
/// any known ROM format.
pub fn hash(path: &Path, platform: Platform) -> Option<Result<String, String>> {
    let strategy = strategy_for(platform)?;
    Some(hash_with_strategy(path, strategy))
}

fn hash_with_strategy(path: &Path, strategy: Strategy) -> Result<String, String> {
    if !is_zip(path) {
        return run_strategy(path, strategy);
    }

    let extract_dir = unique_temp_dir();
    let extracted = extract_rom_from_zip(path, &extract_dir)
        .map_err(|error| format!("Could not extract ROM from zip for hashing: {error}"))?;
    let result = run_strategy(&extracted, strategy);
    let _ = std::fs::remove_dir_all(&extract_dir);
    result
}

static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A throwaway directory for one zip extraction — needs to be unique per
/// call since the interactive per-game fetch and the background sweep can
/// both be hashing ROMs at the same time.
fn unique_temp_dir() -> PathBuf {
    let n = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("wervik-ra-hash-{}-{n}", std::process::id()))
}

fn run_strategy(path: &Path, strategy: Strategy) -> Result<String, String> {
    match strategy {
        Strategy::WholeFile => stream_hash(path, 0),
        Strategy::Header(header_size) => {
            let peek = peek_header(path)?;
            let file_size = file_size(path)?;
            stream_hash(path, header_size(&peek, file_size))
        }
        Strategy::Nintendo64 => hash_n64(path),
    }
}

fn file_size(path: &Path) -> Result<u64, String> {
    std::fs::metadata(path)
        .map(|metadata| metadata.len())
        .map_err(|error| error.to_string())
}

fn peek_header(path: &Path) -> Result<Vec<u8>, String> {
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut buffer = vec![0u8; HEADER_PEEK_LEN];
    let read = read_fill(&mut file, &mut buffer).map_err(|error| error.to_string())?;
    buffer.truncate(read);
    Ok(buffer)
}

/// Reads into `buffer` until it's full or the file is exhausted — unlike a
/// single `Read::read` call, which is allowed to return short even mid-file,
/// this guarantees every non-final chunk `stream_hash`/`hash_n64` reads is
/// exactly `buffer.len()` bytes, which the N64 byteswap needs to stay
/// aligned to 2/4-byte boundaries across chunk boundaries.
fn read_fill(file: &mut std::fs::File, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match file.read(&mut buffer[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

fn hex(digest: impl AsRef<[u8]>) -> String {
    digest.as_ref().iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Hashes `path` from byte `skip` onward, up to RA's `MAX_HASH_BYTES` cap,
/// streaming through a fixed-size buffer rather than reading the whole
/// (capped) range into memory at once.
fn stream_hash(path: &Path, skip: u64) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    file.seek(SeekFrom::Start(skip)).map_err(|error| error.to_string())?;

    let mut hasher = Md5::new();
    let mut buffer = [0u8; 65536];
    let mut remaining = MAX_HASH_BYTES.saturating_sub(skip);
    loop {
        let to_read = remaining.min(buffer.len() as u64) as usize;
        if to_read == 0 {
            break;
        }
        let read = read_fill(&mut file, &mut buffer[..to_read]).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        remaining -= read as u64;
    }
    Ok(hex(hasher.finalize()))
}

fn nes_header_size(peek: &[u8], file_size: u64) -> u64 {
    if file_size > 16 && peek.len() >= 4 && &peek[0..4] == b"NES\x1a" {
        16
    } else {
        0
    }
}

fn snes_header_size(_peek: &[u8], file_size: u64) -> u64 {
    let calc_size = (file_size / 0x2000) * 0x2000;
    if file_size - calc_size == 512 {
        512
    } else {
        0
    }
}

fn atari7800_header_size(peek: &[u8], file_size: u64) -> u64 {
    if file_size > 128 && peek.len() >= 10 && &peek[1..10] == b"ATARI7800" {
        128
    } else {
        0
    }
}

fn atari_lynx_header_size(peek: &[u8], file_size: u64) -> u64 {
    if file_size > 64 && peek.len() >= 5 && &peek[0..5] == b"LYNX\0" {
        64
    } else {
        0
    }
}

fn pc_engine_header_size(_peek: &[u8], file_size: u64) -> u64 {
    if file_size & 512 != 0 {
        512
    } else {
        0
    }
}

/// N64 ROM dumps come in four byte orderings, told apart by the first byte —
/// hashed after normalizing to big-endian (z64) order, same as every real RA
/// client. Streamed the same way `stream_hash` is, except each chunk is
/// byteswapped in place before being fed to MD5 — exactly how `rc_hash_n64`
/// itself does it (see the module doc), so a chunk boundary can only ever
/// fall on a multiple of the buffer size (64KB, itself a multiple of 4)
/// except for the final, possibly-short chunk `read_fill` guarantees is
/// still a real EOF and not a short intermediate read.
fn hash_n64(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut first_byte = [0u8; 1];
    if read_fill(&mut file, &mut first_byte).map_err(|error| error.to_string())? == 0 {
        return Err("Empty file".to_string());
    }
    let swap: fn(&mut [u8]) = match first_byte[0] {
        0x80 => no_swap,           // z64 (big endian, native)
        0x37 => byteswap16,        // v64 (byteswapped)
        0x40 => byteswap32,        // n64 (little endian)
        0xE8 | 0x22 => no_swap,    // ndd
        _ => return Err("Not a recognized Nintendo 64 ROM".to_string()),
    };

    let cap = file_size(path)?.min(MAX_HASH_BYTES);
    file.seek(SeekFrom::Start(0)).map_err(|error| error.to_string())?;

    let mut hasher = Md5::new();
    let mut buffer = [0u8; 65536];
    let mut remaining = cap;
    loop {
        let to_read = remaining.min(buffer.len() as u64) as usize;
        if to_read == 0 {
            break;
        }
        let read = read_fill(&mut file, &mut buffer[..to_read]).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        swap(&mut buffer[..read]);
        hasher.update(&buffer[..read]);
        remaining -= read as u64;
    }
    Ok(hex(hasher.finalize()))
}

fn no_swap(_bytes: &mut [u8]) {}

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
        let dir = std::env::temp_dir().join(format!("wervik-ra-hash-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn md5_hex(bytes: &[u8]) -> String {
        hex(Md5::digest(bytes))
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
        assert_eq!(nes_header_size(&with_header[..4], with_header.len() as u64), 16);

        let without_header = b"not a header, plain PRG data".to_vec();
        assert_eq!(nes_header_size(&without_header[..4], without_header.len() as u64), 0);
    }

    #[test]
    fn strips_a_512_byte_snes_header() {
        // 512 bytes of header + a size that's an exact multiple of 0x2000.
        assert_eq!(snes_header_size(&[], 512 + 0x2000), 512);
        // Already an exact multiple of 0x2000 - no header.
        assert_eq!(snes_header_size(&[], 0x2000), 0);
    }

    #[test]
    fn strips_a_real_atari7800_header() {
        let mut bytes = vec![0u8; 128];
        bytes[1..10].copy_from_slice(b"ATARI7800");
        bytes.extend_from_slice(b"game data");
        assert_eq!(atari7800_header_size(&bytes[..16], bytes.len() as u64), 128);
        assert_eq!(atari7800_header_size(&[0u8; 16], 200), 0);
    }

    #[test]
    fn strips_a_real_lynx_header() {
        let mut bytes = vec![0u8; 64];
        bytes[0..5].copy_from_slice(b"LYNX\0");
        bytes.extend_from_slice(b"game data");
        assert_eq!(atari_lynx_header_size(&bytes[..16], bytes.len() as u64), 64);
        assert_eq!(atari_lynx_header_size(&[0u8; 16], 100), 0);
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

        // A z64 (big-endian) "ROM": header 0x80 marker plus arbitrary data,
        // padded well past one streaming chunk to exercise chunk boundaries.
        let mut z64 = vec![0x80, 0x37, 0x12, 0x40];
        z64.extend((0..70_000u32).flat_map(|i| i.to_be_bytes()));
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
        // Cross-checked independently against a one-shot (non-streaming)
        // MD5 of the same already-normalized buffer.
        assert_eq!(z64_hash, md5_hex(&z64));

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
        assert_eq!(hash(&path, Platform::GameBoyAdvance).unwrap().unwrap(), md5_hex(data));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn write_zip(path: &Path, entry_name: &str, data: &[u8]) {
        let file = std::fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file(entry_name, zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut writer, data).unwrap();
        writer.finish().unwrap();
    }

    #[test]
    fn hashes_the_rom_inside_a_zip_not_the_zip_itself() {
        let dir = temp_dir("zip");
        let data = b"a small gba rom, zipped up";
        let zip_path = dir.join("Some Game (USA).zip");
        write_zip(&zip_path, "Some Game (USA).gba", data);

        let zip_hash = hash(&zip_path, Platform::GameBoyAdvance).unwrap().unwrap();
        assert_eq!(zip_hash, md5_hex(data));
        assert_ne!(
            zip_hash,
            md5_hex(&std::fs::read(&zip_path).unwrap()),
            "must not hash the zip's own bytes"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hashes_an_n64_rom_inside_a_zip() {
        let dir = temp_dir("zip-n64");
        let mut z64 = vec![0x80, 0x37, 0x12, 0x40];
        z64.extend((0..1000u32).flat_map(|i| i.to_be_bytes()));
        let zip_path = dir.join("Some N64 Game (USA).zip");
        write_zip(&zip_path, "Some N64 Game (USA).z64", &z64);

        assert_eq!(hash(&zip_path, Platform::Nintendo64).unwrap().unwrap(), md5_hex(&z64));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn is_hashable_matches_hash_none_ness() {
        assert!(is_hashable(Platform::Nintendo64));
        assert!(is_hashable(Platform::GameBoyAdvance));
        assert!(!is_hashable(Platform::PlayStation));
        assert!(!is_hashable(Platform::Atari5200));
    }

    #[test]
    fn fingerprints_a_loose_rom_by_size_and_mtime_only() {
        let dir = temp_dir("fingerprint-loose");
        let path = dir.join("game.gba");
        std::fs::write(&path, b"some rom bytes").unwrap();

        let fp = fingerprint(&path).unwrap();
        assert_eq!(fp.outer_size, 14);
        assert!(fp.entry_name.is_none());
        assert!(fp.entry_crc32.is_none());
        assert!(fp.entry_size.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fingerprints_a_zip_by_its_chosen_entrys_identity() {
        let dir = temp_dir("fingerprint-zip");
        let data = b"a small gba rom, zipped up";
        let zip_path = dir.join("Some Game (USA).zip");
        write_zip(&zip_path, "Some Game (USA).gba", data);

        let fp = fingerprint(&zip_path).unwrap();
        assert_eq!(fp.entry_name.as_deref(), Some("Some Game (USA).gba"));
        assert_eq!(fp.entry_size, Some(data.len() as u64));
        assert!(fp.entry_crc32.is_some());

        // Re-zipping the exact same entry (different outer file, so a
        // different mtime/possibly different size due to compression
        // timestamps) still fingerprints to the same entry identity - the
        // whole point of tracking the entry separately from the outer file.
        let rezipped_path = dir.join("Same Game, Repacked.zip");
        write_zip(&rezipped_path, "Some Game (USA).gba", data);
        let fp2 = fingerprint(&rezipped_path).unwrap();
        assert_eq!(fp.entry_name, fp2.entry_name);
        assert_eq!(fp.entry_crc32, fp2.entry_crc32);
        assert_eq!(fp.entry_size, fp2.entry_size);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
