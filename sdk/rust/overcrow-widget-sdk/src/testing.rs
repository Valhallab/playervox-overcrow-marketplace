use alloc::borrow::ToOwned;
use core::{error::Error, fmt};

use crate::{
    GuestError, GuestOutput, HostEvent, Interaction, InteractionKind, Locale, OverlayModeCode,
    ViewNode, Widget, WidgetContext,
};

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
        let context = WidgetContext::for_testing(locale);
        let output = widget.init(&context)?;
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

    pub fn click(&mut self, element_id: &str) -> Result<(), HarnessError> {
        self.require_element(element_id, |node| matches!(node, ViewNode::Button(_)))?;
        self.interact(element_id, InteractionKind::Clicked)
    }

    pub fn value_changed(&mut self, element_id: &str, value: &str) -> Result<(), HarnessError> {
        self.require_element(element_id, |node| matches!(node, ViewNode::TextInput(_)))?;
        self.interact(element_id, InteractionKind::ValueChanged(value.to_owned()))
    }

    pub fn locale_changed(&mut self, locale: &str) -> Result<(), HarnessError> {
        self.send(HostEvent::LocaleChanged(locale.to_owned()))
    }

    pub fn send(&mut self, event: HostEvent) -> Result<(), HarnessError> {
        self.context.apply_event(&event)?;
        self.output = self.widget.handle(event, &self.context)?;
        Ok(())
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

    fn interact(&mut self, element_id: &str, kind: InteractionKind) -> Result<(), HarnessError> {
        if self.mode != OverlayModeCode::Interactive {
            return Err(HarnessError::Passive);
        }
        self.send(HostEvent::Interaction(Interaction {
            element_id: element_id.to_owned(),
            kind,
        }))
    }

    fn require_element(
        &self,
        element_id: &str,
        expected: impl FnOnce(&ViewNode) -> bool,
    ) -> Result<(), HarnessError> {
        let node = self
            .nodes()
            .find(|node| match node {
                ViewNode::Button((id, _)) | ViewNode::TextInput((id, _, _)) => id == element_id,
                _ => false,
            })
            .ok_or(HarnessError::UnknownElement)?;
        expected(node)
            .then_some(())
            .ok_or(HarnessError::ElementKind)
    }

    fn nodes(&self) -> impl Iterator<Item = &ViewNode> {
        self.output
            .view
            .as_ref()
            .into_iter()
            .flat_map(|view| view.nodes.iter())
    }
}
