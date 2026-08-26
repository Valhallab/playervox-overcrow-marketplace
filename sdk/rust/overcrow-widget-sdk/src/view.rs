use alloc::{
    borrow::ToOwned,
    collections::BTreeSet,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use core::{error::Error, fmt};

use crate::{GuestError, GuestOutput, HostCommand, Locale, LocalizedText, View, ViewNode};

const MAX_VIEW_NODES: usize = 512;
const MAX_VIEW_DEPTH: usize = 32;
const MAX_VIEW_TEXT_BYTES: usize = 64 * 1024;
const MAX_VIEW_STRING_BYTES: usize = 4 * 1024;
const MAX_VIEW_VARIABLE_BYTES: usize = 96 * 1024;
const MAX_ELEMENT_ID_BYTES: usize = 64;
const MAX_GUEST_COMMANDS: usize = 32;
const MAX_GUEST_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_HTTP_CONCURRENT_REQUESTS: usize = 2;
const MAX_STORAGE_KEY_BYTES: usize = 128;
const MAX_STORAGE_VALUE_BYTES: usize = 64 * 1024;
const MAX_PROVIDER_PAYLOAD_BYTES: usize = 256 * 1024;
const MAX_SCHEMA_ID_BYTES: usize = 192;
const MAX_COMMAND_STRING_BYTES: usize = 4 * 1024;
const MIN_NEXT_WAKE_MS: u32 = 100;
const MAX_NEXT_WAKE_MS: u32 = 60 * 60 * 1_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NodeId(u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuildError {
    NodeLimit,
    InvalidRoot,
    InvalidTree,
    DepthLimit,
    ElementId,
    DuplicateElementId,
    StringLimit,
    TextLimit,
    DataLimit,
    CommandLimit,
    OutputLimit,
    HttpConcurrency,
    RequestId,
    StorageKeyLimit,
    StorageValueLimit,
    ProviderPayloadLimit,
    SchemaLimit,
    TimerLimit,
    Locale,
    LocaleLimit,
    DuplicateLocale,
}

impl fmt::Display for BuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NodeLimit => "view node limit exceeded",
            Self::InvalidRoot => "view root is invalid",
            Self::InvalidTree => "view must be one connected tree",
            Self::DepthLimit => "view depth limit exceeded",
            Self::ElementId => "element ID is invalid",
            Self::DuplicateElementId => "element ID is duplicated",
            Self::StringLimit => "string limit exceeded",
            Self::TextLimit => "view text limit exceeded",
            Self::DataLimit => "view data limit exceeded",
            Self::CommandLimit => "command limit exceeded",
            Self::OutputLimit => "guest output limit exceeded",
            Self::HttpConcurrency => "HTTP concurrency limit exceeded",
            Self::RequestId => "request IDs must be nonzero and increasing",
            Self::StorageKeyLimit => "storage key limit exceeded",
            Self::StorageValueLimit => "storage value limit exceeded",
            Self::ProviderPayloadLimit => "provider payload limit exceeded",
            Self::SchemaLimit => "provider schema limit exceeded",
            Self::TimerLimit => "wake timer is outside host limits",
            Self::Locale => "locale is invalid",
            Self::LocaleLimit => "translation limit exceeded",
            Self::DuplicateLocale => "translation locale is duplicated",
        })
    }
}

impl Error for BuildError {}

impl From<BuildError> for GuestError {
    fn from(_: BuildError) -> Self {
        Self::InvalidInput
    }
}

pub struct ViewBuilder {
    locale: Locale,
    nodes: Vec<ViewNode>,
    ids: BTreeSet<String>,
    text_bytes: usize,
    variable_bytes: usize,
}

impl ViewBuilder {
    pub fn new(locale: &Locale) -> Self {
        Self {
            locale: locale.clone(),
            nodes: Vec::new(),
            ids: BTreeSet::new(),
            text_bytes: 0,
            variable_bytes: 0,
        }
    }

    pub fn text(&mut self, text: LocalizedText) -> Result<NodeId, BuildError> {
        let text = text.resolve(&self.locale).to_owned();
        self.add_text(&text)?;
        self.push(ViewNode::Text(text))
    }

    pub fn button(&mut self, id: &str, label: LocalizedText) -> Result<NodeId, BuildError> {
        self.add_id(id)?;
        let label = label.resolve(&self.locale).to_owned();
        self.add_text(&label)?;
        self.push(ViewNode::Button((id.to_owned(), label)))
    }

