//! Steam store account port: the session derived from the stored cookie and the owned and
//! family-shared catalogs read with its short-lived web API token.
//!
//! The types here carry the credentials the HTTP adapter needs but never expose them: the token and
//! cookie use [`SessionCredential`], whose `Debug` and `Display` are redacted, and [`SteamError`]
//! carries only a category. Logs and UI use the category, never a raw Steam response.

use async_trait::async_trait;

use gamenode_plugin_protocol::models::{CatalogError, SessionCredential};

/// A validated Steam store session.
///
/// `token` is the short-lived web API token read from the store page on every check; it is never
/// persisted. `cookie` is the minimal store cookie header, possibly rotated by Steam during the
/// check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SteamSession {
    pub steam_id: String,
    pub account_name: Option<String>,
    pub token: SessionCredential,
    pub cookie: SessionCredential,
}

/// One game from the owned or Steam Family catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SteamOwnedGame {
    pub app_id: String,
    pub name: String,
}

/// The games shared with the account through Steam Family.
///
/// `group_id` is `None` when the account belongs to no family; that is a successful empty result,
/// not an error.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SteamFamily {
    pub group_id: Option<String>,
    pub games: Vec<SteamOwnedGame>,
}

/// One achievement definition read from the game schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SteamAchievementDefinition {
    pub api_name: String,
    pub display_name: String,
    pub description: String,
    pub hidden: bool,
    pub icon: String,
    pub icon_gray: String,
}

/// The account's unlock state for one achievement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SteamAchievementState {
    pub api_name: String,
    pub achieved: bool,
    pub unlock_time: Option<i64>,
    /// Localized by Steam for the requested language; the schema text is the fallback.
    pub display_name: String,
    pub description: String,
}

/// A definition joined with the account's unlock state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SteamAchievement {
    pub id: String,
    pub name: String,
    pub description: String,
    pub hidden: bool,
    pub icon_url: String,
    pub icon_gray_url: String,
    pub unlocked: bool,
    pub unlocked_at: Option<i64>,
}

/// Steam text occasionally contains tabs and C1 controls. They are valid JSON strings but the
/// plugin protocol rejects control characters, so replace each with a space and collapse runs.
pub(crate) fn clean_achievement_text(value: &str) -> String {
    let mut text = String::with_capacity(value.len());
    let mut previous_space = false;
    for character in value.chars() {
        let character = if character.is_control() {
            ' '
        } else {
            character
        };
        if character == ' ' {
            if !previous_space {
                text.push(' ');
            }
            previous_space = true;
        } else {
            text.push(character);
            previous_space = false;
        }
    }
    text.trim().to_owned()
}

/// Why a Steam request failed. Every variant is a category: no cookie, token, URL or page body is
/// carried in the value, its `Debug`, or its `Display`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SteamError {
    #[error("Steam could not be reached")]
    Unreachable,
    #[error("Steam is rate limiting requests")]
    RateLimited,
    #[error("Steam returned a server error")]
    Server,
    #[error("the Steam session is no longer valid")]
    Expired,
    #[error("the Steam account does not disclose its library")]
    Privacy,
    #[error("Steam changed the response this client expects")]
    Changed,
    #[error("the saved Steam cookies are invalid")]
    InvalidCookies,
}
impl SteamError {
    /// The stable catalog category for a failed owned-catalog fetch.
    pub const fn catalog_error(self) -> CatalogError {
        match self {
            Self::Unreachable => CatalogError::Unreachable,
            Self::RateLimited => CatalogError::RateLimited,
            Self::Server => CatalogError::Server,
            Self::Expired => CatalogError::Expired,
            Self::Privacy => CatalogError::Privacy,
            Self::Changed => CatalogError::Changed,
            Self::InvalidCookies => CatalogError::InvalidCredential,
        }
    }

    /// The category for a failed family fetch. Family failures are always reported as a
    /// family-specific category so they never look like an owned-catalog failure.
    pub const fn family_catalog_error(self) -> CatalogError {
        CatalogError::FamilyUnavailable
    }
}

impl From<SteamError> for CatalogError {
    fn from(error: SteamError) -> Self {
        error.catalog_error()
    }
}

/// The Steam store account operations implemented by the plugin.
#[async_trait]
pub trait SteamWebApi: Send + Sync {
    /// Validates `cookie` against the authenticated store page and derives the account identity,
    /// the short-lived token and the (possibly rotated) minimal cookie header.
    async fn session(&self, cookie: &SessionCredential) -> Result<SteamSession, SteamError>;

