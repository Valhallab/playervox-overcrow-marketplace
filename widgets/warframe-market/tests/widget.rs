use overcrow_widget_sdk::{
    ContainerLayout, GrantedCapabilities, GuestError, HarnessError, HostCommand, HostEvent,
    HttpResponseMetadata, InitInput, Interaction, InteractionKind, Layout, OverlayModeCode,
    SessionData, ViewNode, WidgetHarness,
};
use serde::Deserialize;
use serde_json::json;

use super::{
    CATALOG_TTL_MS, HTTP_HOST, ITEMS_PATH, WarframeMarketWidget,
    cache::{MANIFEST_KEY, encode as encode_cache},
    parse::{CatalogStream, parse_orders},
};

const ITEMS: &[u8] = include_bytes!("fixtures/items.json");
const ORDERS: &[u8] = include_bytes!("fixtures/orders.json");
const HOSTILE: &[u8] = include_bytes!("fixtures/hostile.json");
const NOW_MS: u64 = 1_777_000_000_000;

#[test]
fn active_market_uses_structured_bounded_layout() {
    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    bootstrap(&mut harness);
    submit_query(&mut harness, "arcane");
    let request_id = http_get(harness.output()).expect("catalog request").0;
    deliver_http(&mut harness, request_id, Some(200), ITEMS).expect("catalog response");

    let nodes = &harness.output().view.as_ref().expect("market view").nodes;
    assert!(
        nodes
            .iter()
            .any(|node| matches!(node, ViewNode::Surface(_)))
    );
    assert!(nodes.iter().any(|node| matches!(node, ViewNode::Scroll(_))));
    assert!(nodes.iter().any(|node| matches!(
        node,
        ViewNode::Container((Layout::Linear(ContainerLayout::Row), _))
    )));
}

#[test]
fn first_search_streams_catalog_and_publishes_cache_manifest_last() {
    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    assert_eq!(
        storage_gets(harness.output()),
        vec![(1, "state"), (2, MANIFEST_KEY)]
    );
    bootstrap(&mut harness);

    submit_query(&mut harness, "arcane");
    let request_id = http_get(harness.output()).expect("catalog request").0;
    assert_eq!(
        http_get(harness.output()).map(|value| value.2),
        Some(ITEMS_PATH)
    );
    deliver_http(&mut harness, request_id, Some(200), ITEMS).expect("stream catalog");
    assert!(button_id(harness.output(), "Arcane Energize").is_some());

    let part_sets = storage_sets(harness.output());
    assert!(!part_sets.is_empty());
    assert!(
        part_sets
            .iter()
            .all(|(_, key, _)| key.starts_with("catalog-") && *key != MANIFEST_KEY)
    );
    let part_request_ids = part_sets.iter().map(|(id, _, _)| *id).collect::<Vec<_>>();
    for request_id in part_request_ids {
        harness
            .send(HostEvent::StorageResult((request_id, None)))
            .expect("cache part stored");
    }
    let manifest_set = storage_sets(harness.output());
    assert_eq!(manifest_set.len(), 1);
    assert_eq!(manifest_set[0].1, MANIFEST_KEY);
}

#[test]
fn fresh_compact_cache_avoids_the_catalog_download() {
    let items = fixture_items();
    let encoded = encode_cache(&items, NOW_MS - 1_000).expect("encode fixture cache");
    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    harness
        .send(HostEvent::StorageResult((1, None)))
        .expect("default preferences");
    harness
        .send(HostEvent::StorageResult((
            2,
            Some(encoded.manifest.clone()),
        )))
        .expect("cache manifest");
    let part_gets = storage_gets(harness.output())
        .into_iter()
        .map(|(request_id, key)| (request_id, key.to_owned()))
        .collect::<Vec<_>>();
    assert_eq!(part_gets.len(), encoded.parts.len());
    for ((request_id, key), (expected_key, part)) in part_gets
        .into_iter()
        .zip(encoded.part_keys.iter().zip(encoded.parts))
    {
        assert_eq!(&key, expected_key);
        harness
            .send(HostEvent::StorageResult((request_id, Some(part))))
            .expect("cache part");
    }
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");

    submit_query(&mut harness, "arcane");
    assert!(http_get(harness.output()).is_none());
    assert!(button_id(harness.output(), "Arcane Energize").is_some());
}

#[test]
fn expired_cache_refreshes_once_but_keeps_search_results_available() {
    let items = fixture_items();
    let encoded = encode_cache(&items, NOW_MS - CATALOG_TTL_MS).expect("encode stale cache");
    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    harness
        .send(HostEvent::StorageResult((1, None)))
        .expect("default preferences");
    harness
        .send(HostEvent::StorageResult((
            2,
            Some(encoded.manifest.clone()),
        )))
        .expect("cache manifest");
    let part_request_ids = storage_gets(harness.output())
        .into_iter()
        .map(|(request_id, _)| request_id)
        .collect::<Vec<_>>();
    for (request_id, part) in part_request_ids.into_iter().zip(encoded.parts) {
        harness
            .send(HostEvent::StorageResult((request_id, Some(part))))
            .expect("cache part");
    }
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");

    submit_query(&mut harness, "arcane");
    assert_eq!(
        http_get(harness.output()).map(|value| value.2),
        Some(ITEMS_PATH)
    );
    assert!(button_id(harness.output(), "Arcane Energize").is_some());
}

