use overcrow_widget_sdk::{
    GrantedCapabilities, GuestError, HarnessError, HostCommand, HostEvent, InitInput, Interaction,
    InteractionKind, OverlayModeCode, SessionData, ViewNode, WidgetHarness,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{InvasionsWidget, PROVIDER_ID, PROVIDER_SCHEMA};

const WORLDSTATE: &[u8] = include_bytes!("../../fixtures/worldstate-activities-v1.json");
const NOW_MS: u64 = 1_777_000_000_000;

#[test]
fn completion_is_instance_scoped_and_survives_reordering() {
    let mut widget = InvasionsWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("invasions init");
    assert_eq!(storage_get(harness.output()), Some((1, "state")));
    harness
        .send(HostEvent::StorageResult((1, None)))
        .expect("default state");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(provider_data(1, WORLDSTATE))
        .expect("invasion data");
    assert_eq!(toggle(harness.output(), "inv:instance-a"), Some(false));
    assert_eq!(toggle(harness.output(), "inv:instance-b"), Some(false));
    assert!(toggle(harness.output(), "inv:instance-completed").is_none());

    harness
        .send(toggled("inv:instance-b", true))
        .expect("complete second invasion");
    let (request_id, bytes) = storage_set(harness.output()).expect("persist completion");
    assert_eq!(request_id, 2);
    let state = StoredState::from(bytes);
    assert_eq!(state.entries.len(), 1);
    assert_eq!(state.entries[0].key, "inv:instance-b");
    assert_eq!(state.entries[0].completed_at_secs, NOW_MS / 1_000);
    assert_eq!(toggle(harness.output(), "inv:instance-a"), Some(false));
    assert_eq!(toggle(harness.output(), "inv:instance-b"), Some(true));

    harness
        .send(provider_data(2, &reordered_invasions()))
        .expect("reordered invasions");
    assert_eq!(toggle(harness.output(), "inv:instance-a"), Some(false));
    assert_eq!(toggle(harness.output(), "inv:instance-b"), Some(true));
}

#[test]
fn factions_rewards_progress_and_compact_view_are_localized() {
    let mut widget = InvasionsWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("invasions init");
    harness
        .send(HostEvent::StorageResult((1, None)))
        .expect("default state");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(provider_data(1, WORLDSTATE))
        .expect("invasion data");
    assert!(texts(harness.output()).contains(&"Warframe Invasions"));
    assert_eq!(
        toggle_label(harness.output(), "inv:instance-a"),
        Some("Cassini · Saturn · Grineer vs Corpus")
    );
    assert!(texts(harness.output()).contains(&"Detonite Injector ×1"));
    assert_eq!(progress_values(harness.output()), vec![250, 500]);
    harness
        .send(toggled("view-compact", true))
        .expect("enable compact view");
    assert_eq!(toggle(harness.output(), "view-compact"), Some(true));
    assert_eq!(storage_set(harness.output()).map(|value| value.0), Some(2));

    harness
        .send(HostEvent::LocaleChanged("fr".to_owned()))
        .expect("French locale");
    assert!(texts(harness.output()).contains(&"Invasions Warframe"));
    assert_eq!(
        toggle_label(harness.output(), "view-compact"),
        Some("Vue compacte")
    );
    harness
        .send(HostEvent::LocaleChanged("de".to_owned()))
        .expect("default locale fallback");
    assert!(texts(harness.output()).contains(&"Warframe Invasions"));
}

#[test]
fn stored_completion_is_pruned_when_instance_disappears() {
    let mut widget = InvasionsWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("invasions init");
    harness
        .send(HostEvent::StorageResult((
            1,
            Some(stored_state(&["inv:instance-b", "inv:missing"], false)),
        )))
        .expect("stored state");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(provider_data(1, WORLDSTATE))
        .expect("prune unknown instance");
    let (request_id, bytes) = storage_set(harness.output()).expect("persist first prune");
    assert_eq!(request_id, 2);
    assert_eq!(StoredState::from(bytes).entries[0].key, "inv:instance-b");
    harness
        .send(HostEvent::StorageResult((2, None)))
        .expect("first prune stored");

    harness
        .send(provider_data(2, &without_instance_b()))
        .expect("instance disappears");
    let (request_id, bytes) = storage_set(harness.output()).expect("persist second prune");
    assert_eq!(request_id, 3);
    assert!(StoredState::from(bytes).entries.is_empty());
}

#[test]
fn passive_mode_suppresses_completion_and_view_toggles() {
    let mut widget = InvasionsWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("invasions init");
    harness
        .send(HostEvent::StorageResult((1, None)))
        .expect("default state");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(provider_data(1, WORLDSTATE))
        .expect("invasion data");
    harness.set_mode(OverlayModeCode::Passive);
    for id in ["view-compact", "inv:instance-a", "inv:instance-b"] {
        assert!(matches!(
            harness.send(toggled(id, true)),
            Err(HarnessError::Passive)
        ));
    }
}

#[test]
fn maximum_view_is_bounded_and_malformed_progress_fails_closed() {
    let mut widget = InvasionsWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("invasions init");
    harness
        .send(HostEvent::StorageResult((1, None)))
        .expect("default state");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(provider_data(1, &payload_with_invasions(32)))
        .expect("maximum invasion view");
    assert_eq!(invasion_toggle_count(harness.output()), 32);
    assert_eq!(progress_values(harness.output()).len(), 32);

    harness
        .send(provider_data(2, &zero_goal_invasion()))
        .expect("invalid progress invalidates provider data");
    assert!(texts(harness.output()).contains(&"Worldstate data is unavailable."));
    harness
        .send(provider_data(3, WORLDSTATE))
        .expect("valid provider recovers");
    harness
        .send(HostEvent::Tick(NOW_MS + 300_000))
        .expect("exact stale boundary");
    assert!(texts(harness.output()).contains(&"Worldstate data is unavailable."));
}

#[test]
fn invasions_require_exact_authority_and_empty_settings() {
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
        let mut widget = InvasionsWidget::default();
        let mut input = init("en");
        input.granted_capabilities = capabilities;
        assert!(matches!(
            WidgetHarness::from_init(&mut widget, input),
            Err(GuestError::Unavailable)
        ));
    }
    let mut widget = InvasionsWidget::default();
    let mut input = init("en");
    input.settings = b"{}".to_vec();
    assert!(matches!(
        WidgetHarness::from_init(&mut widget, input),
        Err(GuestError::InvalidInput)
    ));
}

