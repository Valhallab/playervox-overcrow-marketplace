use overcrow_widget_sdk::{
    GrantedCapabilities, GuestError, HarnessError, HostCommand, HostEvent, HttpResponseMetadata,
    InitInput, Interaction, InteractionKind, OverlayModeCode, SessionData, ViewNode, WidgetHarness,
};
use serde::Deserialize;
use serde_json::json;

use super::{WarframeMarketWidget, parse::parse_orders};

const ITEMS: &[u8] = include_bytes!("fixtures/items.json");
const ORDERS: &[u8] = include_bytes!("fixtures/orders.json");
const HOSTILE: &[u8] = include_bytes!("fixtures/hostile.json");
const NOW_MS: u64 = 1_777_000_000_000;

#[test]
fn submitted_query_uses_only_the_declared_items_request() {
    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    assert_eq!(storage_get(harness.output()), Some((1, "state")));
    harness
        .send(HostEvent::StorageResult((1, None)))
        .expect("default state");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");

    harness
        .send(interaction(
            "market-query",
            InteractionKind::Submitted("  arcane énergize  ".to_owned()),
        ))
        .expect("submit query");
    assert_eq!(
        text_input(harness.output(), "market-query"),
        Some("arcane énergize")
    );
    assert_eq!(
        http_get(harness.output()),
        Some((2, "api.warframe.market", "/v2/items"))
    );
    assert!(clipboard(harness.output()).is_none());
}

#[test]
fn item_selection_orders_and_clipboard_use_stable_public_identity() {
    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    bootstrap(&mut harness);
    load_items(&mut harness, "arcane", 2);
    let energize_id = button_id(harness.output(), "Arcane Energize").expect("energize result");
    assert!(energize_id.starts_with("item-"));

    harness
        .send(interaction(&energize_id, InteractionKind::Clicked))
        .expect("select item");
    assert!(http_get(harness.output()).is_none());
    assert!(storage_set(harness.output()).is_some());
    harness
        .send(HostEvent::Tick(NOW_MS + 15_000))
        .expect("host cadence");
    let (orders_request, host, path) = http_get(harness.output()).expect("orders request");
    assert_eq!(host, "api.warframe.market");
    assert_eq!(path, "/v2/orders/item/arcane_energize");
    harness
        .send(http_result(orders_request, Some(200), ORDERS))
        .expect("orders response");
    assert!(texts(harness.output()).contains(&"Arcane Energize"));
    assert!(texts(harness.output()).contains(&"SellerOne · online · 100p"));
    assert!(
        !texts(harness.output())
            .iter()
            .any(|text| text.contains("Hidden"))
    );

    let whisper = button_id(harness.output(), "Whisper SellerOne").expect("seller action");
    assert!(whisper.starts_with("order-"));
    harness
        .send(interaction(&whisper, InteractionKind::Clicked))
        .expect("copy whisper");
    assert_eq!(
        clipboard(harness.output()),
        Some("/w SellerOne Hi, WTB Arcane Energize for 100p")
    );
    assert!(http_get(harness.output()).is_none());
}

#[test]
fn passive_mode_suppresses_search_selection_and_trade_actions() {
    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    bootstrap(&mut harness);
    load_items(&mut harness, "arcane", 2);
    let result = button_id(harness.output(), "Arcane Energize").expect("result");
    harness
        .send(interaction(&result, InteractionKind::Clicked))
        .expect("select item");
    harness
        .send(HostEvent::Tick(NOW_MS + 15_000))
        .expect("host cadence");
    let request_id = http_get(harness.output()).expect("orders request").0;
    harness
        .send(http_result(request_id, Some(200), ORDERS))
        .expect("orders response");
    let whisper = button_id(harness.output(), "Whisper SellerOne").expect("trade action");
    harness.set_mode(OverlayModeCode::Passive);

    for event in [
        interaction(
            "market-query",
            InteractionKind::Submitted("forma".to_owned()),
        ),
        interaction(&result, InteractionKind::Clicked),
        interaction(&whisper, InteractionKind::Clicked),
    ] {
        assert!(matches!(harness.send(event), Err(HarnessError::Passive)));
    }
}

