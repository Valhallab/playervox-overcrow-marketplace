#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use alloc::{collections::BTreeSet, string::String, vec::Vec};

use serde::Deserialize;

pub const PROVIDER_ID: &str = "com.playervox.overcrow.warframe.worldstate";
pub const PROVIDER_SCHEMA: &str = "com.playervox.overcrow.warframe.worldstate/worldstate.v1";
pub const SESSION_SCHEMA: &str = "overcrow.session.v1";
pub const STEAM_APP_ID: u32 = 230_410;
pub const STALE_MS: u64 = 300_000;
pub const MAX_HOST_UTC_MS: u64 = 253_402_300_799_999;

const MAX_PAYLOAD_BYTES: usize = 256 * 1024;
const MAX_JSON_DEPTH: usize = 32;
const MAX_JSON_STRING_BYTES: usize = 512;
const MAX_FIELD_BYTES: usize = 96;
const MAX_ID_BYTES: usize = 64;
const MAX_EPOCH_SECS: u64 = 253_402_300_799;
const MAX_STATUS_ROWS: usize = 6;
const MAX_FISSURES: usize = 96;
const MAX_ACTIVITY_MISSIONS: usize = 3;
const MAX_INVASIONS: usize = 32;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Worldstate {
    pub captured_at_secs: u64,
    pub status: Status,
    pub fissures: Vec<Fissure>,
    pub sortie: Option<Activity>,
    pub archon: Option<Activity>,
    pub invasions: Vec<Invasion>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Status {
    pub rows: Vec<StatusRow>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StatusRow {
    pub id: String,
    pub state: Option<String>,
    pub activation_secs: Option<u64>,
    pub expires_at_secs: u64,
    pub location: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "lowercase")]
pub enum Era {
    Lith,
    Meso,
    Neo,
    Axi,
    Requiem,
    Omnia,
}

impl Era {
    pub const ALL: [Self; 6] = [
        Self::Lith,
        Self::Meso,
        Self::Neo,
        Self::Axi,
        Self::Requiem,
        Self::Omnia,
    ];

    pub const fn id(self) -> &'static str {
        match self {
            Self::Lith => "lith",
            Self::Meso => "meso",
            Self::Neo => "neo",
            Self::Axi => "axi",
            Self::Requiem => "requiem",
            Self::Omnia => "omnia",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Lith => "Lith",
            Self::Meso => "Meso",
            Self::Neo => "Neo",
            Self::Axi => "Axi",
            Self::Requiem => "Requiem",
            Self::Omnia => "Omnia",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Fissure {
    pub instance_id: String,
    pub era: Era,
    pub mission_type: String,
    pub node: String,
    pub expires_at_secs: u64,
    pub steel_path: bool,
    pub railjack: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Activity {
    pub id: String,
    pub boss: String,
    pub expires_at_secs: u64,
    pub missions: Vec<ActivityMission>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivityMission {
    pub id: String,
    pub mission_type: String,
    pub node: String,
    pub modifier: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Invasion {
    pub instance_id: String,
    pub node: String,
    pub attacker_faction: String,
    pub defender_faction: String,
    pub attacker_reward: Option<Reward>,
    pub defender_reward: Option<Reward>,
    pub count: i64,
    pub goal: i64,
    pub completed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reward {
    pub item_key: String,
    pub label: String,
    pub count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DataError;

pub fn parse(bytes: &[u8]) -> Result<Worldstate, DataError> {
    validate_envelope(bytes)?;
    let mut value: Worldstate = serde_json::from_slice(bytes).map_err(|_| DataError)?;
    validate_timestamp(value.captured_at_secs, true)?;
    validate_status(&mut value.status)?;
    validate_fissures(&mut value.fissures)?;
    if let Some(sortie) = &mut value.sortie {
        validate_activity(sortie)?;
    }
    if let Some(archon) = &mut value.archon {
        validate_activity(archon)?;
    }
    validate_invasions(&mut value.invasions)?;
    Ok(value)
}

impl Worldstate {
    pub fn is_fresh_at(&self, now_ms: u64) -> bool {
        if now_ms > MAX_HOST_UTC_MS {
            return false;
        }
        self.captured_at_secs
            .checked_mul(1_000)
            .and_then(|captured| now_ms.checked_sub(captured))
            .is_some_and(|elapsed| elapsed < STALE_MS)
    }
}

pub fn remaining_minutes(expires_at_secs: u64, now_ms: u64) -> Option<u64> {
    let now_secs = now_ms.checked_div(1_000)?;
    let seconds = expires_at_secs
        .checked_sub(now_secs)
        .filter(|value| *value > 0)?;
    seconds.checked_add(59)?.checked_div(60)
}

fn validate_status(status: &mut Status) -> Result<(), DataError> {
    if status.rows.len() > MAX_STATUS_ROWS {
        return Err(DataError);
    }
    let mut ids = BTreeSet::new();
    for row in &status.rows {
        if !ids.insert(row.id.as_str()) {
            return Err(DataError);
        }
        validate_timestamp(row.expires_at_secs, false)?;
        if row
            .activation_secs
            .is_some_and(|value| validate_timestamp(value, false).is_err())
            || row
                .location
                .as_deref()
                .is_some_and(|value| !valid_display(value))
            || !valid_status_shape(row)
        {
            return Err(DataError);
        }
    }
    status.rows.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(())
}

fn valid_status_shape(row: &StatusRow) -> bool {
    let known = match row.id.as_str() {
        "cetus" => matches!(row.state.as_deref(), Some("day" | "night")),
        "cambion" => matches!(row.state.as_deref(), Some("fass" | "vome")),
        "vallis" => matches!(row.state.as_deref(), Some("warm" | "cold")),
        "zariman" => matches!(row.state.as_deref(), Some("corpus" | "grineer")),
        "daily-reset" => row.state.is_none(),
        "baro" => {
            matches!(row.state.as_deref(), Some("present" | "incoming"))
                && row
                    .activation_secs
                    .is_some_and(|activation| activation < row.expires_at_secs)
                && row.location.is_some()
        }
        _ => false,
    };
    known
        && if row.id == "baro" {
            true
        } else {
            row.activation_secs.is_none() && row.location.is_none()
        }
}

fn validate_fissures(fissures: &mut [Fissure]) -> Result<(), DataError> {
    if fissures.len() > MAX_FISSURES {
        return Err(DataError);
    }
    let mut ids = BTreeSet::new();
    for fissure in fissures.iter() {
        if !valid_id(&fissure.instance_id)
            || !ids.insert(fissure.instance_id.as_str())
            || !valid_display(&fissure.mission_type)
            || !valid_display(&fissure.node)
            || (fissure.steel_path && fissure.railjack)
        {
            return Err(DataError);
        }
        validate_timestamp(fissure.expires_at_secs, false)?;
    }
    fissures.sort_by(|left, right| left.instance_id.cmp(&right.instance_id));
    Ok(())
}

fn validate_activity(activity: &mut Activity) -> Result<(), DataError> {
    if !valid_id(&activity.id)
        || !valid_display(&activity.boss)
        || activity.missions.is_empty()
        || activity.missions.len() > MAX_ACTIVITY_MISSIONS
    {
        return Err(DataError);
    }
    validate_timestamp(activity.expires_at_secs, false)?;
    let mut ids = BTreeSet::new();
    for mission in &activity.missions {
        if !valid_id(&mission.id)
            || !ids.insert(mission.id.as_str())
            || !valid_display(&mission.mission_type)
            || !valid_display(&mission.node)
            || mission
                .modifier
                .as_deref()
                .is_some_and(|value| !valid_display(value))
        {
            return Err(DataError);
        }
    }
    activity
        .missions
        .sort_by(|left, right| left.id.cmp(&right.id));
    Ok(())
}

fn validate_invasions(invasions: &mut [Invasion]) -> Result<(), DataError> {
    if invasions.len() > MAX_INVASIONS {
        return Err(DataError);
    }
    let mut ids = BTreeSet::new();
    for invasion in invasions.iter() {
        if !valid_id(&invasion.instance_id)
            || !ids.insert(invasion.instance_id.as_str())
            || !valid_display(&invasion.node)
            || !valid_display(&invasion.attacker_faction)
            || !valid_display(&invasion.defender_faction)
            || invasion
                .attacker_reward
                .as_ref()
                .is_some_and(|reward| !valid_reward(reward))
            || invasion
                .defender_reward
                .as_ref()
                .is_some_and(|reward| !valid_reward(reward))
        {
            return Err(DataError);
        }
    }
    invasions.sort_by(|left, right| left.instance_id.cmp(&right.instance_id));
    Ok(())
}

fn valid_reward(reward: &Reward) -> bool {
    reward.count > 0 && valid_code(&reward.item_key) && valid_display(&reward.label)
}

fn validate_timestamp(value: u64, allow_zero: bool) -> Result<(), DataError> {
    (value <= MAX_EPOCH_SECS && (allow_zero || value > 0))
        .then_some(())
        .ok_or(DataError)
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ID_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
}

fn valid_code(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_FIELD_BYTES
        && value.is_ascii()
        && !value.bytes().any(|byte| byte.is_ascii_control())
}

fn valid_display(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_FIELD_BYTES && !value.chars().any(char::is_control)
}

fn validate_envelope(bytes: &[u8]) -> Result<(), DataError> {
    if bytes.is_empty() || bytes.len() > MAX_PAYLOAD_BYTES {
        return Err(DataError);
    }
    let mut depth = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut string_bytes = 0_usize;
    for byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
                string_bytes = string_bytes.checked_add(1).ok_or(DataError)?;
            } else {
                match byte {
                    b'\\' => escaped = true,
                    b'"' => {
                        in_string = false;
                        string_bytes = 0;
                    }
                    0x00..=0x1f => return Err(DataError),
                    _ => string_bytes = string_bytes.checked_add(1).ok_or(DataError)?,
                }
            }
            if string_bytes > MAX_JSON_STRING_BYTES {
                return Err(DataError);
            }
        } else {
            match byte {
                b'"' => in_string = true,
                b'{' | b'[' => {
                    depth = depth.checked_add(1).ok_or(DataError)?;
                    if depth > MAX_JSON_DEPTH {
                        return Err(DataError);
                    }
                }
                b'}' | b']' => depth = depth.checked_sub(1).ok_or(DataError)?,
                _ => {}
            }
        }
    }
    (depth == 0 && !in_string && !escaped)
        .then_some(())
        .ok_or(DataError)
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_FISSURES, MAX_HOST_UTC_MS, MAX_JSON_DEPTH, MAX_JSON_STRING_BYTES, MAX_PAYLOAD_BYTES,
        parse, validate_envelope,
    };
    use alloc::vec;
    use serde_json::{Value, json};

    const FIXTURE: &[u8] = include_bytes!("../../fixtures/worldstate-v1.json");

    #[test]
    fn fissure_source_is_unambiguous() {
        let mut value: Value = serde_json::from_slice(FIXTURE).expect("worldstate fixture");
        value["fissures"][0]["steelPath"] = true.into();
        value["fissures"][0]["railjack"] = true.into();

        assert!(parse(&serde_json::to_vec(&value).expect("fixture JSON")).is_err());
    }

    #[test]
    fn baro_arrival_must_precede_departure() {
        let mut value: Value = serde_json::from_slice(FIXTURE).expect("worldstate fixture");
        value["status"]["rows"][5]["activationSecs"] = 1_777_003_601_u64.into();

        assert!(parse(&serde_json::to_vec(&value).expect("fixture JSON")).is_err());
    }

    #[test]
    fn fissure_limit_accepts_96_and_rejects_97() {
        let mut value: Value = serde_json::from_slice(FIXTURE).expect("worldstate fixture");
        let fissures = (0..MAX_FISSURES)
            .map(|index| fissure(&format!("fissure-{index}")))
            .collect::<Vec<_>>();
        value["fissures"] = fissures.clone().into();
        assert_eq!(
            parse(&serde_json::to_vec(&value).expect("fixture JSON"))
                .expect("96 fissures")
                .fissures
                .len(),
            MAX_FISSURES
        );

        value["fissures"] = fissures
            .into_iter()
            .chain([fissure("fissure-over-limit")])
            .collect::<Vec<_>>()
            .into();
        assert!(parse(&serde_json::to_vec(&value).expect("fixture JSON")).is_err());
    }

    #[test]
    fn fissures_are_sorted_and_duplicate_instances_are_rejected() {
        let mut value: Value = serde_json::from_slice(FIXTURE).expect("worldstate fixture");
        value["fissures"] = vec![fissure("fissure-b"), fissure("fissure-a")].into();
        let parsed =
            parse(&serde_json::to_vec(&value).expect("fixture JSON")).expect("reordered fissures");
        assert_eq!(parsed.fissures[0].instance_id, "fissure-a");
        assert_eq!(parsed.fissures[1].instance_id, "fissure-b");

        value["fissures"] = vec![fissure("fissure-a"), fissure("fissure-a")].into();
        assert!(parse(&serde_json::to_vec(&value).expect("fixture JSON")).is_err());
    }

    #[test]
    fn envelope_bounds_are_exact() {
        let mut exact_payload = FIXTURE.to_vec();
        exact_payload.resize(MAX_PAYLOAD_BYTES, b' ');
        assert!(parse(&exact_payload).is_ok());
        exact_payload.push(b' ');
        assert!(parse(&exact_payload).is_err());

        let nested = |depth: usize| {
            let mut bytes = vec![b'['; depth];
            bytes.push(b'0');
            bytes.extend(vec![b']'; depth]);
            bytes
        };
        assert!(validate_envelope(&nested(MAX_JSON_DEPTH)).is_ok());
        assert!(validate_envelope(&nested(MAX_JSON_DEPTH + 1)).is_err());

        let json_string = |length: usize| {
            let mut bytes = vec![b'"'];
            bytes.extend(vec![b'a'; length]);
            bytes.push(b'"');
            bytes
        };
        assert!(validate_envelope(&json_string(MAX_JSON_STRING_BYTES)).is_ok());
        assert!(validate_envelope(&json_string(MAX_JSON_STRING_BYTES + 1)).is_err());
    }

    #[test]
    fn timestamp_bounds_and_future_capture_fail_closed() {
        let mut value: Value = serde_json::from_slice(FIXTURE).expect("worldstate fixture");
        value["capturedAtSecs"] = 253_402_300_799_u64.into();
        let parsed =
            parse(&serde_json::to_vec(&value).expect("fixture JSON")).expect("maximum timestamp");
        assert!(parsed.is_fresh_at(MAX_HOST_UTC_MS));
        assert!(!parsed.is_fresh_at(MAX_HOST_UTC_MS + 1));

        value["capturedAtSecs"] = 253_402_300_800_u64.into();
        assert!(parse(&serde_json::to_vec(&value).expect("fixture JSON")).is_err());

        let parsed = parse(FIXTURE).expect("worldstate fixture");
        assert!(!parsed.is_fresh_at(1_776_999_999_999));
    }

    fn fissure(instance_id: &str) -> Value {
        json!({
            "instanceId": instance_id,
            "era": "axi",
            "missionType": "Defense",
            "node": "Galatea · Neptune",
            "expiresAtSecs": 1_777_000_900_u64,
            "steelPath": false,
            "railjack": false
        })
    }
}