    pub fn text_input(
        &mut self,
        id: &str,
        value: &str,
        placeholder: LocalizedText,
    ) -> Result<NodeId, BuildError> {
        self.add_id(id)?;
        self.add_text(value)?;
        let placeholder = placeholder.resolve(&self.locale).to_owned();
        self.add_text(&placeholder)?;
        self.push(ViewNode::TextInput((
            id.to_owned(),
            value.to_owned(),
            placeholder,
        )))
    }

    pub fn container(&mut self, children: &[NodeId]) -> Result<NodeId, BuildError> {
        self.add_variable(children.len().checked_mul(4).ok_or(BuildError::DataLimit)?)?;
        self.push(ViewNode::Container(
            children.iter().map(|child| child.0).collect(),
        ))
    }

    pub fn finish(self, root: NodeId, revision: u64) -> Result<View, BuildError> {
        if revision == 0
            || usize::try_from(root.0)
                .ok()
                .filter(|index| *index < self.nodes.len())
                .is_none()
        {
            return Err(BuildError::InvalidRoot);
        }
        validate_tree(&self.nodes, root)?;
        Ok(View {
            revision,
            root: root.0,
            nodes: self.nodes,
        })
    }

    fn push(&mut self, node: ViewNode) -> Result<NodeId, BuildError> {
        if self.nodes.len() >= MAX_VIEW_NODES {
            return Err(BuildError::NodeLimit);
        }
        let id = u32::try_from(self.nodes.len()).map_err(|_| BuildError::NodeLimit)?;
        self.nodes.push(node);
        Ok(NodeId(id))
    }

    fn add_id(&mut self, id: &str) -> Result<(), BuildError> {
        if id.is_empty() || id.len() > MAX_ELEMENT_ID_BYTES {
            return Err(BuildError::ElementId);
        }
        if !self.ids.insert(id.to_owned()) {
            return Err(BuildError::DuplicateElementId);
        }
        self.add_variable(id.len())
    }

    fn add_text(&mut self, text: &str) -> Result<(), BuildError> {
        if text.len() > MAX_VIEW_STRING_BYTES {
            return Err(BuildError::StringLimit);
        }
        self.text_bytes = self
            .text_bytes
            .checked_add(text.len())
            .filter(|bytes| *bytes <= MAX_VIEW_TEXT_BYTES)
            .ok_or(BuildError::TextLimit)?;
        self.add_variable(text.len())
    }

    fn add_variable(&mut self, bytes: usize) -> Result<(), BuildError> {
        self.variable_bytes = self
            .variable_bytes
            .checked_add(bytes)
            .filter(|total| *total <= MAX_VIEW_VARIABLE_BYTES)
            .ok_or(BuildError::DataLimit)?;
        Ok(())
    }
}

fn validate_tree(nodes: &[ViewNode], root: NodeId) -> Result<(), BuildError> {
    let mut parents = vec![0_u8; nodes.len()];
    for node in nodes {
        if let ViewNode::Container(children) = node {
            let mut unique = BTreeSet::new();
            for child in children {
                let child = usize::try_from(*child).map_err(|_| BuildError::InvalidTree)?;
                if child >= nodes.len() || !unique.insert(child) {
                    return Err(BuildError::InvalidTree);
                }
                parents[child] = parents[child]
                    .checked_add(1)
                    .filter(|count| *count <= 1)
                    .ok_or(BuildError::InvalidTree)?;
            }
        }
    }
    let root = usize::try_from(root.0).map_err(|_| BuildError::InvalidRoot)?;
    if parents[root] != 0
        || parents
            .iter()
            .enumerate()
            .any(|(index, count)| index != root && *count != 1)
    {
        return Err(BuildError::InvalidTree);
    }
    let mut visited = vec![false; nodes.len()];
    visit(nodes, root, 1, &mut visited)?;
    visited
        .iter()
        .all(|seen| *seen)
        .then_some(())
        .ok_or(BuildError::InvalidTree)
}

