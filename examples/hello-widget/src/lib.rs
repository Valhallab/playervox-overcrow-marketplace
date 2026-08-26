#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

use alloc::{format, string::String};

use overcrow_widget_sdk::{
    GuestError, GuestOutput, HostEvent, InteractionKind, LocalizedText, OutputBuilder, ViewBuilder,
    Widget, WidgetContext,
};

#[derive(Default)]
struct HelloWidget {
    name: String,
    greeted: bool,
    revision: u64,
}

impl HelloWidget {
    fn render(&mut self, context: &mut WidgetContext) -> Result<GuestOutput, GuestError> {
        self.revision += 1;
        let greeting = if self.greeted && !self.name.is_empty() {
            LocalizedText::new(format!("Hello {}!", self.name))
                .with_translation("fr", format!("Bonjour {} !", self.name))?
        } else {
            LocalizedText::new("Hello from OverCrow!")
                .with_translation("fr", "Bonjour depuis OverCrow !")?
        };

        let mut view = ViewBuilder::new(context.locale());
        let greeting = view.text(greeting)?;
        let button = view.button(
            "greet",
            LocalizedText::new("Greet").with_translation("fr", "Saluer")?,
        )?;
        let input = view.text_input(
            "name",
            &self.name,
            LocalizedText::new("Name").with_translation("fr", "Nom")?,
        )?;
        let root = view.container(&[greeting, button, input])?;
        let view = view.finish(root, self.revision)?;
        Ok(OutputBuilder::new(context).view(view)?.finish())
    }
}

impl Widget for HelloWidget {
    fn init(&mut self, context: &mut WidgetContext) -> Result<GuestOutput, GuestError> {
        self.render(context)
    }

    fn handle(
        &mut self,
        event: HostEvent,
        context: &mut WidgetContext,
    ) -> Result<GuestOutput, GuestError> {
        if let HostEvent::Interaction(interaction) = event {
            match (interaction.element_id.as_str(), interaction.kind) {
                ("greet", InteractionKind::Clicked) => self.greeted = true,
                ("name", InteractionKind::ValueChanged(value))
                | ("name", InteractionKind::Submitted(value)) => self.name = value,
                ("greet" | "name", InteractionKind::Focused(_))
                | ("greet" | "name", InteractionKind::Hovered(_)) => {}
                _ => return Err(GuestError::InvalidInput),
            }
        }
        self.render(context)
    }
}

overcrow_widget_sdk::export_widget!(crate::HelloWidget);

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use overcrow_widget_sdk::{HostEvent, Interaction, InteractionKind, Locale, WidgetHarness};
    use wasmparser::{ComponentExternalKind, ComponentTypeRef, Encoding, Parser, Payload};

    use super::HelloWidget;

    #[test]
    fn hello_widget_renders_english_and_french_and_handles_scoped_events() {
        let mut widget = HelloWidget::default();
        let locale = Locale::parse("en").expect("valid locale");
        let mut harness = WidgetHarness::new(&mut widget, locale).expect("widget init");
        let interaction = |element_id: &str, kind| {
            HostEvent::Interaction(Interaction {
                element_id: element_id.to_owned(),
                kind,
            })
        };

        assert_eq!(harness.button_label("greet"), Some("Greet"));
        harness
            .send(HostEvent::LocaleChanged("fr".to_owned()))
            .expect("French locale");
        assert_eq!(harness.button_label("greet"), Some("Saluer"));
        harness
            .send(interaction(
                "name",
                InteractionKind::ValueChanged("Ada".to_owned()),
            ))
            .expect("host-owned text value");
        assert_eq!(harness.text_input_value("name"), Some("Ada"));
        harness
            .send(interaction("greet", InteractionKind::Clicked))
            .expect("scoped click");
        assert_eq!(harness.text_at(0), Some("Bonjour Ada !"));
        assert!(
            harness
                .send(interaction("name", InteractionKind::Clicked))
                .is_err()
        );
        assert!(
            harness
                .send(interaction("unknown", InteractionKind::Clicked))
                .is_err()
        );
    }

    #[test]
    #[ignore = "requires a release wasm32-wasip2 component path"]
    fn built_component_has_no_imports_and_exact_lifecycle_exports() {
        let path = env::var_os("OVERCROW_HELLO_COMPONENT")
            .expect("set OVERCROW_HELLO_COMPONENT to the built component");
        let bytes = fs::read(path).expect("read built component");
        let mut root_encoding = None;
        let mut depth = 0_u32;
        let mut imports = Vec::new();
        let mut exports = Vec::new();

        for payload in Parser::new(0).parse_all(&bytes) {
            match payload.expect("valid WebAssembly component") {
                Payload::Version { encoding, .. } if root_encoding.is_none() => {
                    root_encoding = Some(encoding);
                }
                Payload::ComponentImportSection(section) if depth == 0 => {
                    for import in section {
                        let import = import.expect("valid component import");
                        if !matches!(import.ty, ComponentTypeRef::Type(_)) {
                            imports.push(import.name.0.to_owned());
                        }
                    }
                }
                Payload::ComponentExportSection(section) if depth == 0 => {
                    for export in section {
                        let export = export.expect("valid component export");
                        exports.push((export.name.0.to_owned(), export.kind));
                    }
                }
                Payload::ModuleSection { .. } | Payload::ComponentSection { .. } => depth += 1,
                Payload::End(_) if depth > 0 => depth -= 1,
                _ => {}
            }
        }

        assert_eq!(root_encoding, Some(Encoding::Component));
        assert!(imports.is_empty(), "forbidden imports: {imports:?}");
        exports.sort_by(|left, right| left.0.cmp(&right.0));
        assert_eq!(
            exports,
            [
                ("handle".to_owned(), ComponentExternalKind::Func),
                ("init".to_owned(), ComponentExternalKind::Func),
                ("stop".to_owned(), ComponentExternalKind::Func),
            ]
        );
    }
}
