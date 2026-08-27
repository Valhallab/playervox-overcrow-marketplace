use overcrow_widget_sdk::{
    GrantedCapabilities, GuestError, HarnessError, HostCommand, HostEvent, HttpResponseMetadata,
    InitInput, WidgetHarness,
};
use serde::Deserialize;

use super::parse::{
    Activity, ActivityMission, Fissure, Invasion, Reward, Status, StatusRow, WorldstatePayload,
};
use super::{WorldstateProvider, encode_worldstate, parse_worldstate};

const MINIMAL: &[u8] = include_bytes!("fixtures/minimal.json");
const HOSTILE: &[u8] = include_bytes!("fixtures/hostile.json");
const NOW_SECS: u64 = 1_777_000_000;

#[test]
fn minimal_worldstate_is_bounded_and_publishes_every_widget_section() {
    let parsed = parse_worldstate(MINIMAL, NOW_SECS).expect("bounded fixture");

    assert!(!parsed.status.rows.is_empty());
    assert_eq!(parsed.fissures.len(), 2);
    assert!(parsed.sortie.is_some());
    assert!(parsed.archon.is_some());
    assert_eq!(
        parsed.sortie.as_ref().map(|activity| activity.id.len()),
        Some(60)
    );
    assert_eq!(parsed.invasions.len(), 2);
    assert!(parsed.fissures.iter().any(|fissure| {
        fissure.mission_type == "Defense" && fissure.node == "Galatea · Neptune"
    }));
    assert_eq!(
        parsed
            .sortie
            .as_ref()
            .map(|activity| activity.boss.as_str()),
        Some("Lephantis")
    );
    assert!(parsed.invasions.iter().any(|invasion| {
        invasion.attacker_faction == "Grineer"
            && invasion
                .attacker_reward
                .as_ref()
                .is_some_and(|reward| reward.label == "Detonite Injector")
    }));
    let zariman = parsed
        .status
        .rows
        .iter()
        .find(|row| row.id == "zariman")
        .expect("Zariman row");
    assert_eq!(zariman.state.as_deref(), Some("grineer"));
    assert!(
        parsed
            .fissures
            .iter()
            .filter(|fissure| fissure.railjack)
            .all(|fissure| fissure.instance_id.len() == 62)
    );
    assert!(
        parsed
            .sortie
            .iter()
            .flat_map(|activity| &activity.missions)
            .all(|mission| mission.id.len() == 64)
    );
    assert!(encode_worldstate(&parsed).is_ok());
}

#[test]
fn maximum_schema_shape_stays_below_the_host_payload_limit() {
    let field = "x".repeat(96);
    let status_row = StatusRow {
        id: "x".repeat(64),
        state: Some(field.clone()),
        activation_secs: Some(u64::MAX),
        expires_at_secs: u64::MAX,
        location: Some(field.clone()),
    };
    let fissure = Fissure {
        instance_id: "x".repeat(64),
        era: field.clone(),
        mission_type: field.clone(),
        node: field.clone(),
        expires_at_secs: u64::MAX,
        steel_path: true,
        railjack: true,
    };
    let mission = ActivityMission {
        id: "x".repeat(64),
        mission_type: field.clone(),
        node: field.clone(),
        modifier: Some(field.clone()),
    };
    let activity = Activity {
        id: "x".repeat(64),
        boss: field.clone(),
        expires_at_secs: u64::MAX,
        missions: vec![mission; 3],
    };
    let reward = Reward {
        item_key: field.clone(),
        label: field.clone(),
        count: u32::MAX,
    };
    let invasion = Invasion {
        instance_id: "x".repeat(64),
        node: field.clone(),
        attacker_faction: field.clone(),
        defender_faction: field,
        attacker_reward: Some(reward.clone()),
        defender_reward: Some(reward),
        count: i64::MIN,
        goal: i64::MAX,
        completed: true,
    };
    let payload = WorldstatePayload {
        captured_at_secs: u64::MAX,
        status: Status {
            rows: vec![status_row; 6],
        },
        fissures: vec![fissure; 96],
        sortie: Some(activity.clone()),
        archon: Some(activity),
        invasions: vec![invasion; 32],
    };

    assert!(encode_worldstate(&payload).is_ok());
}