fn visit(
    nodes: &[ViewNode],
    index: usize,
    depth: usize,
    visited: &mut [bool],
) -> Result<(), BuildError> {
    if depth > MAX_VIEW_DEPTH {
        return Err(BuildError::DepthLimit);
    }
    if visited[index] {
        return Err(BuildError::InvalidTree);
    }
    visited[index] = true;
    if let ViewNode::Container(children) = &nodes[index] {
        for child in children {
            visit(
                nodes,
                usize::try_from(*child).map_err(|_| BuildError::InvalidTree)?,
                depth + 1,
                visited,
            )?;
        }
    }
    Ok(())
}

pub struct OutputBuilder {
    view: Option<View>,
    commands: Vec<HostCommand>,
    next_wake_ms: Option<u32>,
    last_request_id: Option<u32>,
    output_bytes: usize,
    http_requests: usize,
}

impl OutputBuilder {
    pub const fn new() -> Self {
        Self {
            view: None,
            commands: Vec::new(),
            next_wake_ms: None,
            last_request_id: None,
            output_bytes: 64,
            http_requests: 0,
        }
    }

    pub fn view(mut self, view: View) -> Result<Self, BuildError> {
        self.add_output_bytes(view_wire_bytes(&view)?)?;
        self.view = Some(view);
        Ok(self)
    }

    pub fn next_wake_ms(&mut self, value: u32) -> Result<(), BuildError> {
        if !(MIN_NEXT_WAKE_MS..=MAX_NEXT_WAKE_MS).contains(&value) {
            return Err(BuildError::TimerLimit);
        }
        self.next_wake_ms = Some(value);
        Ok(())
    }

    pub fn http_get(&mut self, request_id: u32, host: &str, path: &str) -> Result<(), BuildError> {
        if host.is_empty()
            || host.len() > MAX_COMMAND_STRING_BYTES
            || path.is_empty()
            || path.len() > MAX_COMMAND_STRING_BYTES
            || !path.starts_with('/')
            || path.contains('#')
            || !path.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
        {
            return Err(BuildError::StringLimit);
        }
        if self.http_requests >= MAX_HTTP_CONCURRENT_REQUESTS {
            return Err(BuildError::HttpConcurrency);
        }
        self.validate_request(request_id)?;
        self.push(
            HostCommand::HttpGet((request_id, host.to_owned(), path.to_owned())),
            64 + host.len() + path.len(),
        )?;
        self.last_request_id = Some(request_id);
        self.http_requests += 1;
        Ok(())
    }

    pub fn storage_get(&mut self, request_id: u32, key: &str) -> Result<(), BuildError> {
        validate_storage_key(key)?;
        self.validate_request(request_id)?;
        self.push(
            HostCommand::StorageGet((request_id, key.to_owned())),
            64 + key.len(),
        )?;
        self.last_request_id = Some(request_id);
        Ok(())
    }

    pub fn storage_set(
        &mut self,
        request_id: u32,
        key: &str,
        value: &[u8],
    ) -> Result<(), BuildError> {
        validate_storage_key(key)?;
        if value.len() > MAX_STORAGE_VALUE_BYTES {
            return Err(BuildError::StorageValueLimit);
        }
        self.validate_request(request_id)?;
        self.push(
            HostCommand::StorageSet((request_id, key.to_owned(), value.to_vec())),
            64 + key.len() + value.len(),
        )?;
        self.last_request_id = Some(request_id);
        Ok(())
    }

    pub fn storage_delete(&mut self, request_id: u32, key: &str) -> Result<(), BuildError> {
        validate_storage_key(key)?;
        self.validate_request(request_id)?;
        self.push(
            HostCommand::StorageDelete((request_id, key.to_owned())),
            64 + key.len(),
        )?;
        self.last_request_id = Some(request_id);
        Ok(())
    }

    pub fn clipboard_write(&mut self, text: &str) -> Result<(), BuildError> {
        if text.len() > MAX_VIEW_STRING_BYTES {
            return Err(BuildError::StringLimit);
        }
        self.push(
            HostCommand::ClipboardWrite(text.to_owned()),
            64 + text.len(),
        )
    }

    pub fn provider_publish(
        &mut self,
        schema_id: &str,
        revision: u64,
        payload: &[u8],
    ) -> Result<(), BuildError> {
        if !valid_schema_id(schema_id) || revision == 0 {
            return Err(BuildError::SchemaLimit);
        }
        if payload.len() > MAX_PROVIDER_PAYLOAD_BYTES {
            return Err(BuildError::ProviderPayloadLimit);
        }
        self.push(
            HostCommand::ProviderPublish((schema_id.to_owned(), revision, payload.to_vec())),
            64 + schema_id.len() + payload.len(),
        )
    }

