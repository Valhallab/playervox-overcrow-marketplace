use alloc::{borrow::ToOwned, vec::Vec};
use core::{error::Error, fmt};

use crate::{
    GrantedCapabilities, GuestError, GuestOutput, HostEvent, InitInput, Interaction,
    InteractionKind, Locale, OverlayModeCode, ViewNode, Widget, WidgetContext,
};

const MAX_VIEW_STRING_BYTES: usize = 4 * 1024;
const MAX_SCROLL_DELTA_MILLI: u32 = 1_000_000;
const MAX_ELEMENT_ID_BYTES: usize = 64;

#[derive(Debug)]
pub enum HarnessError {
    Widget(GuestError),
    Passive,
    UnknownElement,
    ElementKind,
}

impl fmt::Display for HarnessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Widget(_) => "widget rejected the event",
            Self::Passive => "passive overlays receive no interactions",
            Self::UnknownElement => "element ID is absent from the current view",
            Self::ElementKind => "semantic event does not match the element kind",
        })
    }
}

impl Error for HarnessError {}

impl From<GuestError> for HarnessError {
    fn from(error: GuestError) -> Self {
        Self::Widget(error)
    }
}

pub struct WidgetHarness<'a, W> {
    widget: &'a mut W,
    context: WidgetContext,
    output: GuestOutput,
    mode: OverlayModeCode,
}

impl<'a, W: Widget> WidgetHarness<'a, W> {
    pub fn new(widget: &'a mut W, locale: Locale) -> Result<Self, GuestError> {
        Self::from_init(
            widget,
            InitInput {
                locale: locale.as_str().to_owned(),
                granted_capabilities: GrantedCapabilities {
                    http_hosts: Vec::new(),
                    game_data: Vec::new(),
                    storage: false,
                    clipboard_write: false,
                    provider: false,
                },
                settings: Vec::new(),
                session_data: None,
            },
        )
    }

    pub fn from_init(widget: &'a mut W, input: InitInput) -> Result<Self, GuestError> {
        let mut context = WidgetContext::from_init(input)?;
        let output = widget.init(&mut context)?;
        Ok(Self {
            widget,
            context,
            output,
            mode: OverlayModeCode::Interactive,
        })
    }

    pub fn set_mode(&mut self, mode: OverlayModeCode) {
        self.mode = mode;
    }

    pub fn send(&mut self, event: HostEvent) -> Result<(), HarnessError> {
        if let HostEvent::Interaction(interaction) = &event {
            self.validate_interaction(interaction)?;
        }
        self.context.apply_event(&event)?;
        self.output = self.widget.handle(event, &mut self.context)?;
        Ok(())
    }

    pub fn output(&self) -> &GuestOutput {
        &self.output
    }

    pub fn text_at(&self, index: usize) -> Option<&str> {
        self.nodes()
            .filter_map(|node| match node {
                ViewNode::Text(text) => Some(text.as_str()),
                _ => None,
            })
            .nth(index)
    }

    pub fn button_label(&self, element_id: &str) -> Option<&str> {
        self.nodes().find_map(|node| match node {
            ViewNode::Button((id, label)) if id == element_id => Some(label.as_str()),
            _ => None,
        })
    }

    pub fn text_input_value(&self, element_id: &str) -> Option<&str> {
        self.nodes().find_map(|node| match node {
            ViewNode::TextInput((id, value, _)) if id == element_id => Some(value.as_str()),
            _ => None,
        })
    }

    fn validate_interaction(&self, interaction: &Interaction) -> Result<(), HarnessError> {
        if self.mode != OverlayModeCode::Interactive {
            return Err(HarnessError::Passive);
        }
        if interaction.element_id.is_empty() || interaction.element_id.len() > MAX_ELEMENT_ID_BYTES
        {
            return Err(HarnessError::UnknownElement);
        }
        let node = self
            .nodes()
            .find(|node| match node {
                ViewNode::Button((id, _))
                | ViewNode::Toggle((id, _, _))
                | ViewNode::TextInput((id, _, _))
                | ViewNode::Selection((id, _, _))
                | ViewNode::Canvas((id, _)) => id == &interaction.element_id,
                _ => false,
            })
            .ok_or(HarnessError::UnknownElement)?;
        let valid = match (&interaction.kind, node) {
            (InteractionKind::Clicked, ViewNode::Button(_))
            | (InteractionKind::Toggled(_), ViewNode::Toggle(_))
            | (
                InteractionKind::Focused(_),
                ViewNode::Button(_)
                | ViewNode::Toggle(_)
                | ViewNode::TextInput(_)
                | ViewNode::Selection(_),
            )
            | (
                InteractionKind::Hovered(_),
                ViewNode::Button(_)
                | ViewNode::Toggle(_)
                | ViewNode::TextInput(_)
                | ViewNode::Selection(_)
                | ViewNode::Canvas(_),
            ) => true,
            (
                InteractionKind::ValueChanged(value) | InteractionKind::Submitted(value),
                ViewNode::TextInput(_),
            ) => value.len() <= MAX_VIEW_STRING_BYTES,
            (InteractionKind::SelectionChanged(index), ViewNode::Selection((_, options, _))) => {
                usize::try_from(*index).is_ok_and(|index| index < options.len())
            }
            (InteractionKind::Scrolled(delta), ViewNode::Canvas(_)) => {
                delta.unsigned_abs() <= MAX_SCROLL_DELTA_MILLI
            }
            (InteractionKind::Dragged((x, y, _)), ViewNode::Canvas(_)) => {
                *x <= 1_000 && *y <= 1_000
            }
            _ => false,
        };
        valid.then_some(()).ok_or(HarnessError::ElementKind)
    }

    fn nodes(&self) -> impl Iterator<Item = &ViewNode> {
        self.output
            .view
            .as_ref()
            .into_iter()
            .flat_map(|view| view.nodes.iter())
    }
}
