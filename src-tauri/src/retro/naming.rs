//! Derives a clean display name from a ROM filename. Conservative by
//! design: it only strips parenthesized/bracketed scene tags like "(USA)"
//! or "[!]", and never touches the file on disk — the original filename
//! stays exactly as the user has it.

use std::path::Path;

pub fn clean_display_name(file_name: &str) -> String {
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(file_name);

    let mut result = String::with_capacity(stem.len());
    let mut paren_depth = 0i32;
    let mut bracket_depth = 0i32;
    for c in stem.chars() {
        match c {
            '(' => paren_depth += 1,
            ')' => paren_depth = (paren_depth - 1).max(0),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = (bracket_depth - 1).max(0),
            _ if paren_depth == 0 && bracket_depth == 0 => result.push(c),
            _ => {}
        }
    }

    let cleaned = result.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        stem.to_string()
    } else {
        cleaned
    }
}

/// Extracts each `(...)` tag from a filename stem, in order, without
/// discarding their content (unlike `clean_display_name` above, which
/// throws tags away) — used wherever a tag's actual content matters, like
/// guessing a box-art region (`artwork::region_candidates`) or ranking a
/// dump's region for deduplication (`region_rank` below).
pub(crate) fn parenthesized_tags(stem: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for c in stem.chars() {
        match c {
            '(' => {
                if depth == 0 {
                    current.clear();
                }
                depth += 1;
            }
            ')' => {
                depth -= 1;
                if depth == 0 {
                    tags.push(std::mem::take(&mut current));
                }
            }
            _ if depth > 0 => current.push(c),
            _ => {}
        }
    }
    tags
}

/// Ranks a ROM filename's region tag for tie-breaking during
/// deduplication (see `discovery::dedupe_by_name`) — lower is preferred.
/// USA/World dumps are NTSC, running at the console's full 60Hz; some N64
/// (and other) PAL releases in particular run their actual gameplay
/// simulation measurably slower even at a "full" 50Hz framerate — a real
/// characteristic of those specific cartridges/dumps, not something an
/// emulator setting can fix. Everything else (Europe, Japan, no
/// recognizable region tag at all) ranks the same, second place: there's
/// no reason to prefer one of those over another.
pub(crate) fn region_rank(file_name: &str) -> u8 {
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(file_name);
    for tag in parenthesized_tags(stem) {
        for part in tag.split(',') {
            if matches!(
                part.trim().to_ascii_lowercase().as_str(),
                "usa" | "u" | "world"
            ) {
                return 0;
            }
        }
    }
    1
}

/// A human label for a ROM dump's region, for a variant picker in the UI
/// (see `discovery::RomVariant`) — the first recognized region tag on the
/// filename (abbreviated GoodTools-style codes included), or "Original"
/// when none is found.
pub(crate) fn region_label(file_name: &str) -> String {
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(file_name);
    for tag in parenthesized_tags(stem) {
        for part in tag.split(',') {
            if let Some(label) = named_region(part.trim()) {
                return label.to_string();
            }
        }
    }
    "Original".to_string()
}

fn named_region(tag: &str) -> Option<&'static str> {
    Some(match tag.to_ascii_lowercase().as_str() {
        "usa" | "u" => "USA",
        "europe" | "e" => "Europe",
        "japan" | "j" => "Japan",
        "world" | "w" => "World",
        "australia" | "a" => "Australia",
        "brazil" | "b" => "Brazil",
        "china" | "c" => "China",
        "france" | "f" => "France",
        "germany" | "g" => "Germany",
        "italy" | "i" => "Italy",
        "korea" | "k" => "Korea",
        "spain" | "s" => "Spain",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_common_scene_tags() {
        assert_eq!(
            clean_display_name("Super Mario World (USA) (Rev 1).sfc"),
            "Super Mario World"
        );
        assert_eq!(
            clean_display_name("Sonic The Hedgehog (Europe).md"),
            "Sonic The Hedgehog"
        );
        assert_eq!(
            clean_display_name("Chrono Trigger [!].sfc"),
            "Chrono Trigger"
        );
        assert_eq!(
            clean_display_name("Pokemon - Emerald Version (U) [v1.1].gba"),
            "Pokemon - Emerald Version"
        );
    }

    #[test]
    fn falls_back_to_the_stem_when_stripping_would_empty_it() {
        assert_eq!(clean_display_name("(USA).nes"), "(USA)");
    }

    #[test]
    fn leaves_plain_names_untouched() {
        assert_eq!(clean_display_name("Chrono Trigger.sfc"), "Chrono Trigger");
    }

    #[test]
    fn extracts_each_top_level_parenthesized_tag_in_order() {
        assert_eq!(
            parenthesized_tags("Super Mario World (USA) (Rev 1)"),
            vec!["USA", "Rev 1"]
        );
        assert_eq!(parenthesized_tags("Plain Name"), Vec::<String>::new());
    }

    #[test]
    fn usa_world_and_the_goodtools_u_abbreviation_rank_best() {
        assert_eq!(region_rank("Chrono Trigger (USA).sfc"), 0);
        assert_eq!(region_rank("Chrono Trigger (World).sfc"), 0);
        assert_eq!(region_rank("Chrono Trigger (U) [!].sfc"), 0);
        // A combined-region tag counts too, wherever USA appears in it.
        assert_eq!(region_rank("Chrono Trigger (Europe, USA).sfc"), 0);
    }

    #[test]
    fn every_other_region_or_no_tag_at_all_ranks_the_same_second_place() {
        assert_eq!(region_rank("Chrono Trigger (Europe).sfc"), 1);
        assert_eq!(region_rank("Chrono Trigger (Japan).sfc"), 1);
        assert_eq!(region_rank("Chrono Trigger.sfc"), 1);
    }

    #[test]
    fn labels_recognized_regions_and_falls_back_to_original() {
        assert_eq!(region_label("Chrono Trigger (USA).sfc"), "USA");
        assert_eq!(region_label("Chrono Trigger (Europe).sfc"), "Europe");
        assert_eq!(region_label("Chrono Trigger (J).sfc"), "Japan");
        assert_eq!(region_label("Chrono Trigger (Rev 1).sfc"), "Original");
        assert_eq!(region_label("Chrono Trigger.sfc"), "Original");
    }
}
