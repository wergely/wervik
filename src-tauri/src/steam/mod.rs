pub mod achievements;
pub mod artwork;
pub mod discovery;
pub mod launch;
pub mod launch_options;
pub mod library;
pub mod owned;
mod vdf;

/// Where owned-games and achievements requests get their Steam Web API
/// access from: the user's own key (Settings), or — once a Wervik Account is
/// connected — Wervik's own proxy, which needs no key from the user at all.
/// `Wervik` always wins when an account is connected (see
/// `AppState::steam_credentials`), so this is never a user-facing toggle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SteamCredentials {
    Manual(String),
    Wervik(String),
}
