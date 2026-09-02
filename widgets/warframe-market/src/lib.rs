#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

mod cache;
mod model;
mod parse;
mod state;

use alloc::{format, string::String, vec, vec::Vec};
use core::cmp::Reverse;

use cache::{EncodedCache, MANIFEST_KEY, Manifest, decode as decode_cache, encode as encode_cache};
use model::{
    MAX_ORDERS_PER_SIDE, MAX_RESULTS, MarketDetail, MarketItem, MarketOrder, Presence, SideFilter,
    StatusFilter, TradeSide, display_slug, item_matches, normalize_query, stable_element_id,
    whisper_line,
};
use overcrow_widget_sdk::{
    GuestError, GuestOutput, HostEvent, HttpResponseMetadata, Interaction, InteractionKind,
    LocalizedText, OutputBuilder, OverlayModeCode, TextRole, ViewBuilder, Widget, WidgetContext,
};
use parse::{CatalogStream, parse_orders};
use state::{Preferences, STORAGE_KEY};

const HTTP_HOST: &str = "api.warframe.market";
const ITEMS_PATH: &str = "/v2/items";
const SESSION_SCHEMA: &str = "overcrow.session.v1";
const STEAM_APP_ID: u32 = 230_410;
const MAX_HOST_UTC_MS: u64 = 253_402_300_799_999;
const HTTP_CADENCE_MS: u64 = 15_000;
const REFRESH_MS: u64 = 30_000;
const STALE_MS: u64 = 120_000;
const CATALOG_TTL_MS: u64 = 24 * 60 * 60 * 1_000;
const MAX_HTTP_BODY_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone)]
enum PendingHttp {
    Items { request_id: u32 },
    Orders { request_id: u32, slug: String },
}

impl PendingHttp {
    fn request_id(&self) -> u32 {
        match self {
            Self::Items { request_id } | Self::Orders { request_id, .. } => *request_id,
        }
    }
}

enum Action {
    HttpGet(u32, String),
    StorageGet(u32, String),
    StorageSet(u32, String, Vec<u8>),
    StorageDelete(u32, String),
    Clipboard(String),
}

enum HttpTransfer {
    Catalog(CatalogStream),
    Buffered {
        expected: usize,
        received: usize,
        next_sequence: u8,
        bytes: Vec<u8>,
    },
    Discard {
        expected: usize,
        received: usize,
        next_sequence: u8,
    },
}

struct CacheLoad {
    manifest: Manifest,
    requests: Vec<(u32, usize)>,
    parts: Vec<Option<Vec<u8>>>,
}

struct CacheWrite {
    encoded: EncodedCache,
    part_requests: Vec<u32>,
    manifest_request: Option<u32>,
}

#[derive(Clone, Copy)]
enum ErrorMessage {
    Unavailable,
    RequestFailed,
}

#[derive(Default)]
struct WarframeMarketWidget {
    preferences: Preferences,
    query: String,
    items: Vec<MarketItem>,
    detail: Option<MarketDetail>,
    now_ms: Option<u64>,
    last_http_attempt_ms: Option<u64>,
    last_orders_attempt_ms: Option<u64>,
    items_queued: bool,
    pending_http: Option<PendingHttp>,
    http_transfer: Option<HttpTransfer>,
    load_request: Option<u32>,
    cache_manifest_request: Option<u32>,
    cache_load: Option<CacheLoad>,
    cache_write: Option<CacheWrite>,
    cache_cleanup_requests: Vec<u32>,
    committed_part_keys: Vec<String>,
    catalog_fetched_at_ms: Option<u64>,
    store_request: Option<u32>,
    next_request_id: u32,
    preferences_changed: bool,
    store_queued: bool,
    error: Option<ErrorMessage>,
    view_revision: u64,
}

impl Widget for WarframeMarketWidget {
    fn init(&mut self, context: &mut WidgetContext) -> Result<GuestOutput, GuestError> {
        validate_init(context)?;
        let preferences_request = self.allocate_request()?;
        let cache_request = self.allocate_request()?;
        self.load_request = Some(preferences_request);
        self.cache_manifest_request = Some(cache_request);
        self.render(
            context,
            vec![
                Action::StorageGet(preferences_request, STORAGE_KEY.into()),
                Action::StorageGet(cache_request, MANIFEST_KEY.into()),
            ],
        )
    }

