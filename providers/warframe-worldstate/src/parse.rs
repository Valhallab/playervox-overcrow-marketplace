use alloc::{
    collections::{BTreeMap, btree_map::Entry},
    string::{String, ToString},
    vec::Vec,
};

use serde::{
    Deserialize, Deserializer, Serialize,
    de::{Error as _, IgnoredAny, MapAccess, SeqAccess, Visitor, value::MapAccessDeserializer},
};
use sha2::{Digest as _, Sha256};

use crate::labels::Labels;

const MAX_INPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_PAYLOAD_BYTES: usize = 256 * 1024;
const MAX_JSON_DEPTH: usize = 32;
const MAX_JSON_STRING_BYTES: usize = 512;
const MAX_FIELD_BYTES: usize = 96;
const MAX_EPOCH_SECS: u64 = 253_402_300_799;
const MAX_SYNDICATES: usize = 128;
const MAX_TRADERS: usize = 16;
const MAX_FISSURES: usize = 96;
const MAX_ACTIVITIES: usize = 3;
const MAX_INVASIONS: usize = 32;
const MAX_REWARDS: usize = 16;
const ZARIMAN_EPOCH_SECS: i128 = 1_655_182_800;
const ZARIMAN_FULL_SECS: i128 = 18_000;
const ZARIMAN_HALF_SECS: i128 = 9_000;

#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorldstatePayload {
    pub(crate) captured_at_secs: u64,
    pub(crate) status: Status,
    pub(crate) fissures: Vec<Fissure>,
    pub(crate) sortie: Option<Activity>,
    pub(crate) archon: Option<Activity>,
    pub(crate) invasions: Vec<Invasion>,
}

#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Status {
    pub(crate) rows: Vec<StatusRow>,
}

#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StatusRow {
    pub(crate) id: String,
    pub(crate) state: Option<String>,
    pub(crate) activation_secs: Option<u64>,
    pub(crate) expires_at_secs: u64,
    pub(crate) location: Option<String>,
}

#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Fissure {
    pub(crate) instance_id: String,
    pub(crate) era: String,
    pub(crate) mission_type: String,
    pub(crate) node: String,
    pub(crate) expires_at_secs: u64,
    pub(crate) steel_path: bool,
    pub(crate) railjack: bool,
}

#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Activity {
    pub(crate) id: String,
    pub(crate) boss: String,
    pub(crate) expires_at_secs: u64,
    pub(crate) missions: Vec<ActivityMission>,
}

#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ActivityMission {
    pub(crate) id: String,
    pub(crate) mission_type: String,
    pub(crate) node: String,
    pub(crate) modifier: Option<String>,
}

#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Invasion {
    pub(crate) instance_id: String,
    pub(crate) node: String,
    pub(crate) attacker_faction: String,
    pub(crate) defender_faction: String,
    pub(crate) attacker_reward: Option<Reward>,
    pub(crate) defender_reward: Option<Reward>,
    pub(crate) count: i64,
    pub(crate) goal: i64,
    pub(crate) completed: bool,
}

#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Reward {
    pub(crate) item_key: String,
    pub(crate) label: String,
    pub(crate) count: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ParseError;

pub(crate) struct ValidatedWorldstate {
    syndicates: Vec<Syndicate>,
    traders: Vec<Trader>,
    fissures: Vec<Fissure>,
    sorties: Vec<Activity>,
    archons: Vec<Activity>,
    invasions: Vec<Invasion>,
}

struct Syndicate {
    tag: String,
    expiry: u64,
}

struct Trader {
    character: String,
    node: String,
    activation: u64,
    expiry: u64,
}

