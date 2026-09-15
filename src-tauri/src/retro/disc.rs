//! Confirms a `.cue`/`.chd` candidate is actually the specific CD-based
//! system it claims to be before it's accepted as one — unlike every
//! other extension `Platform::from_unambiguous_extension` maps, `.cue`
//! and `.chd` are shared by several CD-based systems (PlayStation, Sega
//! CD, Neo Geo CD, PC-FX, 3DO...), so the extension alone was found to
//! misclassify real Sega CD/Neo Geo CD dumps as PlayStation — every one
//! of them, in a real library that happened to have no actual PS1 games
//! in it at all.

use std::io::Read;
use std::path::Path;

/// Enough of a disc's data track to reach and read its Primary Volume
/// Descriptor at logical sector 16 (where PlayStation's signature lives)
/// and its root directory a few sectors after (where Neo Geo CD's lives),
/// whether the track is a raw 2352-byte Mode 1/2352 image or an
/// already-"cooked" 2048-byte-sector one.
const DISC_HEADER_PROBE_SIZE: usize = 64 * 1024;

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.len() <= haystack.len() && haystack.windows(needle.len()).any(|w| w == needle)
}

/// Real signature every licensed PS1 disc carries: its data track is an
/// ISO9660 filesystem whose Primary Volume Descriptor identifies it as
/// "PLAYSTATION" — checking for the generic ISO9660 marker ("CD001") alone
/// isn't enough, since a Sega CD/Neo Geo CD disc is an ISO9660 filesystem
/// too; verified directly against a real Neo Geo CD dump, which has the
/// former but not the latter (its volume label is the game's own title
/// instead — Sony enforces "PLAYSTATION" console-wide, SNK doesn't
/// enforce anything equivalent).
fn looks_like_playstation_disc(data: &[u8]) -> bool {
    contains(data, b"CD001") && contains(data, b"PLAYSTATION")
}

/// Real Neo Geo CD signature: every disc's root directory contains an
/// `IPL.TXT` boot script (the console's BIOS reads it to find which
/// `.PRG` files to load) — verified directly against two real Neo Geo CD
/// dumps of different games, both containing it at the same rough offset.
/// Unlike PlayStation, Neo Geo CD's own ISO9660 volume label isn't
/// console-wide fixed (seen holding the game's own title, or a mastering
/// tool's own default string), so that field isn't a usable signature here.
fn looks_like_neo_geo_cd_disc(data: &[u8]) -> bool {
    contains(data, b"CD001") && contains(data, b"IPL.TXT")
}

/// Real signature every PS2 disc carries: its `SYSTEM.CNF` boot file has a
/// `BOOT2 = cdrom0:\...` line — the field the PS2 BIOS itself reads to find
/// the boot ELF. PS1's own `SYSTEM.CNF` says `BOOT = cdrom:\...` instead (no
/// "2"), so this only matches a real PS2 disc, not a PS1 one that happens to
/// also be dumped as a single-track `.iso`. Checked before
/// `looks_like_playstation_disc` wherever a candidate could be either, so a
/// disc carrying both signatures (if that ever happens) still resolves to
/// the more specific PS2 match.
fn looks_like_playstation2_disc(data: &[u8]) -> bool {
    contains(data, b"CD001") && contains(data, b"BOOT2")
}

/// GameCube's own disc magic word — a fixed 4-byte value every real GameCube
/// disc carries at a fixed header offset (0x1C), zero on every other disc.
/// This is the same field Dolphin itself reads to identify a GameCube disc
/// (as opposed to `looks_like_playstation_disc`'s pair of markers, this is a
/// single exact value at a known position, so no separate "is this at least
/// some known-good ISO9660" pre-check is needed).
const GAMECUBE_DISC_MAGIC_OFFSET: usize = 0x1c;
const GAMECUBE_DISC_MAGIC: [u8; 4] = [0xc2, 0x33, 0x9f, 0x3d];

/// Wii's counterpart to `GAMECUBE_DISC_MAGIC` — same idea, a different fixed
/// value at a different offset (0x18) in the same header. A disc only ever
/// carries one of the two (the other field reads zero), so checking either
/// one alone is enough to tell GameCube and Wii apart.
const WII_DISC_MAGIC_OFFSET: usize = 0x18;
const WII_DISC_MAGIC: [u8; 4] = [0x5d, 0x1c, 0x9e, 0xa3];

