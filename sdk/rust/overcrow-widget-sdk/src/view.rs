use alloc::{
    borrow::ToOwned,
    collections::BTreeSet,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use core::{error::Error, fmt};

#[cfg(feature = "api-v2")]
use crate::{ContainerLayout, GridLayout, Layout, TextRole};
use crate::{
    GuestError, GuestOutput, HostCommand, Locale, LocalizedText, View, ViewNode, WidgetContext,
    state::{OutputState, RequestKind},
};

const MAX_VIEW_NODES: usize = 512;
const MAX_VIEW_DEPTH: usize = 32;
const MAX_VIEW_TEXT_BYTES: usize = 64 * 1024;
const MAX_VIEW_STRING_BYTES: usize = 4 * 1024;
const MAX_VIEW_VARIABLE_BYTES: usize = 96 * 1024;
const MAX_ELEMENT_ID_BYTES: usize = 64;
const MAX_ASSET_ID_BYTES: usize = 64;
const MAX_VIEW_IMAGE_HANDLES: usize = 64;
const MAX_CANVAS_PRIMITIVES: usize = 256;
const MAX_SELECTION_OPTIONS: usize = 128;
const MAX_VIEW_SELECTION_OPTIONS: usize = 512;
const MAX_GUEST_COMMANDS: usize = 32;
const MAX_GUEST_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_HTTP_CONCURRENT_REQUESTS: usize = 2;
const MAX_OUTSTANDING_REQUESTS: usize = 64;
const MAX_PROVIDER_SCHEMAS: usize = 64;
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
    ImageHandleLimit,
    ImageHandle,
    CanvasPrimitiveLimit,
    CanvasCoordinate,
    SelectionOptionLimit,
    Selection,
    Progress,
    StringLimit,
    TextLimit,
    DataLimit,
    CommandLimit,
    OutputLimit,
    OutstandingRequestLimit,
    RequestId,
    StorageKeyLimit,
    StorageValueLimit,
    ProviderPayloadLimit,
    ProviderRevision,
    ProviderLimit,
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
            Self::ImageHandleLimit => "image handle limit exceeded",
            Self::ImageHandle => "image handle is invalid",
            Self::CanvasPrimitiveLimit => "canvas primitive limit exceeded",
            Self::CanvasCoordinate => "canvas coordinate is invalid",
            Self::SelectionOptionLimit => "selection option limit exceeded",
            Self::Selection => "selection index is invalid",
            Self::Progress => "progress value is invalid",
            Self::StringLimit => "string limit exceeded",
            Self::TextLimit => "view text limit exceeded",
            Self::DataLimit => "view data limit exceeded",
            Self::CommandLimit => "command limit exceeded",
            Self::OutputLimit => "guest output limit exceeded",
            Self::OutstandingRequestLimit => "outstanding request limit exceeded",
            Self::RequestId => "request IDs must be nonzero and increasing",
            Self::StorageKeyLimit => "storage key limit exceeded",
            Self::StorageValueLimit => "storage value limit exceeded",
            Self::ProviderPayloadLimit => "provider payload limit exceeded",
            Self::ProviderRevision => "provider revision must increase",
            Self::ProviderLimit => "provider schema limit exceeded",
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
    image_handles: usize,
    canvas_primitives: usize,
    selection_options: usize,
}

impl ViewBuilder {
    pub fn new(locale: &Locale) -> Self {
        Self {
            locale: locale.clone(),
            nodes: Vec::new(),
            ids: BTreeSet::new(),
            text_bytes: 0,
            variable_bytes: 0,
            image_handles: 0,
            canvas_primitives: 0,
            selection_options: 0,
        }
    }

    pub fn text(&mut self, text: LocalizedText) -> Result<NodeId, BuildError> {
        #[cfg(feature = "api-v1")]
        {
            let text = text.resolve(&self.locale).to_owned();
            let budget = self.check_node(None, &[&text], 0)?;
            Ok(self.push(ViewNode::Text(text), None, budget))
        }
        #[cfg(feature = "api-v2")]
        {
            self.text_role(TextRole::Body, text)
        }
    }

    #[cfg(feature = "api-v2")]
    pub fn text_role(&mut self, role: TextRole, text: LocalizedText) -> Result<NodeId, BuildError> {
        let text = text.resolve(&self.locale).to_owned();
        let budget = self.check_node(None, &[&text], 0)?;
        Ok(self.push(ViewNode::Text((role, text)), None, budget))
    }