    fn handle(
        &mut self,
        event: HostEvent,
        context: &mut WidgetContext,
    ) -> Result<GuestOutput, GuestError> {
        let mut actions = Vec::new();
        match event {
            HostEvent::Tick(now_ms) => {
                if now_ms > MAX_HOST_UTC_MS || self.now_ms.is_some_and(|last| now_ms < last) {
                    return Err(GuestError::InvalidInput);
                }
                self.now_ms = Some(now_ms);
                self.request_if_due(context, &mut actions)?;
            }
            HostEvent::Interaction(interaction) => {
                self.handle_interaction(interaction, context, &mut actions)?;
            }
            HostEvent::StorageResult((request_id, value)) => {
                self.handle_storage(request_id, value.as_deref(), context, &mut actions)?;
            }
            HostEvent::SessionData(_) => self.request_if_due(context, &mut actions)?,
            HostEvent::LocaleChanged(_) => {}
            HostEvent::SettingsChanged(_) => {
                if !context.settings().is_empty() {
                    return Err(GuestError::InvalidInput);
                }
            }
            HostEvent::ProviderData(_) => return Err(GuestError::InvalidInput),
        }
        self.render(context, actions)
    }

    fn http_response_start(
        &mut self,
        request_id: u32,
        status: Option<u16>,
        body_length: u32,
        _metadata: HttpResponseMetadata,
        _context: &mut WidgetContext,
    ) -> Result<GuestOutput, GuestError> {
        if self.http_transfer.is_some()
            || self.pending_http.as_ref().map(PendingHttp::request_id) != Some(request_id)
        {
            return Err(GuestError::InvalidInput);
        }
        let expected = usize::try_from(body_length).map_err(|_| GuestError::InvalidInput)?;
        if expected == 0 || expected > MAX_HTTP_BODY_BYTES {
            return Err(GuestError::InvalidInput);
        }
        self.http_transfer = Some(if status != Some(200) {
            HttpTransfer::Discard {
                expected,
                received: 0,
                next_sequence: 0,
            }
        } else if matches!(self.pending_http, Some(PendingHttp::Items { .. })) {
            HttpTransfer::Catalog(
                CatalogStream::start(body_length).map_err(|_| GuestError::InvalidInput)?,
            )
        } else {
            HttpTransfer::Buffered {
                expected,
                received: 0,
                next_sequence: 0,
                bytes: Vec::with_capacity(expected.min(64 * 1024)),
            }
        });
        Ok(empty_output())
    }

    fn http_response_chunk(
        &mut self,
        request_id: u32,
        sequence: u8,
        bytes: Vec<u8>,
        _context: &mut WidgetContext,
    ) -> Result<GuestOutput, GuestError> {
        if self.pending_http.as_ref().map(PendingHttp::request_id) != Some(request_id) {
            return Err(GuestError::InvalidInput);
        }
        match self
            .http_transfer
            .as_mut()
            .ok_or(GuestError::InvalidState)?
        {
            HttpTransfer::Catalog(stream) => stream
                .push(sequence, &bytes)
                .map_err(|_| GuestError::InvalidInput)?,
            HttpTransfer::Buffered {
                expected,
                received,
                next_sequence,
                bytes: body,
            } => {
                push_http_chunk(*expected, received, next_sequence, sequence, &bytes)?;
                body.extend_from_slice(&bytes);
            }
            HttpTransfer::Discard {
                expected,
                received,
                next_sequence,
            } => push_http_chunk(*expected, received, next_sequence, sequence, &bytes)?,
        }
        Ok(empty_output())
    }

    fn http_response_end(
        &mut self,
        request_id: u32,
        context: &mut WidgetContext,
    ) -> Result<GuestOutput, GuestError> {
        let mut actions = Vec::new();
        self.finish_http(request_id, context, &mut actions)?;
        self.render(context, actions)
    }
}

impl WarframeMarketWidget {
    fn allocate_request(&mut self) -> Result<u32, GuestError> {
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .ok_or(GuestError::Unavailable)?;
        Ok(self.next_request_id)
    }

    fn dispatch_items(&mut self, now_ms: u64, actions: &mut Vec<Action>) -> Result<(), GuestError> {
        let request_id = self.allocate_request()?;
        self.pending_http = Some(PendingHttp::Items { request_id });
        self.items_queued = false;
        self.last_http_attempt_ms = Some(now_ms);
        self.error = None;
        actions.push(Action::HttpGet(request_id, ITEMS_PATH.into()));
        Ok(())
    }

