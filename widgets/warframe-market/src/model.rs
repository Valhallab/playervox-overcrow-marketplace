use alloc::{format, string::String, vec::Vec};

use serde::{Deserialize, Serialize};

pub const MAX_QUERY_CHARS: usize = 64;
pub const MAX_SLUG_BYTES: usize = 96;
pub const MAX_RESULTS: usize = 12;
pub const MAX_ORDERS_PER_SIDE: usize = 6;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SideFilter {
    #[default]
    All,
    Sellers,
    Buyers,
}

impl SideFilter {
    pub const fn index(self) -> u32 {
        match self {
            Self::All => 0,
            Self::Sellers => 1,
            Self::Buyers => 2,
        }
    }

    pub fn from_index(index: u32) -> Option<Self> {
        match index {
            0 => Some(Self::All),
            1 => Some(Self::Sellers),
            2 => Some(Self::Buyers),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StatusFilter {
    #[default]
    Online,
    Ingame,
    Any,
}

impl StatusFilter {
    pub const fn index(self) -> u32 {
        match self {
            Self::Online => 0,
            Self::Ingame => 1,
            Self::Any => 2,
        }
    }

    pub fn from_index(index: u32) -> Option<Self> {
        match index {
            0 => Some(Self::Online),
            1 => Some(Self::Ingame),
            2 => Some(Self::Any),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TradeSide {
    Sell,
    Buy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Presence {
    Ingame,
    Online,
    Offline,
    Unknown,
}

impl Presence {
    pub const fn rank(self) -> u8 {
        match self {
            Self::Ingame => 0,
            Self::Online => 1,
            Self::Unknown => 2,
            Self::Offline => 3,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarketItem {
    pub name: String,
    pub slug: String,
    pub element_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarketOrder {
    pub public_id: String,
    pub element_id: String,
    pub side: TradeSide,
    pub platinum: u32,
    pub trader: String,
    pub presence: Presence,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarketDetail {
    pub name: String,
    pub slug: String,
    pub orders: Vec<MarketOrder>,
    pub fetched_at_ms: u64,
}

pub fn normalize_query(value: &str) -> Option<String> {
    let value = value.trim();
    if value.chars().count() > MAX_QUERY_CHARS
        || value.len() > MAX_QUERY_CHARS * 4
        || value.chars().any(char::is_control)
    {
        return None;
    }
    Some(value.into())
}

pub fn safe_slug(value: &str) -> bool {
    (1..=MAX_SLUG_BYTES).contains(&value.len())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
}

pub fn safe_public_token(value: &str) -> bool {
    (1..=96).contains(&value.len())
        && value.is_ascii()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub fn sanitize_display(value: &str, maximum_chars: usize) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        return None;
    }
    let mut output = value.chars().take(maximum_chars).collect::<String>();
    if value.chars().count() > maximum_chars {
        output.pop();
        output.push('…');
    }
    Some(output)
}

pub fn sanitize_trader(value: &str) -> Option<String> {
    let output = value
        .chars()
        .filter(|character| !character.is_control() && !matches!(character, '/' | '\\'))
        .take(32)
        .collect::<String>();
    let output = output.trim();
    (!output.is_empty()).then(|| output.into())
}

pub fn stable_element_id(prefix: &str, identity: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in identity.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{prefix}{hash:016x}")
}

pub fn item_matches(item: &MarketItem, query: &str) -> bool {
    let query = query.to_lowercase();
    item.name.to_lowercase().contains(&query) || item.slug.contains(&query)
}

pub fn display_slug(slug: &str) -> String {
    slug.replace(['_', '-'], " ")
}

pub fn whisper_line(order: &MarketOrder, item: &str) -> String {
    let intent = match order.side {
        TradeSide::Sell => "WTB",
        TradeSide::Buy => "WTS",
    };
    format!(
        "/w {} Hi, {intent} {item} for {}p",
        order.trader, order.platinum
    )
}
