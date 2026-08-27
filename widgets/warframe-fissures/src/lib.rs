#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use alloc::{format, string::String, vec::Vec};

use overcrow_widget_sdk::{
    GuestError, GuestOutput, HostEvent, InteractionKind, LocalizedText, OutputBuilder, ViewBuilder,
    Widget, WidgetContext,
};
use serde::{Deserialize, Serialize};
use warframe_widget_data::{
    Era, Fissure, MAX_HOST_UTC_MS, SESSION_SCHEMA, STEAM_APP_ID, Worldstate, parse,
    remaining_minutes,
};

pub(crate) use warframe_widget_data::{PROVIDER_ID, PROVIDER_SCHEMA};

const FILTERS_KEY: &str = "filters";
const FILTERS_SCHEMA_VERSION: u8 = 1;
const MAX_FILTER_BYTES: usize = 512;
const WAKE_MS: u32 = 60_000;

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Filters {
    schema_version: u8,
    lith: bool,
    meso: bool,
    neo: bool,
    axi: bool,
    requiem: bool,
    omnia: bool,
    normal: bool,
    railjack: bool,
}

impl Default for Filters {
    fn default() -> Self {
        Self {
            schema_version: FILTERS_SCHEMA_VERSION,
            lith: true,
            meso: true,
            neo: true,
            axi: true,
            requiem: true,
            omnia: true,
            normal: true,
            railjack: true,
        }
    }
}

enum StorageAction {
    Get(u32),
    Set(u32, Vec<u8>),
}

#[derive(Default)]
struct FissuresWidget {
    data: Option<Worldstate>,
    last_provider_revision: u64,
    now_ms: Option<u64>,
    view_revision: u64,
    filters: Filters,
    load_request: Option<u32>,
    store_request: Option<u32>,
    next_request_id: u32,
    preferences_changed: bool,
    store_queued: bool,
}

impl Widget for FissuresWidget {
    fn init(&mut self, context: &mut WidgetContext) -> Result<GuestOutput, GuestError> {
        if !context.settings().is_empty() {
            return Err(GuestError::InvalidInput);
        }
        let grants = context.granted_capabilities();
        if !grants.http_hosts.is_empty()
            || !matches!(grants.game_data.as_slice(), [value] if value == SESSION_SCHEMA)
            || !grants.storage
            || grants.clipboard_write
            || grants.provider
        {
            return Err(GuestError::Unavailable);
        }
        let request_id = self.allocate_request()?;
        self.load_request = Some(request_id);
        self.render(context, Some(StorageAction::Get(request_id)))
    }

    fn handle(
        &mut self,
        event: HostEvent,
        context: &mut WidgetContext,
    ) -> Result<GuestOutput, GuestError> {
        let mut storage_action = None;
        match event {
            HostEvent::Tick(now_ms) => {
                if now_ms > MAX_HOST_UTC_MS || self.now_ms.is_some_and(|previous| now_ms < previous)
                {
                    return Err(GuestError::InvalidInput);
                }
                self.now_ms = Some(now_ms);
            }
            HostEvent::ProviderData((provider, schema, revision, payload)) => {
                if provider != PROVIDER_ID || schema != PROVIDER_SCHEMA || revision == 0 {
                    return Err(GuestError::InvalidInput);
                }
                if revision > self.last_provider_revision {
                    self.last_provider_revision = revision;
                    self.data = parse(&payload).ok();
                }
            }
            HostEvent::Interaction(interaction) => {
                let changed = match interaction.kind {
                    InteractionKind::Toggled(value) => {
                        self.filters.set(&interaction.element_id, value)?
                    }
                    InteractionKind::Focused(_) | InteractionKind::Hovered(_) => {
                        if !Filters::known_id(&interaction.element_id) {
                            return Err(GuestError::InvalidInput);
                        }
                        false
                    }
                    _ => return Err(GuestError::InvalidInput),
                };
                if changed {
                    self.preferences_changed = true;
                    if self.store_request.is_some() {
                        self.store_queued = true;
                    } else {
                        storage_action = Some(self.start_store()?);
                    }
                }
            }
            HostEvent::StorageResult((request_id, value)) => {
                if self.load_request == Some(request_id) {
                    self.load_request = None;
                    if !self.preferences_changed {
                        self.filters = value
                            .as_deref()
                            .and_then(Filters::parse)
                            .unwrap_or_default();
                    }
                } else if self.store_request == Some(request_id) {
                    self.store_request = None;
                    if self.store_queued {
                        self.store_queued = false;
                        storage_action = Some(self.start_store()?);
                    }
                } else {
                    return Err(GuestError::InvalidInput);
                }
            }
            HostEvent::LocaleChanged(_) | HostEvent::SessionData(_) => {}
            HostEvent::SettingsChanged(_) => {
                if !context.settings().is_empty() {
                    return Err(GuestError::InvalidInput);
                }
            }
            HostEvent::HttpResult(_) => return Err(GuestError::InvalidInput),
        }
        self.render(context, storage_action)
    }
}

impl FissuresWidget {
    fn allocate_request(&mut self) -> Result<u32, GuestError> {
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .ok_or(GuestError::Unavailable)?;
        Ok(self.next_request_id)
    }

    fn start_store(&mut self) -> Result<StorageAction, GuestError> {
        let request_id = self.allocate_request()?;
        let bytes = serde_json::to_vec(&self.filters).map_err(|_| GuestError::Unavailable)?;
        self.store_request = Some(request_id);
        Ok(StorageAction::Set(request_id, bytes))
    }