    fn dispatch_orders(
        &mut self,
        now_ms: u64,
        actions: &mut Vec<Action>,
    ) -> Result<(), GuestError> {
        let Some(slug) = self.preferences.selected_slug.clone() else {
            return Ok(());
        };
        if !model::safe_slug(&slug) {
            return Err(GuestError::InvalidInput);
        }
        let request_id = self.allocate_request()?;
        let path = format!("/v2/orders/item/{slug}");
        self.pending_http = Some(PendingHttp::Orders { request_id, slug });
        self.last_http_attempt_ms = Some(now_ms);
        self.last_orders_attempt_ms = Some(now_ms);
        self.error = None;
        actions.push(Action::HttpGet(request_id, path));
        Ok(())
    }

    fn request_if_due(
        &mut self,
        context: &WidgetContext,
        actions: &mut Vec<Action>,
    ) -> Result<(), GuestError> {
        if !interactive_active(context) || self.pending_http.is_some() {
            return Ok(());
        }
        let Some(now_ms) = self.now_ms else {
            return Ok(());
        };
        if !self.query.is_empty() && self.catalog_needs_refresh(now_ms) {
            self.items_queued = true;
        }
        if self
            .last_http_attempt_ms
            .is_some_and(|last| now_ms.saturating_sub(last) < HTTP_CADENCE_MS)
        {
            return Ok(());
        }
        let orders_due = self.preferences.selected_slug.is_some()
            && self
                .last_orders_attempt_ms
                .is_none_or(|last| now_ms.saturating_sub(last) >= REFRESH_MS);
        if orders_due {
            self.dispatch_orders(now_ms, actions)?;
        } else if self.items_queued
            && !self.query.is_empty()
            && !self.cache_load_pending()
            && self.catalog_needs_refresh(now_ms)
        {
            self.dispatch_items(now_ms, actions)?;
        }
        Ok(())
    }

    fn finish_http(
        &mut self,
        request_id: u32,
        context: &WidgetContext,
        actions: &mut Vec<Action>,
    ) -> Result<(), GuestError> {
        let pending = self
            .pending_http
            .take()
            .filter(|pending| pending.request_id() == request_id)
            .ok_or(GuestError::InvalidInput)?;
        let transfer = self.http_transfer.take().ok_or(GuestError::InvalidState)?;
        match pending {
            PendingHttp::Items { .. } => {
                match transfer {
                    HttpTransfer::Catalog(stream) => match stream.finish() {
                        Ok(items) => {
                            let fetched_at_ms = self.now_ms.ok_or(GuestError::InvalidState)?;
                            self.items = items;
                            self.catalog_fetched_at_ms = Some(fetched_at_ms);
                            self.start_cache_write(fetched_at_ms, actions)?;
                            self.error = None;
                        }
                        Err(_) => self.error = Some(ErrorMessage::Unavailable),
                    },
                    HttpTransfer::Discard {
                        expected, received, ..
                    } if expected == received => self.error = Some(ErrorMessage::RequestFailed),
                    _ => return Err(GuestError::InvalidInput),
                }
                self.request_if_due(context, actions)?;
            }
            PendingHttp::Orders { slug, .. } => {
                if self.preferences.selected_slug.as_deref() != Some(slug.as_str()) {
                    self.request_if_due(context, actions)?;
                    return Ok(());
                }
                match transfer {
                    HttpTransfer::Buffered {
                        expected,
                        received,
                        bytes,
                        ..
                    } if expected == received => match parse_orders(&bytes) {
                        Ok(orders) => {
                            let name = self.selected_name(&slug);
                            self.detail = Some(MarketDetail {
                                name,
                                slug,
                                orders,
                                fetched_at_ms: self.now_ms.unwrap_or(0),
                            });
                            self.error = None;
                        }
                        Err(_) => self.error = Some(ErrorMessage::Unavailable),
                    },
                    HttpTransfer::Discard {
                        expected, received, ..
                    } if expected == received => self.error = Some(ErrorMessage::RequestFailed),
                    _ => return Err(GuestError::InvalidInput),
                }
                self.request_if_due(context, actions)?;
            }
        }
        Ok(())
    }

