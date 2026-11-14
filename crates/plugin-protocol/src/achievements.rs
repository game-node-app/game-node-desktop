//! Bounds and wire conversion for per-game achievement reads. The host rejects any result that
//! exceeds these limits before the engine sees it.

use std::collections::BTreeSet;

use crate::{WireError, models::*, proto};

pub const MAX_GAMES: usize = 20;
pub const MAX_ACHIEVEMENTS: usize = 2000;
pub const MAX_ID: usize = 256;
pub const MAX_NAME: usize = 1024;
pub const MAX_DESCRIPTION: usize = 8192;
pub const MAX_URL: usize = 4096;
pub const MAX_LANGUAGE: usize = 32;

fn bounded_text(value: &str, max: usize, allow_empty: bool) -> bool {
    (allow_empty || !value.is_empty()) && value.len() <= max && !value.chars().any(char::is_control)
}

pub fn validate_request(request: &AchievementsRequest) -> Result<(), WireError> {
    if request.game_ids.is_empty()
        || request.game_ids.len() > MAX_GAMES
        || !bounded_text(&request.language, MAX_LANGUAGE, false)
        || request.language.contains('/')
    {
        return Err(WireError);
    }
    let mut seen = BTreeSet::new();
    if request
        .game_ids
        .iter()
        .any(|id| !bounded_text(id, MAX_ID, false) || !seen.insert(id))
    {
        return Err(WireError);
    }
    Ok(())
}

pub fn parse_outcomes(
    outcomes: &[proto::AchievementOutcome],
    requested: &BTreeSet<String>,
    require_all: bool,
) -> Result<Vec<AchievementOutcome>, WireError> {
    if outcomes.len() > MAX_GAMES {
        return Err(WireError);
    }
    let mut seen = BTreeSet::new();
    let mut parsed = Vec::with_capacity(outcomes.len());
    for outcome in outcomes {
        if !bounded_text(&outcome.game_id, MAX_ID, false)
            || !requested.contains(&outcome.game_id)
            || !seen.insert(outcome.game_id.clone())
        {
            return Err(WireError);
        }
        let value = match outcome.outcome.clone().ok_or(WireError)? {
            proto::achievement_outcome::Outcome::Ready(game) => AchievementOutcome::Ready {
                game_id: outcome.game_id.clone(),
                achievements: game
                    .achievements
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<Vec<_>, _>>()?,
            },
            proto::achievement_outcome::Outcome::None(true) => AchievementOutcome::None {
                game_id: outcome.game_id.clone(),
            },
            proto::achievement_outcome::Outcome::Failed(code) => AchievementOutcome::Failed {
                game_id: outcome.game_id.clone(),
                error: proto::CatalogFailure::try_from(code)
                    .map_err(|_| WireError)?
                    .try_into()?,
            },
            _ => return Err(WireError),
        };
        if let AchievementOutcome::Ready { achievements, .. } = &value
            && (achievements.is_empty() || achievements.len() > MAX_ACHIEVEMENTS)
        {
            return Err(WireError);
        }
        parsed.push(value);
    }
    if require_all && seen != *requested {
        return Err(WireError);
    }
    Ok(parsed)
}

pub fn outcome_to_proto(outcome: AchievementOutcome) -> proto::AchievementOutcome {
    proto::AchievementOutcome {
        game_id: outcome.game_id().to_owned(),
        outcome: Some(match outcome {
            AchievementOutcome::Ready { achievements, .. } => {
                proto::achievement_outcome::Outcome::Ready(proto::AchievementGame {
                    achievements: achievements.into_iter().map(Into::into).collect(),
                })
            }
            AchievementOutcome::None { .. } => proto::achievement_outcome::Outcome::None(true),
            AchievementOutcome::Failed { error, .. } => {
                proto::achievement_outcome::Outcome::Failed(
                    proto::CatalogFailure::from(error) as i32
                )
            }
        }),
    }
}

impl From<AchievementsRequest> for proto::AchievementsRequest {
    fn from(request: AchievementsRequest) -> Self {
        Self {
            game_ids: request.game_ids,
            language: request.language,
        }
    }
}

impl TryFrom<proto::AchievementsRequest> for AchievementsRequest {
    type Error = WireError;

    fn try_from(request: proto::AchievementsRequest) -> Result<Self, Self::Error> {
        let request = Self {
            game_ids: request.game_ids,
            language: request.language,
        };
        validate_request(&request)?;
        Ok(request)
    }
}

impl From<Achievement> for proto::Achievement {
    fn from(achievement: Achievement) -> Self {
        Self {
            id: achievement.id,
            name: achievement.name,
            description: achievement.description,
            hidden: achievement.hidden,
            icon_url: achievement.icon_url,
            icon_gray_url: achievement.icon_gray_url,
            unlocked: achievement.unlocked,
            unlocked_at: achievement.unlocked_at,
        }
    }
}

impl TryFrom<proto::Achievement> for Achievement {
    type Error = WireError;

    fn try_from(achievement: proto::Achievement) -> Result<Self, Self::Error> {
        if !bounded_text(&achievement.id, MAX_ID, false)
            || !bounded_text(&achievement.name, MAX_NAME, true)
            || !bounded_text(&achievement.description, MAX_DESCRIPTION, true)
            || !bounded_text(&achievement.icon_url, MAX_URL, true)
            || !bounded_text(&achievement.icon_gray_url, MAX_URL, true)
            || achievement.unlocked_at.is_some_and(|time| time <= 0)
            || (!achievement.unlocked && achievement.unlocked_at.is_some())
        {
            return Err(WireError);
        }
        Ok(Self {
            id: achievement.id,
            name: achievement.name,
            description: achievement.description,
            hidden: achievement.hidden,
            icon_url: achievement.icon_url,
            icon_gray_url: achievement.icon_gray_url,
            unlocked: achievement.unlocked,
            unlocked_at: achievement.unlocked_at,
        })
    }
}
