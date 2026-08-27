use overcrow_widget_sdk::{
    GrantedCapabilities, GuestError, HarnessError, HostCommand, HostEvent, InitInput, Interaction,
    InteractionKind, OverlayModeCode, SessionData, ViewNode, WidgetHarness,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{FissuresWidget, PROVIDER_ID, PROVIDER_SCHEMA};

const WORLDSTATE: &[u8] = include_bytes!("../../fixtures/worldstate-v1.json");
const NOW_MS: u64 = 1_777_000_000_000;

#[test]
fn filters_render_and_persist_with_coalesced_storage_writes() {
    let mut widget = FissuresWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("fissures init");
    assert_eq!(storage_get(harness.output()), Some((1, "filters")));
    harness
        .send(HostEvent::StorageResult((1, None)))
        .expect("missing preferences use defaults");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(provider_data(1, WORLDSTATE))
        .expect("valid provider data");
    assert_eq!(toggle(harness.output(), "era-axi"), Some(true));
    assert!(
        texts(harness.output())
            .iter()
            .any(|text| text.contains("Axi · Defense · Galatea · Neptune · 15 min"))
    );
    assert!(
        texts(harness.output())
            .iter()
            .any(|text| text.contains("Lith · Railjack · Void Storm"))
    );
    assert!(
        !texts(harness.output())
            .iter()
            .any(|text| text.contains("Neo ·"))
    );

    harness
        .send(toggled("era-axi", false))
        .expect("disable Axi");
    let (request_id, key, bytes) = storage_set(harness.output()).expect("persist filters");
    assert_eq!((request_id, key), (2, "filters"));
    assert!(!StoredFilters::from(bytes).axi);
    assert!(
        !texts(harness.output())
            .iter()
            .any(|text| text.contains("Axi ·"))
    );

    harness
        .send(toggled("source-railjack", false))
        .expect("coalesce while store is pending");
    assert!(storage_set(harness.output()).is_none());
    harness
        .send(HostEvent::StorageResult((2, None)))
        .expect("first store completes");
    let (request_id, _, bytes) = storage_set(harness.output()).expect("latest filters persisted");
    assert_eq!(request_id, 3);
    let stored = StoredFilters::from(bytes);
    assert!(!stored.axi);
    assert!(!stored.railjack);
}

#[test]
fn french_and_default_locale_views_are_complete() {
    let mut widget = FissuresWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("fr")).expect("fissures init");
    harness
        .send(HostEvent::StorageResult((1, None)))
        .expect("default filters");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(provider_data(1, WORLDSTATE))
        .expect("provider data");
    assert!(texts(harness.output()).contains(&"Fissures du Néant"));
    assert_eq!(
        toggle_label(harness.output(), "source-normal"),
        Some("Carte stellaire")
    );

    harness
        .send(HostEvent::LocaleChanged("de".to_owned()))
        .expect("default fallback");
    assert!(texts(harness.output()).contains(&"Void Fissures"));
    assert_eq!(
        toggle_label(harness.output(), "source-normal"),
        Some("Star Chart")
    );
}

#[test]
fn passive_mode_suppresses_every_filter_interaction() {
    let mut widget = FissuresWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("fissures init");
    harness.set_mode(OverlayModeCode::Passive);
    for id in [
        "era-lith",
        "era-meso",
        "era-neo",
        "era-axi",
        "era-requiem",
        "era-omnia",
        "source-normal",
        "source-railjack",
    ] {
        assert!(matches!(
            harness.send(toggled(id, false)),
            Err(HarnessError::Passive)
        ));
    }
}

#[test]
fn malformed_storage_and_provider_data_fail_closed_without_stale_reuse() {
    let mut widget = FissuresWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("fissures init");
    harness
        .send(HostEvent::StorageResult((1, Some(b"not-json".to_vec()))))
        .expect("malformed preferences use safe defaults");
    assert_eq!(toggle(harness.output(), "era-axi"), Some(true));
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(provider_data(2, WORLDSTATE))
        .expect("fresh provider data");
    harness
        .send(provider_data(1, b"not-json"))
        .expect("old revision ignored");
    assert!(
        texts(harness.output())
            .iter()
            .any(|text| text.contains("Axi ·"))
    );
    harness
        .send(provider_data(3, b"{}"))
        .expect("new malformed revision invalidates data");
    assert!(texts(harness.output()).contains(&"Worldstate data is unavailable."));
    harness
        .send(provider_data(4, WORLDSTATE))
        .expect("valid revision recovers");
    harness
        .send(HostEvent::Tick(NOW_MS + 300_000))
        .expect("exact stale boundary");
    assert!(texts(harness.output()).contains(&"Worldstate data is unavailable."));
}

