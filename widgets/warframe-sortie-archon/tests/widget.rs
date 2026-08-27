use overcrow_widget_sdk::{
    GrantedCapabilities, GuestError, HarnessError, HostCommand, HostEvent, InitInput, Interaction,
    InteractionKind, OverlayModeCode, SessionData, ViewNode, WidgetHarness,
};
use serde::Deserialize;
use serde_json::Value;

use super::{PROVIDER_ID, PROVIDER_SCHEMA, SortieArchonWidget};

const WORLDSTATE: &[u8] = include_bytes!("../../fixtures/worldstate-activities-v1.json");
const NOW_MS: u64 = 1_777_000_000_000;

#[test]
fn mission_completion_uses_provider_identity_across_reordering() {
    let mut widget = SortieArchonWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("activity init");
    assert_eq!(storage_get(harness.output()), Some((1, "completion")));
    harness
        .send(HostEvent::StorageResult((1, None)))
        .expect("default completion state");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(provider_data(1, WORLDSTATE))
        .expect("activity data");
    assert_eq!(toggle(harness.output(), "mission-sortie-b"), Some(false));
    harness
        .send(toggled("mission-sortie-b", true))
        .expect("complete second mission");
    let (request_id, bytes) = storage_set(harness.output()).expect("persist completion");
    assert_eq!(request_id, 2);
    let stored = StoredCompletion::from(bytes);
    assert_eq!(stored.entries.len(), 1);
    assert_eq!(stored.entries[0].key, "mission-sortie-b");
    assert_eq!(stored.entries[0].completed_at_secs, NOW_MS / 1_000);

    harness
        .send(provider_data(2, &reordered_activities()))
        .expect("reordered provider data");
    assert_eq!(toggle(harness.output(), "mission-sortie-b"), Some(true));
    assert_eq!(toggle(harness.output(), "mission-sortie-a"), Some(false));
}

#[test]
fn block_completion_and_provider_pruning_are_persisted() {
    let mut widget = SortieArchonWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("activity init");
    harness
        .send(HostEvent::StorageResult((1, None)))
        .expect("default completion state");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(provider_data(1, WORLDSTATE))
        .expect("activity data");
    harness
        .send(toggled("act-sortie", true))
        .expect("complete sortie block");
    for id in ["mission-sortie-a", "mission-sortie-b", "mission-sortie-c"] {
        assert_eq!(toggle(harness.output(), id), Some(true));
    }
    harness
        .send(HostEvent::StorageResult((2, None)))
        .expect("block completion stored");

    harness
        .send(provider_data(2, &without_sortie()))
        .expect("sortie expired from provider");
    let (request_id, bytes) = storage_set(harness.output()).expect("pruned completion persisted");
    assert_eq!(request_id, 3);
    assert!(StoredCompletion::from(bytes).entries.is_empty());
    assert!(toggle(harness.output(), "mission-sortie-a").is_none());
}

#[test]
fn stale_snapshot_still_prunes_a_known_expired_activity() {
    let mut widget = SortieArchonWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("activity init");
    harness
        .send(HostEvent::StorageResult((1, None)))
        .expect("default completion state");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(provider_data(1, WORLDSTATE))
        .expect("activity data");
    harness
        .send(toggled("mission-sortie-a", true))
        .expect("complete sortie mission");
    harness
        .send(HostEvent::StorageResult((2, None)))
        .expect("completion stored");

    harness
        .send(HostEvent::Tick(NOW_MS + 3_600_000))
        .expect("known activity expires after snapshot is stale");
    let (request_id, bytes) = storage_set(harness.output()).expect("expiry pruning persisted");
    assert_eq!(request_id, 3);
    assert!(StoredCompletion::from(bytes).entries.is_empty());
    assert!(texts(harness.output()).contains(&"Worldstate data is unavailable."));
}

#[test]
fn activities_render_localized_bounded_rows_and_stale_state() {
    let mut widget = SortieArchonWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("fr")).expect("activity init");
    harness
        .send(HostEvent::StorageResult((1, None)))
        .expect("default completion state");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(provider_data(1, WORLDSTATE))
        .expect("activity data");
    assert!(texts(harness.output()).contains(&"Sortie et Chasse à l’Archonte"));
    assert_eq!(
        toggle_label(harness.output(), "act-sortie"),
        Some("Sortie · Kela De Thaym · 60 min")
    );
    assert!(texts(harness.output()).contains(&"Éclat d’Archonte azur"));
    assert_eq!(
        toggle_label(harness.output(), "mission-sortie-a"),
        Some("Extermination · Adaro · Sedna · Enemy Physical Enhancement")
    );

    harness
        .send(HostEvent::LocaleChanged("de".to_owned()))
        .expect("default locale fallback");
    assert!(texts(harness.output()).contains(&"Sortie & Archon Hunt"));
    assert!(texts(harness.output()).contains(&"Azure Archon Shard"));
    harness
        .send(HostEvent::Tick(NOW_MS + 300_000))
        .expect("exact stale boundary");
    assert!(texts(harness.output()).contains(&"Worldstate data is unavailable."));
}