fn magic_at(data: &[u8], offset: usize, magic: &[u8; 4]) -> bool {
    data.get(offset..offset + magic.len()) == Some(magic.as_slice())
}

fn looks_like_gamecube_disc(data: &[u8]) -> bool {
    magic_at(data, GAMECUBE_DISC_MAGIC_OFFSET, &GAMECUBE_DISC_MAGIC)
}

fn looks_like_wii_disc(data: &[u8]) -> bool {
    magic_at(data, WII_DISC_MAGIC_OFFSET, &WII_DISC_MAGIC)
}

/// The filename a `.cue` sheet's first `FILE "..."` line references — the
/// disc's actual data track, which is what needs checking, not the cue
/// sheet's own (tiny, plain-text) content.
fn first_referenced_file(cue_text: &str) -> Option<&str> {
    let line = cue_text
        .lines()
        .find(|l| l.trim_start().to_ascii_uppercase().starts_with("FILE"))?;
    let start = line.find('"')? + 1;
    let end = start + line[start..].find('"')?;
    Some(&line[start..end])
}

fn read_prefix<R: Read>(reader: R) -> Vec<u8> {
    let mut buf = Vec::with_capacity(DISC_HEADER_PROBE_SIZE);
    let _ = reader
        .take(DISC_HEADER_PROBE_SIZE as u64)
        .read_to_end(&mut buf);
    buf
}

/// Checks a loose `.cue` file's referenced data track against `matches`.
fn cue_matches(cue_path: &Path, matches: impl Fn(&[u8]) -> bool) -> bool {
    let Ok(cue_text) = std::fs::read_to_string(cue_path) else {
        return false;
    };
    let Some(bin_name) = first_referenced_file(&cue_text) else {
        return false;
    };
    let Some(parent) = cue_path.parent() else {
        return false;
    };
    let Ok(file) = std::fs::File::open(parent.join(bin_name)) else {
        return false;
    };
    matches(&read_prefix(file))
}

pub(crate) fn cue_references_a_playstation_disc(cue_path: &Path) -> bool {
    cue_matches(cue_path, looks_like_playstation_disc)
}

pub(crate) fn cue_references_a_neo_geo_cd_disc(cue_path: &Path) -> bool {
    cue_matches(cue_path, looks_like_neo_geo_cd_disc)
}

/// Checks a `.chd`'s own embedded disc data against `matches` — unlike
/// `.cue`, a CHD is self-contained, so there's no separate track file to
/// look up; its first hunk(s) are decompressed instead, only as many as
/// needed to cover the probe size regardless of the CHD's actual hunk size.
fn chd_matches(chd_path: &Path, matches: impl Fn(&[u8]) -> bool) -> bool {
    let Ok(file) = std::fs::File::open(chd_path) else {
        return false;
    };
    let mut reader = std::io::BufReader::new(file);
    let Ok(mut chd) = chd::Chd::open(&mut reader, None) else {
        return false;
    };

    let hunk_count = chd.header().hunk_count();
    let mut hunk_buf = chd.get_hunksized_buffer();
    let mut cmp_buf = Vec::new();
    let mut collected = Vec::with_capacity(DISC_HEADER_PROBE_SIZE);
    for hunk_num in 0..hunk_count {
        if collected.len() >= DISC_HEADER_PROBE_SIZE {
            break;
        }
        let Ok(mut hunk) = chd.hunk(hunk_num) else {
            return false;
        };
        if hunk.read_hunk_in(&mut cmp_buf, &mut hunk_buf).is_err() {
            return false;
        }
        collected.extend_from_slice(&hunk_buf);
    }
    matches(&collected)
}

pub(crate) fn chd_is_a_playstation_disc(chd_path: &Path) -> bool {
    chd_matches(chd_path, looks_like_playstation_disc)
}

pub(crate) fn chd_is_a_neo_geo_cd_disc(chd_path: &Path) -> bool {
    chd_matches(chd_path, looks_like_neo_geo_cd_disc)
}