#[test]
fn fissures_requires_exact_session_and_storage_authority() {
    for capabilities in [
        GrantedCapabilities {
            storage: false,
            ..storage_grants()
        },
        GrantedCapabilities {
            http_hosts: vec!["api.warframe.com".to_owned()],
            ..storage_grants()
        },
        GrantedCapabilities {
            provider: true,
            ..storage_grants()
        },
        GrantedCapabilities {
            game_data: Vec::new(),
            ..storage_grants()
        },
        GrantedCapabilities {
            game_data: vec!["example.invalid".to_owned()],
            ..storage_grants()
        },
    ] {
        let mut widget = FissuresWidget::default();
        let mut input = init("en");
        input.granted_capabilities = capabilities;
        assert!(matches!(
            WidgetHarness::from_init(&mut widget, input),
            Err(GuestError::Unavailable)
        ));
    }
}

#[test]
fn fissures_rejects_undeclared_settings() {
    let mut widget = FissuresWidget::default();
    let mut input = init("en");
    input.settings = b"{}".to_vec();
    assert!(matches!(
        WidgetHarness::from_init(&mut widget, input),
        Err(GuestError::InvalidInput)
    ));

    let mut widget = FissuresWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("fissures init");
    assert!(matches!(
        harness.send(HostEvent::SettingsChanged((1, b"{}".to_vec()))),
        Err(HarnessError::Widget(GuestError::InvalidInput))
    ));
}

#[test]
fn maximum_and_empty_fissure_views_remain_bounded() {
    let mut widget = FissuresWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("fissures init");
    harness
        .send(HostEvent::StorageResult((1, None)))
        .expect("default filters");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(provider_data(1, &payload_with_fissures(96)))
        .expect("maximum fissure view");
    assert_eq!(
        texts(harness.output())
            .into_iter()
            .filter(|text| text.contains("Axi · Defense · Galatea · Neptune"))
            .count(),
        96
    );

    harness
        .send(provider_data(2, &payload_with_fissures(0)))
        .expect("empty fissure view");
    assert!(texts(harness.output()).contains(&"No fissures match these filters."));
}

#[test]
fn filter_load_is_versioned_and_cannot_overwrite_newer_interaction() {
    let mut widget = FissuresWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("fissures init");
    harness
        .send(toggled("era-axi", false))
        .expect("new preference before load completes");
    assert_eq!(storage_set(harness.output()).map(|value| value.0), Some(2));
    harness
        .send(HostEvent::StorageResult((1, Some(stored_filters(1, true)))))
        .expect("late load response");
    assert_eq!(toggle(harness.output(), "era-axi"), Some(false));
    harness
        .send(HostEvent::StorageResult((2, None)))
        .expect("preference store completes");

    let mut widget = FissuresWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("fissures init");
    harness
        .send(HostEvent::StorageResult((
            1,
            Some(stored_filters(2, false)),
        )))
        .expect("unknown preference version defaults");
    assert_eq!(toggle(harness.output(), "era-axi"), Some(true));
}

fn init(locale: &str) -> InitInput {
    InitInput {
        locale: locale.to_owned(),
        granted_capabilities: storage_grants(),
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

fn storage_grants() -> GrantedCapabilities {
    GrantedCapabilities {
        http_hosts: Vec::new(),
        game_data: vec!["overcrow.session.v1".to_owned()],
        storage: true,
        clipboard_write: false,
        provider: false,
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

fn payload_with_fissures(count: usize) -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(WORLDSTATE).expect("worldstate fixture");
    value["fissures"] = (0..count)
        .map(|index| {
            json!({
                "instanceId": format!("fissure-{index}"),
                "era": "axi",
                "missionType": "Defense",
                "node": "Galatea · Neptune",
                "expiresAtSecs": 1_777_000_900_u64,
                "steelPath": false,
                "railjack": false
            })
        })
        .collect::<Vec<_>>()
        .into();
    serde_json::to_vec(&value).expect("worldstate JSON")
}

fn stored_filters(schema_version: u8, axi: bool) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schemaVersion": schema_version,
        "lith": true,
        "meso": true,
        "neo": true,
        "axi": axi,
        "requiem": true,
        "omnia": true,
        "normal": true,
        "railjack": true
    }))
    .expect("stored filters JSON")
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

fn storage_set(output: &overcrow_widget_sdk::GuestOutput) -> Option<(u32, &str, &[u8])> {
    output.commands.iter().find_map(|command| match command {
        HostCommand::StorageSet((request_id, key, bytes)) => {
            Some((*request_id, key.as_str(), bytes.as_slice()))
        }
        _ => None,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredFilters {
    axi: bool,
    railjack: bool,
}

impl StoredFilters {
    fn from(bytes: &[u8]) -> Self {
        serde_json::from_slice(bytes).expect("stored filters schema")
    }
}
