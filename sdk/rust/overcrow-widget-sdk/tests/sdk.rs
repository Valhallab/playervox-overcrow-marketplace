use overcrow_widget_sdk::{
    BuildError, GuestError, GuestOutput, HostEvent, InteractionKind, Locale, LocalizedText,
    OutputBuilder, OverlayModeCode, ViewBuilder, Widget, WidgetContext, WidgetHarness,
};

#[derive(Default)]
struct CounterWidget {
    count: u32,
    name: String,
    revision: u64,
}

impl CounterWidget {
    fn render(&mut self, context: &WidgetContext) -> Result<GuestOutput, GuestError> {
        self.revision += 1;
        let mut view = ViewBuilder::new(context.locale());
        let count = view.text(LocalizedText::new(self.count.to_string()))?;
        let increment = view.button(
            "increment",
            LocalizedText::new("Increment").with_translation("fr", "Incrémenter")?,
        )?;
        let name = view.text_input("name", &self.name, LocalizedText::new("Name"))?;
        let root = view.container(&[count, increment, name])?;
        let view = view.finish(root, self.revision)?;
        Ok(OutputBuilder::new().view(view)?.finish())
    }
}

impl Widget for CounterWidget {
    fn init(&mut self, context: &WidgetContext) -> Result<GuestOutput, GuestError> {
        self.render(context)
    }

    fn handle(
        &mut self,
        event: HostEvent,
        context: &WidgetContext,
    ) -> Result<GuestOutput, GuestError> {
        if let HostEvent::Interaction(interaction) = event {
            match (interaction.element_id.as_str(), interaction.kind) {
                ("increment", InteractionKind::Clicked) => self.count += 1,
                ("name", InteractionKind::ValueChanged(value)) => self.name = value,
                _ => return Err(GuestError::InvalidInput),
            }
        }
        self.render(context)
    }
}

#[test]
fn stable_element_ids_route_only_scoped_semantic_events() {
    let mut widget = CounterWidget::default();
    let locale = Locale::parse("en").expect("valid locale");
    let mut harness = WidgetHarness::new(&mut widget, locale).expect("widget init");

    assert_eq!(harness.text_at(0), Some("0"));
    harness.click("increment").expect("semantic click");
    assert_eq!(harness.text_at(0), Some("1"));
    harness
        .value_changed("name", "Ada")
        .expect("host-owned input value");
    assert_eq!(harness.text_input_value("name"), Some("Ada"));
    assert!(harness.click("name").is_err());
}

#[test]
fn passive_mode_delivers_no_interaction() {
    let mut widget = CounterWidget::default();
    let locale = Locale::parse("en").expect("valid locale");
    let mut harness = WidgetHarness::new(&mut widget, locale).expect("widget init");
    harness.set_mode(OverlayModeCode::Passive);

    assert!(harness.click("increment").is_err());
    assert_eq!(harness.text_at(0), Some("0"));
}

#[test]
fn locale_changes_use_exact_translation_then_default_fallback() {
    let mut widget = CounterWidget::default();
    let locale = Locale::parse("en").expect("valid locale");
    let mut harness = WidgetHarness::new(&mut widget, locale).expect("widget init");

    harness.locale_changed("fr").expect("French locale");
    assert_eq!(harness.button_label("increment"), Some("Incrémenter"));
    harness.locale_changed("de").expect("untranslated locale");
    assert_eq!(harness.button_label("increment"), Some("Increment"));
}

#[test]
fn builders_reject_duplicate_ids_and_host_limit_overruns() {
    let locale = Locale::parse("en").expect("valid locale");
    let mut view = ViewBuilder::new(&locale);
    view.button("same", LocalizedText::new("One"))
        .expect("first ID");
    assert_eq!(
        view.button("same", LocalizedText::new("Two")),
        Err(BuildError::DuplicateElementId)
    );

    let mut oversized = ViewBuilder::new(&locale);
    assert_eq!(
        oversized.text(LocalizedText::new("x".repeat(4_097))),
        Err(BuildError::StringLimit)
    );

    let mut output = OutputBuilder::new();
    for request_id in 1..=32 {
        output
            .storage_get(request_id, "state")
            .expect("command within limit");
    }
    assert_eq!(
        output.storage_get(33, "state"),
        Err(BuildError::CommandLimit)
    );
}

#[test]
fn command_builders_match_host_path_key_schema_and_output_bounds() {
    let mut output = OutputBuilder::new();
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

    let payload = vec![0; 256 * 1024];
    for version in 1..=7 {
        output
            .provider_publish(
                &format!("com.example.provider/value.v{version}"),
                1,
                &payload,
            )
            .expect("aggregate output within limit");
    }
    assert_eq!(
        output.provider_publish("com.example.provider/value.v8", 1, &payload),
        Err(BuildError::OutputLimit)
    );
}