/// Checks a loose `.iso`'s own content against `matches` — unlike `.cue`,
/// a `.iso` is the complete disc image itself, so (like `.chd`) there's no
/// separate track file to look up.
fn iso_matches(iso_path: &Path, matches: impl Fn(&[u8]) -> bool) -> bool {
    let Ok(file) = std::fs::File::open(iso_path) else {
        return false;
    };
    matches(&read_prefix(file))
}

pub(crate) fn iso_is_a_playstation2_disc(iso_path: &Path) -> bool {
    iso_matches(iso_path, looks_like_playstation2_disc)
}

pub(crate) fn iso_is_a_playstation_disc(iso_path: &Path) -> bool {
    iso_matches(iso_path, looks_like_playstation_disc)
}

pub(crate) fn iso_is_a_gamecube_disc(iso_path: &Path) -> bool {
    iso_matches(iso_path, looks_like_gamecube_disc)
}

pub(crate) fn iso_is_a_wii_disc(iso_path: &Path) -> bool {
    iso_matches(iso_path, looks_like_wii_disc)
}

/// Same idea as `cue_matches`, but for a `.cue` entry *inside a zip* —
/// reads the referenced track straight from the archive instead of the
/// filesystem, and only decompresses as much of it as the probe size
/// needs (cheap regardless of the track's real, often huge, size — no
/// full decompression).
fn zip_cue_matches<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    cue_entry_name: &str,
    matches: impl Fn(&[u8]) -> bool,
) -> bool {
    let Ok(mut cue_entry) = archive.by_name(cue_entry_name) else {
        return false;
    };
    let mut cue_text = String::new();
    if cue_entry.read_to_string(&mut cue_text).is_err() {
        return false;
    }
    drop(cue_entry);

    let Some(bin_name) = first_referenced_file(&cue_text) else {
        return false;
    };
    // A cue sheet references its sibling track by bare filename — find the
    // matching entry among the archive's own (full-path) names rather than
    // assuming it's stored under the exact same path as the `.cue`.
    let Some(matching_name) = archive
        .file_names()
        .find(|n| Path::new(n).file_name().and_then(|f| f.to_str()) == Some(bin_name))
        .map(str::to_string)
    else {
        return false;
    };
    let Ok(bin_entry) = archive.by_name(&matching_name) else {
        return false;
    };
    matches(&read_prefix(bin_entry))
}

pub(crate) fn zip_cue_references_a_playstation_disc<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    cue_entry_name: &str,
) -> bool {
    zip_cue_matches(archive, cue_entry_name, looks_like_playstation_disc)
}

pub(crate) fn zip_cue_references_a_neo_geo_cd_disc<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    cue_entry_name: &str,
) -> bool {
    zip_cue_matches(archive, cue_entry_name, looks_like_neo_geo_cd_disc)
}

/// Same idea as `zip_cue_matches`, but for a `.iso` entry inside a zip —
/// no cue-sheet indirection needed, since the `.iso` entry itself is the
/// whole disc image.
fn zip_iso_matches<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    iso_entry_name: &str,
    matches: impl Fn(&[u8]) -> bool,
) -> bool {
    let Ok(entry) = archive.by_name(iso_entry_name) else {
        return false;
    };
    matches(&read_prefix(entry))
}

pub(crate) fn zip_iso_is_a_playstation2_disc<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    iso_entry_name: &str,
) -> bool {
    zip_iso_matches(archive, iso_entry_name, looks_like_playstation2_disc)
}

pub(crate) fn zip_iso_is_a_playstation_disc<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    iso_entry_name: &str,
) -> bool {
    zip_iso_matches(archive, iso_entry_name, looks_like_playstation_disc)
}

pub(crate) fn zip_iso_is_a_gamecube_disc<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    iso_entry_name: &str,
) -> bool {
    zip_iso_matches(archive, iso_entry_name, looks_like_gamecube_disc)
}