    pub fn image(&mut self, asset_id: &str) -> Result<NodeId, BuildError> {
        let image_handles = self
            .image_handles
            .checked_add(1)
            .filter(|count| *count <= MAX_VIEW_IMAGE_HANDLES)
            .ok_or(BuildError::ImageHandleLimit)?;
        if !valid_asset_id(asset_id) {
            return Err(BuildError::ImageHandle);
        }
        let budget = self.check_node(None, &[], asset_id.len())?;
        let node = self.push(ViewNode::Image(asset_id.to_owned()), None, budget);
        self.image_handles = image_handles;
        Ok(node)
    }

    pub fn button(&mut self, id: &str, label: LocalizedText) -> Result<NodeId, BuildError> {
        let label = label.resolve(&self.locale).to_owned();
        let budget = self.check_node(Some(id), &[&label], 0)?;
        Ok(self.push(ViewNode::Button((id.to_owned(), label)), Some(id), budget))
    }

    pub fn toggle(
        &mut self,
        id: &str,
        label: LocalizedText,
        value: bool,
    ) -> Result<NodeId, BuildError> {
        let label = label.resolve(&self.locale).to_owned();
        let budget = self.check_node(Some(id), &[&label], 0)?;
        Ok(self.push(
            ViewNode::Toggle((id.to_owned(), label, value)),
            Some(id),
            budget,
        ))
    }

    pub fn text_input(
        &mut self,
        id: &str,
        value: &str,
        placeholder: LocalizedText,
    ) -> Result<NodeId, BuildError> {
        let placeholder = placeholder.resolve(&self.locale).to_owned();
        let budget = self.check_node(Some(id), &[value, &placeholder], 0)?;
        Ok(self.push(
            ViewNode::TextInput((id.to_owned(), value.to_owned(), placeholder)),
            Some(id),
            budget,
        ))
    }

    pub fn selection(
        &mut self,
        id: &str,
        options: Vec<LocalizedText>,
        selected: u32,
    ) -> Result<NodeId, BuildError> {
        if options.is_empty() || options.len() > MAX_SELECTION_OPTIONS {
            return Err(BuildError::SelectionOptionLimit);
        }
        let selected = usize::try_from(selected).map_err(|_| BuildError::Selection)?;
        if selected >= options.len() {
            return Err(BuildError::Selection);
        }
        let selection_options = self
            .selection_options
            .checked_add(options.len())
            .filter(|count| *count <= MAX_VIEW_SELECTION_OPTIONS)
            .ok_or(BuildError::SelectionOptionLimit)?;
        let options: Vec<String> = options
            .into_iter()
            .map(|text| text.resolve(&self.locale).to_owned())
            .collect();
        let text: Vec<&str> = options.iter().map(String::as_str).collect();
        let budget = self.check_node(Some(id), &text, 0)?;
        let node = self.push(
            ViewNode::Selection((id.to_owned(), options, selected as u32)),
            Some(id),
            budget,
        );
        self.selection_options = selection_options;
        Ok(node)
    }

    pub fn progress(
        &mut self,
        label: LocalizedText,
        value_milli: u16,
    ) -> Result<NodeId, BuildError> {
        if value_milli > 1_000 {
            return Err(BuildError::Progress);
        }
        let label = label.resolve(&self.locale).to_owned();
        let budget = self.check_node(None, &[&label], 0)?;
        Ok(self.push(ViewNode::Progress((label, value_milli)), None, budget))
    }

    pub fn canvas(
        &mut self,
        id: &str,
        primitives: Vec<crate::CanvasPrimitive>,
    ) -> Result<NodeId, BuildError> {
        let canvas_primitives = self
            .canvas_primitives
            .checked_add(primitives.len())
            .filter(|count| *count <= MAX_CANVAS_PRIMITIVES)
            .ok_or(BuildError::CanvasPrimitiveLimit)?;
        let mut texts = Vec::new();
        for primitive in &primitives {
            validate_canvas_primitive(primitive)?;
            if let crate::CanvasPrimitive::Text(text) = primitive {
                texts.push(text.text.as_str());
            }
        }
        let budget = self.check_node(Some(id), &texts, 0)?;
        let node = self.push(
            ViewNode::Canvas((id.to_owned(), primitives)),
            Some(id),
            budget,
        );
        self.canvas_primitives = canvas_primitives;
        Ok(node)
    }