#[test]
fn passive_session_stops_network_and_wakes_until_interactive() {
    let mut widget = WarframeMarketWidget::default();
    let mut input = init("en");
    input.session_data = Some(session_with_mode(true, OverlayModeCode::Passive));
    let mut harness = WidgetHarness::from_init(&mut widget, input).expect("passive market");
    harness
        .send(HostEvent::StorageResult((1, Some(stored_state()))))
        .expect("stored selection");
    harness.send(HostEvent::Tick(NOW_MS)).expect("passive time");
    assert!(http_get(harness.output()).is_none());
    assert!(harness.output().next_wake_ms.is_none());

    harness
        .send(HostEvent::SessionData(session_with_mode(
            true,
            OverlayModeCode::Interactive,
        )))
        .expect("interactive transition");
    let request_id = http_get(harness.output()).expect("interactive orders").0;
    harness
        .send(http_result(request_id, Some(200), ORDERS))
        .expect("orders response");
    harness
        .send(HostEvent::SessionData(session_with_mode(
            true,
            OverlayModeCode::Passive,
        )))
        .expect("passive transition");
    harness
        .send(HostEvent::Tick(NOW_MS + 30_000))
        .expect("passive refresh boundary");
    assert!(http_get(harness.output()).is_none());
    assert!(harness.output().next_wake_ms.is_none());
}

#[test]
fn late_orders_are_discarded_and_refresh_cadence_is_exact() {
    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    bootstrap(&mut harness);
    load_items(&mut harness, "arcane", 2);
    let energize = button_id(harness.output(), "Arcane Energize").expect("energize result");
    let grace = button_id(harness.output(), "Arcane Grace").expect("grace result");
    harness
        .send(interaction(&energize, InteractionKind::Clicked))
        .expect("first selection");
    harness
        .send(HostEvent::Tick(NOW_MS + 15_000))
        .expect("host cadence");
    let first_request = http_get(harness.output()).expect("first orders").0;
    harness
        .send(interaction(&grace, InteractionKind::Clicked))
        .expect("new selection while pending");
    assert!(http_get(harness.output()).is_none());

    harness
        .send(http_result(first_request, Some(200), ORDERS))
        .expect("late first orders");
    assert!(http_get(harness.output()).is_none());
    harness
        .send(HostEvent::Tick(NOW_MS + 44_999))
        .expect("before orders cadence");
    assert!(http_get(harness.output()).is_none());
    harness
        .send(HostEvent::Tick(NOW_MS + 45_000))
        .expect("exact orders cadence");
    let (second_request, _, path) = http_get(harness.output()).expect("latest selection request");
    assert_eq!(path, "/v2/orders/item/arcane_grace");
    assert!(!texts(harness.output()).contains(&"SellerOne · online · 100p"));
    harness
        .send(http_result(second_request, Some(200), ORDERS))
        .expect("latest orders");
    assert!(texts(harness.output()).contains(&"Arcane Grace"));

    harness
        .send(HostEvent::Tick(NOW_MS + 74_999))
        .expect("before refresh cadence");
    assert!(http_get(harness.output()).is_none());
    harness
        .send(HostEvent::Tick(NOW_MS + 75_000))
        .expect("exact refresh cadence");
    assert_eq!(
        http_get(harness.output()).map(|value| value.2),
        Some("/v2/orders/item/arcane_grace")
    );
}

#[test]
fn storage_is_bounded_versioned_coalesced_and_excludes_queries() {
    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(HostEvent::StorageResult((1, Some(stored_state()))))
        .expect("stored preferences");
    assert_eq!(selection(harness.output(), "filter-side"), Some(1));
    assert_eq!(selection(harness.output(), "filter-platform"), None);
    assert_eq!(toggle(harness.output(), "watch-selected"), Some(true));
    assert!(button_id(harness.output(), "primed flow").is_some());
    assert_eq!(
        http_get(harness.output()).map(|value| value.2),
        Some("/v2/orders/item/arcane_energize")
    );

    harness
        .send(interaction(
            "filter-status",
            InteractionKind::SelectionChanged(2),
        ))
        .expect("change status filter");
    let (store_id, bytes) = storage_set(harness.output()).expect("persist preferences");
    let stored = Stored::from(bytes);
    assert_eq!(stored.status, "any");
    assert!(!String::from_utf8_lossy(bytes).contains("market-query"));
    assert!(!String::from_utf8_lossy(bytes).contains("private search"));

    harness
        .send(interaction(
            "watch-selected",
            InteractionKind::Toggled(false),
        ))
        .expect("coalesced watchlist change");
    assert!(storage_set(harness.output()).is_none());
    harness
        .send(HostEvent::StorageResult((store_id, None)))
        .expect("first store complete");
    let (_, bytes) = storage_set(harness.output()).expect("queued store");
    assert_eq!(Stored::from(bytes).watchlist, ["primed_flow"]);

    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    harness
        .send(HostEvent::StorageResult((1, Some(oversized_state()))))
        .expect("oversized state defaults");
    assert_eq!(selection(harness.output(), "filter-side"), Some(0));
}