#[test]
fn epoch_forms_activity_identity_and_reward_counts_are_unambiguous() {
    let seconds = br#"{"ActiveMissions":[{"_id":{"$oid":"000000000000000000000001"},"Modifier":"VoidT1","MissionType":"MT_DEFENSE","Node":"SolNode1","Expiry":10000000001}]}"#;
    assert_eq!(
        parse_worldstate(seconds, NOW_SECS)
            .expect("bare seconds stay seconds")
            .fissures[0]
            .expires_at_secs,
        10_000_000_001
    );

    let zero_reward = br#"{"Invasions":[{"_id":{"$oid":"000000000000000000000001"},"Node":"SolNode1","Faction":"FC_GRINEER","DefenderFaction":"FC_CORPUS","AttackerReward":{"countedItems":[{"ItemType":"/Lotus/Item","ItemCount":0}]}}]}"#;
    assert!(parse_worldstate(zero_reward, NOW_SECS).is_err());

    let duplicate_activities = br#"{"Sorties":[{"Boss":"BOSS","Expiry":1777001000,"Variants":[{"missionType":"MT_A","node":"NODE_A"}]},{"Boss":"BOSS","Expiry":1777001000,"Variants":[{"missionType":"MT_A","node":"NODE_A"}]}]}"#;
    let sortie = parse_worldstate(duplicate_activities, NOW_SECS)
        .expect("identical activity duplicate")
        .sortie
        .expect("deduplicated activity");
    assert_eq!(sortie.missions.len(), 1);
    assert_eq!(sortie.missions[0].id.len(), 64);
}

#[test]
fn parser_rejects_hostile_depth_size_arrays_strings_dates_and_ids() {
    let too_many = format!(
        "{{\"ActiveMissions\":[{}]}}",
        (0..97)
            .map(|index| format!(
                "{{\"_id\":{{\"$oid\":\"{index:024x}\"}},\"Modifier\":\"VoidT1\",\"MissionType\":\"MT_DEFENSE\",\"Node\":\"SolNode1\",\"Expiry\":1777000100}}"
            ))
            .collect::<Vec<_>>()
            .join(",")
    );
    let overlong = format!(
        "{{\"ActiveMissions\":[{{\"_id\":{{\"$oid\":\"000000000000000000000001\"}},\"Modifier\":\"VoidT1\",\"MissionType\":\"MT_DEFENSE\",\"Node\":\"{}\",\"Expiry\":1777000100}}]}}",
        "x".repeat(513)
    );
    let invalid_date = br#"{"ActiveMissions":[{"_id":{"$oid":"000000000000000000000001"},"Modifier":"VoidT1","MissionType":"MT_DEFENSE","Node":"SolNode1","Expiry":-1}]}"#;
    let invalid_id = br#"{"Invasions":[{"_id":{"$oid":"NOT-AN-OID"},"Node":"SolNode1"}]}"#;
    let oversized = vec![b' '; 2 * 1024 * 1024 + 1];

    for (name, bytes) in [
        ("depth", HOSTILE),
        ("array", too_many.as_bytes()),
        ("string", overlong.as_bytes()),
        ("date", invalid_date.as_slice()),
        ("id", invalid_id.as_slice()),
        ("body", oversized.as_slice()),
    ] {
        assert!(
            parse_worldstate(bytes, NOW_SECS).is_err(),
            "hostile {name} must fail closed"
        );
    }
}