    fn handle_storage(
        &mut self,
        request_id: u32,
        value: Option<&[u8]>,
        context: &WidgetContext,
        actions: &mut Vec<Action>,
    ) -> Result<(), GuestError> {
        if self.load_request == Some(request_id) {
            self.load_request = None;
            if !self.preferences_changed {
                self.preferences = value.and_then(Preferences::parse).unwrap_or_default();
            }
            self.request_if_due(context, actions)?;
        } else if self.cache_manifest_request == Some(request_id) {
            self.cache_manifest_request = None;
            if let Some(manifest) = value.and_then(|bytes| Manifest::parse(bytes).ok()) {
                let keys = manifest.part_keys();
                let mut requests = Vec::with_capacity(keys.len());
                for (index, key) in keys.iter().enumerate() {
                    let request_id = self.allocate_request()?;
                    requests.push((request_id, index));
                    actions.push(Action::StorageGet(request_id, key.clone()));
                }
                self.committed_part_keys = keys;
                self.cache_load = Some(CacheLoad {
                    parts: (0..requests.len()).map(|_| None).collect(),
                    manifest,
                    requests,
                });
            } else {
                self.request_if_due(context, actions)?;
            }
        } else if let Some((position, part_index)) = self.cache_load.as_ref().and_then(|load| {
            load.requests
                .iter()
                .position(|(candidate, _)| *candidate == request_id)
                .map(|position| (position, load.requests[position].1))
        }) {
            if let Some(bytes) = value {
                let load = self.cache_load.as_mut().ok_or(GuestError::InvalidState)?;
                load.requests.remove(position);
                load.parts[part_index] = Some(bytes.to_vec());
                if load.requests.is_empty() {
                    let load = self.cache_load.take().ok_or(GuestError::InvalidState)?;
                    if let Some(parts) = load.parts.into_iter().collect::<Option<Vec<_>>>()
                        && let Ok(items) = decode_cache(&load.manifest, &parts)
                    {
                        self.items = items;
                        self.catalog_fetched_at_ms = Some(load.manifest.fetched_at_ms);
                    }
                    self.request_if_due(context, actions)?;
                }
            } else {
                self.cache_load = None;
                self.request_if_due(context, actions)?;
            }
        } else if self
            .cache_write
            .as_ref()
            .is_some_and(|write| write.part_requests.contains(&request_id))
        {
            let write = self.cache_write.as_mut().ok_or(GuestError::InvalidState)?;
            let position = write
                .part_requests
                .iter()
                .position(|candidate| *candidate == request_id)
                .ok_or(GuestError::InvalidState)?;
            write.part_requests.remove(position);
            if write.part_requests.is_empty() {
                let request_id = self.allocate_request()?;
                let write = self.cache_write.as_mut().ok_or(GuestError::InvalidState)?;
                write.manifest_request = Some(request_id);
                actions.push(Action::StorageSet(
                    request_id,
                    MANIFEST_KEY.into(),
                    write.encoded.manifest.clone(),
                ));
            }
        } else if self
            .cache_write
            .as_ref()
            .and_then(|write| write.manifest_request)
            == Some(request_id)
        {
            let write = self.cache_write.take().ok_or(GuestError::InvalidState)?;
            let previous = core::mem::replace(
                &mut self.committed_part_keys,
                write.encoded.part_keys.clone(),
            );
            let current = self.committed_part_keys.clone();
            for key in previous.into_iter().filter(|key| !current.contains(key)) {
                let request_id = self.allocate_request()?;
                self.cache_cleanup_requests.push(request_id);
                actions.push(Action::StorageDelete(request_id, key));
            }
        } else if let Some(position) = self
            .cache_cleanup_requests
            .iter()
            .position(|candidate| *candidate == request_id)
        {
            self.cache_cleanup_requests.remove(position);
        } else if self.store_request == Some(request_id) {
            self.store_request = None;
            if self.store_queued {
                self.store_queued = false;
                self.start_store(actions)?;
            }
        } else {
            return Err(GuestError::InvalidInput);
        }
        Ok(())
    }