#[test]
fn watchlist_entries_are_visible_and_can_be_reselected() {
    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(HostEvent::StorageResult((1, Some(stored_state()))))
        .expect("stored watchlist");
    let first_request = http_get(harness.output())
        .expect("stored selection request")
        .0;
    let watched = button_id(harness.output(), "primed flow").expect("watched item");
    assert!(watched.starts_with("watch-"));

    harness
        .send(interaction(&watched, InteractionKind::Clicked))
        .expect("select watched item");
    assert!(storage_set(harness.output()).is_some());
    assert!(http_get(harness.output()).is_none());
    harness
        .send(http_result(first_request, Some(200), ORDERS))
        .expect("discard old selection");
    assert!(http_get(harness.output()).is_none());
    harness
        .send(HostEvent::Tick(NOW_MS + 30_000))
        .expect("orders cadence");
    assert_eq!(
        http_get(harness.output()).map(|value| value.2),
        Some("/v2/orders/item/primed_flow")
    );
}

#[test]
fn late_storage_load_cannot_starve_the_selected_item_request() {
    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(interaction(
            "market-query",
            InteractionKind::Submitted("arcane".to_owned()),
        ))
        .expect("search while storage is pending");
    let items_request = http_get(harness.output()).expect("items request").0;

    harness
        .send(HostEvent::StorageResult((1, Some(stored_state()))))
        .expect("late preferences");
    assert!(http_get(harness.output()).is_none());
    harness
        .send(http_result(items_request, Some(200), ITEMS))
        .expect("items complete");
    assert!(http_get(harness.output()).is_none());
    harness
        .send(HostEvent::Tick(NOW_MS + 15_000))
        .expect("host cadence");
    assert_eq!(
        http_get(harness.output()).map(|value| value.2),
        Some("/v2/orders/item/arcane_energize")
    );
}

#[test]
fn malformed_timeout_and_stale_responses_fail_closed() {
    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    bootstrap(&mut harness);
    harness
        .send(interaction(
            "market-query",
            InteractionKind::Submitted("arcane".to_owned()),
        ))
        .expect("submit query");
    let request_id = http_get(harness.output()).expect("items request").0;
    harness
        .send(http_result(request_id, Some(200), HOSTILE))
        .expect("malformed items response");
    assert!(texts(harness.output()).contains(&"Market data is unavailable."));

    harness
        .send(interaction(
            "market-query",
            InteractionKind::Submitted("forma".to_owned()),
        ))
        .expect("retry query");
    assert!(http_get(harness.output()).is_none());

    harness
        .send(HostEvent::Tick(NOW_MS + 15_000))
        .expect("retry cadence");
    let request_id = http_get(harness.output()).expect("retry items").0;
    harness
        .send(http_result(request_id, None, b"private timeout body"))
        .expect("timeout result");
    assert!(texts(harness.output()).contains(&"Market request failed."));

    harness
        .send(interaction(
            "market-query",
            InteractionKind::Submitted("arcane".to_owned()),
        ))
        .expect("load valid catalog");
    harness
        .send(HostEvent::Tick(NOW_MS + 30_000))
        .expect("catalog cadence");
    let request_id = http_get(harness.output()).expect("valid items request").0;
    harness
        .send(http_result(request_id, Some(200), ITEMS))
        .expect("valid items response");
    let result = button_id(harness.output(), "Arcane Energize").expect("result");
    harness
        .send(interaction(&result, InteractionKind::Clicked))
        .expect("select item");
    harness
        .send(HostEvent::Tick(NOW_MS + 45_000))
        .expect("orders cadence");
    let request_id = http_get(harness.output()).expect("orders request").0;
    harness
        .send(http_result(request_id, Some(200), ORDERS))
        .expect("orders response");
    harness
        .send(HostEvent::Tick(NOW_MS + 165_000))
        .expect("orders become stale");
    assert!(texts(harness.output()).contains(&"Market data is stale."));
    assert!(!texts(harness.output()).contains(&"SellerOne · online · 100p"));
}

