use alloc::{
    collections::{BTreeMap, btree_map::Entry},
    string::{String, ToString},
    vec,
    vec::Vec,
};

use serde::{
    Deserialize, Deserializer,
    de::{Error as _, SeqAccess, Visitor},
};

use crate::cache::MAX_ITEMS;
use crate::model::{
    MarketItem, MarketOrder, Presence, TradeSide, safe_public_token, safe_slug, sanitize_display,
    sanitize_trader, stable_element_id,
};

const MAX_ITEMS_BYTES: usize = 2 * 1024 * 1024;
const MAX_ORDERS_BYTES: usize = 2 * 1024 * 1024;
const MAX_JSON_DEPTH: usize = 32;
const MAX_JSON_STRING_BYTES: usize = 512;
const MAX_ORDERS: usize = 4_096;
const MAX_NAME_CHARS: usize = 96;
const MAX_PLATINUM: u64 = 900_000;
const MAX_ITEM_OBJECT_BYTES: usize = 64 * 1024;
const MAX_ENVELOPE_BYTES: usize = 4 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ParseError;

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

pub(crate) struct CatalogStream {
    body_length: usize,
    received: usize,
    next_sequence: u8,
    prefix: Vec<u8>,
    suffix: Vec<u8>,
    phase: CatalogPhase,
    current: Option<ItemFrame>,
    after_item: bool,
    by_slug: BTreeMap<String, MarketItem>,
    identities: BTreeMap<String, String>,
    failed: bool,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum CatalogPhase {
    Prefix,
    Items,
    Suffix,
}

struct ItemFrame {
    bytes: Vec<u8>,
    depth: usize,
    in_string: bool,
    escaped: bool,
}

impl CatalogStream {
    pub(crate) fn start(body_length: u32) -> Result<Self, ParseError> {
        let body_length = usize::try_from(body_length).map_err(|_| ParseError)?;
        if !(1..=MAX_ITEMS_BYTES).contains(&body_length) {
            return Err(ParseError);
        }
        Ok(Self {
            body_length,
            received: 0,
            next_sequence: 0,
            prefix: Vec::new(),
            suffix: Vec::new(),
            phase: CatalogPhase::Prefix,
            current: None,
            after_item: false,
            by_slug: BTreeMap::new(),
            identities: BTreeMap::new(),
            failed: false,
        })
    }