#[derive(Deserialize)]
struct RawWorldstate {
    #[serde(
        rename = "SyndicateMissions",
        default,
        deserialize_with = "deserialize_syndicates"
    )]
    syndicates: Vec<RawSyndicate>,
    #[serde(
        rename = "VoidTraders",
        default,
        deserialize_with = "deserialize_traders"
    )]
    traders: Vec<RawTrader>,
    #[serde(
        rename = "ActiveMissions",
        default,
        deserialize_with = "deserialize_active_missions"
    )]
    active_missions: Vec<RawActiveMission>,
    #[serde(
        rename = "VoidStorms",
        default,
        deserialize_with = "deserialize_void_storms"
    )]
    void_storms: Vec<RawVoidStorm>,
    #[serde(rename = "Sorties", default, deserialize_with = "deserialize_sorties")]
    sorties: Vec<RawActivity>,
    #[serde(
        rename = "LiteSorties",
        default,
        deserialize_with = "deserialize_lite_sorties"
    )]
    lite_sorties: Vec<RawActivity>,
    #[serde(
        rename = "Invasions",
        default,
        deserialize_with = "deserialize_invasions"
    )]
    invasions: Vec<RawInvasion>,
}

#[derive(Deserialize)]
struct RawSyndicate {
    #[serde(rename = "Tag")]
    tag: String,
    #[serde(rename = "Expiry")]
    expiry: RawEpoch,
}

#[derive(Deserialize)]
struct RawTrader {
    #[serde(rename = "Character")]
    character: String,
    #[serde(rename = "Node")]
    node: String,
    #[serde(rename = "Activation")]
    activation: RawEpoch,
    #[serde(rename = "Expiry")]
    expiry: RawEpoch,
}

#[derive(Deserialize)]
struct RawActiveMission {
    #[serde(rename = "_id")]
    id: RawId,
    #[serde(rename = "Modifier")]
    modifier: String,
    #[serde(rename = "MissionType")]
    mission_type: String,
    #[serde(rename = "Node")]
    node: String,
    #[serde(rename = "Expiry")]
    expiry: RawEpoch,
    #[serde(rename = "Hard", default)]
    hard: bool,
}

#[derive(Deserialize)]
struct RawVoidStorm {
    #[serde(rename = "ActiveMissionTier")]
    tier: String,
    #[serde(rename = "Node")]
    node: String,
    #[serde(rename = "Expiry")]
    expiry: RawEpoch,
}

#[derive(Deserialize)]
struct RawActivity {
    #[serde(rename = "Boss")]
    boss: String,
    #[serde(rename = "Expiry")]
    expiry: RawEpoch,
    #[serde(
        rename = "Variants",
        alias = "Missions",
        default,
        deserialize_with = "deserialize_activity_missions"
    )]
    missions: Vec<RawActivityMission>,
}

#[derive(Deserialize)]
struct RawActivityMission {
    #[serde(rename = "missionType")]
    mission_type: String,
    #[serde(rename = "node")]
    node: String,
    #[serde(rename = "modifierType", default)]
    modifier: Option<String>,
}