#[test]
fn maximum_views_queries_slugs_and_order_ranking_are_bounded() {
    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    bootstrap(&mut harness);
    harness
        .send(interaction(
            "market-query",
            InteractionKind::Submitted("item".to_owned()),
        ))
        .expect("submit maximum query");
    let request_id = http_get(harness.output()).expect("items request").0;
    harness
        .send(http_result(request_id, Some(200), &items_payload(50_000)))
        .expect("maximum item catalog");
    assert_eq!(result_button_count(harness.output()), 12);

    let first = first_result_button(harness.output()).expect("first result");
    harness
        .send(interaction(&first, InteractionKind::Clicked))
        .expect("select maximum result");
    harness
        .send(HostEvent::Tick(NOW_MS + 15_000))
        .expect("host cadence");
    let request_id = http_get(harness.output()).expect("orders request").0;
    harness
        .send(http_result(request_id, Some(200), &orders_payload(1_507)))
        .expect("representative public orders");
    assert_eq!(order_button_count(harness.output()), 12);

    assert!(matches!(
        harness.send(interaction(
            "market-query",
            InteractionKind::Submitted("é".repeat(65)),
        )),
        Err(HarnessError::Widget(GuestError::InvalidInput))
    ));
}

#[test]
fn conflicting_duplicates_and_oversized_arrays_fail_closed() {
    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    bootstrap(&mut harness);
    harness
        .send(interaction(
            "market-query",
            InteractionKind::Submitted("arcane".to_owned()),
        ))
        .expect("submit query");
    let request_id = http_get(harness.output()).expect("items request").0;
    let conflicting = serde_json::to_vec(&json!({"data": [
        {"slug": "arcane_energize", "i18n": {"en": {"name": "Arcane Energize"}}},
        {"slug": "arcane_energize", "i18n": {"en": {"name": "Different"}}}
    ]}))
    .expect("conflicting items");
    harness
        .send(http_result(request_id, Some(200), &conflicting))
        .expect("reject conflicting items");
    assert!(texts(harness.output()).contains(&"Market data is unavailable."));

    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    bootstrap(&mut harness);
    harness
        .send(interaction(
            "market-query",
            InteractionKind::Submitted("item".to_owned()),
        ))
        .expect("submit oversized catalog");
    let request_id = http_get(harness.output()).expect("items request").0;
    harness
        .send(http_result(request_id, Some(200), &items_payload(50_001)))
        .expect("reject oversized catalog");
    assert_eq!(result_button_count(harness.output()), 0);
    assert!(texts(harness.output()).contains(&"Market data is unavailable."));
}

#[test]
fn official_order_shape_and_order_array_limit_are_enforced() {
    assert_eq!(parse_orders(ORDERS).expect("official order shape").len(), 4);
    assert_eq!(
        parse_orders(&orders_payload(1_507))
            .expect("representative public response")
            .len(),
        1_507
    );
    assert!(parse_orders(&orders_payload(4_097)).is_err());
}

#[test]
fn host_time_must_be_bounded_and_monotonic() {
    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    harness.send(HostEvent::Tick(NOW_MS)).expect("first time");
    assert!(matches!(
        harness.send(HostEvent::Tick(NOW_MS - 1)),
        Err(HarnessError::Widget(GuestError::InvalidInput))
    ));

    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    assert!(matches!(
        harness.send(HostEvent::Tick(u64::MAX)),
        Err(HarnessError::Widget(GuestError::InvalidInput))
    ));
}

#[test]
fn restored_selection_waits_for_time_and_does_not_loop_on_response() {
    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    harness
        .send(HostEvent::StorageResult((1, Some(stored_state()))))
        .expect("preferences before time");
    assert!(http_get(harness.output()).is_none());

    harness.send(HostEvent::Tick(NOW_MS)).expect("first time");
    let request_id = http_get(harness.output()).expect("first orders").0;
    harness
        .send(http_result(request_id, Some(200), ORDERS))
        .expect("orders response");
    assert!(http_get(harness.output()).is_none());
}