#[test]
fn item_orders_and_clipboard_use_stable_public_identity() {
    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    bootstrap(&mut harness);
    submit_query(&mut harness, "arcane");
    let request_id = http_get(harness.output()).expect("catalog request").0;
    deliver_http(&mut harness, request_id, Some(200), ITEMS).expect("catalog response");
    let item_id = button_id(harness.output(), "Arcane Energize").expect("search result");

    harness
        .send(interaction(&item_id, InteractionKind::Clicked))
        .expect("select item");
    harness
        .send(HostEvent::Tick(NOW_MS + 15_000))
        .expect("HTTP cadence");
    let (orders_request, host, path) = http_get(harness.output()).expect("orders request");
    assert_eq!((host, path), (HTTP_HOST, "/v2/orders/item/arcane_energize"));
    deliver_http(&mut harness, orders_request, Some(200), ORDERS).expect("orders response");
    assert!(texts(harness.output()).contains(&"SellerOne · online · 100p"));
    assert!(
        !texts(harness.output())
            .iter()
            .any(|text| text.contains("Hidden"))
    );

    let whisper = button_id(harness.output(), "Whisper SellerOne").expect("trade action");
    harness
        .send(interaction(&whisper, InteractionKind::Clicked))
        .expect("copy whisper");
    assert_eq!(
        clipboard(harness.output()),
        Some("/w SellerOne Hi, WTB Arcane Energize for 100p")
    );
}

#[test]
fn malformed_and_failed_responses_fail_closed_without_retaining_payloads() {
    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    bootstrap(&mut harness);
    submit_query(&mut harness, "arcane");
    let request_id = http_get(harness.output()).expect("catalog request").0;
    deliver_http(&mut harness, request_id, Some(200), HOSTILE).expect("malformed response drained");
    assert!(texts(harness.output()).contains(&"Market data is unavailable."));

    harness
        .send(HostEvent::Tick(NOW_MS + 15_000))
        .expect("retry cadence");
    let request_id = http_get(harness.output()).expect("retry request").0;
    deliver_http(&mut harness, request_id, None, b"private timeout body")
        .expect("failed response drained");
    assert!(texts(harness.output()).contains(&"Market request failed."));
}

#[test]
fn passive_mode_blocks_interaction_and_network_wakes() {
    let mut widget = WarframeMarketWidget::default();
    let mut input = init("en");
    input.session_data = Some(session_with_mode(true, OverlayModeCode::Passive));
    let mut harness = WidgetHarness::from_init(&mut widget, input).expect("passive market");
    harness
        .send(HostEvent::StorageResult((1, None)))
        .expect("preferences");
    harness
        .send(HostEvent::StorageResult((2, None)))
        .expect("cache");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    assert!(matches!(
        harness.send(interaction(
            "market-query",
            InteractionKind::Submitted("arcane".into())
        )),
        Err(HarnessError::Passive)
    ));
    assert!(http_get(harness.output()).is_none());
    assert!(harness.output().next_wake_ms.is_none());
}

#[test]
fn preferences_are_bounded_coalesced_and_exclude_queries() {
    let mut widget = WarframeMarketWidget::default();
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    harness
        .send(HostEvent::StorageResult((2, None)))
        .expect("no cache");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(HostEvent::StorageResult((1, Some(stored_state()))))
        .expect("stored preferences");
    assert_eq!(selection(harness.output(), "filter-side"), Some(1));
    assert_eq!(toggle(harness.output(), "watch-selected"), Some(true));

    harness
        .send(interaction(
            "filter-status",
            InteractionKind::SelectionChanged(2),
        ))
        .expect("change filter");
    let (store_id, bytes) = preference_storage_set(harness.output()).expect("persist preferences");
    let stored = Stored::from(bytes);
    assert_eq!(stored.status, "any");
    assert!(!String::from_utf8_lossy(bytes).contains("market-query"));

    harness
        .send(interaction(
            "watch-selected",
            InteractionKind::Toggled(false),
        ))
        .expect("queue watchlist update");
    assert!(preference_storage_set(harness.output()).is_none());
    harness
        .send(HostEvent::StorageResult((store_id, None)))
        .expect("first store complete");
    let (_, bytes) = preference_storage_set(harness.output()).expect("coalesced store");
    assert_eq!(Stored::from(bytes).watchlist, ["primed_flow"]);
}

