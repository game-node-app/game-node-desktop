//! The shared-data exchange recipes run against the same fixture host as the raw RPC tests.
//!
//! Each scenario also reads the checked-in contract fixture
//! `crates/plugin-protocol/contracts/data_exchange_fixtures.json`, which the Go example reads too.
//! The fixture is the parity source for identities, units and expected outcomes; the host still
//! decides every authorization answer.

use gamenode_plugin_sdk::proto::{
    CommonDataCategory, ContentRegistrationRequest, ExtensionPayload, MutationOutcome,
    RuntimeArgumentTemplate as RuntimeArgumentProto, RuntimeRecord, RuntimeRegisterRequest,
    runtime_argument_template::Value as RuntimeArgumentValue,
};
use serde_json::Value;

use super::data::consumer_session_with_grants;
use super::saves::{INSTALLATION, directory, profile};
use super::*;

#[path = "../../../../../plugin-sdk/examples/data_exchange_recipes.rs"]
mod recipes;

const CONTRACT: &str =
    include_str!("../../../../../plugin-protocol/contracts/data_exchange_fixtures.json");

const PERFORMANCE: &[&str] = &[
    cap::GAME_LAUNCH,
    cap::GAME_PERFORMANCE_PUBLISH,
    cap::GAME_PERFORMANCE_READ,
];
const SAVE_AND_LOCATION: &[&str] = &[
    cap::GAME_SAVES_READ,
    cap::GAME_SAVES_CREATE,
    cap::GAME_SAVES_UPDATE,
    cap::GAME_SAVES_INSPECT,
    cap::GAME_STORAGE_INSPECT,
    cap::FILES_READ,
];
const RUNTIME: &[&str] = &[
    cap::DISCOVERY,
    cap::GAME_CUSTOM_LAUNCH,
    cap::FILES_READ,
    cap::GAME_RUNTIME_CREATE,
    cap::GAME_RUNTIME_READ,
];

fn contract() -> Value {
    serde_json::from_str(CONTRACT).unwrap()
}

macro_rules! text {
    ($value:expr) => {
        $value.as_str().unwrap()
    };
}