#[test]
fn exact_authority_locales_settings_and_active_session_are_enforced() {
    for capabilities in [
        GrantedCapabilities {
            http_hosts: Vec::new(),
            ..grants()
        },
        GrantedCapabilities {
            http_hosts: vec!["api.warframe.market".to_owned(), "evil.invalid".to_owned()],
            ..grants()
        },
        GrantedCapabilities {
            game_data: Vec::new(),
            ..grants()
        },
        GrantedCapabilities {
            storage: false,
            ..grants()
        },
        GrantedCapabilities {
            clipboard_write: false,
            ..grants()
        },
        GrantedCapabilities {
            provider: true,
            ..grants()
        },
    ] {
        let mut widget = WarframeMarketWidget::default();
        let mut input = init("en");
        input.granted_capabilities = capabilities;
        assert!(matches!(
            WidgetHarness::from_init(&mut widget, input),
            Err(GuestError::Unavailable)
        ));
    }

    let mut widget = WarframeMarketWidget::default();
    let mut input = init("en");
    input.settings = b"{}".to_vec();
    assert!(matches!(
        WidgetHarness::from_init(&mut widget, input),
        Err(GuestError::InvalidInput)
    ));

    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("fr")).expect("French market");
    assert!(texts(harness.output()).contains(&"Marché Warframe"));
    harness
        .send(HostEvent::LocaleChanged("de".to_owned()))
        .expect("default locale fallback");
    assert!(texts(harness.output()).contains(&"Warframe Market"));
    assert!(matches!(
        harness.send(HostEvent::SettingsChanged((1, b"{}".to_vec()))),
        Err(HarnessError::Widget(GuestError::InvalidInput))
    ));

    let mut widget = WarframeMarketWidget::default();
    let mut inactive = init("en");
    inactive.session_data = Some(session(false));
    let harness = WidgetHarness::from_init(&mut widget, inactive).expect("inactive market");
    assert!(text_input(harness.output(), "market-query").is_none());
    assert!(texts(harness.output()).contains(&"Waiting for an active Warframe session."));
}

fn init(locale: &str) -> InitInput {
    InitInput {
        locale: locale.to_owned(),
        granted_capabilities: grants(),
        settings: Vec::new(),
        session_data: Some(session(true)),
    }
}

fn grants() -> GrantedCapabilities {
    GrantedCapabilities {
        http_hosts: vec!["api.warframe.market".to_owned()],
        game_data: vec!["overcrow.session.v1".to_owned()],
        storage: true,
        clipboard_write: true,
        provider: false,
    }
}

fn session(selected_active: bool) -> SessionData {
    session_with_mode(selected_active, OverlayModeCode::Interactive)
}

fn session_with_mode(selected_active: bool, overlay_mode: OverlayModeCode) -> SessionData {
    SessionData {
        selected_active,
        steam_app_id: Some(230_410),
        session_elapsed_ms: Some(1_000),
        overlay_mode,
        cpu_percent_hundredths: None,
        resident_bytes: None,
        cpu_temperature_millicelsius: None,
        gpu_temperature_millicelsius: None,
    }
}

fn bootstrap(harness: &mut WidgetHarness<'_, WarframeMarketWidget>) {
    harness
        .send(HostEvent::StorageResult((1, None)))
        .expect("default market state");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
}

fn load_items(
    harness: &mut WidgetHarness<'_, WarframeMarketWidget>,
    query: &str,
    expected_request: u32,
) {
    harness
        .send(interaction(
            "market-query",
            InteractionKind::Submitted(query.to_owned()),
        ))
        .expect("submit item search");
    let request_id = http_get(harness.output()).expect("items request").0;
    assert_eq!(request_id, expected_request);
    harness
        .send(http_result(request_id, Some(200), ITEMS))
        .expect("items response");
}

fn interaction(id: &str, kind: InteractionKind) -> HostEvent {
    HostEvent::Interaction(Interaction {
        element_id: id.to_owned(),
        kind,
    })
}

fn http_result(request_id: u32, status: Option<u16>, body: &[u8]) -> HostEvent {
    HostEvent::HttpResult((
        request_id,
        status,
        body.to_vec(),
        HttpResponseMetadata {
            content_type: Some("application/json".to_owned()),
            headers: Vec::new(),
        },
    ))
}