    fn handle_interaction(
        &mut self,
        interaction: Interaction,
        context: &WidgetContext,
        actions: &mut Vec<Action>,
    ) -> Result<(), GuestError> {
        if !interactive_active(context) {
            return Err(GuestError::InvalidInput);
        }
        match interaction.kind {
            InteractionKind::Submitted(value) if interaction.element_id == "market-query" => {
                self.query = normalize_query(&value).ok_or(GuestError::InvalidInput)?;
                self.items_queued = !self.query.is_empty()
                    && self
                        .now_ms
                        .is_none_or(|now_ms| self.catalog_needs_refresh(now_ms));
                self.request_if_due(context, actions)?;
            }
            InteractionKind::ValueChanged(value) if interaction.element_id == "market-query" => {
                self.query = normalize_query(&value).ok_or(GuestError::InvalidInput)?;
            }
            InteractionKind::Clicked if interaction.element_id == "market-search" => {
                self.items_queued = !self.query.is_empty()
                    && self
                        .now_ms
                        .is_none_or(|now_ms| self.catalog_needs_refresh(now_ms));
                self.request_if_due(context, actions)?;
            }
            InteractionKind::Clicked if interaction.element_id == "market-clear" => {
                self.query.clear();
                self.items_queued = false;
                self.error = None;
            }
            InteractionKind::Clicked if interaction.element_id.starts_with("item-") => {
                let item = self
                    .search_results()
                    .into_iter()
                    .find(|item| item.element_id == interaction.element_id)
                    .cloned()
                    .ok_or(GuestError::InvalidInput)?;
                if self.preferences.selected_slug.as_deref() != Some(item.slug.as_str()) {
                    self.preferences.selected_slug = Some(item.slug);
                    self.detail = None;
                    self.mark_preferences_changed(actions)?;
                    self.request_if_due(context, actions)?;
                }
            }
            InteractionKind::Clicked if interaction.element_id.starts_with("watch-") => {
                let slug = self
                    .preferences
                    .watchlist
                    .iter()
                    .find(|slug| stable_element_id("watch-", slug) == interaction.element_id)
                    .cloned()
                    .ok_or(GuestError::InvalidInput)?;
                if self.preferences.selected_slug.as_deref() != Some(slug.as_str()) {
                    self.preferences.selected_slug = Some(slug);
                    self.detail = None;
                    self.mark_preferences_changed(actions)?;
                    self.request_if_due(context, actions)?;
                }
            }
            InteractionKind::Clicked if interaction.element_id.starts_with("order-") => {
                let order = self
                    .visible_orders()
                    .into_iter()
                    .find(|order| order.element_id == interaction.element_id)
                    .ok_or(GuestError::InvalidInput)?;
                let item = self
                    .detail
                    .as_ref()
                    .map(|detail| detail.name.as_str())
                    .ok_or(GuestError::InvalidInput)?;
                actions.push(Action::Clipboard(whisper_line(order, item)));
            }
            InteractionKind::Toggled(value) if interaction.element_id == "watch-selected" => {
                if self.preferences.set_selected_watched(value) {
                    self.mark_preferences_changed(actions)?;
                }
            }
            InteractionKind::SelectionChanged(index) if interaction.element_id == "filter-side" => {
                let value = SideFilter::from_index(index).ok_or(GuestError::InvalidInput)?;
                if self.preferences.side != value {
                    self.preferences.side = value;
                    self.mark_preferences_changed(actions)?;
                }
            }
            InteractionKind::SelectionChanged(index)
                if interaction.element_id == "filter-status" =>
            {
                let value = StatusFilter::from_index(index).ok_or(GuestError::InvalidInput)?;
                if self.preferences.status != value {
                    self.preferences.status = value;
                    self.mark_preferences_changed(actions)?;
                }
            }
            InteractionKind::Focused(_) | InteractionKind::Hovered(_)
                if self.known_element(&interaction.element_id) => {}
            _ => return Err(GuestError::InvalidInput),
        }
        Ok(())
    }

    fn mark_preferences_changed(&mut self, actions: &mut Vec<Action>) -> Result<(), GuestError> {
        self.preferences_changed = true;
        if self.store_request.is_some() {
            self.store_queued = true;
            Ok(())
        } else {
            self.start_store(actions)
        }
    }

    fn start_store(&mut self, actions: &mut Vec<Action>) -> Result<(), GuestError> {
        let bytes = self.preferences.encode().ok_or(GuestError::Unavailable)?;
        let request_id = self.allocate_request()?;
        self.store_request = Some(request_id);
        actions.push(Action::StorageSet(request_id, STORAGE_KEY.into(), bytes));
        Ok(())
    }

