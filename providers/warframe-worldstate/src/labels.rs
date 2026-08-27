use alloc::{
    collections::BTreeMap,
    format,
    string::{String, ToString},
};

use serde::Deserialize;

use crate::parse::ParseError;

const DISPLAY_BYTES: usize = 96;

#[derive(Deserialize)]
struct NamedEntry {
    value: String,
}

pub(crate) struct Labels {
    bosses: BTreeMap<String, String>,
    factions: BTreeMap<String, String>,
    items: BTreeMap<String, String>,
    mission_types: BTreeMap<String, NamedEntry>,
    modifiers: BTreeMap<String, String>,
    nodes: BTreeMap<String, NamedEntry>,
}

impl Labels {
    pub(crate) fn load() -> Result<Self, ParseError> {
        Ok(Self {
            bosses: table(include_str!("../data/sortie_bosses.json"))?,
            factions: table(include_str!("../data/factions.json"))?,
            items: table(include_str!("../data/item_names.json"))?,
            mission_types: table(include_str!("../data/mission_types.json"))?,
            modifiers: table(include_str!("../data/sortie_modifiers.json"))?,
            nodes: table(include_str!("../data/sol_nodes.json"))?,
        })
    }

    pub(crate) fn boss(&self, code: &str) -> Result<String, ParseError> {
        self.code_label(&self.bosses, code)
    }

    pub(crate) fn faction(&self, code: &str) -> Result<String, ParseError> {
        self.code_label(&self.factions, code)
    }

    pub(crate) fn item(&self, path: &str) -> Result<String, ParseError> {
        self.code_label(&self.items, path)
    }

    pub(crate) fn mission_type(&self, code: &str) -> Result<String, ParseError> {
        match self.mission_types.get(code) {
            Some(entry) => bounded_display(&entry.value),
            None => fallback_label(code),
        }
    }

    pub(crate) fn modifier(&self, code: &str) -> Result<String, ParseError> {
        self.code_label(&self.modifiers, code)
    }

    pub(crate) fn node(&self, code: &str) -> Result<String, ParseError> {
        let Some(entry) = self.nodes.get(code) else {
            return bounded_display(code);
        };
        if let Some((node, suffix)) = entry.value.rsplit_once(" (")
            && let Some(planet) = suffix.strip_suffix(')')
            && !node.is_empty()
            && !planet.is_empty()
            && node != code
        {
            return bounded_display(&format!("{node} · {planet}"));
        }
        bounded_display(&entry.value)
    }

    fn code_label(
        &self,
        values: &BTreeMap<String, String>,
        code: &str,
    ) -> Result<String, ParseError> {
        let tail = code.rsplit('/').next().ok_or(ParseError)?;
        match values.get(code).or_else(|| values.get(tail)) {
            Some(label) => bounded_display(label),
            None => fallback_label(tail),
        }
    }
}

fn table<T: for<'de> Deserialize<'de>>(source: &str) -> Result<T, ParseError> {
    serde_json::from_str(source).map_err(|_| ParseError)
}

fn fallback_label(code: &str) -> Result<String, ParseError> {
    let code = ["SORTIE_MODIFIER_", "SORTIE_BOSS_", "MT_", "FC_"]
        .into_iter()
        .find_map(|prefix| code.strip_prefix(prefix))
        .unwrap_or(code);
    if code.is_empty() {
        return Err(ParseError);
    }

    let mut words = String::new();
    let mut previous_lower = false;
    for character in code.chars() {
        if character == '_' {
            words.push(' ');
            previous_lower = false;
            continue;
        }
        if previous_lower && character.is_ascii_uppercase() {
            words.push(' ');
        }
        words.push(character);
        previous_lower = character.is_ascii_lowercase();
    }

    let mut pretty = String::new();
    for word in words.split_whitespace() {
        if !pretty.is_empty() {
            pretty.push(' ');
        }
        let lowercase = word.to_ascii_lowercase();
        let mut characters = lowercase.chars();
        let first = characters.next().ok_or(ParseError)?;
        pretty.extend(first.to_uppercase());
        pretty.push_str(characters.as_str());
    }
    bounded_display(&pretty)
}

fn bounded_display(value: &str) -> Result<String, ParseError> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(ParseError);
    }
    if value.len() <= DISPLAY_BYTES {
        return Ok(value.to_string());
    }
    let boundary = value
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= DISPLAY_BYTES)
        .last()
        .ok_or(ParseError)?;
    Ok(value[..boundary].to_string())
}