    pub fn finish(self) -> GuestOutput {
        GuestOutput {
            view: self.view,
            commands: self.commands,
            next_wake_ms: self.next_wake_ms,
        }
    }

    fn validate_request(&self, request_id: u32) -> Result<(), BuildError> {
        if request_id == 0 || self.last_request_id.is_some_and(|last| request_id <= last) {
            return Err(BuildError::RequestId);
        }
        Ok(())
    }

    fn push(&mut self, command: HostCommand, bytes: usize) -> Result<(), BuildError> {
        if self.commands.len() >= MAX_GUEST_COMMANDS {
            return Err(BuildError::CommandLimit);
        }
        self.add_output_bytes(bytes)?;
        self.commands.push(command);
        Ok(())
    }

    fn add_output_bytes(&mut self, bytes: usize) -> Result<(), BuildError> {
        self.output_bytes = self
            .output_bytes
            .checked_add(bytes)
            .filter(|total| *total <= MAX_GUEST_OUTPUT_BYTES)
            .ok_or(BuildError::OutputLimit)?;
        Ok(())
    }
}

impl Default for OutputBuilder {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_storage_key(key: &str) -> Result<(), BuildError> {
    if key.is_empty() || key.len() > MAX_STORAGE_KEY_BYTES || key.chars().any(char::is_control) {
        Err(BuildError::StorageKeyLimit)
    } else {
        Ok(())
    }
}

fn valid_schema_id(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_SCHEMA_ID_BYTES {
        return false;
    }
    let Some((provider, local)) = value.split_once('/') else {
        return false;
    };
    if local.contains('/') || !valid_extension_id(provider) {
        return false;
    }
    let Some((name, version)) = local.rsplit_once(".v") else {
        return false;
    };
    if name.is_empty()
        || name.split('.').any(|segment| {
            segment.is_empty()
                || segment.len() > 63
                || segment.starts_with('-')
                || segment.ends_with('-')
                || !segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
    {
        return false;
    }
    version
        .parse::<u32>()
        .ok()
        .is_some_and(|parsed| parsed > 0 && parsed.to_string() == version)
}

fn valid_extension_id(value: &str) -> bool {
    (3..=128).contains(&value.len())
        && value.is_ascii()
        && value.split('.').count() >= 2
        && value.split('.').all(|segment| {
            !segment.is_empty()
                && segment.len() <= 63
                && !segment.starts_with('-')
                && !segment.ends_with('-')
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

fn view_wire_bytes(view: &View) -> Result<usize, BuildError> {
    let mut bytes = 64_usize;
    for node in &view.nodes {
        bytes = bytes.checked_add(64).ok_or(BuildError::OutputLimit)?;
        let variable = match node {
            ViewNode::Container(children) => children
                .len()
                .checked_mul(4)
                .ok_or(BuildError::OutputLimit)?,
            ViewNode::Text(text) | ViewNode::Image(text) => text.len(),
            ViewNode::Button((id, label)) | ViewNode::Toggle((id, label, _)) => id
                .len()
                .checked_add(label.len())
                .ok_or(BuildError::OutputLimit)?,
            ViewNode::TextInput((id, value, placeholder)) => id
                .len()
                .checked_add(value.len())
                .and_then(|total| total.checked_add(placeholder.len()))
                .ok_or(BuildError::OutputLimit)?,
            ViewNode::Selection((id, options, _)) => {
                options.iter().try_fold(id.len(), |total, option| {
                    total
                        .checked_add(option.len())
                        .ok_or(BuildError::OutputLimit)
                })?
            }
            ViewNode::Progress((label, _)) => label.len(),
            ViewNode::Canvas((id, primitives)) => {
                primitives.iter().try_fold(id.len(), |total, primitive| {
                    let primitive_bytes = 32
                        + match primitive {
                            crate::CanvasPrimitive::Text(text) => text.text.len(),
                            crate::CanvasPrimitive::Line(_) | crate::CanvasPrimitive::Rect(_) => 0,
                        };
                    total
                        .checked_add(primitive_bytes)
                        .ok_or(BuildError::OutputLimit)
                })?
            }
        };
        bytes = bytes.checked_add(variable).ok_or(BuildError::OutputLimit)?;
    }
    Ok(bytes)
}