#[test]
fn invasion_storage_is_ordered_coalesced_and_not_overwritten_by_a_late_load() {
    let mut widget = InvasionsWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("invasions init");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(provider_data(1, WORLDSTATE))
        .expect("invasion data");
    harness
        .send(toggled("inv:instance-b", true))
        .expect("completion before load");
    assert_eq!(storage_set(harness.output()).map(|value| value.0), Some(2));

    harness
        .send(HostEvent::StorageResult((
            1,
            Some(stored_state(&["inv:instance-a"], true)),
        )))
        .expect("late load ignored");
    assert_eq!(toggle(harness.output(), "view-compact"), Some(false));
    assert_eq!(toggle(harness.output(), "inv:instance-a"), Some(false));
    assert_eq!(toggle(harness.output(), "inv:instance-b"), Some(true));

    harness
        .send(toggled("view-compact", true))
        .expect("coalesced view preference");
    assert!(storage_set(harness.output()).is_none());
    harness
        .send(HostEvent::StorageResult((2, None)))
        .expect("first store completes");
    let (request_id, bytes) = storage_set(harness.output()).expect("queued state stored");
    assert_eq!(request_id, 3);
    let stored = StoredState::from(bytes);
    assert!(stored.compact);
    assert_eq!(stored.entries[0].key, "inv:instance-b");
}

