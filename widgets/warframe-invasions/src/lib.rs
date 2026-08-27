#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use alloc::{borrow::ToOwned, format, string::String, vec, vec::Vec};

use overcrow_widget_sdk::{
    GuestError, GuestOutput, HostEvent, InteractionKind, LocalizedText, OutputBuilder, ViewBuilder,
    Widget, WidgetContext,
};
use serde::{Deserialize, Serialize};
use warframe_widget_data::{
    Invasion, MAX_HOST_UTC_MS, Reward, SESSION_SCHEMA, STEAM_APP_ID, Worldstate, parse,
    valid_public_id,
};

pub(crate) use warframe_widget_data::{PROVIDER_ID, PROVIDER_SCHEMA};

const STORAGE_KEY: &str = "state";
const STORAGE_SCHEMA_VERSION: u8 = 1;
const MAX_STORAGE_BYTES: usize = 4 * 1024;
const MAX_COMPLETIONS: usize = 32;
const WAKE_MS: u32 = 60_000;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredState {
    schema_version: u8,
    compact: bool,
    entries: Vec<Completion>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Completion {
    key: String,
    completed_at_secs: u64,
}

impl Default for StoredState {
    fn default() -> Self {
        Self {
            schema_version: STORAGE_SCHEMA_VERSION,
            compact: false,
            entries: Vec::new(),
        }
    }
}

enum StorageAction {
    Get(u32),
    Set(u32, Vec<u8>),
}

#[derive(Default)]
struct InvasionsWidget {
    data: Option<Worldstate>,
    last_provider_revision: u64,
    now_ms: Option<u64>,
    view_revision: u64,
    state: StoredState,
    load_request: Option<u32>,
    store_request: Option<u32>,
    next_request_id: u32,
    state_changed: bool,
    store_queued: bool,
}

impl Widget for InvasionsWidget {
    fn init(&mut self, context: &mut WidgetContext) -> Result<GuestOutput, GuestError> {
        validate_authority(context)?;
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
                if self.prune_completions() {
                    storage_action = self.queue_store()?;
                }
            }
            HostEvent::ProviderData((provider, schema, revision, payload)) => {
                if provider != PROVIDER_ID || schema != PROVIDER_SCHEMA || revision == 0 {
                    return Err(GuestError::InvalidInput);
                }
                if revision > self.last_provider_revision {
                    self.last_provider_revision = revision;
                    self.data = parse(&payload).ok();
                    if self.prune_completions() {
                        storage_action = self.queue_store()?;
                    }
                }
            }
            HostEvent::Interaction(interaction) => {
                if !warframe_active(context) {
                    return Err(GuestError::Unavailable);
                }
                let changed = if interaction.element_id == "view-compact" {
                    match interaction.kind {
                        InteractionKind::Toggled(value) => {
                            let changed = self.state.compact != value;
                            self.state.compact = value;
                            changed
                        }
                        InteractionKind::Focused(_) | InteractionKind::Hovered(_) => false,
                        _ => return Err(GuestError::InvalidInput),
                    }
                } else {
                    let target = self.completion_target(&interaction.element_id)?;
                    match interaction.kind {
                        InteractionKind::Toggled(value) => self.set_completed(&target, value)?,
                        InteractionKind::Focused(_) | InteractionKind::Hovered(_) => false,
                        _ => return Err(GuestError::InvalidInput),
                    }
                };
                if changed {
                    storage_action = self.queue_store()?;
                }
            }
            HostEvent::StorageResult((request_id, value)) => {
                if self.load_request == Some(request_id) {
                    self.load_request = None;
                    if !self.state_changed {
                        self.state = value
                            .as_deref()
                            .and_then(StoredState::parse)
                            .unwrap_or_default();
                        if self.prune_completions() {
                            storage_action = self.queue_store()?;
                        }
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

impl InvasionsWidget {
    fn allocate_request(&mut self) -> Result<u32, GuestError> {
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .ok_or(GuestError::Unavailable)?;
        Ok(self.next_request_id)
    }

    fn start_store(&mut self) -> Result<StorageAction, GuestError> {
        let request_id = self.allocate_request()?;
        let bytes = serde_json::to_vec(&self.state).map_err(|_| GuestError::Unavailable)?;
        self.store_request = Some(request_id);
        Ok(StorageAction::Set(request_id, bytes))
    }

    fn queue_store(&mut self) -> Result<Option<StorageAction>, GuestError> {
        self.state_changed = true;
        if self.store_request.is_some() {
            self.store_queued = true;
            Ok(None)
        } else {
            self.start_store().map(Some)
        }
    }

    fn current_data(&self) -> Option<&Worldstate> {
        let now_ms = self.now_ms?;
        self.data.as_ref().filter(|data| data.is_fresh_at(now_ms))
    }

    fn completion_target(&self, id: &str) -> Result<String, GuestError> {
        self.current_data()
            .and_then(|data| {
                data.invasions
                    .iter()
                    .filter(|invasion| !invasion.completed)
                    .map(invasion_id)
                    .find(|candidate| candidate == id)
            })
            .ok_or(GuestError::InvalidInput)
    }

    fn set_completed(&mut self, target: &str, value: bool) -> Result<bool, GuestError> {
        let completed_at_secs = self
            .now_ms
            .and_then(|now_ms| now_ms.checked_div(1_000))
            .filter(|value| *value > 0)
            .ok_or(GuestError::Unavailable)?;
        match self
            .state
            .entries
            .binary_search_by(|entry| entry.key.as_str().cmp(target))
        {
            Ok(index) if !value => {
                self.state.entries.remove(index);
                Ok(true)
            }
            Err(index) if value => {
                if self.state.entries.len() >= MAX_COMPLETIONS {
                    return Err(GuestError::Unavailable);
                }
                self.state.entries.insert(
                    index,
                    Completion {
                        key: target.to_owned(),
                        completed_at_secs,
                    },
                );
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn prune_completions(&mut self) -> bool {
        let Some(data) = self.current_data() else {
            return false;
        };
        let valid_ids = data
            .invasions
            .iter()
            .filter(|invasion| !invasion.completed)
            .map(invasion_id)
            .collect::<Vec<_>>();
        let previous = self.state.entries.len();
        self.state
            .entries
            .retain(|entry| valid_ids.binary_search(&entry.key).is_ok());
        self.state.entries.len() != previous
    }

    fn is_completed(&self, id: &str) -> bool {
        self.state
            .entries
            .binary_search_by(|entry| entry.key.as_str().cmp(id))
            .is_ok()
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
        let mut nodes = vec![builder.text(localized("Warframe Invasions", "Invasions Warframe")?)?];
        nodes.push(builder.toggle(
            "view-compact",
            localized("Compact view", "Vue compacte")?,
            self.state.compact,
        )?);

        if !warframe_active(context) {
            nodes.push(builder.text(localized(
                "Waiting for an active Warframe session.",
                "En attente d’une session Warframe active.",
            )?)?);
        } else if let Some(data) = self.current_data() {
            let mut found = false;
            for invasion in data.invasions.iter().filter(|invasion| !invasion.completed) {
                found = true;
                let id = invasion_id(invasion);
                nodes.push(builder.toggle(
                    &id,
                    LocalizedText::new(invasion_label(invasion, self.state.compact)),
                    self.is_completed(&id),
                )?);
                nodes.push(builder.progress(
                    LocalizedText::new(format!(
                        "{} / {}",
                        invasion.attacker_faction, invasion.defender_faction
                    )),
                    invasion_progress(invasion),
                )?);
                for reward in [
                    invasion.attacker_reward.as_ref(),
                    invasion.defender_reward.as_ref(),
                ]
                .into_iter()
                .flatten()
                {
                    nodes.push(builder.text(LocalizedText::new(reward_label(reward)))?);
                }
            }
            if !found {
                nodes.push(builder.text(localized(
                    "No active invasions.",
                    "Aucune invasion active.",
                )?)?);
            }
        } else if self.data.is_some() && self.now_ms.is_none() {
            nodes.push(builder.text(localized("Synchronizing time…", "Synchronisation…")?)?);
        } else {
            nodes.push(builder.text(unavailable()?)?);
        }

        let root = builder.container(&nodes)?;
        let view = builder.finish(root, self.view_revision)?;
        let mut output = OutputBuilder::new(context).view(view)?;
        match storage_action {
            Some(StorageAction::Get(request_id)) => output.storage_get(request_id, STORAGE_KEY)?,
            Some(StorageAction::Set(request_id, bytes)) => {
                output.storage_set(request_id, STORAGE_KEY, &bytes)?;
            }
            None => {}
        }
        output.next_wake_ms(WAKE_MS)?;
        Ok(output.finish())
    }
}

impl StoredState {
    fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.is_empty() || bytes.len() > MAX_STORAGE_BYTES {
            return None;
        }
        let state = serde_json::from_slice::<Self>(bytes).ok()?;
        if state.schema_version != STORAGE_SCHEMA_VERSION || state.entries.len() > MAX_COMPLETIONS {
            return None;
        }
        for (index, entry) in state.entries.iter().enumerate() {
            if !valid_stored_id(&entry.key)
                || entry.completed_at_secs == 0
                || entry.completed_at_secs > MAX_HOST_UTC_MS / 1_000
                || (index > 0 && state.entries[index - 1].key >= entry.key)
            {
                return None;
            }
        }
        Some(state)
    }
}

fn validate_authority(context: &WidgetContext) -> Result<(), GuestError> {
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
    Ok(())
}

fn warframe_active(context: &WidgetContext) -> bool {
    context.session_data().is_some_and(|session| {
        session.selected_active && session.steam_app_id == Some(STEAM_APP_ID)
    })
}

fn invasion_id(invasion: &Invasion) -> String {
    format!("inv:{}", invasion.instance_id)
}

fn valid_stored_id(id: &str) -> bool {
    id.len() <= 64 && id.strip_prefix("inv:").is_some_and(valid_public_id)
}

fn invasion_label(invasion: &Invasion, compact: bool) -> String {
    if compact {
        invasion.node.clone()
    } else {
        format!(
            "{} · {} vs {}",
            invasion.node, invasion.attacker_faction, invasion.defender_faction
        )
    }
}

fn invasion_progress(invasion: &Invasion) -> u16 {
    let goal = invasion.goal.unsigned_abs();
    let progress = invasion.count.unsigned_abs().min(goal);
    ((u128::from(progress) * 1_000) / u128::from(goal)) as u16
}

fn reward_label(reward: &Reward) -> String {
    format!("{} ×{}", reward.label, reward.count)
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

overcrow_widget_sdk::export_widget!(crate::InvasionsWidget);

#[cfg(test)]
#[path = "../tests/widget.rs"]
mod widget_tests;