    fn render(
        &mut self,
        context: &mut WidgetContext,
        storage_action: Option<StorageAction>,
    ) -> Result<GuestOutput, GuestError> {
        self.view_revision = self
            .view_revision
            .checked_add(1)
            .ok_or(GuestError::Unavailable)?;
        let mut builder = ViewBuilder::new(context.locale());
        let mut nodes = Vec::new();
        nodes.push(builder.text(localized("Void Fissures", "Fissures du Néant")?)?);
        for era in Era::ALL {
            nodes.push(builder.toggle(
                &format!("era-{}", era.id()),
                LocalizedText::new(era.label()),
                self.filters.era_enabled(era),
            )?);
        }
        nodes.push(builder.toggle(
            "source-normal",
            localized("Star Chart", "Carte stellaire")?,
            self.filters.normal,
        )?);
        nodes.push(builder.toggle(
            "source-railjack",
            localized("Railjack", "Railjack")?,
            self.filters.railjack,
        )?);

        if !warframe_active(context) {
            nodes.push(builder.text(localized(
                "Waiting for an active Warframe session.",
                "En attente d’une session Warframe active.",
            )?)?);
        } else if let (Some(now_ms), Some(data)) = (self.now_ms, &self.data) {
            if data.is_fresh_at(now_ms) {
                let mut found = false;
                for fissure in &data.fissures {
                    if self.filters.includes(fissure)
                        && let Some((english, french)) = fissure_text(fissure, now_ms)
                    {
                        found = true;
                        nodes.push(builder.text(localized(english, french)?)?);
                    }
                }
                if !found {
                    nodes.push(builder.text(localized(
                        "No fissures match these filters.",
                        "Aucune fissure ne correspond à ces filtres.",
                    )?)?);
                }
            } else {
                nodes.push(builder.text(unavailable()?)?);
            }
        } else if self.data.is_some() {
            nodes.push(builder.text(localized("Synchronizing time…", "Synchronisation…")?)?);
        } else {
            nodes.push(builder.text(unavailable()?)?);
        }

        let root = builder.container(&nodes)?;
        let view = builder.finish(root, self.view_revision)?;
        let mut output = OutputBuilder::new(context).view(view)?;
        match storage_action {
            Some(StorageAction::Get(request_id)) => output.storage_get(request_id, FILTERS_KEY)?,
            Some(StorageAction::Set(request_id, bytes)) => {
                output.storage_set(request_id, FILTERS_KEY, &bytes)?;
            }
            None => {}
        }
        output.next_wake_ms(WAKE_MS)?;
        Ok(output.finish())
    }
}

impl Filters {
    fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.is_empty() || bytes.len() > MAX_FILTER_BYTES {
            return None;
        }
        serde_json::from_slice::<Self>(bytes)
            .ok()
            .filter(|filters| filters.schema_version == FILTERS_SCHEMA_VERSION)
    }

    fn known_id(id: &str) -> bool {
        matches!(
            id,
            "era-lith"
                | "era-meso"
                | "era-neo"
                | "era-axi"
                | "era-requiem"
                | "era-omnia"
                | "source-normal"
                | "source-railjack"
        )
    }

    fn set(&mut self, id: &str, value: bool) -> Result<bool, GuestError> {
        let current = match id {
            "era-lith" => &mut self.lith,
            "era-meso" => &mut self.meso,
            "era-neo" => &mut self.neo,
            "era-axi" => &mut self.axi,
            "era-requiem" => &mut self.requiem,
            "era-omnia" => &mut self.omnia,
            "source-normal" => &mut self.normal,
            "source-railjack" => &mut self.railjack,
            _ => return Err(GuestError::InvalidInput),
        };
        let changed = *current != value;
        *current = value;
        Ok(changed)
    }

    fn era_enabled(self, era: Era) -> bool {
        match era {
            Era::Lith => self.lith,
            Era::Meso => self.meso,
            Era::Neo => self.neo,
            Era::Axi => self.axi,
            Era::Requiem => self.requiem,
            Era::Omnia => self.omnia,
        }
    }

    fn includes(self, fissure: &Fissure) -> bool {
        self.era_enabled(fissure.era)
            && if fissure.railjack {
                self.railjack
            } else {
                self.normal
            }
    }
}

fn warframe_active(context: &WidgetContext) -> bool {
    context.session_data().is_some_and(|session| {
        session.selected_active && session.steam_app_id == Some(STEAM_APP_ID)
    })
}

fn fissure_text(fissure: &Fissure, now_ms: u64) -> Option<(String, String)> {
    let minutes = remaining_minutes(fissure.expires_at_secs, now_ms)?;
    let source = if fissure.railjack {
        Some(("Railjack", "Railjack"))
    } else if fissure.steel_path {
        Some(("Steel Path", "Route de l’Acier"))
    } else {
        None
    };
    Some((
        format_fissure(fissure, source.map(|labels| labels.0), minutes),
        format_fissure(fissure, source.map(|labels| labels.1), minutes),
    ))
}

fn format_fissure(fissure: &Fissure, source: Option<&str>, minutes: u64) -> String {
    match source {
        Some(source) => format!(
            "{} · {source} · {} · {} · {minutes} min",
            fissure.era.label(),
            fissure.mission_type,
            fissure.node
        ),
        None => format!(
            "{} · {} · {} · {minutes} min",
            fissure.era.label(),
            fissure.mission_type,
            fissure.node
        ),
    }
}

fn localized(
    english: impl Into<String>,
    french: impl Into<String>,
) -> Result<LocalizedText, GuestError> {
    Ok(LocalizedText::new(english).with_translation("fr", french)?)
}

fn unavailable() -> Result<LocalizedText, GuestError> {
    localized(
        "Worldstate data is unavailable.",
        "Les données de l’état mondial sont indisponibles.",
    )
}

overcrow_widget_sdk::export_widget!(crate::FissuresWidget);

#[cfg(test)]
#[path = "../tests/widget.rs"]
mod widget_tests;
