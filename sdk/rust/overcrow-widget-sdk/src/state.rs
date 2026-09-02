use alloc::{collections::BTreeMap, string::String, vec::Vec};
use core::{error::Error, fmt};

use crate::{BuildError, GrantedCapabilities, GuestError, HostEvent, InitInput, SessionData};

const MAX_LOCALE_BYTES: usize = 5;
const MAX_LOCALIZED_STRINGS: usize = 32;
const MAX_SETTINGS_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum RequestKind {
    Http,
    Storage,
}

#[derive(Clone, Default)]
pub(crate) struct OutputState {
    pub(crate) last_request_id: Option<u32>,
    pub(crate) outstanding: BTreeMap<u32, RequestKind>,
    pub(crate) published_revisions: BTreeMap<String, u64>,
}

impl OutputState {
    fn complete(&mut self, request_id: u32, kind: RequestKind) -> Result<(), GuestError> {
        if request_id == 0 || self.outstanding.get(&request_id) != Some(&kind) {
            return Err(GuestError::InvalidInput);
        }
        self.outstanding.remove(&request_id);
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Locale(String);

impl Locale {
    pub fn parse(value: impl Into<String>) -> Result<Self, LocaleError> {
        let value = value.into();
        if value.is_empty() || value.len() > MAX_LOCALE_BYTES || !value.is_ascii() {
            return Err(LocaleError);
        }
        let valid = match value.split_once('-') {
            None => value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_lowercase()),
            Some((language, region)) => {
                language.len() == 2
                    && language.bytes().all(|byte| byte.is_ascii_lowercase())
                    && region.len() == 2
                    && region.bytes().all(|byte| byte.is_ascii_uppercase())
            }
        };
        valid.then_some(Self(value)).ok_or(LocaleError)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocaleError;

impl fmt::Display for LocaleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("locale must be a canonical language or language-region ID")
    }
}

impl Error for LocaleError {}

pub struct LocalizedText {
    default: String,
    translations: BTreeMap<Locale, String>,
}

impl LocalizedText {
    pub fn new(default: impl Into<String>) -> Self {
        Self {
            default: default.into(),
            translations: BTreeMap::new(),
        }
    }

    pub fn with_translation(
        mut self,
        locale: impl Into<String>,
        text: impl Into<String>,
    ) -> Result<Self, BuildError> {
        if self.translations.len() >= MAX_LOCALIZED_STRINGS {
            return Err(BuildError::LocaleLimit);
        }
        let locale = Locale::parse(locale).map_err(|_| BuildError::Locale)?;
        if self.translations.insert(locale, text.into()).is_some() {
            return Err(BuildError::DuplicateLocale);
        }
        Ok(self)
    }

    pub fn resolve<'a>(&'a self, locale: &Locale) -> &'a str {
        self.translations
            .get(locale)
            .map_or(self.default.as_str(), String::as_str)
    }
}

pub struct WidgetContext {
    locale: Locale,
    granted_capabilities: GrantedCapabilities,
    settings: Vec<u8>,
    session_data: Option<SessionData>,
    output_state: OutputState,
}

impl WidgetContext {
    pub fn locale(&self) -> &Locale {
        &self.locale
    }

    pub fn granted_capabilities(&self) -> &GrantedCapabilities {
        &self.granted_capabilities
    }

    pub fn settings(&self) -> &[u8] {
        &self.settings
    }

    pub fn session_data(&self) -> Option<&SessionData> {
        self.session_data.as_ref()
    }

    pub(crate) fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    pub(crate) fn from_init(input: InitInput) -> Result<Self, GuestError> {
        if input.settings.len() > MAX_SETTINGS_BYTES {
            return Err(GuestError::InvalidInput);
        }
        Ok(Self {
            locale: Locale::parse(input.locale).map_err(|_| GuestError::InvalidInput)?,
            granted_capabilities: input.granted_capabilities,
            settings: input.settings,
            session_data: input.session_data,
            output_state: OutputState::default(),
        })
    }

    pub(crate) fn apply_event(&mut self, event: &HostEvent) -> Result<(), GuestError> {
        match event {
            #[cfg(feature = "api-v1")]
            HostEvent::HttpResult((request_id, _, _, _)) => {
                self.output_state.complete(*request_id, RequestKind::Http)?;
            }
            HostEvent::StorageResult((request_id, _)) => {
                self.output_state
                    .complete(*request_id, RequestKind::Storage)?;
            }
            HostEvent::LocaleChanged(locale) => {
                self.locale =
                    Locale::parse(locale.clone()).map_err(|_| GuestError::InvalidInput)?;
            }
            HostEvent::SettingsChanged((_, settings)) => {
                if settings.len() > MAX_SETTINGS_BYTES {
                    return Err(GuestError::InvalidInput);
                }
                self.settings.clone_from(settings);
            }
            HostEvent::SessionData(session_data) => self.session_data = Some(*session_data),
            _ => {}
        }
        Ok(())
    }

    #[cfg(feature = "api-v2")]
    pub(crate) fn complete_http(&mut self, request_id: u32) -> Result<(), GuestError> {
        self.output_state.complete(request_id, RequestKind::Http)
    }
}