    fn start_cache_write(
        &mut self,
        fetched_at_ms: u64,
        actions: &mut Vec<Action>,
    ) -> Result<(), GuestError> {
        let encoded =
            encode_cache(&self.items, fetched_at_ms).map_err(|_| GuestError::Unavailable)?;
        let mut part_requests = Vec::with_capacity(encoded.parts.len());
        for (key, bytes) in encoded.part_keys.iter().zip(&encoded.parts) {
            let request_id = self.allocate_request()?;
            part_requests.push(request_id);
            actions.push(Action::StorageSet(request_id, key.clone(), bytes.clone()));
        }
        self.cache_write = Some(CacheWrite {
            encoded,
            part_requests,
            manifest_request: None,
        });
        Ok(())
    }

    fn selected_name(&self, slug: &str) -> String {
        self.items
            .iter()
            .find(|item| item.slug == slug)
            .map_or_else(|| display_slug(slug), |item| item.name.clone())
    }

    fn cache_load_pending(&self) -> bool {
        self.cache_manifest_request.is_some() || self.cache_load.is_some()
    }

    fn catalog_needs_refresh(&self, now_ms: u64) -> bool {
        self.items.is_empty()
            || self.catalog_fetched_at_ms.is_none_or(|fetched_at_ms| {
                fetched_at_ms > now_ms || now_ms.saturating_sub(fetched_at_ms) >= CATALOG_TTL_MS
            })
    }

    fn search_results(&self) -> Vec<&MarketItem> {
        if self.query.is_empty() {
            return Vec::new();
        }
        self.items
            .iter()
            .filter(|item| item_matches(item, &self.query))
            .take(MAX_RESULTS)
            .collect()
    }

    fn detail_is_fresh(&self) -> bool {
        match (self.now_ms, &self.detail) {
            (Some(now), Some(detail)) => now.saturating_sub(detail.fetched_at_ms) < STALE_MS,
            _ => false,
        }
    }

    fn visible_orders(&self) -> Vec<&MarketOrder> {
        let Some(detail) = self.detail.as_ref().filter(|_| self.detail_is_fresh()) else {
            return Vec::new();
        };
        if self.preferences.selected_slug.as_deref() != Some(detail.slug.as_str()) {
            return Vec::new();
        }
        let matches = |order: &&MarketOrder| {
            side_matches(self.preferences.side, order.side)
                && status_matches(self.preferences.status, order.presence)
        };
        let mut sellers = detail
            .orders
            .iter()
            .filter(|order| order.side == TradeSide::Sell)
            .filter(matches)
            .collect::<Vec<_>>();
        sellers.sort_by_key(|order| {
            (
                order.presence.rank(),
                order.platinum,
                order.trader.as_str(),
                order.public_id.as_str(),
            )
        });
        sellers.truncate(MAX_ORDERS_PER_SIDE);

        let mut buyers = detail
            .orders
            .iter()
            .filter(|order| order.side == TradeSide::Buy)
            .filter(matches)
            .collect::<Vec<_>>();
        buyers.sort_by_key(|order| {
            (
                order.presence.rank(),
                Reverse(order.platinum),
                order.trader.as_str(),
                order.public_id.as_str(),
            )
        });
        buyers.truncate(MAX_ORDERS_PER_SIDE);
        sellers.extend(buyers);
        sellers
    }

    fn known_element(&self, id: &str) -> bool {
        matches!(
            id,
            "market-query"
                | "market-search"
                | "market-clear"
                | "filter-side"
                | "filter-status"
                | "watch-selected"
        ) || self
            .search_results()
            .iter()
            .any(|item| item.element_id == id)
            || self
                .visible_orders()
                .iter()
                .any(|order| order.element_id == id)
            || self
                .preferences
                .watchlist
                .iter()
                .any(|slug| stable_element_id("watch-", slug) == id)
    }

