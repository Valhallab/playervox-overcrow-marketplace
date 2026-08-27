use alloc::{string::String, vec::Vec};

use serde::{Deserialize, Serialize};

use crate::model::{SideFilter, StatusFilter, safe_slug};

pub(crate) const STORAGE_KEY: &str = "state";
const SCHEMA_VERSION: u8 = 1;
const MAX_STORAGE_BYTES: usize = 4 * 1024;
const MAX_WATCHLIST: usize = 16;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Preferences {
    schema_version: u8,
    pub(crate) side: SideFilter,
    pub(crate) status: StatusFilter,
    pub(crate) selected_slug: Option<String>,
    pub(crate) watchlist: Vec<String>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            side: SideFilter::default(),
            status: StatusFilter::default(),
            selected_slug: None,
            watchlist: Vec::new(),
        }
    }
}

impl Preferences {
    pub(crate) fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() > MAX_STORAGE_BYTES {
            return None;
        }
        let mut value: Self = serde_json::from_slice(bytes).ok()?;
        if value.schema_version != SCHEMA_VERSION
            || value
                .selected_slug
                .as_deref()
                .is_some_and(|slug| !safe_slug(slug))
            || value.watchlist.len() > MAX_WATCHLIST
            || value.watchlist.iter().any(|slug| !safe_slug(slug))
        {
            return None;
        }
        value.watchlist.sort();
        value.watchlist.dedup();
        Some(value)
    }

    pub(crate) fn encode(&self) -> Option<Vec<u8>> {
        let bytes = serde_json::to_vec(self).ok()?;
        (bytes.len() <= MAX_STORAGE_BYTES).then_some(bytes)
    }

    pub(crate) fn selected_is_watched(&self) -> bool {
        self.selected_slug
            .as_ref()
            .is_some_and(|selected| self.watchlist.binary_search(selected).is_ok())
    }

    pub(crate) fn set_selected_watched(&mut self, watched: bool) -> bool {
        let Some(selected) = self.selected_slug.clone() else {
            return false;
        };
        match (self.watchlist.binary_search(&selected), watched) {
            (Ok(index), false) => {
                self.watchlist.remove(index);
                true
            }
            (Err(index), true) if self.watchlist.len() < MAX_WATCHLIST => {
                self.watchlist.insert(index, selected);
                true
            }
            _ => false,
        }
    }
}
