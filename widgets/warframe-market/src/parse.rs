use alloc::{
    collections::{BTreeMap, btree_map::Entry},
    string::{String, ToString},
    vec::Vec,
};

use serde::{
    Deserialize, Deserializer,
    de::{Error as _, SeqAccess, Visitor},
};

use crate::model::{
    MarketItem, MarketOrder, Presence, TradeSide, safe_public_token, safe_slug, sanitize_display,
    sanitize_trader, stable_element_id,
};

const MAX_ITEMS_BYTES: usize = 8 * 1024 * 1024;
const MAX_ORDERS_BYTES: usize = 2 * 1024 * 1024;
const MAX_JSON_DEPTH: usize = 32;
const MAX_JSON_STRING_BYTES: usize = 512;
const MAX_ITEMS: usize = 50_000;
const MAX_ORDERS: usize = 4_096;
const MAX_NAME_CHARS: usize = 96;
const MAX_PLATINUM: u64 = 900_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ParseError;

#[derive(Deserialize)]
struct ItemsEnvelope {
    #[serde(deserialize_with = "deserialize_items")]
    data: Vec<RawItem>,
}

#[derive(Deserialize)]
struct RawItem {
    slug: String,
    i18n: RawI18n,
}

#[derive(Deserialize)]
struct RawI18n {
    en: RawItemName,
}

#[derive(Deserialize)]
struct RawItemName {
    name: String,
}

#[derive(Deserialize)]
struct OrdersEnvelope {
    #[serde(deserialize_with = "deserialize_orders")]
    data: Vec<RawOrder>,
}

#[derive(Deserialize)]
struct RawOrder {
    id: String,
    #[serde(rename = "type")]
    side: String,
    platinum: u64,
    visible: bool,
    user: RawUser,
}

#[derive(Deserialize)]
struct RawUser {
    #[serde(rename = "ingameName")]
    ingame_name: String,
    status: String,
    platform: String,
}

macro_rules! capped_vec_deserializer {
    ($function:ident, $item:ty, $maximum:expr) => {
        fn $function<'de, D>(deserializer: D) -> Result<Vec<$item>, D::Error>
        where
            D: Deserializer<'de>,
        {
            struct CappedVisitor;

            impl<'de> Visitor<'de> for CappedVisitor {
                type Value = Vec<$item>;

                fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                    formatter.write_str("a bounded array")
                }

                fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
                where
                    A: SeqAccess<'de>,
                {
                    if sequence.size_hint().is_some_and(|size| size > $maximum) {
                        return Err(A::Error::custom("array limit exceeded"));
                    }
                    let mut values = Vec::new();
                    while let Some(value) = sequence.next_element()? {
                        if values.len() == $maximum {
                            return Err(A::Error::custom("array limit exceeded"));
                        }
                        values.push(value);
                    }
                    Ok(values)
                }
            }

            deserializer.deserialize_seq(CappedVisitor)
        }
    };
}

capped_vec_deserializer!(deserialize_items, RawItem, MAX_ITEMS);
capped_vec_deserializer!(deserialize_orders, RawOrder, MAX_ORDERS);

pub(crate) fn parse_items(bytes: &[u8]) -> Result<Vec<MarketItem>, ParseError> {
    validate_envelope(bytes, MAX_ITEMS_BYTES)?;
    let raw: ItemsEnvelope = serde_json::from_slice(bytes).map_err(|_| ParseError)?;
    let mut by_slug = BTreeMap::new();
    let mut identities = BTreeMap::new();
    for raw in raw.data {
        if !safe_slug(&raw.slug) {
            return Err(ParseError);
        }
        let name = sanitize_display(&raw.i18n.en.name, MAX_NAME_CHARS).ok_or(ParseError)?;
        let item = MarketItem {
            element_id: stable_element_id("item-", &raw.slug),
            name,
            slug: raw.slug.clone(),
        };
        insert_identity(&mut identities, &item.element_id, &item.slug)?;
        insert_consistent(&mut by_slug, raw.slug, item)?;
    }
    Ok(by_slug.into_values().collect())
}

