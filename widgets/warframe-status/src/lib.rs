#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use alloc::{format, string::String, vec::Vec};

use overcrow_widget_sdk::{
    GuestError, GuestOutput, HostEvent, LocalizedText, OutputBuilder, ViewBuilder, Widget,
    WidgetContext,
};
use warframe_widget_data::{
    MAX_HOST_UTC_MS, SESSION_SCHEMA, STEAM_APP_ID, StatusRow, Worldstate, parse, remaining_minutes,
};

pub(crate) use warframe_widget_data::{PROVIDER_ID, PROVIDER_SCHEMA};

const WAKE_MS: u32 = 60_000;

#[derive(Default)]
struct StatusWidget {
    data: Option<Worldstate>,
    last_provider_revision: u64,
    now_ms: Option<u64>,
    view_revision: u64,
}

impl Widget for StatusWidget {
    fn init(&mut self, context: &mut WidgetContext) -> Result<GuestOutput, GuestError> {
        if !context.settings().is_empty() {
            return Err(GuestError::InvalidInput);
        }
        let grants = context.granted_capabilities();
        if !grants.http_hosts.is_empty()
            || !matches!(grants.game_data.as_slice(), [value] if value == SESSION_SCHEMA)
            || grants.storage
            || grants.clipboard_write
            || grants.provider
        {
            return Err(GuestError::Unavailable);
        }
        self.render(context)
    }

    fn handle(
        &mut self,
        event: HostEvent,
        context: &mut WidgetContext,
    ) -> Result<GuestOutput, GuestError> {
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
            HostEvent::LocaleChanged(_) | HostEvent::SessionData(_) => {}
            HostEvent::SettingsChanged(_) => {
                if !context.settings().is_empty() {
                    return Err(GuestError::InvalidInput);
                }
            }
            HostEvent::Interaction(_) | HostEvent::HttpResult(_) | HostEvent::StorageResult(_) => {
                return Err(GuestError::InvalidInput);
            }
        }
        self.render(context)
    }
}

impl StatusWidget {
    fn render(&mut self, context: &mut WidgetContext) -> Result<GuestOutput, GuestError> {
        self.view_revision = self
            .view_revision
            .checked_add(1)
            .ok_or(GuestError::Unavailable)?;
        let mut builder = ViewBuilder::new(context.locale());
        let mut nodes = Vec::new();
        nodes.push(builder.text(localized("Warframe Status", "Statut Warframe")?)?);

        if !warframe_active(context) {
            nodes.push(builder.text(localized(
                "Waiting for an active Warframe session.",
                "En attente d’une session Warframe active.",
            )?)?);
        } else if let (Some(now_ms), Some(data)) = (self.now_ms, &self.data) {
            if data.is_fresh_at(now_ms) {
                for row in &data.status.rows {
                    if let Some((english, french)) = status_text(row, now_ms) {
                        nodes.push(builder.text(localized(english, french)?)?);
                    }
                }
                if nodes.len() == 1 {
                    nodes.push(builder.text(localized(
                        "No active worldstate rows.",
                        "Aucun statut mondial actif.",
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
        output.next_wake_ms(WAKE_MS)?;
        Ok(output.finish())
    }
}

fn warframe_active(context: &WidgetContext) -> bool {
    context.session_data().is_some_and(|session| {
        session.selected_active && session.steam_app_id == Some(STEAM_APP_ID)
    })
}

fn status_text(row: &StatusRow, now_ms: u64) -> Option<(String, String)> {
    let (target_secs, state_override) =
        if row.id == "baro" && row.state.as_deref() == Some("incoming") {
            let activation_secs = row.activation_secs?;
            if now_ms / 1_000 < activation_secs {
                (activation_secs, None)
            } else {
                (row.expires_at_secs, Some(("Present", "Présent")))
            }
        } else {
            (row.expires_at_secs, None)
        };
    let minutes = remaining_minutes(target_secs, now_ms)?;
    let (label_en, label_fr) = match row.id.as_str() {
        "cetus" => ("Cetus", "Cetus"),
        "cambion" => ("Cambion Drift", "Puy de Cambion"),
        "vallis" => ("Orb Vallis", "Vallée Orbis"),
        "zariman" => ("Zariman", "Zariman"),
        "daily-reset" => ("Daily Reset", "Réinitialisation quotidienne"),
        "baro" => ("Baro Ki'Teer", "Baro Ki'Teer"),
        _ => return None,
    };
    let state = state_override.or_else(|| row.state.as_deref().and_then(state_labels));
    let english = format_status(
        label_en,
        state.map(|value| value.0),
        row.location.as_deref(),
        minutes,
    );
    let french = format_status(
        label_fr,
        state.map(|value| value.1),
        row.location.as_deref(),
        minutes,
    );
    Some((english, french))
}

fn state_labels(state: &str) -> Option<(&'static str, &'static str)> {
    match state {
        "day" => Some(("Day", "Jour")),
        "night" => Some(("Night", "Nuit")),
        "fass" => Some(("Fass", "Fass")),
        "vome" => Some(("Vome", "Vome")),
        "warm" => Some(("Warm", "Chaud")),
        "cold" => Some(("Cold", "Froid")),
        "corpus" => Some(("Corpus", "Corpus")),
        "grineer" => Some(("Grineer", "Grineer")),
        "present" => Some(("Present", "Présent")),
        "incoming" => Some(("Incoming", "À venir")),
        _ => None,
    }
}

fn format_status(label: &str, state: Option<&str>, location: Option<&str>, minutes: u64) -> String {
    match (state, location) {
        (Some(state), Some(location)) => {
            format!("{label} · {state} · {location} · {minutes} min")
        }
        (Some(state), None) => format!("{label} · {state} · {minutes} min"),
        (None, Some(location)) => format!("{label} · {location} · {minutes} min"),
        (None, None) => format!("{label} · {minutes} min"),
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

overcrow_widget_sdk::export_widget!(crate::StatusWidget);

#[cfg(test)]
#[path = "../tests/widget.rs"]
mod widget_tests;