pub(crate) fn zip_iso_is_a_wii_disc<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    iso_entry_name: &str,
) -> bool {
    zip_iso_matches(archive, iso_entry_name, looks_like_wii_disc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_first_referenced_file_from_a_cue_sheet() {
        let cue =
            "FILE \"Game (Track 01).bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n";
        assert_eq!(first_referenced_file(cue), Some("Game (Track 01).bin"));
    }

    #[test]
    fn no_referenced_file_when_the_cue_sheet_has_no_file_line() {
        assert_eq!(first_referenced_file("not a real cue sheet"), None);
    }

    /// The exact real-world case that motivated this whole module: a real
    /// Neo Geo CD track has a genuine ISO9660 filesystem ("CD001") but not
    /// the PlayStation-specific volume label — so it must be rejected, not
    /// just "looks like some CD-ROM".
    #[test]
    fn an_iso9660_filesystem_alone_is_not_enough_to_look_like_playstation() {
        let mut fake_iso9660_track = vec![0u8; 40_000];
        fake_iso9660_track[37649..37649 + 5].copy_from_slice(b"CD001");
        assert!(!looks_like_playstation_disc(&fake_iso9660_track));
    }

    #[test]
    fn both_markers_present_looks_like_a_playstation_disc() {
        let mut fake_ps1_track = vec![0u8; 40_000];
        fake_ps1_track[37649..37649 + 5].copy_from_slice(b"CD001");
        fake_ps1_track[37700..37700 + 11].copy_from_slice(b"PLAYSTATION");
        assert!(looks_like_playstation_disc(&fake_ps1_track));
    }

    /// The Neo Geo CD counterpart: an ISO9660 filesystem alone (as any
    /// CD-ROM has) isn't enough — `IPL.TXT` specifically is what a real
    /// disc carries.
    #[test]
    fn an_iso9660_filesystem_alone_is_not_enough_to_look_like_neo_geo_cd() {
        let mut fake_iso9660_track = vec![0u8; 50_000];
        fake_iso9660_track[37649..37649 + 5].copy_from_slice(b"CD001");
        assert!(!looks_like_neo_geo_cd_disc(&fake_iso9660_track));
    }

    #[test]
    fn ipl_txt_alongside_iso9660_looks_like_neo_geo_cd() {
        let mut fake_ngcd_track = vec![0u8; 50_000];
        fake_ngcd_track[37649..37649 + 5].copy_from_slice(b"CD001");
        fake_ngcd_track[48483..48483 + 7].copy_from_slice(b"IPL.TXT");
        assert!(looks_like_neo_geo_cd_disc(&fake_ngcd_track));
        // And, same as the real thing, it must not also look like PlayStation.
        assert!(!looks_like_playstation_disc(&fake_ngcd_track));
    }

    /// The real distinction between a PS1 and PS2 disc that motivated this
    /// check: both carry a `SYSTEM.CNF`, but only PS2's says `BOOT2` (PS1's
    /// says plain `BOOT`, no digit).
    #[test]
    fn boot2_is_required_to_look_like_playstation2() {
        let mut fake_ps1_track = vec![0u8; 40_000];
        fake_ps1_track[37649..37649 + 5].copy_from_slice(b"CD001");
        fake_ps1_track[37700..37700 + 4].copy_from_slice(b"BOOT");
        assert!(!looks_like_playstation2_disc(&fake_ps1_track));

        let mut fake_ps2_track = vec![0u8; 40_000];
        fake_ps2_track[37649..37649 + 5].copy_from_slice(b"CD001");
        fake_ps2_track[37700..37700 + 5].copy_from_slice(b"BOOT2");
        assert!(looks_like_playstation2_disc(&fake_ps2_track));
    }

    #[test]
    fn iso_is_a_playstation2_disc_checks_the_files_own_content() {
        let dir = std::env::temp_dir().join(format!("wervik-disc-iso-ps2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut image = vec![0u8; 40_000];
        image[32768..32768 + 5].copy_from_slice(b"CD001");
        image[33000..33000 + 5].copy_from_slice(b"BOOT2");
        let iso_path = dir.join("Game.iso");
        std::fs::write(&iso_path, &image).unwrap();

        assert!(iso_is_a_playstation2_disc(&iso_path));
        assert!(!iso_is_a_playstation_disc(&iso_path));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// GameCube and Wii dumps carry no ISO9660 markers at all (`.gcm`/raw
    /// discs use their own proprietary filesystem) — just the one exact
    /// magic word each, at each one's own fixed offset, with the other
    /// platform's offset reading zero.
    #[test]
    fn gamecube_and_wii_magic_words_are_at_distinct_offsets() {
        let mut fake_gamecube_disc = vec![0u8; 40_000];
        fake_gamecube_disc[0x1c..0x1c + 4].copy_from_slice(&GAMECUBE_DISC_MAGIC);
        assert!(looks_like_gamecube_disc(&fake_gamecube_disc));
        assert!(!looks_like_wii_disc(&fake_gamecube_disc));

        let mut fake_wii_disc = vec![0u8; 40_000];
        fake_wii_disc[0x18..0x18 + 4].copy_from_slice(&WII_DISC_MAGIC);
        assert!(looks_like_wii_disc(&fake_wii_disc));
        assert!(!looks_like_gamecube_disc(&fake_wii_disc));
    }

    #[test]
    fn iso_is_a_gamecube_disc_checks_the_files_own_content() {
        let dir = std::env::temp_dir().join(format!("wervik-disc-iso-gcn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut image = vec![0u8; 40_000];
        image[0x1c..0x1c + 4].copy_from_slice(&GAMECUBE_DISC_MAGIC);
        let iso_path = dir.join("Game.iso");
        std::fs::write(&iso_path, &image).unwrap();

        assert!(iso_is_a_gamecube_disc(&iso_path));
        assert!(!iso_is_a_wii_disc(&iso_path));
        assert!(!iso_is_a_playstation2_disc(&iso_path));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn iso_is_a_wii_disc_checks_the_files_own_content() {
        let dir = std::env::temp_dir().join(format!("wervik-disc-iso-wii-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut image = vec![0u8; 40_000];
        image[0x18..0x18 + 4].copy_from_slice(&WII_DISC_MAGIC);
        let iso_path = dir.join("Game.iso");
        std::fs::write(&iso_path, &image).unwrap();

        assert!(iso_is_a_wii_disc(&iso_path));
        assert!(!iso_is_a_gamecube_disc(&iso_path));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cue_references_a_playstation_disc_checks_the_referenced_track_not_the_cue_itself() {
        let dir = std::env::temp_dir().join(format!("wervik-disc-cue-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut track = vec![0u8; 40_000];
        track[37649..37649 + 5].copy_from_slice(b"CD001");
        track[37700..37700 + 11].copy_from_slice(b"PLAYSTATION");
        std::fs::write(dir.join("Game (Track 01).bin"), &track).unwrap();
        std::fs::write(
            dir.join("Game.cue"),
            "FILE \"Game (Track 01).bin\" BINARY\n",
        )
        .unwrap();

        assert!(cue_references_a_playstation_disc(&dir.join("Game.cue")));
        assert!(!cue_references_a_neo_geo_cd_disc(&dir.join("Game.cue")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cue_referencing_a_non_playstation_track_is_rejected() {
        let dir =
            std::env::temp_dir().join(format!("wervik-disc-cue-reject-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut track = vec![0u8; 40_000];
        track[37649..37649 + 5].copy_from_slice(b"CD001");
        std::fs::write(dir.join("Game (Track 01).bin"), &track).unwrap();
        std::fs::write(
            dir.join("Game.cue"),
            "FILE \"Game (Track 01).bin\" BINARY\n",
        )
        .unwrap();

        assert!(!cue_references_a_playstation_disc(&dir.join("Game.cue")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cue_references_a_neo_geo_cd_disc_checks_the_referenced_track() {
        let dir = std::env::temp_dir().join(format!("wervik-disc-ngcd-cue-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut track = vec![0u8; 50_000];
        track[37649..37649 + 5].copy_from_slice(b"CD001");
        track[48483..48483 + 7].copy_from_slice(b"IPL.TXT");
        std::fs::write(dir.join("Game (Track 01).bin"), &track).unwrap();
        std::fs::write(
            dir.join("Game.cue"),
            "FILE \"Game (Track 01).bin\" BINARY\n",
        )
        .unwrap();

        assert!(cue_references_a_neo_geo_cd_disc(&dir.join("Game.cue")));
        assert!(!cue_references_a_playstation_disc(&dir.join("Game.cue")));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
