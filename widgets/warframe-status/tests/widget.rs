use overcrow_widget_sdk::{
    GrantedCapabilities, GuestError, HarnessError, HostEvent, InitInput, OverlayModeCode,
    SessionData, ViewNode, WidgetHarness,
};

use super::{PROVIDER_ID, PROVIDER_SCHEMA, StatusWidget, status_text};
use warframe_widget_data::StatusRow;

const WORLDSTATE: &[u8] = include_bytes!("../../fixtures/worldstate-v1.json");
const NOW_MS: u64 = 1_777_000_000_000;

#[test]
fn fresh_status_is_directly_readable_in_english_and_french() {
    let mut widget = StatusWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("status init");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(provider_data(1, WORLDSTATE))
        .expect("valid provider data");
    let visible_text = texts(harness.output());
    assert!(visible_text.contains(&"Warframe Status"));
    assert!(visible_text.contains(&"Cetus · Day · 15 min"));
    assert!(visible_text.contains(&"Baro Ki'Teer · Present · Larunda Relay · Mercury · 60 min"));

    harness
        .send(HostEvent::LocaleChanged("fr".to_owned()))
        .expect("French locale");
    let visible_text = texts(harness.output());
    assert!(visible_text.contains(&"Statut Warframe"));
    assert!(visible_text.contains(&"Cetus · Jour · 15 min"));
    harness
        .send(HostEvent::LocaleChanged("de".to_owned()))
        .expect("default locale fallback");
    assert!(texts(harness.output()).contains(&"Warframe Status"));
}

#[test]
fn status_ignores_old_revisions_and_fails_closed_when_data_expires() {
    let mut widget = StatusWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("status init");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(provider_data(2, WORLDSTATE))
        .expect("current provider data");
    harness
        .send(provider_data(1, b"not-json"))
        .expect("older revision ignored");
    assert!(
        texts(harness.output())
            .iter()
            .any(|text| text.contains("Cetus"))
    );

    harness
        .send(HostEvent::Tick(NOW_MS + 299_999))
        .expect("last fresh millisecond");
    assert!(
        !texts(harness.output())
            .iter()
            .any(|text| text.contains("unavailable"))
    );
    harness
        .send(HostEvent::Tick(NOW_MS + 300_000))
        .expect("stale view");
    assert!(texts(harness.output()).contains(&"Worldstate data is unavailable."));

    harness
        .send(provider_data(3, b"{}"))
        .expect("malformed newer data becomes unavailable");
    assert!(texts(harness.output()).contains(&"Worldstate data is unavailable."));
}

#[test]
fn status_requires_exact_session_only_authority_and_provider_identity() {
    for capabilities in [
        GrantedCapabilities {
            http_hosts: vec!["api.warframe.com".to_owned()],
            ..session_grants()
        },
        GrantedCapabilities {
            storage: true,
            ..session_grants()
        },
        GrantedCapabilities {
            provider: true,
            ..session_grants()
        },
        GrantedCapabilities {
            game_data: Vec::new(),
            ..session_grants()
        },
        GrantedCapabilities {
            game_data: vec!["example.invalid".to_owned()],
            ..session_grants()
        },
    ] {
        let mut widget = StatusWidget::default();
        let mut input = init("en");
        input.granted_capabilities = capabilities;
        assert!(matches!(
            WidgetHarness::from_init(&mut widget, input),
            Err(GuestError::Unavailable)
        ));
    }

    let mut widget = StatusWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("status init");
    assert!(matches!(
        harness.send(HostEvent::ProviderData((
            "example.invalid".to_owned(),
            PROVIDER_SCHEMA.to_owned(),
            1,
            WORLDSTATE.to_vec(),
        ))),
        Err(HarnessError::Widget(GuestError::InvalidInput))
    ));
}

#[test]
fn status_rejects_undeclared_settings() {
    let mut widget = StatusWidget::default();
    let mut input = init("en");
    input.settings = b"{}".to_vec();
    assert!(matches!(
        WidgetHarness::from_init(&mut widget, input),
        Err(GuestError::InvalidInput)
    ));

    let mut widget = StatusWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("status init");
    assert!(matches!(
        harness.send(HostEvent::SettingsChanged((1, b"{}".to_vec()))),
        Err(HarnessError::Widget(GuestError::InvalidInput))
    ));
}

#[test]
fn incoming_baro_counts_down_to_arrival_then_switches_to_departure() {
    let row = StatusRow {
        id: "baro".to_owned(),
        state: Some("incoming".to_owned()),
        activation_secs: Some(1_060),
        expires_at_secs: 4_660,
        location: Some("Larunda Relay".to_owned()),
    };

    assert_eq!(
        status_text(&row, 1_000_000).map(|labels| labels.0),
        Some("Baro Ki'Teer · Incoming · Larunda Relay · 1 min".to_owned())
    );
    assert_eq!(
        status_text(&row, 1_060_000).map(|labels| labels.0),
        Some("Baro Ki'Teer · Present · Larunda Relay · 60 min".to_owned())
    );
}

fn init(locale: &str) -> InitInput {
    InitInput {
        locale: locale.to_owned(),
        granted_capabilities: session_grants(),
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

fn session_grants() -> GrantedCapabilities {
    GrantedCapabilities {
        http_hosts: Vec::new(),
        game_data: vec!["overcrow.session.v1".to_owned()],
        storage: false,
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