    fn render(
        &mut self,
        context: &mut WidgetContext,
        actions: Vec<Action>,
    ) -> Result<GuestOutput, GuestError> {
        self.view_revision = self
            .view_revision
            .checked_add(1)
            .ok_or(GuestError::Unavailable)?;
        let mut builder = ViewBuilder::new(context.locale());
        let mut nodes = Vec::new();
        nodes.push(builder.text_role(
            TextRole::Heading,
            localized("Warframe Market", "Marché Warframe")?,
        )?);

        if !warframe_active(context) {
            nodes.push(builder.text(localized(
                "Waiting for an active Warframe session.",
                "En attente d’une session Warframe active.",
            )?)?);
        } else {
            nodes.push(builder.text(localized(
                "PC market · crossplay off",
                "Marché PC · cross-play désactivé",
            )?)?);
            nodes.push(builder.text_input(
                "market-query",
                &self.query,
                localized("Search items", "Rechercher des objets")?,
            )?);
            nodes.push(builder.button("market-search", localized("Search", "Rechercher")?)?);
            nodes.push(builder.button("market-clear", localized("Clear", "Effacer")?)?);
            nodes.push(builder.selection(
                "filter-side",
                vec![
                    localized("All orders", "Toutes les offres")?,
                    localized("Sellers", "Vendeurs")?,
                    localized("Buyers", "Acheteurs")?,
                ],
                self.preferences.side.index(),
            )?);
            nodes.push(builder.selection(
                "filter-status",
                vec![
                    localized("Online", "En ligne")?,
                    localized("In game", "En jeu")?,
                    localized("Any status", "Tous les statuts")?,
                ],
                self.preferences.status.index(),
            )?);

            for item in self.search_results() {
                nodes.push(builder.button(&item.element_id, LocalizedText::new(&item.name))?);
            }
            if !self.preferences.watchlist.is_empty() {
                nodes.push(builder.text(localized("Watchlist", "Liste de suivi")?)?);
                for slug in &self.preferences.watchlist {
                    nodes.push(builder.button(
                        &stable_element_id("watch-", slug),
                        LocalizedText::new(self.selected_name(slug)),
                    )?);
                }
            }
            self.render_detail(&mut builder, &mut nodes)?;
            if let Some(error) = self.error {
                nodes.push(builder.text(error_text(error)?)?);
            }
        }

        let root = builder.container(&nodes)?;
        let view = builder.finish(root, self.view_revision)?;
        let next_wake_ms = interactive_active(context)
            .then(|| self.next_wake_ms())
            .flatten();
        let mut output = OutputBuilder::new(context).view(view)?;
        for action in actions {
            match action {
                Action::HttpGet(request_id, path) => {
                    output.http_get(request_id, HTTP_HOST, &path)?;
                }
                Action::StorageGet(request_id, key) => {
                    output.storage_get(request_id, &key)?;
                }
                Action::StorageSet(request_id, key, bytes) => {
                    output.storage_set(request_id, &key, &bytes)?;
                }
                Action::StorageDelete(request_id, key) => {
                    output.storage_delete(request_id, &key)?
                }
                Action::Clipboard(text) => output.clipboard_write(&text)?,
            }
        }
        if let Some(next_wake_ms) = next_wake_ms {
            output.next_wake_ms(next_wake_ms)?;
        }
        Ok(output.finish())
    }

    fn render_detail(
        &self,
        builder: &mut ViewBuilder,
        nodes: &mut Vec<overcrow_widget_sdk::NodeId>,
    ) -> Result<(), GuestError> {
        let Some(selected) = self.preferences.selected_slug.as_deref() else {
            return Ok(());
        };
        nodes.push(builder.toggle(
            "watch-selected",
            localized("Watch this item", "Suivre cet objet")?,
            self.preferences.selected_is_watched(),
        )?);
        let name = self.selected_name(selected);
        nodes.push(builder.text(LocalizedText::new(name))?);

        if self
            .detail
            .as_ref()
            .is_some_and(|detail| detail.slug == selected)
        {
            if self.now_ms.is_none() {
                nodes.push(builder.text(localized("Synchronizing time…", "Synchronisation…")?)?);
            } else if !self.detail_is_fresh() {
                nodes.push(builder.text(localized(
                    "Market data is stale.",
                    "Les données du marché sont périmées.",
                )?)?);
            } else {
                for order in self.visible_orders() {
                    nodes.push(builder.text(order_text(order)?)?);
                    nodes.push(builder.button(
                        &order.element_id,
                        localized(
                            format!("Whisper {}", order.trader),
                            format!("Contacter {}", order.trader),
                        )?,
                    )?);
                }
            }
        }
        Ok(())
    }

