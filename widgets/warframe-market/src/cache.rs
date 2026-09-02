use alloc::{borrow::ToOwned, format, string::String, vec::Vec};

use sha2::{Digest, Sha256};

use crate::model::{MarketItem, safe_slug, sanitize_display, stable_element_id};

const INDEX_MAGIC: &[u8; 4] = b"WFI1";
const MANIFEST_MAGIC: &[u8; 4] = b"WFC1";
const MANIFEST_BYTES: usize = 4 + 8 + 4 + 1 + 32;
pub(crate) const MANIFEST_KEY: &str = "catalog-current";
pub(crate) const MAX_ITEMS: usize = 8_192;
pub(crate) const MAX_PART_BYTES: usize = 60 * 1024;
const MAX_PARTS: usize = 8;
const MAX_CACHE_BYTES: usize = MAX_PART_BYTES * MAX_PARTS;
const MAX_SLUG_BYTES: usize = 96;
pub(crate) const MAX_NAME_BYTES: usize = 96 * 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CacheError;

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct EncodedCache {
    pub(crate) manifest: Vec<u8>,
    pub(crate) part_keys: Vec<String>,
    pub(crate) parts: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Manifest {
    pub(crate) fetched_at_ms: u64,
    total_bytes: usize,
    part_count: usize,
    digest: [u8; 32],
}

impl Manifest {
    pub(crate) fn parse(bytes: &[u8]) -> Result<Self, CacheError> {
        if bytes.len() != MANIFEST_BYTES || &bytes[..4] != MANIFEST_MAGIC {
            return Err(CacheError);
        }
        let fetched_at_ms = read_u64(&bytes[4..12])?;
        let total_bytes = usize::try_from(read_u32(&bytes[12..16])?).map_err(|_| CacheError)?;
        let part_count = usize::from(bytes[16]);
        if total_bytes == 0
            || total_bytes > MAX_CACHE_BYTES
            || !(1..=MAX_PARTS).contains(&part_count)
            || part_count != total_bytes.div_ceil(MAX_PART_BYTES)
        {
            return Err(CacheError);
        }
        let mut digest = [0_u8; 32];
        digest.copy_from_slice(&bytes[17..]);
        Ok(Self {
            fetched_at_ms,
            total_bytes,
            part_count,
            digest,
        })
    }

    pub(crate) fn part_keys(&self) -> Vec<String> {
        let digest = hex_digest(&self.digest);
        (0..self.part_count)
            .map(|index| format!("catalog-{digest}-{index}"))
            .collect()
    }
}

pub(crate) fn encode(items: &[MarketItem], fetched_at_ms: u64) -> Result<EncodedCache, CacheError> {
    if items.is_empty() || items.len() > MAX_ITEMS {
        return Err(CacheError);
    }
    let count = u16::try_from(items.len()).map_err(|_| CacheError)?;
    let mut ordered = items.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.slug.cmp(&right.slug));

    let mut bytes = Vec::new();
    bytes.extend_from_slice(INDEX_MAGIC);
    bytes.extend_from_slice(&count.to_le_bytes());
    let mut previous_slug: Option<&str> = None;
    for item in ordered {
        if !safe_slug(&item.slug)
            || item.slug.len() > MAX_SLUG_BYTES
            || item.name.is_empty()
            || item.name.len() > MAX_NAME_BYTES
            || sanitize_display(&item.name, 96).as_deref() != Some(item.name.as_str())
            || previous_slug.is_some_and(|previous| previous >= item.slug.as_str())
        {
            return Err(CacheError);
        }
        let slug_len = u8::try_from(item.slug.len()).map_err(|_| CacheError)?;
        let name_len = u16::try_from(item.name.len()).map_err(|_| CacheError)?;
        bytes.push(slug_len);
        bytes.extend_from_slice(&name_len.to_le_bytes());
        bytes.extend_from_slice(item.slug.as_bytes());
        bytes.extend_from_slice(item.name.as_bytes());
        if bytes.len() > MAX_CACHE_BYTES {
            return Err(CacheError);
        }
        previous_slug = Some(&item.slug);
    }

    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    let parts = bytes
        .chunks(MAX_PART_BYTES)
        .map(<[u8]>::to_vec)
        .collect::<Vec<_>>();
    let part_count = u8::try_from(parts.len()).map_err(|_| CacheError)?;
    let total_bytes = u32::try_from(bytes.len()).map_err(|_| CacheError)?;
    let mut manifest = Vec::with_capacity(MANIFEST_BYTES);
    manifest.extend_from_slice(MANIFEST_MAGIC);
    manifest.extend_from_slice(&fetched_at_ms.to_le_bytes());
    manifest.extend_from_slice(&total_bytes.to_le_bytes());
    manifest.push(part_count);
    manifest.extend_from_slice(&digest);
    let parsed = Manifest::parse(&manifest)?;
    Ok(EncodedCache {
        part_keys: parsed.part_keys(),
        manifest,
        parts,
    })
}

