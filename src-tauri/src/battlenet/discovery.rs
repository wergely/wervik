//! Discovers installed Battle.net products from the launcher's product.db.
//!
//! Battle.net keeps this file in a small protobuf-like database under
//! `%ProgramData%`. We decode only the fields needed by Wervik, so the app does
//! not need a database dependency or a full Battle.net client API.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct BattleNetGame {
    /// Stable Wervik id, based on Battle.net's product code.
    pub id: String,
    pub name: String,
    pub launch_code: String,
    pub install_dir: PathBuf,
}

#[derive(Debug, Default)]
struct ProductInstall {
    uid: String,
    product_code: String,
    install_path: String,
    installed: bool,
    playable: bool,
}

#[cfg(windows)]
pub fn scan() -> Vec<BattleNetGame> {
    let Some(db_path) = product_db_path() else {
        return Vec::new();
    };
    let Ok(bytes) = std::fs::read(db_path) else {
        return Vec::new();
    };

    let mut products = parse_database(&bytes);
    if products.is_empty() {
        // Some Battle.net versions write a single ProductInstall message
        // instead of the usual Database wrapper while updating the file.
        if let Some(product) = parse_product_install(&bytes) {
            products.push(product);
        }
    }

    let mut seen = std::collections::HashSet::new();
    products
        .into_iter()
        .filter_map(|product| {
            let code = product.product_code.trim().to_ascii_lowercase();
            let install_dir = PathBuf::from(product.install_path);
            if code.is_empty()
                || code == "agent"
                || code == "bna"
                || !product.installed
                || !product.playable
                || !install_dir.is_dir()
            {
                return None;
            }

            let name = product_name(&code)?.to_string();
            let id = format!("battlenet:{code}");
            if !seen.insert(id.clone()) {
                return None;
            }

            Some(BattleNetGame {
                id,
                name,
                launch_code: launch_code(&code).to_string(),
                install_dir,
            })
        })
        .collect()
}

#[cfg(windows)]
fn product_db_path() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(program_data) = std::env::var_os("PROGRAMDATA") {
        candidates.push(PathBuf::from(program_data).join("Battle.net/Agent/product.db"));
    }
    candidates.push(PathBuf::from(r"C:\ProgramData\Battle.net\Agent\product.db"));
    candidates.into_iter().find(|path| path.is_file())
}

#[cfg(windows)]
fn product_name(code: &str) -> Option<&'static str> {
    // Battle.net's product.db contains product codes, not display names.
    // Keep this list local and conservative so agent/helper entries do not
    // become games in the library.
    Some(match code {
        "s1" => "StarCraft",
        "s2" => "StarCraft II",
        "wow" => "World of Warcraft",
        "wow_classic" | "wow-classic" => "World of Warcraft Classic",
        "prometheus" | "pro" => "Overwatch 2",
        "w3" => "Warcraft III",
        "hsb" | "hearthstone" | "wtcg" => "Hearthstone",
        "hero" => "Heroes of the Storm",
        "d3" | "d3cn" => "Diablo III",
        "fenris" | "d4" => "Diablo IV",
        "viper" => "Call of Duty: Black Ops 4",
        "odin" => "Call of Duty: Modern Warfare",
        "lazarus" => "Call of Duty: MW2 Campaign Remastered",
        "zeus" => "Call of Duty: Black Ops Cold War",
        "auks" => "Call of Duty: Modern Warfare II",
        "codhq" => "Call of Duty HQ",
        "rtro" => "Blizzard Arcade Collection",
        "wlby" => "Crash Bandicoot 4: It's About Time",
        "osi" => "Diablo II: Resurrected",
        "fore" => "Call of Duty: Vanguard",
        "d2" => "Diablo II",
        "d2lod" => "Diablo II: Lord of Destruction",
        "w2bn" => "Warcraft II: Battle.net Edition",
        "w2r" => "Warcraft II: Remastered",
        "w3roc" => "Warcraft III: Reign of Chaos",
        "w3tft" => "Warcraft III: The Frozen Throne",
        "sca" => "StarCraft: Anthology",
        _ => return None,
    })
}

#[cfg(windows)]
fn launch_code(code: &str) -> &'static str {
    // product.db stores the NGDP code, while Battle.net's --exec launch
    // command uses the product code used by the client UI.
    match code {
        "s1" => "S1",
        "s2" => "S2",
        "wow" => "WoW",
        "wow_classic" | "wow-classic" => "WoW_wow_classic",
        "prometheus" | "pro" => "Pro",
        "w3" => "W3",
        "hsb" | "hearthstone" | "wtcg" => "WTCG",
        "hero" => "Hero",
        "d3" | "d3cn" => "D3",
        "fenris" | "d4" => "Fen",
        "viper" => "VIPR",
        "odin" => "ODIN",
        "lazarus" => "LAZR",
        "zeus" => "ZEUS",
        "auks" => "AUKS",
        "codhq" => "CODHQ",
        "rtro" => "RTRO",
        "wlby" => "WLBY",
        "osi" => "OSI",
        "fore" => "FORE",
        "d2" => "Diablo II",
        "d2lod" => "Diablo II",
        "w2bn" => "W2BN",
        "w2r" => "W2R",
        "w3roc" => "Warcraft III",
        "w3tft" => "Warcraft III",
        "sca" => "Starcraft",
        _ => "",
    }
}