#[test]
fn passive_mode_suppresses_every_completion_toggle() {
    let mut widget = SortieArchonWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("activity init");
    harness
        .send(HostEvent::StorageResult((1, None)))
        .expect("default completion state");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(provider_data(1, WORLDSTATE))
        .expect("activity data");
    harness.set_mode(OverlayModeCode::Passive);
    for id in [
        "act-sortie",
        "mission-sortie-a",
        "mission-sortie-b",
        "mission-sortie-c",
        "act-archon",
        "mission-archon-a",
        "mission-archon-b",
        "mission-archon-c",
    ] {
        assert!(matches!(
            harness.send(toggled(id, true)),
            Err(HarnessError::Passive)
        ));
    }
}

#[test]
fn malformed_storage_provider_and_authority_fail_closed() {
    let mut widget = SortieArchonWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("activity init");
    harness
        .send(HostEvent::StorageResult((
            1,
            Some(oversized_completion_state()),
        )))
        .expect("oversized completion set defaults");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(provider_data(1, WORLDSTATE))
        .expect("activity data");
    assert_eq!(toggle(harness.output(), "mission-sortie-a"), Some(false));
    harness
        .send(provider_data(2, b"{}"))
        .expect("malformed provider invalidates view");
    assert!(texts(harness.output()).contains(&"Worldstate data is unavailable."));

    for capabilities in [
        GrantedCapabilities {
            storage: false,
            ..grants()
        },
        GrantedCapabilities {
            game_data: Vec::new(),
            ..grants()
        },
        GrantedCapabilities {
            http_hosts: vec!["api.warframe.com".to_owned()],
            ..grants()
        },
        GrantedCapabilities {
            provider: true,
            ..grants()
        },
    ] {
        let mut widget = SortieArchonWidget::default();
        let mut input = init("en");
        input.granted_capabilities = capabilities;
        assert!(matches!(
            WidgetHarness::from_init(&mut widget, input),
            Err(GuestError::Unavailable)
        ));
    }
}

#[test]
fn completion_storage_is_ordered_coalesced_and_not_overwritten_by_a_late_load() {
    let mut widget = SortieArchonWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("activity init");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(provider_data(1, WORLDSTATE))
        .expect("activity data");
    harness
        .send(toggled("mission-sortie-a", true))
        .expect("completion before load");
    assert_eq!(storage_set(harness.output()).map(|value| value.0), Some(2));

    harness
        .send(HostEvent::StorageResult((
            1,
            Some(stored_completion(&["mission-sortie-b"])),
        )))
        .expect("late load ignored");
    assert_eq!(toggle(harness.output(), "mission-sortie-a"), Some(true));
    assert_eq!(toggle(harness.output(), "mission-sortie-b"), Some(false));

    harness
        .send(toggled("mission-sortie-c", true))
        .expect("coalesced completion");
    assert!(storage_set(harness.output()).is_none());
    harness
        .send(HostEvent::StorageResult((2, None)))
        .expect("first store completes");
    let (request_id, bytes) = storage_set(harness.output()).expect("queued state stored");
    assert_eq!(request_id, 3);
    assert_eq!(
        StoredCompletion::from(bytes)
            .entries
            .iter()
            .map(|entry| entry.key.as_str())
            .collect::<Vec<_>>(),
        vec!["mission-sortie-a", "mission-sortie-c"]
    );
}

#[test]
fn runtime_settings_time_and_session_boundaries_fail_closed() {
    let mut widget = SortieArchonWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("activity init");
    assert!(matches!(
        harness.send(HostEvent::SettingsChanged((1, b"{}".to_vec()))),
        Err(HarnessError::Widget(GuestError::InvalidInput))
    ));

    let mut widget = SortieArchonWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("activity init");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    assert!(matches!(
        harness.send(HostEvent::Tick(NOW_MS - 1)),
        Err(HarnessError::Widget(GuestError::InvalidInput))
    ));

    let mut widget = SortieArchonWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("activity init");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(provider_data(1, WORLDSTATE))
        .expect("activity data");
    harness
        .send(HostEvent::SessionData(inactive_session()))
        .expect("inactive session");
    assert!(matches!(
        harness.send(toggled("mission-sortie-a", true)),
        Err(HarnessError::UnknownElement)
    ));
}

