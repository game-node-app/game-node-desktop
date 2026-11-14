use std::collections::BTreeSet;

use gamenode_plugin_sdk::{achievements, models, proto::*};

fn ready() -> Achievement {
    Achievement {
        id: "ACH_WIN".into(),
        name: "Win".into(),
        description: "Win once".into(),
        hidden: false,
        icon_url: "https://cdn.example/a.jpg".into(),
        icon_gray_url: "https://cdn.example/a_gray.jpg".into(),
        unlocked: true,
        unlocked_at: Some(1_700_000_000),
    }
}

#[test]
fn sdk_contract_round_trips_ready_none_and_failed_outcomes() {
    let parsed = vec![
        models::AchievementOutcome::Ready {
            game_id: "10".into(),
            achievements: vec![models::Achievement {
                id: "ACH_WIN".into(),
                name: "Win".into(),
                description: "Win once".into(),
                hidden: false,
                icon_url: "https://cdn.example/a.jpg".into(),
                icon_gray_url: "https://cdn.example/a_gray.jpg".into(),
                unlocked: true,
                unlocked_at: Some(1_700_000_000),
            }],
        },
        models::AchievementOutcome::None {
            game_id: "20".into(),
        },
        models::AchievementOutcome::Failed {
            game_id: "30".into(),
            error: models::CatalogError::Privacy,
        },
    ];
    let wire = parsed
        .iter()
        .cloned()
        .map(achievements::outcome_to_proto)
        .collect::<Vec<_>>();
    let requested: BTreeSet<String> = ["10", "20", "30"].map(str::to_owned).into();
    let restored = achievements::parse_outcomes(&wire, &requested, true).unwrap();
    assert_eq!(restored, parsed);
    assert!(matches!(
        &restored[0],
        models::AchievementOutcome::Ready { achievements, .. }
            if achievements.len() == 1 && achievements[0].unlocked_at == Some(1_700_000_000)
    ));
    let encoded = achievements::outcome_to_proto(restored[1].clone());
    assert_eq!(encoded.game_id, "20");
    assert_eq!(
        encoded.outcome,
        Some(achievement_outcome::Outcome::None(true))
    );
}

#[test]
fn sdk_contract_rejects_unrequested_and_oversized_outcomes() {
    let requested: BTreeSet<String> = ["10".to_owned()].into();
    let unrequested = AchievementOutcome {
        game_id: "11".into(),
        outcome: Some(achievement_outcome::Outcome::None(true)),
    };
    assert!(achievements::parse_outcomes(&[unrequested], &requested, false).is_err());

    let oversized = AchievementOutcome {
        game_id: "10".into(),
        outcome: Some(achievement_outcome::Outcome::Ready(AchievementGame {
            achievements: (0..=achievements::MAX_ACHIEVEMENTS)
                .map(|index| Achievement {
                    id: format!("a{index}"),
                    ..ready()
                })
                .collect(),
        })),
    };
    assert!(achievements::parse_outcomes(&[oversized], &requested, false).is_err());

    let request = models::AchievementsRequest {
        game_ids: vec!["10".into()],
        language: "en".into(),
    };
    assert!(achievements::validate_request(&request).is_ok());
    assert!(
        achievements::validate_request(&models::AchievementsRequest {
            game_ids: (0..=achievements::MAX_GAMES)
                .map(|index| index.to_string())
                .collect(),
            language: "en".into(),
        })
        .is_err()
    );
}
