#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use alloc::{borrow::ToOwned, format, string::String, vec, vec::Vec};

use overcrow_widget_sdk::{
    GuestError, GuestOutput, HostEvent, InteractionKind, LocalizedText, OutputBuilder, ViewBuilder,
    Widget, WidgetContext,
};
use serde::{Deserialize, Serialize};
use warframe_widget_data::{
    Activity, MAX_HOST_UTC_MS, SESSION_SCHEMA, STEAM_APP_ID, Worldstate, parse, remaining_minutes,
    valid_public_id,
};

pub(crate) use warframe_widget_data::{PROVIDER_ID, PROVIDER_SCHEMA};

const STORAGE_KEY: &str = "completion";
const STORAGE_SCHEMA_VERSION: u8 = 1;
const MAX_STORAGE_BYTES: usize = 4 * 1024;
const MAX_COMPLETIONS: usize = 16;
const WAKE_MS: u32 = 60_000;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredCompletion {
    schema_version: u8,
    entries: Vec<Completion>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Completion {
    key: String,
    completed_at_secs: u64,
}

impl Default for StoredCompletion {
    fn default() -> Self {
        Self {
            schema_version: STORAGE_SCHEMA_VERSION,
            entries: Vec::new(),
        }
    }
}

enum StorageAction {
    Get(u32),
    Set(u32, Vec<u8>),
}

#[derive(Default)]
struct SortieArchonWidget {
    data: Option<Worldstate>,
    last_provider_revision: u64,
    now_ms: Option<u64>,
    view_revision: u64,
    completion: StoredCompletion,
    load_request: Option<u32>,
    store_request: Option<u32>,
    next_request_id: u32,
    state_changed: bool,
    store_queued: bool,
}

impl Widget for SortieArchonWidget {
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
                if self.prune_completion_state() {
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
                    if self.prune_completion_state() {
                        storage_action = self.queue_store()?;
                    }
                }
            }
            HostEvent::Interaction(interaction) => {
                if !warframe_active(context) {
                    return Err(GuestError::Unavailable);
                }
                let targets = self.completion_targets(&interaction.element_id)?;
                let changed = match interaction.kind {
                    InteractionKind::Toggled(value) => self.set_completed(&targets, value)?,
                    InteractionKind::Focused(_) | InteractionKind::Hovered(_) => false,
                    _ => return Err(GuestError::InvalidInput),
                };
                if changed {
                    storage_action = self.queue_store()?;
                }
            }
            HostEvent::StorageResult((request_id, value)) => {
                if self.load_request == Some(request_id) {
                    self.load_request = None;
                    if !self.state_changed {
                        self.completion = value
                            .as_deref()
                            .and_then(StoredCompletion::parse)
                            .unwrap_or_default();
                        if self.prune_completion_state() {
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

impl SortieArchonWidget {
    fn allocate_request(&mut self) -> Result<u32, GuestError> {
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .ok_or(GuestError::Unavailable)?;
        Ok(self.next_request_id)
    }

    fn start_store(&mut self) -> Result<StorageAction, GuestError> {
        let request_id = self.allocate_request()?;
        let bytes = serde_json::to_vec(&self.completion).map_err(|_| GuestError::Unavailable)?;
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

    fn current_data(&self) -> Option<(&Worldstate, u64)> {
        let now_ms = self.now_ms?;
        self.data
            .as_ref()
            .filter(|data| data.is_fresh_at(now_ms))
            .map(|data| (data, now_ms))
    }

    fn completion_targets(&self, id: &str) -> Result<Vec<String>, GuestError> {
        let (data, now_ms) = self.current_data().ok_or(GuestError::Unavailable)?;
        for activity in [&data.sortie, &data.archon].into_iter().flatten() {
            if remaining_minutes(activity.expires_at_secs, now_ms).is_none() {
                continue;
            }
            if activity.id == id {
                return Ok(activity
                    .missions
                    .iter()
                    .map(|mission| mission.id.clone())
                    .collect());
            }
            if activity.missions.iter().any(|mission| mission.id == id) {
                return Ok(vec![id.to_owned()]);
            }
        }
        Err(GuestError::InvalidInput)
    }

    fn set_completed(&mut self, targets: &[String], value: bool) -> Result<bool, GuestError> {
        let completed_at_secs = self
            .now_ms
            .and_then(|now_ms| now_ms.checked_div(1_000))
            .filter(|value| *value > 0)
            .ok_or(GuestError::Unavailable)?;
        let mut changed = false;
        for target in targets {
            match self
                .completion
                .entries
                .binary_search_by(|entry| entry.key.as_str().cmp(target))
            {
                Ok(index) if !value => {
                    self.completion.entries.remove(index);
                    changed = true;
                }
                Err(index) if value => {
                    if self.completion.entries.len() >= MAX_COMPLETIONS {
                        return Err(GuestError::Unavailable);
                    }
                    self.completion.entries.insert(
                        index,
                        Completion {
                            key: target.clone(),
                            completed_at_secs,
                        },
                    );
                    changed = true;
                }
                _ => {}
            }
        }
        Ok(changed)
    }

    fn prune_completion_state(&mut self) -> bool {
        let expired = self.prune_expired_completions();
        let unknown = self.prune_unknown_completions();
        expired || unknown
    }

    fn prune_expired_completions(&mut self) -> bool {
        let (Some(data), Some(now_ms)) = (&self.data, self.now_ms) else {
            return false;
        };
        let mut expired_ids = [&data.sortie, &data.archon]
            .into_iter()
            .flatten()
            .filter(|activity| remaining_minutes(activity.expires_at_secs, now_ms).is_none())
            .flat_map(|activity| activity.missions.iter().map(|mission| mission.id.clone()))
            .collect::<Vec<_>>();
        expired_ids.sort();
        let previous = self.completion.entries.len();
        self.completion
            .entries
            .retain(|entry| expired_ids.binary_search(&entry.key).is_err());
        self.completion.entries.len() != previous
    }

    fn prune_unknown_completions(&mut self) -> bool {
        let Some((data, now_ms)) = self.current_data() else {
            return false;
        };
        let mut valid_ids = [&data.sortie, &data.archon]
            .into_iter()
            .flatten()
            .filter(|activity| remaining_minutes(activity.expires_at_secs, now_ms).is_some())
            .flat_map(|activity| activity.missions.iter().map(|mission| mission.id.clone()))
            .collect::<Vec<_>>();
        valid_ids.sort();
        let previous = self.completion.entries.len();
        self.completion
            .entries
            .retain(|entry| valid_ids.binary_search(&entry.key).is_ok());
        self.completion.entries.len() != previous
    }

    fn is_completed(&self, id: &str) -> bool {
        self.completion
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
        let mut nodes = vec![builder.text(localized(
            "Sortie & Archon Hunt",
            "Sortie et Chasse à l’Archonte",
        )?)?];

        if !warframe_active(context) {
            nodes.push(builder.text(localized(
                "Waiting for an active Warframe session.",
                "En attente d’une session Warframe active.",
            )?)?);
        } else if let Some((data, now_ms)) = self.current_data() {
            let mut found = false;
            if let Some(activity) = &data.sortie
                && render_activity(&mut builder, &mut nodes, activity, now_ms, false, |id| {
                    self.is_completed(id)
                })?
            {
                found = true;
            }
            if let Some(activity) = &data.archon
                && render_activity(&mut builder, &mut nodes, activity, now_ms, true, |id| {
                    self.is_completed(id)
                })?
            {
                found = true;
            }
            if !found {
                nodes.push(builder.text(localized(
                    "No active Sortie or Archon Hunt.",
                    "Aucune Sortie ni Chasse à l’Archonte active.",
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

impl StoredCompletion {
    fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.is_empty() || bytes.len() > MAX_STORAGE_BYTES {
            return None;
        }
        let state = serde_json::from_slice::<Self>(bytes).ok()?;
        if state.schema_version != STORAGE_SCHEMA_VERSION || state.entries.len() > MAX_COMPLETIONS {
            return None;
        }
        for (index, entry) in state.entries.iter().enumerate() {
            if !valid_public_id(&entry.key)
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

fn render_activity(
    builder: &mut ViewBuilder,
    nodes: &mut Vec<overcrow_widget_sdk::NodeId>,
    activity: &Activity,
    now_ms: u64,
    archon: bool,
    is_completed: impl Fn(&str) -> bool,
) -> Result<bool, GuestError> {
    let Some(minutes) = remaining_minutes(activity.expires_at_secs, now_ms) else {
        return Ok(false);
    };
    let block_completed = activity
        .missions
        .iter()
        .all(|mission| is_completed(&mission.id));
    let english_kind = if archon { "Archon Hunt" } else { "Sortie" };
    let french_kind = if archon {
        "Chasse à l’Archonte"
    } else {
        "Sortie"
    };
    nodes.push(builder.toggle(
        &activity.id,
        localized(
            format!("{english_kind} · {} · {minutes} min", activity.boss),
            format!("{french_kind} · {} · {minutes} min", activity.boss),
        )?,
        block_completed,
    )?);
    if archon {
        nodes.push(builder.text(shard_label(&activity.boss)?)?);
    }
    for mission in &activity.missions {
        let label = match &mission.modifier {
            Some(modifier) => format!("{} · {} · {modifier}", mission.mission_type, mission.node),
            None => format!("{} · {}", mission.mission_type, mission.node),
        };
        nodes.push(builder.toggle(
            &mission.id,
            LocalizedText::new(label),
            is_completed(&mission.id),
        )?);
    }
    Ok(true)
}

fn shard_label(boss: &str) -> Result<LocalizedText, GuestError> {
    match boss {
        "Archon Boreal" => localized("Azure Archon Shard", "Éclat d’Archonte azur"),
        "Archon Amar" => localized("Crimson Archon Shard", "Éclat d’Archonte pourpre"),
        "Archon Nira" => localized("Amber Archon Shard", "Éclat d’Archonte ambre"),
        _ => localized("Archon Shard", "Éclat d’Archonte"),
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

overcrow_widget_sdk::export_widget!(crate::SortieArchonWidget);

#[cfg(test)]
#[path = "../tests/widget.rs"]
mod widget_tests;