#[derive(Deserialize)]
struct RawInvasion {
    #[serde(rename = "_id", default)]
    id: Option<RawId>,
    #[serde(rename = "Node")]
    node: String,
    #[serde(rename = "Faction")]
    faction: String,
    #[serde(rename = "DefenderFaction")]
    defender_faction: String,
    #[serde(rename = "Count", default)]
    count: i64,
    #[serde(rename = "Goal", default)]
    goal: i64,
    #[serde(rename = "Completed", default)]
    completed: bool,
    #[serde(rename = "AttackerReward", default)]
    attacker_reward: RawReward,
    #[serde(rename = "DefenderReward", default)]
    defender_reward: RawReward,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawId {
    #[serde(rename = "$oid")]
    oid: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawEpoch {
    Seconds(u64),
    Date(RawDate),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDate {
    #[serde(rename = "$date")]
    value: RawDateValue,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawDateValue {
    Milliseconds(u64),
    NumberLong(RawNumberLong),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNumberLong {
    #[serde(rename = "$numberLong")]
    value: String,
}

#[derive(Default)]
struct RawReward {
    counted_items: Vec<RawRewardItem>,
}

#[derive(Deserialize)]
struct RawRewardObject {
    #[serde(
        rename = "countedItems",
        default,
        deserialize_with = "deserialize_reward_items"
    )]
    counted_items: Vec<RawRewardItem>,
}

#[derive(Deserialize)]
struct RawRewardItem {
    #[serde(rename = "ItemType")]
    item_type: String,
    #[serde(rename = "ItemCount", default = "default_reward_count")]
    count: u64,
}

const fn default_reward_count() -> u64 {
    1
}

impl<'de> Deserialize<'de> for RawReward {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct RewardVisitor;

        impl<'de> Visitor<'de> for RewardVisitor {
            type Value = RawReward;

            fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                formatter.write_str("an empty array or a bounded reward object")
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                if sequence.next_element::<IgnoredAny>()?.is_some() {
                    return Err(A::Error::custom("reward array must be empty"));
                }
                Ok(RawReward::default())
            }

            fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let value = RawRewardObject::deserialize(MapAccessDeserializer::new(map))?;
                Ok(RawReward {
                    counted_items: value.counted_items,
                })
            }
        }

        deserializer.deserialize_any(RewardVisitor)
    }
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

capped_vec_deserializer!(deserialize_syndicates, RawSyndicate, MAX_SYNDICATES);
capped_vec_deserializer!(deserialize_traders, RawTrader, MAX_TRADERS);
capped_vec_deserializer!(deserialize_active_missions, RawActiveMission, MAX_FISSURES);
capped_vec_deserializer!(deserialize_void_storms, RawVoidStorm, MAX_FISSURES);
capped_vec_deserializer!(deserialize_sorties, RawActivity, MAX_ACTIVITIES);
capped_vec_deserializer!(deserialize_lite_sorties, RawActivity, MAX_ACTIVITIES);
capped_vec_deserializer!(deserialize_invasions, RawInvasion, MAX_INVASIONS);
capped_vec_deserializer!(
    deserialize_activity_missions,
    RawActivityMission,
    MAX_ACTIVITIES
);
capped_vec_deserializer!(deserialize_reward_items, RawRewardItem, MAX_REWARDS);

#[cfg(test)]
pub(crate) fn parse_worldstate(
    bytes: &[u8],
    captured_at_secs: u64,
) -> Result<WorldstatePayload, ParseError> {
    parse_bounded_worldstate(bytes)?.at(captured_at_secs)
}

pub(crate) fn parse_bounded_worldstate(bytes: &[u8]) -> Result<ValidatedWorldstate, ParseError> {
    validate_json_envelope(bytes)?;
    let raw: RawWorldstate = serde_json::from_slice(bytes).map_err(|_| ParseError)?;
    let labels = Labels::load()?;
    let syndicates = raw
        .syndicates
        .iter()
        .map(|syndicate| {
            Ok(Syndicate {
                tag: bounded(&syndicate.tag)?,
                expiry: parse_epoch(&syndicate.expiry)?,
            })
        })
        .collect::<Result<Vec<_>, ParseError>>()?;
    let traders = raw
        .traders
        .iter()
        .map(|trader| {
            Ok(Trader {
                character: bounded(&trader.character)?,
                node: labels.node(&bounded(&trader.node)?)?,
                activation: parse_epoch(&trader.activation)?,
                expiry: parse_epoch(&trader.expiry)?,
            })
        })
        .collect::<Result<Vec<_>, ParseError>>()?;
    let fissures = parse_fissures(&raw, &labels)?;
    let sorties = parse_activities(&raw.sorties, &labels)?;
    let archons = parse_activities(&raw.lite_sorties, &labels)?;
    let invasions = parse_invasions(&raw.invasions, &labels)?;
    Ok(ValidatedWorldstate {
        syndicates,
        traders,
        fissures,
        sorties,
        archons,
        invasions,
    })
}

impl ValidatedWorldstate {
    pub(crate) fn at(&self, captured_at_secs: u64) -> Result<WorldstatePayload, ParseError> {
        if captured_at_secs > MAX_EPOCH_SECS {
            return Err(ParseError);
        }
        Ok(WorldstatePayload {
            captured_at_secs,
            status: parse_status(self, captured_at_secs)?,
            fissures: self
                .fissures
                .iter()
                .filter(|fissure| fissure.expires_at_secs > captured_at_secs)
                .cloned()
                .collect(),
            sortie: active_activity(&self.sorties, captured_at_secs),
            archon: active_activity(&self.archons, captured_at_secs),
            invasions: self.invasions.clone(),
        })
    }
}

pub(crate) fn encode_worldstate(payload: &WorldstatePayload) -> Result<Vec<u8>, ParseError> {
    let bytes = serde_json::to_vec(payload).map_err(|_| ParseError)?;
    (bytes.len() <= MAX_PAYLOAD_BYTES)
        .then_some(bytes)
        .ok_or(ParseError)
}

fn parse_status(raw: &ValidatedWorldstate, now_secs: u64) -> Result<Status, ParseError> {
    let mut rows = BTreeMap::new();
    for syndicate in &raw.syndicates {
        match syndicate.tag.as_str() {
            "CetusSyndicate" if syndicate.expiry > now_secs => {
                let bounty_end = syndicate.expiry - syndicate.expiry % 60;
                if bounty_end <= now_secs {
                    continue;
                }
                let remaining = bounty_end - now_secs;
                let day = remaining > 3_000;
                let phase_end = if day {
                    bounty_end.checked_sub(3_000).ok_or(ParseError)?
                } else {
                    bounty_end
                };
                insert_consistent(
                    &mut rows,
                    "cetus".to_string(),
                    status_row("cetus", if day { "day" } else { "night" }, phase_end),
                )?;
                insert_consistent(
                    &mut rows,
                    "cambion".to_string(),
                    status_row("cambion", if day { "fass" } else { "vome" }, phase_end),
                )?;
            }
            "ZarimanSyndicate" if syndicate.expiry > now_secs => {
                let bounty_end = syndicate.expiry - syndicate.expiry % 60;
                let phase_probe = i128::from(bounty_end) - 5;
                let elapsed = (phase_probe - ZARIMAN_EPOCH_SECS).rem_euclid(ZARIMAN_FULL_SECS);
                let corpus = ZARIMAN_FULL_SECS - elapsed > ZARIMAN_HALF_SECS;
                insert_consistent(
                    &mut rows,
                    "zariman".to_string(),
                    status_row(
                        "zariman",
                        if corpus { "corpus" } else { "grineer" },
                        syndicate.expiry,
                    ),
                )?;
            }
            _ => {}
        }
    }

    let vallis_offset = (i128::from(now_secs) - 1_770_234_408_i128).rem_euclid(1_600);
    let vallis_offset = u64::try_from(vallis_offset).map_err(|_| ParseError)?;
    let (state, remaining) = if vallis_offset < 400 {
        ("warm", 400 - vallis_offset)
    } else {
        ("cold", 1_600 - vallis_offset)
    };
    rows.insert(
        "vallis".to_string(),
        status_row(
            "vallis",
            state,
            now_secs
                .checked_add(remaining)
                .filter(|expiry| *expiry <= MAX_EPOCH_SECS)
                .ok_or(ParseError)?,
        ),
    );
    rows.insert(
        "daily-reset".to_string(),
        StatusRow {
            id: "daily-reset".to_string(),
            state: None,
            activation_secs: None,
            expires_at_secs: now_secs
                .checked_div(86_400)
                .and_then(|day| day.checked_add(1))
                .and_then(|day| day.checked_mul(86_400))
                .ok_or(ParseError)?,
            location: None,
        },
    );

    let mut traders = Vec::new();
    for trader in &raw.traders {
        if !trader.character.to_ascii_lowercase().contains("baro") {
            continue;
        }
        if trader.expiry > now_secs {
            traders.push((trader.activation, trader.expiry, trader.node.clone()));
        }
    }
    traders.sort();
    if let Some((activation, expiry, location)) = traders.into_iter().next() {
        rows.insert(
            "baro".to_string(),
            StatusRow {
                id: "baro".to_string(),
                state: Some(if activation <= now_secs {
                    "present".to_string()
                } else {
                    "incoming".to_string()
                }),
                activation_secs: Some(activation),
                expires_at_secs: expiry,
                location: Some(location),
            },
        );
    }

    Ok(Status {
        rows: rows.into_values().collect(),
    })
}

fn status_row(id: &str, state: &str, expires_at_secs: u64) -> StatusRow {
    StatusRow {
        id: id.to_string(),
        state: Some(state.to_string()),
        activation_secs: None,
        expires_at_secs,
        location: None,
    }
}

fn parse_fissures(raw: &RawWorldstate, labels: &Labels) -> Result<Vec<Fissure>, ParseError> {
    let mut values = BTreeMap::new();
    for mission in &raw.active_missions {
        let id = canonical_oid(&mission.id.oid)?;
        let expiry = parse_epoch(&mission.expiry)?;
        let mission_type = bounded(&mission.mission_type)?;
        let node = bounded(&mission.node)?;
        let fissure = Fissure {
            instance_id: id.clone(),
            era: era(&mission.modifier)?.to_string(),
            mission_type: labels.mission_type(&mission_type)?,
            node: labels.node(&node)?,
            expires_at_secs: expiry,
            steel_path: mission.hard,
            railjack: false,
        };
        insert_consistent(&mut values, id, fissure)?;
    }
    for storm in &raw.void_storms {
        let expiry = parse_epoch(&storm.expiry)?;
        let node = bounded(&storm.node)?;
        let tier = bounded(&storm.tier)?;
        let id = digest_id("storm", &[&node, &tier, &expiry.to_string()]);
        let fissure = Fissure {
            instance_id: id.clone(),
            era: era(&tier)?.to_string(),
            mission_type: labels.mission_type("MT_VOID_STORM")?,
            node: labels.node(&node)?,
            expires_at_secs: expiry,
            steel_path: false,
            railjack: true,
        };
        insert_consistent(&mut values, id, fissure)?;
    }
    if values.len() > MAX_FISSURES {
        return Err(ParseError);
    }
    Ok(values.into_values().collect())
}

fn parse_activities(values: &[RawActivity], labels: &Labels) -> Result<Vec<Activity>, ParseError> {
    let mut parsed = BTreeMap::new();
    for value in values {
        let expires_at_secs = parse_epoch(&value.expiry)?;
        if value.missions.is_empty() {
            return Err(ParseError);
        }
        let boss_code = bounded(&value.boss)?;
        let activity_id = digest_id("act", &[&boss_code, &expires_at_secs.to_string()]);
        let mut missions = BTreeMap::new();
        for mission in &value.missions {
            let mission_type_code = bounded(&mission.mission_type)?;
            let node_code = bounded(&mission.node)?;
            let modifier_code = mission.modifier.as_deref().map(bounded).transpose()?;
            let id = digest_id(
                "mission",
                &[
                    &activity_id,
                    &mission_type_code,
                    &node_code,
                    modifier_code.as_deref().unwrap_or(""),
                ],
            );
            insert_consistent(
                &mut missions,
                id.clone(),
                ActivityMission {
                    id,
                    mission_type: labels.mission_type(&mission_type_code)?,
                    node: labels.node(&node_code)?,
                    modifier: modifier_code
                        .as_deref()
                        .map(|code| labels.modifier(code))
                        .transpose()?,
                },
            )?;
        }
        insert_consistent(
            &mut parsed,
            activity_id.clone(),
            Activity {
                id: activity_id,
                boss: labels.boss(&boss_code)?,
                expires_at_secs,
                missions: missions.into_values().collect(),
            },
        )?;
    }
    let mut parsed: Vec<_> = parsed.into_values().collect();
    parsed.sort_by(|left, right| {
        (left.expires_at_secs, left.boss.as_str())
            .cmp(&(right.expires_at_secs, right.boss.as_str()))
    });
    Ok(parsed)
}

fn active_activity(values: &[Activity], now_secs: u64) -> Option<Activity> {
    values
        .iter()
        .find(|activity| activity.expires_at_secs > now_secs)
        .cloned()
}

fn parse_invasions(values: &[RawInvasion], labels: &Labels) -> Result<Vec<Invasion>, ParseError> {
    let mut parsed = BTreeMap::new();
    for value in values {
        let node = bounded(&value.node)?;
        let attacker_faction = bounded(&value.faction)?;
        let defender_faction = bounded(&value.defender_faction)?;
        let attacker_reward = parse_reward(&value.attacker_reward, labels)?;
        let defender_reward = parse_reward(&value.defender_reward, labels)?;
        let goal = value.goal;
        let goal_id = goal.to_string();
        let instance_id = match &value.id {
            Some(id) => canonical_oid(&id.oid)?,
            None => digest_id(
                "inv",
                &[
                    &node,
                    &attacker_faction,
                    &defender_faction,
                    &goal_id,
                    attacker_reward
                        .as_ref()
                        .map(|reward| reward.item_key.as_str())
                        .unwrap_or(""),
                    defender_reward
                        .as_ref()
                        .map(|reward| reward.item_key.as_str())
                        .unwrap_or(""),
                ],
            ),
        };
        let invasion = Invasion {
            instance_id: instance_id.clone(),
            node: labels.node(&node)?,
            attacker_faction: labels.faction(&attacker_faction)?,
            defender_faction: labels.faction(&defender_faction)?,
            attacker_reward,
            defender_reward,
            count: value.count,
            goal,
            completed: value.completed,
        };
        insert_consistent(&mut parsed, instance_id, invasion)?;
    }
    Ok(parsed.into_values().collect())
}

fn parse_reward(value: &RawReward, labels: &Labels) -> Result<Option<Reward>, ParseError> {
    let Some(item) = value.counted_items.first() else {
        return Ok(None);
    };
    if item.count == 0 {
        return Err(ParseError);
    }
    let key = item.item_type.rsplit('/').next().ok_or(ParseError)?;
    Ok(Some(Reward {
        item_key: bounded(key)?,
        label: labels.item(&item.item_type)?,
        count: u32::try_from(item.count).map_err(|_| ParseError)?,
    }))
}

fn insert_consistent<T: Eq>(
    values: &mut BTreeMap<String, T>,
    id: String,
    value: T,
) -> Result<(), ParseError> {
    match values.entry(id) {
        Entry::Vacant(entry) => {
            entry.insert(value);
            Ok(())
        }
        Entry::Occupied(entry) if entry.get() == &value => Ok(()),
        Entry::Occupied(_) => Err(ParseError),
    }
}

fn canonical_oid(value: &str) -> Result<String, ParseError> {
    if value.len() == 24
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(value.to_string())
    } else {
        Err(ParseError)
    }
}