    /// Exchanges the long-lived `steamRefresh_steam` cookie for a fresh store access cookie.
    ///
    /// The account service persists the returned header before validating it, so a renewal that
    /// rotates the refresh cookie is never lost. `None` means the credential has no refresh
    /// cookie and cannot be renewed without a new sign-in.
    async fn renew_session(
        &self,
        cookie: &SessionCredential,
    ) -> Result<Option<SessionCredential>, SteamError> {
        let _ = cookie;
        Ok(None)
    }

    /// Every game the account owns, using the session's web API token.
    async fn owned_games(&self, session: &SteamSession) -> Result<Vec<SteamOwnedGame>, SteamError>;

    /// The games shared with the account through Steam Family. An account in no family yields an
    /// empty result.
    async fn family_games(&self, session: &SteamSession) -> Result<SteamFamily, SteamError>;

    /// The game's achievement definitions. `None` means Steam confirmed the app has no stats or no
    /// achievements.
    async fn achievement_definitions(
        &self,
        session: &SteamSession,
        app_id: &str,
        language: &str,
    ) -> Result<Option<Vec<SteamAchievementDefinition>>, SteamError> {
        let _ = (session, app_id, language);
        Err(SteamError::Changed)
    }

    /// The connected account's unlock state. `None` means Steam reported no achievements.
    async fn achievement_states(
        &self,
        session: &SteamSession,
        app_id: &str,
        language: &str,
    ) -> Result<Option<Vec<SteamAchievementState>>, SteamError> {
        let _ = (session, app_id, language);
        Err(SteamError::Changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credential(value: &str) -> SessionCredential {
        SessionCredential::new(value).expect("a test credential")
    }

    fn session() -> SteamSession {
        SteamSession {
            steam_id: "76561198000000001".to_owned(),
            account_name: Some("Player One".to_owned()),
            token: credential("webapi-token-secret"),
            cookie: credential("steamLoginSecure=76561198000000001%7C%7Ccookie-secret"),
        }
    }

    #[test]
    fn session_debug_redacts_token_and_cookie() {
        let debug = format!("{:?}", session());
        assert!(debug.contains("76561198000000001"));
        assert!(!debug.contains("webapi-token-secret"));
        assert!(!debug.contains("cookie-secret"));
    }

    #[test]
    fn errors_carry_no_secret_text() {
        for error in [
            SteamError::Unreachable,
            SteamError::RateLimited,
            SteamError::Server,
            SteamError::Expired,
            SteamError::Privacy,
            SteamError::Changed,
            SteamError::InvalidCookies,
        ] {
            let shown = format!("{error} {error:?}");
            assert!(!shown.contains("secret"));
            assert!(!shown.contains("webapi_token"));
            assert!(!shown.contains("steamLoginSecure"));
            assert!(!shown.contains("Bearer"));
            assert!(!shown.contains("http"));
        }
    }

    #[test]
    fn steam_errors_map_to_catalog_categories() {
        assert_eq!(
            CatalogError::from(SteamError::Unreachable),
            CatalogError::Unreachable
        );
        assert_eq!(
            CatalogError::from(SteamError::RateLimited),
            CatalogError::RateLimited
        );
        assert_eq!(CatalogError::from(SteamError::Server), CatalogError::Server);
        assert_eq!(
            CatalogError::from(SteamError::Expired),
            CatalogError::Expired
        );
        assert_eq!(
            CatalogError::from(SteamError::Privacy),
            CatalogError::Privacy
        );
        assert_eq!(
            CatalogError::from(SteamError::Changed),
            CatalogError::Changed
        );
        assert_eq!(
            CatalogError::from(SteamError::InvalidCookies),
            CatalogError::InvalidCredential
        );
        assert_eq!(
            SteamError::Server.family_catalog_error(),
            CatalogError::FamilyUnavailable
        );
        assert_eq!(
            SteamError::Expired.family_catalog_error(),
            CatalogError::FamilyUnavailable
        );
    }

    #[test]
    fn family_default_is_empty() {
        let family = SteamFamily::default();
        assert!(family.group_id.is_none());
        assert!(family.games.is_empty());
    }

    #[test]
    fn owned_game_keeps_the_steam_app_id_as_text() {
        let game = SteamOwnedGame {
            app_id: "620".to_owned(),
            name: "Portal 2".to_owned(),
        };
        assert_eq!(game.app_id, "620");
    }
}