    fn next_wake_ms(&self) -> Option<u32> {
        if self.pending_http.is_some()
            || (self.preferences.selected_slug.is_none() && !self.items_queued)
        {
            return None;
        }
        let Some(now_ms) = self.now_ms else {
            return Some(100);
        };
        let global_deadline = self
            .last_http_attempt_ms
            .map_or(now_ms, |last| last.saturating_add(HTTP_CADENCE_MS));
        let mut deadline = self.items_queued.then_some(global_deadline);
        if self.preferences.selected_slug.is_some() {
            let orders_deadline = self
                .last_orders_attempt_ms
                .map_or(now_ms, |last| last.saturating_add(REFRESH_MS))
                .max(global_deadline);
            deadline = Some(deadline.map_or(orders_deadline, |value| value.min(orders_deadline)));
        }
        let remaining = deadline?.saturating_sub(now_ms).clamp(100, REFRESH_MS);
        Some(u32::try_from(remaining).unwrap_or(REFRESH_MS as u32))
    }
}

fn empty_output() -> GuestOutput {
    GuestOutput {
        view: None,
        commands: Vec::new(),
        next_wake_ms: None,
    }
}

fn push_http_chunk(
    expected: usize,
    received: &mut usize,
    next_sequence: &mut u8,
    sequence: u8,
    bytes: &[u8],
) -> Result<(), GuestError> {
    if sequence != *next_sequence || bytes.is_empty() || bytes.len() > 64 * 1024 {
        return Err(GuestError::InvalidInput);
    }
    *next_sequence = next_sequence
        .checked_add(1)
        .ok_or(GuestError::InvalidInput)?;
    *received = received
        .checked_add(bytes.len())
        .filter(|received| *received <= expected)
        .ok_or(GuestError::InvalidInput)?;
    Ok(())
}

fn validate_init(context: &WidgetContext) -> Result<(), GuestError> {
    if !context.settings().is_empty() {
        return Err(GuestError::InvalidInput);
    }
    let grants = context.granted_capabilities();
    if !matches!(grants.http_hosts.as_slice(), [host] if host == HTTP_HOST)
        || !matches!(grants.game_data.as_slice(), [schema] if schema == SESSION_SCHEMA)
        || !grants.storage
        || !grants.clipboard_write
        || grants.provider
    {
        return Err(GuestError::Unavailable);
    }
    Ok(())
}

fn warframe_active(context: &WidgetContext) -> bool {
    context.session_data().is_some_and(|session| {
        session.selected_active && session.steam_app_id == Some(STEAM_APP_ID)
    })
}

fn interactive_active(context: &WidgetContext) -> bool {
    warframe_active(context)
        && context
            .session_data()
            .is_some_and(|session| session.overlay_mode == OverlayModeCode::Interactive)
}

fn side_matches(filter: SideFilter, side: TradeSide) -> bool {
    matches!(filter, SideFilter::All)
        || matches!((filter, side), (SideFilter::Sellers, TradeSide::Sell))
        || matches!((filter, side), (SideFilter::Buyers, TradeSide::Buy))
}

fn status_matches(filter: StatusFilter, presence: Presence) -> bool {
    match filter {
        StatusFilter::Online => matches!(presence, Presence::Ingame | Presence::Online),
        StatusFilter::Ingame => presence == Presence::Ingame,
        StatusFilter::Any => true,
    }
}

fn order_text(order: &MarketOrder) -> Result<LocalizedText, GuestError> {
    let (english_status, french_status) = match order.presence {
        Presence::Ingame => ("in game", "en jeu"),
        Presence::Online => ("online", "en ligne"),
        Presence::Offline => ("offline", "hors ligne"),
        Presence::Unknown => ("unknown", "inconnu"),
    };
    localized(
        format!("{} · {english_status} · {}p", order.trader, order.platinum),
        format!("{} · {french_status} · {}p", order.trader, order.platinum),
    )
}

fn error_text(error: ErrorMessage) -> Result<LocalizedText, GuestError> {
    match error {
        ErrorMessage::Unavailable => localized(
            "Market data is unavailable.",
            "Les données du marché sont indisponibles.",
        ),
        ErrorMessage::RequestFailed => {
            localized("Market request failed.", "La requête au marché a échoué.")
        }
    }
}

fn localized(
    english: impl Into<String>,
    french: impl Into<String>,
) -> Result<LocalizedText, GuestError> {
    Ok(LocalizedText::new(english).with_translation("fr", french)?)
}

overcrow_widget_sdk::export_widget!(crate::WarframeMarketWidget);

#[cfg(test)]
#[path = "../tests/widget.rs"]
mod widget_tests;