    pub fn container(&mut self, children: &[NodeId]) -> Result<NodeId, BuildError> {
        #[cfg(feature = "api-v1")]
        let node = ViewNode::Container(children.iter().map(|child| child.0).collect());
        #[cfg(feature = "api-v2")]
        let node = ViewNode::Container((
            Layout::Linear(ContainerLayout::Column),
            children.iter().map(|child| child.0).collect(),
        ));
        let child_bytes = children.len().checked_mul(4).ok_or(BuildError::DataLimit)?;
        let budget = self.check_node(None, &[], child_bytes)?;
        Ok(self.push(node, None, budget))
    }

    #[cfg(feature = "api-v2")]
    pub fn row(&mut self, children: &[NodeId]) -> Result<NodeId, BuildError> {
        self.layout_container(Layout::Linear(ContainerLayout::Row), children)
    }

    #[cfg(feature = "api-v2")]
    pub fn grid(&mut self, columns: u8, children: &[NodeId]) -> Result<NodeId, BuildError> {
        if !(1..=8).contains(&columns) {
            return Err(BuildError::InvalidTree);
        }
        self.layout_container(Layout::Grid(GridLayout { columns }), children)
    }

    #[cfg(feature = "api-v2")]
    pub fn surface(&mut self, children: &[NodeId]) -> Result<NodeId, BuildError> {
        let child_bytes = children.len().checked_mul(4).ok_or(BuildError::DataLimit)?;
        let budget = self.check_node(None, &[], child_bytes)?;
        Ok(self.push(
            ViewNode::Surface(children.iter().map(|child| child.0).collect()),
            None,
            budget,
        ))
    }

    #[cfg(feature = "api-v2")]
    pub fn scroll(&mut self, child: NodeId) -> Result<NodeId, BuildError> {
        let budget = self.check_node(None, &[], 4)?;
        Ok(self.push(ViewNode::Scroll(child.0), None, budget))
    }

