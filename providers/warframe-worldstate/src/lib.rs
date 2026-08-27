#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

mod labels;
mod parse;

use overcrow_widget_sdk::{
    GuestError, GuestOutput, HostEvent, OutputBuilder, Widget, WidgetContext,
};
#[cfg(test)]
use parse::parse_worldstate;
use parse::{ValidatedWorldstate, encode_worldstate, parse_bounded_worldstate};

const HTTP_HOST: &str = "api.warframe.com";
const HTTP_PATH: &str = "/cdn/worldState.php";
const PROVIDER_SCHEMA: &str = "com.playervox.overcrow.warframe.worldstate/worldstate.v1";
const REFRESH_MS: u64 = 60_000;
const STALE_MS: u64 = 300_000;
const MAX_HOST_UTC_MS: u64 = 253_402_300_799_999;

#[derive(Default)]
struct WorldstateProvider {
    last_request_id: u32,
    last_revision: u64,
    in_flight: Option<u32>,
    now_ms: Option<u64>,
    last_request_started_ms: Option<u64>,
    last_success_ms: Option<u64>,
    pending: Option<ValidatedWorldstate>,
    stale_reported: bool,
}

impl Widget for WorldstateProvider {
    fn init(&mut self, context: &mut WidgetContext) -> Result<GuestOutput, GuestError> {
        let grants = context.granted_capabilities();
        if !grants.provider || grants.http_hosts.as_slice() != [HTTP_HOST] {
            return Err(GuestError::Unavailable);
        }
        self.start_request(context)
    }

    fn handle(
        &mut self,
        event: HostEvent,
        context: &mut WidgetContext,
    ) -> Result<GuestOutput, GuestError> {
        match event {
            HostEvent::Tick(now_ms) => self.handle_tick(now_ms, context),
            HostEvent::HttpResult((request_id, status, body, _)) => {
                self.handle_http(request_id, status, &body, context)
            }
            HostEvent::LocaleChanged(_)
            | HostEvent::SettingsChanged(_)
            | HostEvent::SessionData(_) => {
                self.ensure_fresh()?;
                self.idle_output(context)
            }
            HostEvent::Interaction(_)
            | HostEvent::StorageResult(_)
            | HostEvent::ProviderData(_) => Err(GuestError::InvalidInput),
        }
    }
}

impl WorldstateProvider {
    fn handle_tick(
        &mut self,
        now_ms: u64,
        context: &mut WidgetContext,
    ) -> Result<GuestOutput, GuestError> {
        if now_ms > MAX_HOST_UTC_MS || self.now_ms.is_some_and(|previous| now_ms < previous) {
            return Err(GuestError::InvalidInput);
        }
        self.now_ms = Some(now_ms);
        if self.last_request_id != 0 && self.last_request_started_ms.is_none() {
            self.last_request_started_ms = Some(now_ms);
        }
        if let Some(pending) = self.pending.take() {
            return self.publish(pending, now_ms, context);
        }
        let stale = self.is_stale(now_ms)?;
        if stale && !self.stale_reported {
            self.stale_reported = true;
            return Err(GuestError::Unavailable);
        }
        let refresh_due = match self.last_request_started_ms {
            Some(started) => {
                now_ms
                    .checked_sub(started)
                    .ok_or(GuestError::InvalidState)?
                    >= REFRESH_MS
            }
            None => false,
        };
        if self.in_flight.is_none() && refresh_due {
            return self.start_request(context);
        }
        if stale {
            return Err(GuestError::Unavailable);
        }
        self.idle_output(context)
    }

    fn handle_http(
        &mut self,
        request_id: u32,
        status: Option<u16>,
        body: &[u8],
        context: &mut WidgetContext,
    ) -> Result<GuestOutput, GuestError> {
        if self.in_flight != Some(request_id) {
            return Err(GuestError::InvalidInput);
        }
        self.in_flight = None;
        if status != Some(200) {
            return self.refresh_failed(context);
        }
        let parsed = match parse_bounded_worldstate(body) {
            Ok(parsed) => parsed,
            Err(_) => return self.refresh_failed(context),
        };
        if let Some(now_ms) = self.now_ms {
            self.publish(parsed, now_ms, context)
        } else {
            self.pending = Some(parsed);
            self.idle_output(context)
        }
    }