pub(crate) fn decode(
    manifest: &Manifest,
    parts: &[Vec<u8>],
) -> Result<Vec<MarketItem>, CacheError> {
    if parts.len() != manifest.part_count
        || parts.iter().enumerate().any(|(index, part)| {
            part.is_empty()
                || part.len() > MAX_PART_BYTES
                || (index + 1 < parts.len() && part.len() != MAX_PART_BYTES)
        })
    {
        return Err(CacheError);
    }
    let mut bytes = Vec::with_capacity(manifest.total_bytes);
    for part in parts {
        bytes.extend_from_slice(part);
    }
    if bytes.len() != manifest.total_bytes
        || <[u8; 32]>::from(Sha256::digest(&bytes)) != manifest.digest
    {
        return Err(CacheError);
    }
    decode_index(&bytes)
}

fn decode_index(bytes: &[u8]) -> Result<Vec<MarketItem>, CacheError> {
    if bytes.len() < 6 || &bytes[..4] != INDEX_MAGIC {
        return Err(CacheError);
    }
    let count = usize::from(read_u16(&bytes[4..6])?);
    if count == 0 || count > MAX_ITEMS {
        return Err(CacheError);
    }
    let mut offset = 6_usize;
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        let slug_len = usize::from(*bytes.get(offset).ok_or(CacheError)?);
        let name_len = usize::from(read_u16(slice(bytes, offset + 1, 2)?)?);
        offset = offset.checked_add(3).ok_or(CacheError)?;
        if slug_len == 0 || slug_len > MAX_SLUG_BYTES || name_len == 0 || name_len > MAX_NAME_BYTES
        {
            return Err(CacheError);
        }
        let slug = core::str::from_utf8(slice(bytes, offset, slug_len)?)
            .map_err(|_| CacheError)?
            .to_owned();
        offset = offset.checked_add(slug_len).ok_or(CacheError)?;
        let name = core::str::from_utf8(slice(bytes, offset, name_len)?)
            .map_err(|_| CacheError)?
            .to_owned();
        offset = offset.checked_add(name_len).ok_or(CacheError)?;
        if !safe_slug(&slug)
            || sanitize_display(&name, 96).as_deref() != Some(name.as_str())
            || items
                .last()
                .is_some_and(|previous: &MarketItem| previous.slug >= slug)
        {
            return Err(CacheError);
        }
        items.push(MarketItem {
            element_id: stable_element_id("item-", &slug),
            name,
            slug,
        });
    }
    (offset == bytes.len()).then_some(items).ok_or(CacheError)
}

fn slice(bytes: &[u8], offset: usize, length: usize) -> Result<&[u8], CacheError> {
    let end = offset.checked_add(length).ok_or(CacheError)?;
    bytes.get(offset..end).ok_or(CacheError)
}

fn read_u16(bytes: &[u8]) -> Result<u16, CacheError> {
    Ok(u16::from_le_bytes(
        bytes.try_into().map_err(|_| CacheError)?,
    ))
}

fn read_u32(bytes: &[u8]) -> Result<u32, CacheError> {
    Ok(u32::from_le_bytes(
        bytes.try_into().map_err(|_| CacheError)?,
    ))
}

fn read_u64(bytes: &[u8]) -> Result<u64, CacheError> {
    Ok(u64::from_le_bytes(
        bytes.try_into().map_err(|_| CacheError)?,
    ))
}

fn hex_digest(digest: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

#[cfg(test)]
mod tests {
    use alloc::{format, vec};

    use super::*;
    use crate::model::{MarketItem, stable_element_id};

    fn item(index: usize) -> MarketItem {
        let slug = format!("item_{index:04}");
        MarketItem {
            element_id: stable_element_id("item-", &slug),
            name: format!("Representative item name {index:04}"),
            slug,
        }
    }

    #[test]
    fn compact_index_round_trips_across_storage_parts() {
        let items = (0..3_840).map(item).collect::<Vec<_>>();
        let cache = encode(&items, 1_777_000_000_000).expect("encode current-size catalog");
        assert!(cache.parts.len() >= 2);
        assert!(cache.parts.iter().all(|part| part.len() <= MAX_PART_BYTES));

        let manifest = Manifest::parse(&cache.manifest).expect("parse manifest");
        assert_eq!(manifest.fetched_at_ms, 1_777_000_000_000);
        assert_eq!(manifest.part_keys(), cache.part_keys);
        assert_eq!(decode(&manifest, &cache.parts), Ok(items));
    }

    #[test]
    fn missing_or_modified_parts_are_rejected() {
        let items = (0..3_840).map(item).collect::<Vec<_>>();
        let cache = encode(&items, 42).expect("encode catalog");
        let manifest = Manifest::parse(&cache.manifest).expect("parse manifest");

        assert_eq!(decode(&manifest, &cache.parts[..1]), Err(CacheError));
        let mut modified = cache.parts.clone();
        modified[0][0] ^= 1;
        assert_eq!(decode(&manifest, &modified), Err(CacheError));
    }

    #[test]
    fn cache_bounds_item_count_and_encoded_size() {
        let too_many = (0..=MAX_ITEMS).map(item).collect::<Vec<_>>();
        assert_eq!(encode(&too_many, 42), Err(CacheError));

        let invalid = vec![MarketItem {
            element_id: "item-invalid".into(),
            name: "x".repeat(MAX_NAME_BYTES + 1),
            slug: "valid_slug".into(),
        }];
        assert_eq!(encode(&invalid, 42), Err(CacheError));
    }
}