#[tokio::test(flavor = "multi_thread")]
async fn performance_fixture_exchanges_history_and_ends_the_series_explicitly() {
    use crate::plugin_host::sharing_policy::ActivationReadGrant;

    let contract = contract();
    let performance = &contract["performance"];
    let game_id = text!(performance["game_id"]);
    let series_id = text!(performance["series_id"]);
    let fixture = Fixture::start_with_target(
        PERFORMANCE,
        true,
        None,
        Some(TargetGameContext {
            game_id: game_id.into(),
            installation_id: text!(performance["installation_id"]).into(),
            profile_id: String::new(),
            revision: "library-rev-1".into(),
        }),
    )
    .await;
    let context = fixture.context();

    // The example builds the batch from the same fixture the Go client reads. Units, semantics and
    // the host-selected game are explicit; nothing here launches a collector.
    let samples = performance["samples"]
        .as_array()
        .unwrap()
        .iter()
        .map(|sample| {
            recipes::sample(
                sample["sequence"].as_u64().unwrap(),
                sample["observed_at"].as_i64().unwrap(),
                sample["fps"].as_f64().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let batch = recipes::performance_batch(
        &context,
        "performance-batch",
        0,
        series_id,
        game_id,
        samples.clone(),
    );
    assert_eq!(batch.samples.len(), 2);
    assert_eq!(
        batch.sample_interval_ms,
        performance["sample_interval_ms"].as_u64().unwrap() as u32
    );
    assert_eq!(batch.fps_unit, text!(performance["units"]["fps"]));
    assert_eq!(batch.cpu_unit, text!(performance["units"]["cpu"]));
    assert_eq!(batch.memory_unit, text!(performance["units"]["memory"]));
    assert_eq!(
        batch.samples[0].cpu_percent,
        performance["samples"][0]["cpu_percent"].as_f64().unwrap()
    );

    let published = recipes::publish_performance(
        &fixture.host,
        "performance-batch",
        0,
        series_id,
        game_id,
        samples,
    )
    .await
    .unwrap();
    assert_eq!(published.accepted_samples, 2);
    assert!(published.report_id.starts_with("performance-"));

    let consumer = consumer_session_with_grants(&fixture, &[cap::GAME_PERFORMANCE_READ]).await;
    let consumer_context = consumer.host.resource_context().unwrap().clone();
    let identity = super::data::authenticated_identity(&fixture, &consumer_context);

    // Private-only default: the same query without a selector sees the consumer's own empty scope.
    let private = recipes::list_performance(&consumer.host, game_id, None, "", 10)
        .await
        .unwrap();
    assert!(private.series.is_empty());
    // A selector without the current grant still answers empty: common history is not an error,
    // it is simply unauthorized and absent. (The runtime catalog reports the denial explicitly.)
    let selected_but_not_granted = recipes::list_performance(
        &consumer.host,
        game_id,
        Some(recipes::performance_selector()),
        "",
        10,
    )
    .await
    .unwrap();
    assert!(selected_but_not_granted.series.is_empty());

    fixture
        .shared_policies
        .set_activation_grants(
            &identity,
            ActivationReadGrant::new(
                [CommonDataCategory::GamePerformanceHistory],
                std::iter::empty(),
            )
            .unwrap(),
        )
        .unwrap();
    let listed = recipes::list_performance(
        &consumer.host,
        game_id,
        Some(recipes::performance_selector()),
        "",
        10,
    )
    .await
    .unwrap();
    assert_eq!(listed.series.len(), 1);
    let series = &listed.series[0];
    assert_eq!(series.series_id, published.report_id);
    assert_eq!(series.game_id, game_id);
    assert_eq!(
        series.installation_id,
        text!(performance["installation_id"])
    );
    // The host supplied the local computer context; the producer never named one.
    assert_eq!(series.computer_context_id, recipes::LOCAL_COMPUTER);
    assert_eq!(series.source, "overlay-frame-counter");
    assert_eq!(series.tool_name, "Example overlay");
    assert_eq!(series.samples.len(), 2);
    // Another game stays empty even with the same grant and selector.
    assert!(
        recipes::list_performance(
            &consumer.host,
            "6",
            Some(recipes::performance_selector()),
            "",
            10,
        )
        .await
        .unwrap()
        .series
        .is_empty()
    );

    let aggregate = recipes::aggregate_performance(
        &consumer.host,
        &series.series_id,
        "fps",
        "average",
        Some(recipes::performance_selector()),
    )
    .await
    .unwrap();
    assert_eq!(
        aggregate.value,
        performance["average_fps"].as_f64().unwrap()
    );
    assert_eq!(aggregate.unit, text!(performance["metric_units"]["fps"]));
    assert_eq!(aggregate.sample_count, 2);

    // Revocation fences the next explicit query and returns no records again.
    fixture
        .shared_policies
        .clear_activation_grants(&identity)
        .unwrap();
    assert!(
        recipes::list_performance(
            &consumer.host,
            game_id,
            Some(recipes::performance_selector()),
            "",
            10,
        )
        .await
        .unwrap()
        .series
        .is_empty()
    );

    // Cleanup is explicit: ending the series removes the retained history and refuses a later
    // batch, so a stopped collector cannot reopen it.
    let ended_at = performance["samples"][1]["observed_at"].as_i64().unwrap();
    let ended = recipes::end_performance(&fixture.host, series_id, published.revision, 2, ended_at)
        .await
        .unwrap();
    assert_eq!(ended.outcome, MutationOutcome::Applied as i32);
    assert_eq!(
        recipes::publish_performance(
            &fixture.host,
            "performance-after-end",
            ended.revision,
            series_id,
            game_id,
            vec![recipes::sample(3, ended_at + 100, 60.0)],
        )
        .await
        .unwrap_err()
        .code(),
        Code::FailedPrecondition
    );
    // Cleanup finishes the job: the retained series is gone, so a stopped collector's history
    // cannot be read back or extended.
    assert_eq!(
        recipes::query_performance(
            &fixture.host,
            series_id,
            ended_at - 1000,
            ended_at + 1000,
            0,
            None,
        )
        .await
        .unwrap_err()
        .code(),
        Code::NotFound
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn approved_save_folder_is_inspected_without_a_path_and_profiles_stay_owner_only() {
    let contract = contract();
    let saves_contract = &contract["save_profiles"];
    let locations = &contract["locations"];
    let (fixture, saves) = Fixture::emulator(SAVE_AND_LOCATION).await;
    let root = fixture.approve(INSTALLATION, "save", &saves).await;

    let mut new = profile(
        text!(saves_contract["name"]),
        text!(saves_contract["runtime_id"]),
        vec![directory(
            &root,
            text!(saves_contract["directories"][0]["display_name"]),
            &saves_contract["directories"][0]["naming_patterns"]
                .as_array()
                .unwrap()
                .iter()
                .map(|pattern| text!(pattern))
                .collect::<Vec<_>>(),
        )],
    );
    new.format_id = text!(saves_contract["format_id"]).into();
    new.format_version = saves_contract["format_version"].as_u64().unwrap() as u32;
    let created = fixture.make("example-save-profile", new).await.unwrap();
    assert_eq!(created.outcome, MutationOutcome::Applied as i32);

    // The owner reads the profile and its folder as a session-bound handle.
    let read = recipes::read_save_profile(&fixture.host, "", &created.profile_id)
        .await
        .unwrap()
        .profile
        .unwrap();
    assert_eq!(read.name, text!(saves_contract["name"]));
    assert_eq!(
        read.installation_id,
        text!(saves_contract["installation_id"])
    );
    assert_eq!(read.runtime_id, text!(saves_contract["runtime_id"]));
    assert_eq!(read.format_id, text!(saves_contract["format_id"]));
    assert_eq!(read.creator_plugin_id, "fixture.plugin");
    assert!(!read.directories[0].approved_resource_handle.is_empty());

    // Inspection returns an opaque volume token and no filesystem path.
    let inspected = recipes::inspect_location(&fixture.host, &root)
        .await
        .unwrap();
    let location = inspected.location.clone().unwrap();
    assert_eq!(
        location.availability,
        InspectionAvailability::Available as i32
    );
    assert!(!location.volume_identity_token.is_empty());
    assert!(!location.interface_type.is_empty());
    // The answer may describe the volume mount, but never the approved folder path.
    assert!(!location.mount_point.contains(saves.to_str().unwrap()));
    assert!(!format!("{inspected:?}").contains(saves.to_str().unwrap()));

    let measured = recipes::measure_location(&fixture.host, &root)
        .await
        .unwrap();
    assert_eq!(
        (
            measured.logical_bytes,
            measured.file_count,
            measured.directory_count,
            measured.complete,
        ),
        (
            locations["size"]["logical_bytes"].as_u64().unwrap(),
            locations["size"]["file_count"].as_u64().unwrap() as u64,
            locations["size"]["directory_count"].as_u64().unwrap() as u64,
            true,
        )
    );

    // Save profiles stay owner-only: a common-category selector is an explicit unsupported answer
    // rather than a projection of another plugin's folders.
    assert_eq!(
        recipes::list_save_profiles(
            &fixture.host,
            INSTALLATION,
            Some(recipes::category_selector(CommonDataCategory::SaveProfiles)),
        )
        .await
        .unwrap_err()
        .code(),
        Code::Unimplemented
    );
    let owned = recipes::list_save_profiles(&fixture.host, INSTALLATION, None)
        .await
        .unwrap();
    assert_eq!(owned.profiles.len(), 1);
    assert_eq!(owned.profiles[0].profile_id, created.profile_id);
}

#[tokio::test(flavor = "multi_thread")]
async fn one_shared_runtime_lists_several_content_identities_without_paths_or_handles() {
    use crate::plugin_host::sharing_policy::ActivationReadGrant;

    let contract = contract();
    let catalog = &contract["runtime_catalog"];
    let target = TargetGameContext {
        game_id: text!(catalog["game_id"]).into(),
        installation_id: text!(catalog["installation_id"]).into(),
        profile_id: "account-a".into(),
        revision: "1".into(),
    };
    let (fixture, _) = Fixture::approved_extension_game(RUNTIME, target).await;
    let content_root = fixture.game_dir(
        "example-shared-runtime",
        &[("zelda.sfc", b"rom"), ("metroid.sfc", b"rom")],
    );
    fixture.user_approving(&content_root);
    let root = fixture
        .root_handle(text!(catalog["installation_id"]), &["read"])
        .await;
    let executable = fixture.child(&root, "zelda.sfc").await;
    let zelda = fixture.child(&root, "zelda.sfc").await;
    let metroid = fixture.child(&root, "metroid.sfc").await;

    let registered = fixture
        .client()
        .runtimes_register(RuntimeRegisterRequest {
            mutation: Some(fixture.mutation("example-shared-runtime", 0)),
            runtime: Some(RuntimeRecord {
                id: text!(catalog["runtime_id"]).into(),
                name: text!(catalog["runtime_name"]).into(),
                version: text!(catalog["runtime_version"]).into(),
                executable_handle: executable,
                supported_formats: catalog["supported_formats"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|format| text!(format).to_owned())
                    .collect(),
                platform: std::env::consts::OS.into(),
                argument_template: vec![RuntimeArgumentProto {
                    value: Some(RuntimeArgumentValue::ContentPath(
                        gamenode_plugin_sdk::proto::ContentPathArgument {},
                    )),
                }],
                extensions: [(
                    "org.example.compatibility".into(),
                    ExtensionPayload {
                        schema_id: "org.example.compatibility.v1".into(),
                        schema_version: 1,
                        schema_digest: vec![3; 32],
                        data: Some(gamenode_plugin_sdk::prost_types::Struct {
                            fields: [(
                                "console".into(),
                                gamenode_plugin_sdk::prost_types::Value {
                                    kind: Some(
                                        gamenode_plugin_sdk::prost_types::value::Kind::StringValue(
                                            "snes".into(),
                                        ),
                                    ),
                                },
                            )]
                            .into(),
                        }),
                    },
                )]
                .into(),
                ..Default::default()
            }),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(registered.outcome, MutationOutcome::Applied as i32);

    for (index, (content_id, handle)) in [
        text!(&catalog["content_ids"][0]),
        text!(&catalog["content_ids"][1]),
    ]
    .into_iter()
    .zip([zelda, metroid])
    .enumerate()
    {
        let applied = fixture
            .client()
            .games_register_content(ContentRegistrationRequest {
                mutation: Some(fixture.mutation(&format!("example-content-{index}"), 0)),
                game_id: text!(catalog["game_id"]).into(),
                installation_id: text!(catalog["installation_id"]).into(),
                content_handle: handle,
                content_kind: text!(catalog["content_kind"]).into(),
                runtime_id: text!(catalog["runtime_id"]).into(),
                supported_platforms: vec![std::env::consts::OS.into()],
                content_id: content_id.into(),
            })
            .await
            .unwrap()
            .into_inner();
        assert_eq!(applied.outcome, MutationOutcome::Applied as i32);
    }

    let consumer = consumer_session_with_grants(
        &fixture,
        &[
            cap::LIBRARY_READ,
            cap::LIBRARY_READ_ALL,
            cap::GAME_RUNTIME_READ,
            cap::FILES_READ,
        ],
    )
    .await;
    let context = consumer.host.resource_context().unwrap().clone();
    let identity = super::data::authenticated_identity(&fixture, &context);

    // Without the category grant the explicit selector is refused.
    assert_eq!(
        recipes::list_shared_runtimes(&consumer.host)
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
    fixture
        .shared_policies
        .set_activation_grants(
            &identity,
            ActivationReadGrant::new(
                [
                    CommonDataCategory::RuntimeCatalog,
                    CommonDataCategory::ContentAvailability,
                ],
                std::iter::empty(),
            )
            .unwrap(),
        )
        .unwrap();

    let listed = recipes::list_shared_runtimes(&consumer.host).await.unwrap();
    assert_eq!(listed.runtimes.len(), 1);
    let runtime = &listed.runtimes[0];
    assert_eq!(runtime.id, text!(catalog["runtime_id"]));
    assert_eq!(runtime.name, text!(catalog["runtime_name"]));
    // A shared projection never carries the producer's executable handle.
    assert_eq!(
        runtime.executable_handle,
        text!(catalog["executable_handle_in_shared_projection"])
    );
    assert_eq!(
        runtime.provenance.as_ref().unwrap().producer_plugin_id,
        "fixture.plugin"
    );

    let availability =
        recipes::content_availability(&consumer.host, text!(catalog["installation_id"]))
            .await
            .unwrap()
            .entry
            .unwrap();
    let mut content_ids = availability
        .runtime_availability
        .iter()
        .map(|row| row.content_id.as_str())
        .collect::<Vec<_>>();
    content_ids.sort_unstable();
    assert_eq!(content_ids, ["rom:metroid", "rom:zelda"]);
    for row in &availability.runtime_availability {
        assert_eq!(row.runtime_id, text!(catalog["runtime_id"]));
        assert_eq!(row.content_kind, text!(catalog["content_kind"]));
        assert_eq!(row.publisher_id, "fixture.plugin");
    }
    assert!(!format!("{availability:?}").contains(content_root.to_str().unwrap()));

    // Revocation closes the same call again.
    fixture
        .shared_policies
        .clear_activation_grants(&identity)
        .unwrap();
    assert_eq!(
        recipes::list_shared_runtimes(&consumer.host)
            .await
            .unwrap_err()
            .code(),
        Code::PermissionDenied
    );
}