    #[cfg(feature = "api-v2")]
    fn layout_container(
        &mut self,
        layout: Layout,
        children: &[NodeId],
    ) -> Result<NodeId, BuildError> {
        let child_bytes = children.len().checked_mul(4).ok_or(BuildError::DataLimit)?;
        let budget = self.check_node(None, &[], child_bytes)?;
        Ok(self.push(
            ViewNode::Container((layout, children.iter().map(|child| child.0).collect())),
            None,
            budget,
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

    fn push(
        &mut self,
        node: ViewNode,
        element_id: Option<&str>,
        (text_bytes, variable_bytes): (usize, usize),
    ) -> NodeId {
        let id = self.nodes.len() as u32;
        if let Some(element_id) = element_id {
            let inserted = self.ids.insert(element_id.to_owned());
            debug_assert!(inserted);
        }
        self.text_bytes = text_bytes;
        self.variable_bytes = variable_bytes;
        self.nodes.push(node);
        NodeId(id)
    }

    fn check_node(
        &self,
        element_id: Option<&str>,
        texts: &[&str],
        extra_variable_bytes: usize,
    ) -> Result<(usize, usize), BuildError> {
        if self.nodes.len() >= MAX_VIEW_NODES {
            return Err(BuildError::NodeLimit);
        }
        let mut variable_bytes = self.variable_bytes;
        if let Some(element_id) = element_id {
            if element_id.is_empty() || element_id.len() > MAX_ELEMENT_ID_BYTES {
                return Err(BuildError::ElementId);
            }
            if self.ids.contains(element_id) {
                return Err(BuildError::DuplicateElementId);
            }
            variable_bytes = add_bounded(
                variable_bytes,
                element_id.len(),
                MAX_VIEW_VARIABLE_BYTES,
                BuildError::DataLimit,
            )?;
        }
        let mut text_bytes = self.text_bytes;
        for text in texts {
            if text.len() > MAX_VIEW_STRING_BYTES {
                return Err(BuildError::StringLimit);
            }
            text_bytes = add_bounded(
                text_bytes,
                text.len(),
                MAX_VIEW_TEXT_BYTES,
                BuildError::TextLimit,
            )?;
            variable_bytes = add_bounded(
                variable_bytes,
                text.len(),
                MAX_VIEW_VARIABLE_BYTES,
                BuildError::DataLimit,
            )?;
        }
        variable_bytes = add_bounded(
            variable_bytes,
            extra_variable_bytes,
            MAX_VIEW_VARIABLE_BYTES,
            BuildError::DataLimit,
        )?;
        Ok((text_bytes, variable_bytes))
    }
}

fn add_bounded(
    current: usize,
    added: usize,
    maximum: usize,
    error: BuildError,
) -> Result<usize, BuildError> {
    current
        .checked_add(added)
        .filter(|total| *total <= maximum)
        .ok_or(error)
}

fn valid_asset_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ASSET_ID_BYTES
        && value.is_ascii()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn validate_canvas_primitive(primitive: &crate::CanvasPrimitive) -> Result<(), BuildError> {
    match primitive {
        crate::CanvasPrimitive::Line(line) => {
            if [
                line.start_x_milli,
                line.start_y_milli,
                line.end_x_milli,
                line.end_y_milli,
            ]
            .into_iter()
            .any(|value| value > 1_000)
            {
                return Err(BuildError::CanvasCoordinate);
            }
        }
        crate::CanvasPrimitive::Rect(rect) => {
            if u32::from(rect.x_milli) + u32::from(rect.width_milli) > 1_000
                || u32::from(rect.y_milli) + u32::from(rect.height_milli) > 1_000
            {
                return Err(BuildError::CanvasCoordinate);
            }
        }
        crate::CanvasPrimitive::Text(text) => {
            if text.x_milli > 1_000 || text.y_milli > 1_000 {
                return Err(BuildError::CanvasCoordinate);
            }
        }
    }
    Ok(())
}

fn validate_tree(nodes: &[ViewNode], root: NodeId) -> Result<(), BuildError> {
    let mut parents = vec![0_u8; nodes.len()];
    for node in nodes {
        if let Some(children) = child_ids(node) {
            let mut unique = BTreeSet::new();
            for child in children {
                let child = usize::try_from(child).map_err(|_| BuildError::InvalidTree)?;
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
    if let Some(children) = child_ids(&nodes[index]) {
        for child in children {
            visit(
                nodes,
                usize::try_from(child).map_err(|_| BuildError::InvalidTree)?,
                depth + 1,
                visited,
            )?;
        }
    }
    Ok(())
}

#[must_use = "finish the builder and return its output from the current callback"]
pub struct OutputBuilder<'a> {
    view: Option<View>,
    commands: Vec<HostCommand>,
    next_wake_ms: Option<u32>,
    output_bytes: usize,
    session: &'a mut OutputState,
    next_state: OutputState,
}

impl<'a> OutputBuilder<'a> {
    pub fn new(context: &'a mut WidgetContext) -> Self {
        let next_state = context.output_state().clone();
        Self {
            view: None,
            commands: Vec::new(),
            next_wake_ms: None,
            output_bytes: 64,
            session: context.output_state(),
            next_state,
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
        self.validate_request(request_id, RequestKind::Http)?;
        self.push(
            HostCommand::HttpGet((request_id, host.to_owned(), path.to_owned())),
            64 + host.len() + path.len(),
        )?;
        self.add_request(request_id, RequestKind::Http);
        Ok(())
    }

    pub fn storage_get(&mut self, request_id: u32, key: &str) -> Result<(), BuildError> {
        validate_storage_key(key)?;
        self.validate_request(request_id, RequestKind::Storage)?;
        self.push(
            HostCommand::StorageGet((request_id, key.to_owned())),
            64 + key.len(),
        )?;
        self.add_request(request_id, RequestKind::Storage);
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
        self.validate_request(request_id, RequestKind::Storage)?;
        self.push(
            HostCommand::StorageSet((request_id, key.to_owned(), value.to_vec())),
            64 + key.len() + value.len(),
        )?;
        self.add_request(request_id, RequestKind::Storage);
        Ok(())
    }

    pub fn storage_delete(&mut self, request_id: u32, key: &str) -> Result<(), BuildError> {
        validate_storage_key(key)?;
        self.validate_request(request_id, RequestKind::Storage)?;
        self.push(
            HostCommand::StorageDelete((request_id, key.to_owned())),
            64 + key.len(),
        )?;
        self.add_request(request_id, RequestKind::Storage);
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
        if self
            .next_state
            .published_revisions
            .get(schema_id)
            .is_some_and(|current| revision <= *current)
        {
            return Err(BuildError::ProviderRevision);
        }
        if !self.next_state.published_revisions.contains_key(schema_id)
            && self.next_state.published_revisions.len() >= MAX_PROVIDER_SCHEMAS
        {
            return Err(BuildError::ProviderLimit);
        }
        self.push(
            HostCommand::ProviderPublish((schema_id.to_owned(), revision, payload.to_vec())),
            64 + schema_id.len() + payload.len(),
        )?;
        self.next_state
            .published_revisions
            .insert(schema_id.to_owned(), revision);
        Ok(())
    }

    #[must_use = "return this output from the current widget callback"]
    pub fn finish(self) -> GuestOutput {
        *self.session = self.next_state;
        GuestOutput {
            view: self.view,
            commands: self.commands,
            next_wake_ms: self.next_wake_ms,
        }
    }

    fn validate_request(&self, request_id: u32, kind: RequestKind) -> Result<(), BuildError> {
        if request_id == 0
            || self
                .next_state
                .last_request_id
                .is_some_and(|last| request_id <= last)
        {
            return Err(BuildError::RequestId);
        }
        if self.next_state.outstanding.len() >= MAX_OUTSTANDING_REQUESTS {
            return Err(BuildError::OutstandingRequestLimit);
        }
        if kind == RequestKind::Http
            && self
                .next_state
                .outstanding
                .values()
                .filter(|outstanding| **outstanding == RequestKind::Http)
                .count()
                >= MAX_HTTP_CONCURRENT_REQUESTS
        {
            return Err(BuildError::OutstandingRequestLimit);
        }
        Ok(())
    }

    fn add_request(&mut self, request_id: u32, kind: RequestKind) {
        self.next_state.last_request_id = Some(request_id);
        self.next_state.outstanding.insert(request_id, kind);
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
            #[cfg(feature = "api-v1")]
            ViewNode::Container(children) => children
                .len()
                .checked_mul(4)
                .ok_or(BuildError::OutputLimit)?,
            #[cfg(feature = "api-v2")]
            ViewNode::Container((_, children)) | ViewNode::Surface(children) => children
                .len()
                .checked_mul(4)
                .ok_or(BuildError::OutputLimit)?,
            #[cfg(feature = "api-v2")]
            ViewNode::Scroll(_) => 4,
            #[cfg(feature = "api-v1")]
            ViewNode::Text(text) => text.len(),
            #[cfg(feature = "api-v2")]
            ViewNode::Text((_, text)) => text.len(),
            ViewNode::Image(text) => text.len(),
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

fn child_ids(node: &ViewNode) -> Option<Vec<u32>> {
    #[cfg(feature = "api-v1")]
    if let ViewNode::Container(children) = node {
        return Some(children.clone());
    }
    #[cfg(feature = "api-v2")]
    match node {
        ViewNode::Container((_, children)) | ViewNode::Surface(children) => Some(children.clone()),
        ViewNode::Scroll(child) => Some(vec![*child]),
        _ => None,
    }
    #[cfg(feature = "api-v1")]
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GrantedCapabilities, InitInput};
    #[cfg(feature = "api-v1")]
    use crate::{HostEvent, HttpResponseMetadata};

    fn context() -> WidgetContext {
        WidgetContext::from_init(InitInput {
            locale: "en".into(),
            granted_capabilities: GrantedCapabilities {
                http_hosts: Vec::new(),
                game_data: Vec::new(),
                storage: true,
                clipboard_write: false,
                provider: true,
            },
            settings: Vec::new(),
            session_data: None,
        })
        .expect("valid test context")
    }

    #[test]
    #[cfg(feature = "api-v1")]
    fn session_state_enforces_host_aggregate_limits() {
        let mut lifecycle = context();
        let mut output = OutputBuilder::new(&mut lifecycle);
        output
            .http_get(1, "api.example.com", "/one")
            .expect("first HTTP slot");
        output
            .http_get(2, "api.example.com", "/two")
            .expect("second HTTP slot");
        let _ = output.finish();
        let mut output = OutputBuilder::new(&mut lifecycle);
        assert_eq!(
            output.http_get(3, "api.example.com", "/three"),
            Err(BuildError::OutstandingRequestLimit)
        );
        output.storage_get(3, "state").expect("mixed request slot");
        let _ = output.finish();
        let response = HostEvent::HttpResult((
            1,
            Some(200),
            Vec::new(),
            HttpResponseMetadata {
                content_type: None,
                headers: Vec::new(),
            },
        ));
        lifecycle
            .apply_event(&response)
            .expect("matching response releases HTTP slot");
        assert_eq!(
            lifecycle.apply_event(&response),
            Err(GuestError::InvalidInput)
        );
        let mut output = OutputBuilder::new(&mut lifecycle);
        output
            .http_get(4, "api.example.com", "/four")
            .expect("released HTTP slot");
        assert_eq!(output.storage_get(4, "state"), Err(BuildError::RequestId));

        let mut request_context = context();
        for first in [1, 33] {
            let mut output = OutputBuilder::new(&mut request_context);
            for request_id in first..first + 32 {
                output
                    .storage_get(request_id, "state")
                    .expect("request within aggregate limit");
            }
            let _ = output.finish();
        }
        let mut output = OutputBuilder::new(&mut request_context);
        assert_eq!(
            output.storage_get(65, "state"),
            Err(BuildError::OutstandingRequestLimit)
        );

        let mut provider_context = context();
        for first in [0, 32] {
            let mut output = OutputBuilder::new(&mut provider_context);
            for schema in first..first + 32 {
                output
                    .provider_publish(
                        &alloc::format!("com.example.provider/value-{schema}.v1"),
                        1,
                        &[],
                    )
                    .expect("schema within aggregate limit");
            }
            let _ = output.finish();
        }
        let mut output = OutputBuilder::new(&mut provider_context);
        assert_eq!(
            output.provider_publish("com.example.provider/overflow.v1", 1, &[]),
            Err(BuildError::ProviderLimit)
        );

        let mut revision_context = context();
        let mut output = OutputBuilder::new(&mut revision_context);
        output
            .provider_publish("com.example.provider/value.v1", 7, b"first")
            .expect("initial revision");
        let _ = output.finish();
        let mut output = OutputBuilder::new(&mut revision_context);
        assert_eq!(
            output.provider_publish("com.example.provider/value.v1", 7, b"replay"),
            Err(BuildError::ProviderRevision)
        );
        output
            .provider_publish("com.example.provider/value.v1", 8, b"next")
            .expect("strictly newer revision");
    }

    #[test]
    #[cfg(feature = "api-v2")]
    fn v2_layout_builders_validate_grid_and_one_connected_tree() {
        let locale = Locale::parse("en").expect("locale");
        let mut builder = ViewBuilder::new(&locale);
        let heading = builder
            .text_role(TextRole::Heading, LocalizedText::new("Market"))
            .expect("heading");
        let metric = builder
            .text_role(TextRole::Metric, LocalizedText::new("12 platinum"))
            .expect("metric");
        let grid = builder.grid(2, &[heading, metric]).expect("grid");
        let scroll = builder.scroll(grid).expect("scroll");
        let surface = builder.surface(&[scroll]).expect("surface");
        let root = builder.row(&[surface]).expect("row");
        let view = builder.finish(root, 1).expect("connected v2 view");
        assert!(matches!(
            view.nodes.last(),
            Some(ViewNode::Container((
                Layout::Linear(ContainerLayout::Row),
                _
            )))
        ));

        let mut invalid = ViewBuilder::new(&locale);
        assert_eq!(invalid.grid(0, &[]), Err(BuildError::InvalidTree));
        assert_eq!(invalid.grid(9, &[]), Err(BuildError::InvalidTree));
    }

    #[test]
    fn command_builders_keep_existing_host_boundaries() {
        let mut context = context();
        let mut output = OutputBuilder::new(&mut context);
        assert_eq!(
            output.http_get(1, "api.example.com", "relative"),
            Err(BuildError::StringLimit)
        );
        assert_eq!(
            output.storage_get(1, "bad\nkey"),
            Err(BuildError::StorageKeyLimit)
        );
        assert_eq!(
            output.provider_publish("not-a-schema", 1, b"value"),
            Err(BuildError::SchemaLimit)
        );
        let payload = vec![0; MAX_PROVIDER_PAYLOAD_BYTES];
        for version in 1..=7 {
            output
                .provider_publish(
                    &alloc::format!("com.example.provider/value.v{version}"),
                    1,
                    &payload,
                )
                .expect("output within aggregate byte limit");
        }
        assert_eq!(
            output.provider_publish("com.example.provider/value.v8", 1, &payload),
            Err(BuildError::OutputLimit)
        );
    }
}