    fn refresh_failed(&self, context: &mut WidgetContext) -> Result<GuestOutput, GuestError> {
        match (self.now_ms, self.last_success_ms) {
            (Some(now), Some(_)) if !self.is_stale(now)? => self.idle_output(context),
            _ => Err(GuestError::Unavailable),
        }
    }

    fn publish(
        &mut self,
        parsed: ValidatedWorldstate,
        now_ms: u64,
        context: &mut WidgetContext,
    ) -> Result<GuestOutput, GuestError> {
        let payload = parsed
            .at(now_ms / 1_000)
            .map_err(|_| GuestError::Unavailable)?;
        let payload = encode_worldstate(&payload).map_err(|_| GuestError::Unavailable)?;
        let revision = self
            .last_revision
            .checked_add(1)
            .ok_or(GuestError::Unavailable)?;
        let mut output = OutputBuilder::new(context);
        output.provider_publish(PROVIDER_SCHEMA, revision, &payload)?;
        output.next_wake_ms(REFRESH_MS as u32)?;
        let output = output.finish();
        self.last_revision = revision;
        self.last_success_ms = Some(now_ms);
        self.stale_reported = false;
        Ok(output)
    }

    fn start_request(&mut self, context: &mut WidgetContext) -> Result<GuestOutput, GuestError> {
        if self.in_flight.is_some() {
            return Err(GuestError::InvalidState);
        }
        let request_id = self
            .last_request_id
            .checked_add(1)
            .ok_or(GuestError::Unavailable)?;
        let mut output = OutputBuilder::new(context);
        output.http_get(request_id, HTTP_HOST, HTTP_PATH)?;
        output.next_wake_ms(REFRESH_MS as u32)?;
        let output = output.finish();
        self.last_request_id = request_id;
        self.in_flight = Some(request_id);
        if let Some(now_ms) = self.now_ms {
            self.last_request_started_ms = Some(now_ms);
        }
        Ok(output)
    }

    fn idle_output(&self, context: &mut WidgetContext) -> Result<GuestOutput, GuestError> {
        let mut wait_ms = REFRESH_MS;
        if let (Some(now), Some(success)) = (self.now_ms, self.last_success_ms) {
            let elapsed = now.checked_sub(success).ok_or(GuestError::InvalidState)?;
            wait_ms = wait_ms.min(
                STALE_MS
                    .checked_sub(elapsed)
                    .ok_or(GuestError::Unavailable)?,
            );
        }
        if self.in_flight.is_none()
            && let (Some(now), Some(started)) = (self.now_ms, self.last_request_started_ms)
        {
            let elapsed = now.checked_sub(started).ok_or(GuestError::InvalidState)?;
            wait_ms = if elapsed >= REFRESH_MS {
                100
            } else {
                wait_ms.min(REFRESH_MS - elapsed)
            };
        }
        let mut output = OutputBuilder::new(context);
        let wait_ms =
            u32::try_from(wait_ms.clamp(100, REFRESH_MS)).map_err(|_| GuestError::InvalidState)?;
        output.next_wake_ms(wait_ms)?;
        Ok(output.finish())
    }

    fn ensure_fresh(&self) -> Result<(), GuestError> {
        if let Some(now_ms) = self.now_ms
            && self.is_stale(now_ms)?
        {
            return Err(GuestError::Unavailable);
        }
        Ok(())
    }

    fn is_stale(&self, now_ms: u64) -> Result<bool, GuestError> {
        self.last_success_ms.map_or(Ok(false), |success| {
            now_ms
                .checked_sub(success)
                .map(|elapsed| elapsed >= STALE_MS)
                .ok_or(GuestError::InvalidState)
        })
    }
}

overcrow_widget_sdk::export_widget!(crate::WorldstateProvider);

#[cfg(test)]
#[path = "../tests/provider.rs"]
mod provider_tests;