fn init(locale: &str) -> InitInput {
    InitInput {
        locale: locale.to_owned(),
        granted_capabilities: grants(),
        settings: Vec::new(),
        session_data: Some(SessionData {
            selected_active: true,
            steam_app_id: Some(230_410),
            session_elapsed_ms: Some(1_000),
            overlay_mode: OverlayModeCode::Interactive,
            cpu_percent_hundredths: None,
            resident_bytes: None,
            cpu_temperature_millicelsius: None,
            gpu_temperature_millicelsius: None,
        }),
    }
}

fn grants() -> GrantedCapabilities {
    GrantedCapabilities {
        http_hosts: Vec::new(),
        game_data: vec!["overcrow.session.v1".to_owned()],
        storage: true,
        clipboard_write: false,
        provider: false,
    }
}

fn inactive_session() -> SessionData {
    SessionData {
        selected_active: false,
        steam_app_id: Some(230_410),
        session_elapsed_ms: Some(1_000),
        overlay_mode: OverlayModeCode::Interactive,
        cpu_percent_hundredths: None,
        resident_bytes: None,
        cpu_temperature_millicelsius: None,
        gpu_temperature_millicelsius: None,
    }
}

fn provider_data(revision: u64, payload: &[u8]) -> HostEvent {
    HostEvent::ProviderData((
        PROVIDER_ID.to_owned(),
        PROVIDER_SCHEMA.to_owned(),
        revision,
        payload.to_vec(),
    ))
}

fn toggled(id: &str, value: bool) -> HostEvent {
    HostEvent::Interaction(Interaction {
        element_id: id.to_owned(),
        kind: InteractionKind::Toggled(value),
    })
}

fn reordered_activities() -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(WORLDSTATE).expect("activity fixture");
    value["sortie"]["missions"]
        .as_array_mut()
        .expect("sortie missions")
        .reverse();
    value["archon"]["missions"]
        .as_array_mut()
        .expect("archon missions")
        .reverse();
    serde_json::to_vec(&value).expect("activity JSON")
}

fn without_sortie() -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(WORLDSTATE).expect("activity fixture");
    value["sortie"] = Value::Null;
    serde_json::to_vec(&value).expect("activity JSON")
}

fn oversized_completion_state() -> Vec<u8> {
    let entries = (0..17)
        .map(|index| {
            serde_json::json!({
                "key": format!("mission-{index}"),
                "completedAtSecs": 1_777_000_000_u64
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_vec(&serde_json::json!({"schemaVersion": 1, "entries": entries}))
        .expect("completion JSON")
}

fn stored_completion(keys: &[&str]) -> Vec<u8> {
    let entries = keys
        .iter()
        .map(|key| {
            serde_json::json!({
                "key": key,
                "completedAtSecs": NOW_MS / 1_000
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_vec(&serde_json::json!({"schemaVersion": 1, "entries": entries}))
        .expect("completion JSON")
}

fn texts(output: &overcrow_widget_sdk::GuestOutput) -> Vec<&str> {
    output
        .view
        .iter()
        .flat_map(|view| &view.nodes)
        .filter_map(|node| match node {
            ViewNode::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn toggle(output: &overcrow_widget_sdk::GuestOutput, expected: &str) -> Option<bool> {
    output
        .view
        .iter()
        .flat_map(|view| &view.nodes)
        .find_map(|node| match node {
            ViewNode::Toggle((id, _, value)) if id == expected => Some(*value),
            _ => None,
        })
}

fn toggle_label<'a>(
    output: &'a overcrow_widget_sdk::GuestOutput,
    expected: &str,
) -> Option<&'a str> {
    output
        .view
        .iter()
        .flat_map(|view| &view.nodes)
        .find_map(|node| match node {
            ViewNode::Toggle((id, label, _)) if id == expected => Some(label.as_str()),
            _ => None,
        })
}

fn storage_get(output: &overcrow_widget_sdk::GuestOutput) -> Option<(u32, &str)> {
    output.commands.iter().find_map(|command| match command {
        HostCommand::StorageGet((request_id, key)) => Some((*request_id, key.as_str())),
        _ => None,
    })
}

fn storage_set(output: &overcrow_widget_sdk::GuestOutput) -> Option<(u32, &[u8])> {
    output.commands.iter().find_map(|command| match command {
        HostCommand::StorageSet((request_id, _, bytes)) => Some((*request_id, bytes.as_slice())),
        _ => None,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredCompletion {
    entries: Vec<StoredEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredEntry {
    key: String,
    completed_at_secs: u64,
}

impl StoredCompletion {
    fn from(bytes: &[u8]) -> Self {
        serde_json::from_slice(bytes).expect("stored completion schema")
    }
}