fn digest_id(prefix: &str, fields: &[&str]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut hasher = Sha256::new();
    for field in fields {
        hasher.update((field.len() as u64).to_be_bytes());
        hasher.update(field.as_bytes());
    }
    let digest = hasher.finalize();
    let mut output = String::with_capacity(prefix.len() + 57);
    output.push_str(prefix);
    output.push('-');
    for byte in &digest[..28] {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn parse_epoch(value: &RawEpoch) -> Result<u64, ParseError> {
    let number = match value {
        RawEpoch::Seconds(number) => *number,
        RawEpoch::Date(date) => match &date.value {
            RawDateValue::Milliseconds(number) => *number,
            RawDateValue::NumberLong(number) => parse_decimal(&number.value)?,
        },
    };
    let seconds = match value {
        RawEpoch::Seconds(_) => number,
        RawEpoch::Date(_) => number.checked_div(1_000).ok_or(ParseError)?,
    };
    (seconds > 0 && seconds <= MAX_EPOCH_SECS)
        .then_some(seconds)
        .ok_or(ParseError)
}

fn parse_decimal(value: &str) -> Result<u64, ParseError> {
    if value.is_empty()
        || value.len() > 20
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(ParseError);
    }
    value.parse().map_err(|_| ParseError)
}

fn validate_json_envelope(bytes: &[u8]) -> Result<(), ParseError> {
    if bytes.is_empty() || bytes.len() > MAX_INPUT_BYTES {
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
    if depth == 0 && !in_string && !escaped {
        Ok(())
    } else {
        Err(ParseError)
    }
}

fn era(value: &str) -> Result<&'static str, ParseError> {
    match value {
        "VoidT1" => Ok("lith"),
        "VoidT2" => Ok("meso"),
        "VoidT3" => Ok("neo"),
        "VoidT4" => Ok("axi"),
        "VoidT5" => Ok("requiem"),
        "VoidT6" => Ok("omnia"),
        _ => Err(ParseError),
    }
}

fn bounded(value: &str) -> Result<String, ParseError> {
    if !value.is_empty()
        && value.len() <= MAX_FIELD_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii() && !byte.is_ascii_control())
    {
        Ok(value.to_string())
    } else {
        Err(ParseError)
    }
}