pub(crate) fn parse_orders(bytes: &[u8]) -> Result<Vec<MarketOrder>, ParseError> {
    validate_envelope(bytes, MAX_ORDERS_BYTES)?;
    let raw: OrdersEnvelope = serde_json::from_slice(bytes).map_err(|_| ParseError)?;
    let mut by_id = BTreeMap::new();
    let mut identities = BTreeMap::new();
    for raw in raw.data {
        if !raw.visible {
            continue;
        }
        match raw.user.platform.as_str() {
            "pc" => {}
            "ps4" | "xbox" | "switch" | "mobile" => continue,
            _ => return Err(ParseError),
        }
        if !safe_public_token(&raw.id) || raw.platinum == 0 || raw.platinum > MAX_PLATINUM {
            return Err(ParseError);
        }
        let side = match raw.side.as_str() {
            "sell" => TradeSide::Sell,
            "buy" => TradeSide::Buy,
            _ => return Err(ParseError),
        };
        let presence = match raw.user.status.as_str() {
            "ingame" => Presence::Ingame,
            "online" => Presence::Online,
            "offline" => Presence::Offline,
            _ => Presence::Unknown,
        };
        let order = MarketOrder {
            element_id: stable_element_id("order-", &raw.id),
            public_id: raw.id.clone(),
            side,
            platinum: u32::try_from(raw.platinum).map_err(|_| ParseError)?,
            trader: sanitize_trader(&raw.user.ingame_name).ok_or(ParseError)?,
            presence,
        };
        insert_identity(&mut identities, &order.element_id, &order.public_id)?;
        insert_consistent(&mut by_id, raw.id, order)?;
    }
    Ok(by_id.into_values().collect())
}

fn insert_identity(
    identities: &mut BTreeMap<String, String>,
    element_id: &str,
    public_id: &str,
) -> Result<(), ParseError> {
    match identities.entry(element_id.to_string()) {
        Entry::Vacant(entry) => {
            entry.insert(public_id.to_string());
            Ok(())
        }
        Entry::Occupied(entry) if entry.get() == public_id => Ok(()),
        Entry::Occupied(_) => Err(ParseError),
    }
}

fn insert_consistent<T: Eq>(
    values: &mut BTreeMap<String, T>,
    key: String,
    value: T,
) -> Result<(), ParseError> {
    match values.entry(key) {
        Entry::Vacant(entry) => {
            entry.insert(value);
            Ok(())
        }
        Entry::Occupied(entry) if entry.get() == &value => Ok(()),
        Entry::Occupied(_) => Err(ParseError),
    }
}

fn validate_envelope(bytes: &[u8], maximum_bytes: usize) -> Result<(), ParseError> {
    if bytes.is_empty() || bytes.len() > maximum_bytes {
        return Err(ParseError);
    }
    let mut depth = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut string_bytes = 0_usize;
    for byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
                string_bytes = string_bytes.checked_add(1).ok_or(ParseError)?;
            } else {
                match byte {
                    b'\\' => escaped = true,
                    b'"' => {
                        in_string = false;
                        string_bytes = 0;
                    }
                    0x00..=0x1f => return Err(ParseError),
                    _ => string_bytes = string_bytes.checked_add(1).ok_or(ParseError)?,
                }
            }
            if string_bytes > MAX_JSON_STRING_BYTES {
                return Err(ParseError);
            }
        } else {
            match byte {
                b'"' => in_string = true,
                b'{' | b'[' => {
                    depth = depth.checked_add(1).ok_or(ParseError)?;
                    if depth > MAX_JSON_DEPTH {
                        return Err(ParseError);
                    }
                }
                b'}' | b']' => depth = depth.checked_sub(1).ok_or(ParseError)?,
                _ => {}
            }
        }
    }
    (depth == 0 && !in_string && !escaped)
        .then_some(())
        .ok_or(ParseError)
}