#[test]
fn exact_authority_settings_time_and_order_bounds_are_enforced() {
    for capabilities in [
        GrantedCapabilities {
            http_hosts: Vec::new(),
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
    let mut harness = WidgetHarness::from_init(&mut widget, init("en")).expect("market init");
    harness.send(HostEvent::Tick(NOW_MS)).expect("first time");
    assert!(matches!(
        harness.send(HostEvent::Tick(NOW_MS - 1)),
        Err(HarnessError::Widget(GuestError::InvalidInput))
    ));
    assert_eq!(parse_orders(ORDERS).expect("official order shape").len(), 4);
}

fn fixture_items() -> Vec<super::model::MarketItem> {
    let mut stream = CatalogStream::start(ITEMS.len() as u32).expect("fixture stream");
    stream.push(0, ITEMS).expect("fixture chunk");
    stream.finish().expect("fixture catalog")
}

fn init(locale: &str) -> InitInput {
    InitInput {
        locale: locale.to_owned(),
        granted_capabilities: grants(),
        settings: Vec::new(),
        session_data: Some(session_with_mode(true, OverlayModeCode::Interactive)),
    }
}

fn grants() -> GrantedCapabilities {
    GrantedCapabilities {
        http_hosts: vec![HTTP_HOST.to_owned()],
        game_data: vec!["overcrow.session.v1".to_owned()],
        storage: true,
        clipboard_write: true,
        provider: false,
    }
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
        .expect("default preferences");
    harness
        .send(HostEvent::StorageResult((2, None)))
        .expect("empty cache");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
}

fn submit_query(harness: &mut WidgetHarness<'_, WarframeMarketWidget>, query: &str) {
    harness
        .send(interaction(
            "market-query",
            InteractionKind::Submitted(query.to_owned()),
        ))
        .expect("submit query");
}

fn deliver_http(
    harness: &mut WidgetHarness<'_, WarframeMarketWidget>,
    request_id: u32,
    status: Option<u16>,
    body: &[u8],
) -> Result<(), HarnessError> {
    harness.http_response_start(
        request_id,
        status,
        u32::try_from(body.len()).expect("bounded fixture"),
        HttpResponseMetadata {
            content_type: Some("application/json".to_owned()),
            headers: Vec::new(),
        },
    )?;
    for (sequence, chunk) in body.chunks(64 * 1024).enumerate() {
        harness.http_response_chunk(
            request_id,
            u8::try_from(sequence).expect("bounded chunks"),
            chunk.to_vec(),
        )?;
    }
    harness.http_response_end(request_id)
}

fn interaction(id: &str, kind: InteractionKind) -> HostEvent {
    HostEvent::Interaction(Interaction {
        element_id: id.to_owned(),
        kind,
    })
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

fn storage_gets(output: &overcrow_widget_sdk::GuestOutput) -> Vec<(u32, &str)> {
    output
        .commands
        .iter()
        .filter_map(|command| match command {
            HostCommand::StorageGet((id, key)) => Some((*id, key.as_str())),
            _ => None,
        })
        .collect()
}

fn storage_sets(output: &overcrow_widget_sdk::GuestOutput) -> Vec<(u32, &str, &[u8])> {
    output
        .commands
        .iter()
        .filter_map(|command| match command {
            HostCommand::StorageSet((id, key, bytes)) => {
                Some((*id, key.as_str(), bytes.as_slice()))
            }
            _ => None,
        })
        .collect()
}

fn preference_storage_set(output: &overcrow_widget_sdk::GuestOutput) -> Option<(u32, &[u8])> {
    storage_sets(output)
        .into_iter()
        .find(|(_, key, _)| *key == "state")
        .map(|(id, _, bytes)| (id, bytes))
}

fn http_get(output: &overcrow_widget_sdk::GuestOutput) -> Option<(u32, &str, &str)> {
    output.commands.iter().find_map(|command| match command {
        HostCommand::HttpGet((id, host, path)) => Some((*id, host.as_str(), path.as_str())),
        _ => None,
    })
}

fn clipboard(output: &overcrow_widget_sdk::GuestOutput) -> Option<&str> {
    output.commands.iter().find_map(|command| match command {
        HostCommand::ClipboardWrite(text) => Some(text.as_str()),
        _ => None,
    })
}

fn texts(output: &overcrow_widget_sdk::GuestOutput) -> Vec<&str> {
    output
        .view
        .iter()
        .flat_map(|view| &view.nodes)
        .filter_map(|node| match node {
            ViewNode::Text((_, text)) => Some(text.as_str()),
            _ => None,
        })
        .collect()
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

fn selection(output: &overcrow_widget_sdk::GuestOutput, expected: &str) -> Option<u32> {
    output
        .view
        .iter()
        .flat_map(|view| &view.nodes)
        .find_map(|node| match node {
            ViewNode::Selection((id, _, value)) if id == expected => Some(*value),
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    status: String,
    watchlist: Vec<String>,
}

impl<'a> From<&'a [u8]> for Stored {
    fn from(value: &'a [u8]) -> Self {
        serde_json::from_slice(value).expect("stored preferences")
    }
}