#[cfg(windows)]
fn parse_database(bytes: &[u8]) -> Vec<ProductInstall> {
    let mut cursor = 0;
    let mut products = Vec::new();
    while let Some(field) = read_field(bytes, &mut cursor) {
        if field.number == 1 && field.wire_type == 2 {
            if let Some(product) = parse_product_install(field.bytes) {
                products.push(product);
            }
        }
    }
    products
}

#[cfg(windows)]
fn parse_product_install(bytes: &[u8]) -> Option<ProductInstall> {
    let mut cursor = 0;
    let mut product = ProductInstall::default();
    while let Some(field) = read_field(bytes, &mut cursor) {
        match (field.number, field.wire_type) {
            (1, 2) => product.uid = decode_string(field.bytes),
            (2, 2) => product.product_code = decode_string(field.bytes),
            (3, 2) => {
                if let Some(settings) = parse_user_settings(field.bytes) {
                    product.install_path = settings.install_path;
                }
            }
            (4, 2) => {
                let (installed, playable) = parse_cached_product_state(field.bytes);
                product.installed = installed;
                product.playable = playable;
            }
            _ => {}
        }
    }

    if product.product_code.is_empty() {
        None
    } else {
        Some(product)
    }
}

#[cfg(windows)]
fn parse_user_settings(bytes: &[u8]) -> Option<UserSettings> {
    let mut cursor = 0;
    let mut settings = UserSettings::default();
    while let Some(field) = read_field(bytes, &mut cursor) {
        if field.number == 1 && field.wire_type == 2 {
            settings.install_path = decode_string(field.bytes);
        }
    }
    Some(settings)
}

#[cfg(windows)]
fn parse_cached_product_state(bytes: &[u8]) -> (bool, bool) {
    let mut cursor = 0;
    while let Some(field) = read_field(bytes, &mut cursor) {
        if field.number == 1 && field.wire_type == 2 {
            return parse_base_product_state(field.bytes);
        }
    }
    (false, false)
}

#[cfg(windows)]
fn parse_base_product_state(bytes: &[u8]) -> (bool, bool) {
    let mut cursor = 0;
    let mut installed = false;
    let mut playable = false;
    while let Some(field) = read_field(bytes, &mut cursor) {
        match (field.number, field.wire_type) {
            (1, 0) => installed = field.varint == Some(1),
            (2, 0) => playable = field.varint == Some(1),
            _ => {}
        }
    }
    (installed, playable)
}

#[cfg(windows)]
#[derive(Debug)]
struct Field<'a> {
    number: u32,
    wire_type: u8,
    bytes: &'a [u8],
    varint: Option<u64>,
}

#[cfg(windows)]
fn read_field<'a>(bytes: &'a [u8], cursor: &mut usize) -> Option<Field<'a>> {
    let key = read_varint(bytes, cursor)?;
    let number = u32::try_from(key >> 3).ok()?;
    let wire_type = u8::try_from(key & 0x07).ok()?;
    match wire_type {
        0 => Some(Field {
            number,
            wire_type,
            bytes: &[],
            varint: Some(read_varint(bytes, cursor)?),
        }),
        1 => {
            let end = cursor.checked_add(8)?;
            if end > bytes.len() {
                return None;
            }
            let value = &bytes[*cursor..end];
            *cursor = end;
            Some(Field {
                number,
                wire_type,
                bytes: value,
                varint: None,
            })
        }
        2 => {
            let length = usize::try_from(read_varint(bytes, cursor)?).ok()?;
            let end = cursor.checked_add(length)?;
            if end > bytes.len() {
                return None;
            }
            let value = &bytes[*cursor..end];
            *cursor = end;
            Some(Field {
                number,
                wire_type,
                bytes: value,
                varint: None,
            })
        }
        5 => {
            let end = cursor.checked_add(4)?;
            if end > bytes.len() {
                return None;
            }
            let value = &bytes[*cursor..end];
            *cursor = end;
            Some(Field {
                number,
                wire_type,
                bytes: value,
                varint: None,
            })
        }
        _ => None,
    }
}

#[cfg(windows)]
fn read_varint(bytes: &[u8], cursor: &mut usize) -> Option<u64> {
    let mut result = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = *bytes.get(*cursor)?;
        *cursor += 1;
        result |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(result);
        }
    }
    None
}

#[cfg(windows)]
fn decode_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_string()
}

#[cfg(not(windows))]
pub fn scan() -> Vec<BattleNetGame> {
    Vec::new()
}

#[cfg(windows)]
#[derive(Debug, Default)]
struct UserSettings {
    install_path: String,
}