    pub(crate) fn push(&mut self, sequence: u8, bytes: &[u8]) -> Result<(), ParseError> {
        if sequence != self.next_sequence || bytes.is_empty() || bytes.len() > 64 * 1024 {
            return Err(ParseError);
        }
        self.next_sequence = self.next_sequence.checked_add(1).ok_or(ParseError)?;
        self.received = self.received.checked_add(bytes.len()).ok_or(ParseError)?;
        if self.received > self.body_length {
            return Err(ParseError);
        }
        for byte in bytes {
            if !self.failed && self.push_byte(*byte).is_err() {
                self.failed = true;
            }
        }
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<Vec<MarketItem>, ParseError> {
        if self.received != self.body_length
            || self.phase != CatalogPhase::Suffix
            || self.current.is_some()
            || self.by_slug.is_empty()
            || self.failed
        {
            return Err(ParseError);
        }
        let mut envelope = self.prefix;
        envelope.extend_from_slice(b"[]");
        envelope.extend_from_slice(&self.suffix);
        let value: serde_json::Value = serde_json::from_slice(&envelope).map_err(|_| ParseError)?;
        if !value
            .as_object()
            .and_then(|object| object.get("data"))
            .is_some_and(serde_json::Value::is_array)
        {
            return Err(ParseError);
        }
        Ok(self.by_slug.into_values().collect())
    }

    fn push_byte(&mut self, byte: u8) -> Result<(), ParseError> {
        match self.phase {
            CatalogPhase::Prefix => {
                self.prefix.push(byte);
                if self.prefix.len() > MAX_ENVELOPE_BYTES {
                    return Err(ParseError);
                }
                if byte == b'['
                    && let Some(array_offset) = data_array_offset(&self.prefix)?
                {
                    self.prefix.truncate(array_offset);
                    self.phase = CatalogPhase::Items;
                }
            }
            CatalogPhase::Items => self.push_item_byte(byte)?,
            CatalogPhase::Suffix => {
                self.suffix.push(byte);
                if self.suffix.len() > MAX_ENVELOPE_BYTES {
                    return Err(ParseError);
                }
            }
        }
        Ok(())
    }

    fn push_item_byte(&mut self, byte: u8) -> Result<(), ParseError> {
        if let Some(frame) = self.current.as_mut() {
            frame.bytes.push(byte);
            if frame.bytes.len() > MAX_ITEM_OBJECT_BYTES {
                return Err(ParseError);
            }
            if frame.in_string {
                if frame.escaped {
                    frame.escaped = false;
                } else if byte == b'\\' {
                    frame.escaped = true;
                } else if byte == b'"' {
                    frame.in_string = false;
                } else if byte <= 0x1f {
                    return Err(ParseError);
                }
            } else {
                match byte {
                    b'"' => frame.in_string = true,
                    b'{' | b'[' => frame.depth = frame.depth.checked_add(1).ok_or(ParseError)?,
                    b'}' | b']' => {
                        frame.depth = frame.depth.checked_sub(1).ok_or(ParseError)?;
                    }
                    _ => {}
                }
            }
            if frame.depth == 0 {
                if frame.in_string || frame.escaped || byte != b'}' {
                    return Err(ParseError);
                }
                let bytes = self.current.take().ok_or(ParseError)?.bytes;
                self.insert_item(&bytes)?;
                self.after_item = true;
            }
            return Ok(());
        }

        match byte {
            b' ' | b'\n' | b'\r' | b'\t' => {}
            b',' if self.after_item => self.after_item = false,
            b']' => self.phase = CatalogPhase::Suffix,
            b'{' if !self.after_item => {
                self.current = Some(ItemFrame {
                    bytes: vec![b'{'],
                    depth: 1,
                    in_string: false,
                    escaped: false,
                });
            }
            _ => return Err(ParseError),
        }
        Ok(())
    }

    fn insert_item(&mut self, bytes: &[u8]) -> Result<(), ParseError> {
        if self.by_slug.len() == MAX_ITEMS {
            return Err(ParseError);
        }
        let raw: RawItem = serde_json::from_slice(bytes).map_err(|_| ParseError)?;
        let (slug, item) = market_item(raw)?;
        insert_identity(&mut self.identities, &item.element_id, &item.slug)?;
        insert_consistent(&mut self.by_slug, slug, item)
    }
}

fn data_array_offset(bytes: &[u8]) -> Result<Option<usize>, ParseError> {
    let mut depth = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut string_start = 0_usize;
    let mut previous_significant = None;
    let mut index = 0_usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
                if depth == 1
                    && matches!(previous_significant, Some(b'{') | Some(b','))
                    && &bytes[string_start..index] == b"data"
                {
                    let mut cursor = index + 1;
                    skip_whitespace(bytes, &mut cursor);
                    if bytes.get(cursor) != Some(&b':') {
                        return Ok(None);
                    }
                    cursor += 1;
                    skip_whitespace(bytes, &mut cursor);
                    if bytes.get(cursor) == Some(&b'[') {
                        return Ok(Some(cursor));
                    }
                }
            } else if byte <= 0x1f {
                return Err(ParseError);
            }
        } else {
            match byte {
                b'"' => {
                    in_string = true;
                    string_start = index + 1;
                }
                b'{' | b'[' => depth = depth.checked_add(1).ok_or(ParseError)?,
                b'}' | b']' => depth = depth.checked_sub(1).ok_or(ParseError)?,
                b' ' | b'\n' | b'\r' | b'\t' => {}
                _ => previous_significant = Some(byte),
            }
            if matches!(byte, b'{' | b'[' | b'}' | b']' | b',') {
                previous_significant = Some(byte);
            }
        }
        index += 1;
    }
    Ok(None)
}

fn skip_whitespace(bytes: &[u8], cursor: &mut usize) {
    while bytes
        .get(*cursor)
        .is_some_and(|byte| matches!(byte, b' ' | b'\n' | b'\r' | b'\t'))
    {
        *cursor += 1;
    }
}

fn market_item(raw: RawItem) -> Result<(String, MarketItem), ParseError> {
    if !safe_slug(&raw.slug) {
        return Err(ParseError);
    }
    let name = sanitize_display(&raw.i18n.en.name, MAX_NAME_CHARS).ok_or(ParseError)?;
    let item = MarketItem {
        element_id: stable_element_id("item-", &raw.slug),
        name,
        slug: raw.slug.clone(),
    };
    Ok((raw.slug, item))
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

capped_vec_deserializer!(deserialize_orders, RawOrder, MAX_ORDERS);

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

#[cfg(test)]
mod tests {
    use super::*;

    const ITEMS: &[u8] = include_bytes!("../tests/fixtures/items.json");

    #[test]
    fn catalog_stream_parses_items_across_arbitrary_chunk_boundaries() {
        let mut stream = CatalogStream::start(u32::try_from(ITEMS.len()).expect("fixture length"))
            .expect("start stream");
        for (sequence, bytes) in ITEMS.chunks(37).enumerate() {
            stream
                .push(u8::try_from(sequence).expect("bounded sequence"), bytes)
                .expect("stream chunk");
        }
        let items = stream.finish().expect("complete catalog");
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].slug, "arcane_energize");
        assert_eq!(items[3].name, "Primed Flow");
    }

    #[test]
    fn catalog_stream_rejects_sequence_length_and_conflicting_duplicates() {
        let mut stream = CatalogStream::start(ITEMS.len() as u32).expect("start stream");
        assert_eq!(stream.push(1, ITEMS), Err(ParseError));

        let conflicting = br#"{"data":[
            {"slug":"arcane_energize","i18n":{"en":{"name":"Arcane Energize"}}},
            {"slug":"arcane_energize","i18n":{"en":{"name":"Different"}}}
        ]}"#;
        let mut stream = CatalogStream::start(conflicting.len() as u32).expect("start stream");
        stream.push(0, conflicting).expect("bounded transport");
        assert_eq!(stream.finish(), Err(ParseError));
    }
}