fn stored_state() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schemaVersion": 1,
        "side": "sellers",
        "status": "online",
        "selectedSlug": "arcane_energize",
        "watchlist": ["arcane_energize", "primed_flow"]
    }))
    .expect("stored state")
}

fn oversized_state() -> Vec<u8> {
    let watchlist = (0..17)
        .map(|index| format!("item_{index:02}"))
        .collect::<Vec<_>>();
    serde_json::to_vec(&json!({
        "schemaVersion": 1,
        "side": "all",
        "status": "online",
        "selectedSlug": null,
        "watchlist": watchlist
    }))
    .expect("oversized state")
}

fn items_payload(count: usize) -> Vec<u8> {
    let data = (0..count)
        .map(|index| {
            json!({
                "slug": format!("item_{index:05}"),
                "i18n": {"en": {"name": format!("Item {index:05}")}}
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_vec(&json!({"data": data})).expect("items payload")
}

fn orders_payload(count: usize) -> Vec<u8> {
    let data = (0..count)
        .map(|index| {
            json!({
                "id": format!("order-{index:04}"),
                "type": if index % 2 == 0 {"sell"} else {"buy"},
                "platinum": 10 + index,
                "visible": true,
                "user": {
                    "ingameName": format!("Trader{index:04}"),
                    "status": if index % 3 == 0 {"ingame"} else {"online"},
                    "platform": "pc"
                }
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_vec(&json!({"data": data})).expect("orders payload")
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

fn text_input<'a>(output: &'a overcrow_widget_sdk::GuestOutput, expected: &str) -> Option<&'a str> {
    output
        .view
        .iter()
        .flat_map(|view| &view.nodes)
        .find_map(|node| match node {
            ViewNode::TextInput((id, value, _)) if id == expected => Some(value.as_str()),
            _ => None,
        })
}

fn button_id(output: &overcrow_widget_sdk::GuestOutput, label: &str) -> Option<String> {
    output
        .view
        .iter()
        .flat_map(|view| &view.nodes)
        .find_map(|node| match node {
            ViewNode::Button((id, candidate)) if candidate == label => Some(id.clone()),
            _ => None,
        })
}

fn first_result_button(output: &overcrow_widget_sdk::GuestOutput) -> Option<String> {
    output
        .view
        .iter()
        .flat_map(|view| &view.nodes)
        .find_map(|node| match node {
            ViewNode::Button((id, _)) if id.starts_with("item-") => Some(id.clone()),
            _ => None,
        })
}

fn result_button_count(output: &overcrow_widget_sdk::GuestOutput) -> usize {
    output
        .view
        .iter()
        .flat_map(|view| &view.nodes)
        .filter(|node| matches!(node, ViewNode::Button((id, _)) if id.starts_with("item-")))
        .count()
}

fn order_button_count(output: &overcrow_widget_sdk::GuestOutput) -> usize {
    output
        .view
        .iter()
        .flat_map(|view| &view.nodes)
        .filter(|node| matches!(node, ViewNode::Button((id, _)) if id.starts_with("order-")))
        .count()
}

fn selection(output: &overcrow_widget_sdk::GuestOutput, expected: &str) -> Option<u32> {
    output
        .view
        .iter()
        .flat_map(|view| &view.nodes)
        .find_map(|node| match node {
            ViewNode::Selection((id, _, selected)) if id == expected => Some(*selected),
            _ => None,
        })
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

fn http_get(output: &overcrow_widget_sdk::GuestOutput) -> Option<(u32, &str, &str)> {
    output.commands.iter().find_map(|command| match command {
        HostCommand::HttpGet((request_id, host, path)) => {
            Some((*request_id, host.as_str(), path.as_str()))
        }
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

fn clipboard(output: &overcrow_widget_sdk::GuestOutput) -> Option<&str> {
    output.commands.iter().find_map(|command| match command {
        HostCommand::ClipboardWrite(text) => Some(text.as_str()),
        _ => None,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    status: String,
    watchlist: Vec<String>,
}

impl Stored {
    fn from(bytes: &[u8]) -> Self {
        serde_json::from_slice(bytes).expect("stored market state")
    }
}