#[test]
fn malformed_storage_runtime_settings_time_and_session_fail_closed() {
    let mut widget = InvasionsWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("invasions init");
    harness
        .send(HostEvent::StorageResult((
            1,
            Some(oversized_stored_state()),
        )))
        .expect("oversized stored state defaults");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(provider_data(1, WORLDSTATE))
        .expect("invasion data");
    assert_eq!(toggle(harness.output(), "inv:instance-a"), Some(false));

    assert!(matches!(
        harness.send(HostEvent::SettingsChanged((1, b"{}".to_vec()))),
        Err(HarnessError::Widget(GuestError::InvalidInput))
    ));

    let mut widget = InvasionsWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("invasions init");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    assert!(matches!(
        harness.send(HostEvent::Tick(NOW_MS - 1)),
        Err(HarnessError::Widget(GuestError::InvalidInput))
    ));

    let mut widget = InvasionsWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("invasions init");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(provider_data(1, WORLDSTATE))
        .expect("invasion data");
    harness
        .send(HostEvent::SessionData(inactive_session()))
        .expect("inactive session");
    assert!(matches!(
        harness.send(toggled("inv:instance-a", true)),
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

fn reordered_invasions() -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(WORLDSTATE).expect("invasion fixture");
    value["invasions"]
        .as_array_mut()
        .expect("invasions")
        .reverse();
    serde_json::to_vec(&value).expect("invasion JSON")
}

fn without_instance_b() -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(WORLDSTATE).expect("invasion fixture");
    value["invasions"]
        .as_array_mut()
        .expect("invasions")
        .retain(|invasion| invasion["instanceId"] != "instance-b");
    serde_json::to_vec(&value).expect("invasion JSON")
}

fn zero_goal_invasion() -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(WORLDSTATE).expect("invasion fixture");
    value["invasions"][0]["goal"] = 0.into();
    serde_json::to_vec(&value).expect("invasion JSON")
}

fn payload_with_invasions(count: usize) -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(WORLDSTATE).expect("invasion fixture");
    value["invasions"] = (0..count)
        .map(|index| {
            json!({
                "instanceId": format!("instance-{index}"),
                "node": "Cassini · Saturn",
                "attackerFaction": "Grineer",
                "defenderFaction": "Corpus",
                "attackerReward": null,
                "defenderReward": null,
                "count": 25,
                "goal": 100,
                "completed": false
            })
        })
        .collect::<Vec<_>>()
        .into();
    serde_json::to_vec(&value).expect("invasion JSON")
}

fn stored_state(keys: &[&str], compact: bool) -> Vec<u8> {
    let entries = keys
        .iter()
        .map(|key| json!({"key": key, "completedAtSecs": NOW_MS / 1_000}))
        .collect::<Vec<_>>();
    serde_json::to_vec(&json!({
        "schemaVersion": 1,
        "compact": compact,
        "entries": entries
    }))
    .expect("stored state JSON")
}

fn oversized_stored_state() -> Vec<u8> {
    let keys = (0..33)
        .map(|index| format!("inv:instance-{index:02}"))
        .collect::<Vec<_>>();
    let entries = keys
        .iter()
        .map(|key| json!({"key": key, "completedAtSecs": NOW_MS / 1_000}))
        .collect::<Vec<_>>();
    serde_json::to_vec(&json!({
        "schemaVersion": 1,
        "compact": true,
        "entries": entries
    }))
    .expect("stored state JSON")
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

fn progress_values(output: &overcrow_widget_sdk::GuestOutput) -> Vec<u16> {
    output
        .view
        .iter()
        .flat_map(|view| &view.nodes)
        .filter_map(|node| match node {
            ViewNode::Progress((_, value)) => Some(*value),
            _ => None,
        })
        .collect()
}

fn invasion_toggle_count(output: &overcrow_widget_sdk::GuestOutput) -> usize {
    output
        .view
        .iter()
        .flat_map(|view| &view.nodes)
        .filter(|node| matches!(node, ViewNode::Toggle((id, _, _)) if id.starts_with("inv:")))
        .count()
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
struct StoredState {
    compact: bool,
    entries: Vec<StoredEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredEntry {
    key: String,
    completed_at_secs: u64,
}

impl StoredState {
    fn from(bytes: &[u8]) -> Self {
        serde_json::from_slice(bytes).expect("stored invasion state")
    }
}