#[test]
fn current_syndicate_volume_is_accepted_but_the_collection_remains_bounded() {
    let syndicates = (0..37)
        .map(|index| {
            let tag = match index {
                32 => "CetusSyndicate".to_owned(),
                33 => "ZarimanSyndicate".to_owned(),
                _ => format!("UnusedSyndicate{index}"),
            };
            format!(r#"{{"Tag":"{tag}","Expiry":1777009000}}"#)
        })
        .collect::<Vec<_>>()
        .join(",");
    let current = format!(r#"{{"SyndicateMissions":[{syndicates}]}}"#);
    assert!(parse_worldstate(current.as_bytes(), NOW_SECS).is_ok());

    let excessive = format!(
        r#"{{"SyndicateMissions":[{}]}}"#,
        (0..129)
            .map(|index| format!(r#"{{"Tag":"UnusedSyndicate{index}","Expiry":1777009000}}"#))
            .collect::<Vec<_>>()
            .join(",")
    );
    assert!(parse_worldstate(excessive.as_bytes(), NOW_SECS).is_err());
}

#[test]
fn invasion_without_provider_id_uses_stable_raw_field_identity() {
    let first = parse_worldstate(MINIMAL, NOW_SECS)
        .expect("first parse")
        .invasions
        .into_iter()
        .find(|invasion| invasion.node == "Aphrodite · Venus")
        .expect("fixture invasion without provider id")
        .instance_id;
    let second = parse_worldstate(MINIMAL, NOW_SECS + 1)
        .expect("second parse")
        .invasions
        .into_iter()
        .find(|invasion| invasion.node == "Aphrodite · Venus")
        .expect("same fixture invasion")
        .instance_id;

    assert_eq!(first, second);
    assert_eq!(first.len(), 60);
    assert!(first.starts_with("inv-"));
    assert!(
        first["inv-".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    );
}

#[test]
fn unknown_display_codes_use_bounded_readable_fallbacks() {
    let input = br#"{"Sorties":[{"Boss":"SORTIE_BOSS_UNKNOWN_XYZ","Expiry":1777001000,"Variants":[{"missionType":"MT_UNKNOWN_MODE","modifierType":"SORTIE_MODIFIER_UNKNOWN_RULE","node":"UnknownNode"}]}]}"#;
    let sortie = parse_worldstate(input, NOW_SECS)
        .expect("bounded unknown codes")
        .sortie
        .expect("active sortie");

    assert_eq!(sortie.boss, "Unknown Xyz");
    assert_eq!(sortie.missions[0].mission_type, "Unknown Mode");
    assert_eq!(sortie.missions[0].modifier.as_deref(), Some("Unknown Rule"));
    assert_eq!(sortie.missions[0].node, "UnknownNode");
}

#[test]
fn cycle_math_handles_host_dates_before_the_vallis_epoch_without_saturation() {
    let parsed = parse_worldstate(b"{}", 1).expect("explicit pre-epoch cycle");
    let vallis = parsed
        .status
        .rows
        .iter()
        .find(|row| row.id == "vallis")
        .expect("vallis row");

    assert_eq!(vallis.state.as_deref(), Some("cold"));
    assert_eq!(vallis.expires_at_secs, 808);

    const EPOCH: u64 = 1_770_234_408;
    for (now, expected_state, expected_expiry) in [
        (EPOCH, "warm", EPOCH + 400),
        (EPOCH + 399, "warm", EPOCH + 400),
        (EPOCH + 400, "cold", EPOCH + 1_600),
    ] {
        let parsed = parse_worldstate(b"{}", now).expect("Vallis boundary");
        let vallis = parsed
            .status
            .rows
            .iter()
            .find(|row| row.id == "vallis")
            .expect("Vallis row");
        assert_eq!(vallis.state.as_deref(), Some(expected_state));
        assert_eq!(vallis.expires_at_secs, expected_expiry);
    }
}

#[test]
fn cetus_phase_uses_the_minute_aligned_bounty_end() {
    let input = br#"{"SyndicateMissions":[{"Tag":"CetusSyndicate","Expiry":10000}]}"#;
    for (now, expected_state, expected_expiry) in [(8_000, "night", 9_960), (6_000, "day", 6_960)] {
        let parsed = parse_worldstate(input, now).expect("Cetus boundary");
        let cetus = parsed
            .status
            .rows
            .iter()
            .find(|row| row.id == "cetus")
            .expect("Cetus row");
        assert_eq!(cetus.state.as_deref(), Some(expected_state));
        assert_eq!(cetus.expires_at_secs, expected_expiry);
    }
}

#[test]
fn init_requires_the_exact_http_and_provider_grants() {
    let mut provider = WorldstateProvider::default();
    let harness = WidgetHarness::from_init(&mut provider, init_input(true, &["api.warframe.com"]))
        .expect("exact grants");
    assert_eq!(
        http_get(harness.output()),
        Some((1, "api.warframe.com", "/cdn/worldState.php"))
    );

    for (provider_grant, hosts) in [
        (false, &["api.warframe.com"][..]),
        (true, &[][..]),
        (true, &["example.com"][..]),
        (true, &["api.warframe.com", "example.com"][..]),
    ] {
        let mut provider = WorldstateProvider::default();
        assert!(matches!(
            WidgetHarness::from_init(&mut provider, init_input(provider_grant, hosts)),
            Err(GuestError::Unavailable)
        ));
    }
}

#[test]
fn pre_tick_response_publishes_on_first_tick_then_respects_cadence_and_revisions() {
    const NOW_MS: u64 = NOW_SECS * 1_000;
    let mut provider = WorldstateProvider::default();
    let mut harness =
        WidgetHarness::from_init(&mut provider, init_input(true, &["api.warframe.com"]))
            .expect("provider init");

    harness
        .send(http_result(1, Some(200), MINIMAL))
        .expect("bounded pre-tick response");
    assert!(harness.output().commands.is_empty());
    harness
        .send(HostEvent::Tick(NOW_MS))
        .expect("first host time");
    let (revision, payload) = provider_publish(harness.output()).expect("first publish");
    assert_eq!(revision, 1);
    assert_eq!(
        serde_json::from_slice::<CapturedAt>(payload)
            .expect("published schema")
            .captured_at_secs,
        NOW_SECS
    );

    harness
        .send(HostEvent::Tick(NOW_MS))
        .expect("equal tick is allowed");
    harness
        .send(HostEvent::Tick(NOW_MS + 59_999))
        .expect("before refresh cadence");
    assert!(harness.output().commands.is_empty());
    harness
        .send(HostEvent::Tick(NOW_MS + 60_000))
        .expect("refresh cadence");
    assert_eq!(
        http_get(harness.output()),
        Some((2, "api.warframe.com", "/cdn/worldState.php"))
    );
    harness
        .send(http_result(2, Some(200), MINIMAL))
        .expect("second response");
    assert_eq!(
        provider_publish(harness.output()).map(|value| value.0),
        Some(2)
    );
    assert!(matches!(
        harness.send(HostEvent::Tick(NOW_MS + 59_999)),
        Err(HarnessError::Widget(GuestError::InvalidInput))
    ));
}

#[test]
fn invalid_http_results_fail_without_publish_or_raw_retention_and_can_retry() {
    const NOW_MS: u64 = NOW_SECS * 1_000;
    let mut provider = WorldstateProvider::default();
    let mut harness =
        WidgetHarness::from_init(&mut provider, init_input(true, &["api.warframe.com"]))
            .expect("provider init");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");

    assert!(matches!(
        harness.send(http_result(1, Some(599), b"PRIVATE-RAW-RESPONSE")),
        Err(HarnessError::Widget(GuestError::Unavailable))
    ));
    assert!(provider_publish(harness.output()).is_none());
    harness
        .send(HostEvent::Tick(NOW_MS + 60_000))
        .expect("retry after HTTP failure");
    assert_eq!(http_get(harness.output()).map(|value| value.0), Some(2));
    assert!(matches!(
        harness.send(http_result(2, Some(200), HOSTILE)),
        Err(HarnessError::Widget(GuestError::Unavailable))
    ));
    harness
        .send(HostEvent::Tick(NOW_MS + 120_000))
        .expect("retry after parse failure");
    assert_eq!(http_get(harness.output()).map(|value| value.0), Some(3));
    assert!(matches!(
        harness.send(http_result(4, Some(200), MINIMAL)),
        Err(HarnessError::Widget(GuestError::InvalidInput))
    ));
}

#[test]
fn published_data_becomes_unavailable_at_exactly_five_minutes() {
    const NOW_MS: u64 = NOW_SECS * 1_000;
    let mut provider = WorldstateProvider::default();
    let mut harness =
        WidgetHarness::from_init(&mut provider, init_input(true, &["api.warframe.com"]))
            .expect("provider init");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(http_result(1, Some(200), MINIMAL))
        .expect("fresh publish");
    harness
        .send(HostEvent::Tick(NOW_MS + 60_000))
        .expect("second request starts");
    harness
        .send(HostEvent::Tick(NOW_MS + 299_999))
        .expect("last fresh millisecond");
    assert!(matches!(
        harness.send(HostEvent::Tick(NOW_MS + 300_000)),
        Err(HarnessError::Widget(GuestError::Unavailable))
    ));
    assert!(matches!(
        harness.send(HostEvent::LocaleChanged("fr".to_owned())),
        Err(HarnessError::Widget(GuestError::Unavailable))
    ));
}

#[test]
fn refresh_failure_keeps_prior_value_fresh_until_the_exact_ttl() {
    const NOW_MS: u64 = NOW_SECS * 1_000;
    let mut provider = WorldstateProvider::default();
    let mut harness =
        WidgetHarness::from_init(&mut provider, init_input(true, &["api.warframe.com"]))
            .expect("provider init");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(http_result(1, Some(200), MINIMAL))
        .expect("fresh publish");
    harness
        .send(HostEvent::Tick(NOW_MS + 60_000))
        .expect("refresh request");
    harness
        .send(http_result(2, Some(200), HOSTILE))
        .expect("invalid refresh keeps prior freshness");
    assert!(provider_publish(harness.output()).is_none());
    harness
        .send(HostEvent::Tick(NOW_MS + 299_999))
        .expect("prior value remains fresh");
    assert!(matches!(
        harness.send(HostEvent::Tick(NOW_MS + 300_000)),
        Err(HarnessError::Widget(GuestError::Unavailable))
    ));
}

#[test]
fn stale_failure_is_reported_once_then_refresh_can_recover() {
    const NOW_MS: u64 = NOW_SECS * 1_000;
    let mut provider = WorldstateProvider::default();
    let mut harness =
        WidgetHarness::from_init(&mut provider, init_input(true, &["api.warframe.com"]))
            .expect("provider init");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    harness
        .send(http_result(1, Some(200), MINIMAL))
        .expect("initial publish");

    for (request_id, elapsed) in [(2, 60_000), (3, 120_000), (4, 180_000), (5, 240_000)] {
        harness
            .send(HostEvent::Tick(NOW_MS + elapsed))
            .expect("scheduled refresh");
        assert_eq!(
            http_get(harness.output()).map(|request| request.0),
            Some(request_id)
        );
        harness
            .send(http_result(request_id, Some(200), HOSTILE))
            .expect("fresh prior value tolerates invalid refresh");
    }

    assert!(matches!(
        harness.send(HostEvent::Tick(NOW_MS + 300_000)),
        Err(HarnessError::Widget(GuestError::Unavailable))
    ));
    harness
        .send(HostEvent::Tick(NOW_MS + 300_000))
        .expect("retry continues after stale report");
    assert_eq!(http_get(harness.output()).map(|request| request.0), Some(6));
    harness
        .send(http_result(6, Some(200), MINIMAL))
        .expect("valid refresh recovers");
    assert_eq!(
        provider_publish(harness.output()).map(|publish| publish.0),
        Some(2)
    );
}

#[test]
fn request_and_revision_counters_fail_closed_at_exhaustion() {
    const NOW_MS: u64 = NOW_SECS * 1_000;
    let mut request_exhausted = WorldstateProvider {
        last_request_id: u32::MAX,
        ..WorldstateProvider::default()
    };
    assert!(matches!(
        WidgetHarness::from_init(
            &mut request_exhausted,
            init_input(true, &["api.warframe.com"]),
        ),
        Err(GuestError::Unavailable)
    ));

    let mut revision_exhausted = WorldstateProvider {
        last_revision: u64::MAX,
        ..WorldstateProvider::default()
    };
    let mut harness = WidgetHarness::from_init(
        &mut revision_exhausted,
        init_input(true, &["api.warframe.com"]),
    )
    .expect("provider init");
    harness.send(HostEvent::Tick(NOW_MS)).expect("host time");
    assert!(matches!(
        harness.send(http_result(1, Some(200), MINIMAL)),
        Err(HarnessError::Widget(GuestError::Unavailable))
    ));
    assert!(provider_publish(harness.output()).is_none());
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CapturedAt {
    captured_at_secs: u64,
}

fn init_input(provider: bool, hosts: &[&str]) -> InitInput {
    InitInput {
        locale: "en".to_owned(),
        granted_capabilities: GrantedCapabilities {
            http_hosts: hosts.iter().map(|host| (*host).to_owned()).collect(),
            game_data: Vec::new(),
            storage: false,
            clipboard_write: false,
            provider,
        },
        settings: Vec::new(),
        session_data: None,
    }
}

fn http_result(request_id: u32, status: Option<u16>, body: &[u8]) -> HostEvent {
    HostEvent::HttpResult((
        request_id,
        status,
        body.to_vec(),
        HttpResponseMetadata {
            content_type: Some("application/json".to_owned()),
            headers: Vec::new(),
        },
    ))
}

fn http_get(output: &overcrow_widget_sdk::GuestOutput) -> Option<(u32, &str, &str)> {
    output.commands.iter().find_map(|command| match command {
        HostCommand::HttpGet((request_id, host, path)) => {
            Some((*request_id, host.as_str(), path.as_str()))
        }
        _ => None,
    })
}

fn provider_publish(output: &overcrow_widget_sdk::GuestOutput) -> Option<(u64, &[u8])> {
    output.commands.iter().find_map(|command| match command {
        HostCommand::ProviderPublish((schema, revision, payload))
            if schema == "com.playervox.overcrow.warframe.worldstate/worldstate.v1" =>
        {
            Some((*revision, payload.as_slice()))
        }
        _ => None,
    })
}

#[test]
fn parser_deduplicates_and_sorts_reordered_missions() {
    let first = br#"{"ActiveMissions":[{"_id":{"$oid":"000000000000000000000002"},"Modifier":"VoidT2","MissionType":"MT_SURVIVAL","Node":"SolNode2","Expiry":1777000200},{"_id":{"$oid":"000000000000000000000001"},"Modifier":"VoidT1","MissionType":"MT_DEFENSE","Node":"SolNode1","Expiry":1777000100}]}"#;
    let reordered = br#"{"ActiveMissions":[{"_id":{"$oid":"000000000000000000000001"},"Modifier":"VoidT1","MissionType":"MT_DEFENSE","Node":"SolNode1","Expiry":1777000100},{"_id":{"$oid":"000000000000000000000002"},"Modifier":"VoidT2","MissionType":"MT_SURVIVAL","Node":"SolNode2","Expiry":1777000200},{"_id":{"$oid":"000000000000000000000001"},"Modifier":"VoidT1","MissionType":"MT_DEFENSE","Node":"SolNode1","Expiry":1777000100}]}"#;

    let first = parse_worldstate(first, NOW_SECS)
        .expect("first order")
        .fissures;
    let reordered = parse_worldstate(reordered, NOW_SECS)
        .expect("reordered duplicate")
        .fissures;
    assert!(first == reordered);
}

#[test]
fn duplicate_syndicate_rows_accept_only_identical_values() {
    let identical = br#"{"SyndicateMissions":[{"Tag":"CetusSyndicate","Expiry":1777009000},{"Tag":"CetusSyndicate","Expiry":1777009000}]}"#;
    let conflicting = br#"{"SyndicateMissions":[{"Tag":"CetusSyndicate","Expiry":1777009000},{"Tag":"CetusSyndicate","Expiry":1777010000}]}"#;
    let reversed = br#"{"SyndicateMissions":[{"Tag":"CetusSyndicate","Expiry":1777010000},{"Tag":"CetusSyndicate","Expiry":1777009000}]}"#;

    assert!(parse_worldstate(identical, NOW_SECS).is_ok());
    assert!(parse_worldstate(conflicting, NOW_SECS).is_err());
    assert!(parse_worldstate(reversed, NOW_SECS).is_err());
}
