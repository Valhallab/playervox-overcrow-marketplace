#![cfg(feature = "api-v1")]

use overcrow_widget_sdk::{
    BuildError, CanvasLine, CanvasPrimitive, CanvasRect, CanvasText, DragPhase,
    GrantedCapabilities, GuestError, GuestOutput, HarnessError, HostEvent, InitInput, Interaction,
    InteractionKind, Locale, LocalizedText, OutputBuilder, OverlayModeCode, ViewBuilder, Widget,
    WidgetContext, WidgetHarness,
};

#[derive(Default)]
struct CounterWidget {
    count: u32,
    name: String,
    revision: u64,
    handled: u32,
    permissive: bool,
}

impl CounterWidget {
    fn render(&mut self, context: &mut WidgetContext) -> Result<GuestOutput, GuestError> {
        self.revision += 1;
        let mut view = ViewBuilder::new(context.locale());
        let count = view.text(LocalizedText::new(self.count.to_string()))?;
        let increment = view.button(
            "increment",
            LocalizedText::new("Increment").with_translation("fr", "Incrémenter")?,
        )?;
        let name = view.text_input("name", &self.name, LocalizedText::new("Name"))?;
        let selection = view.selection(
            "selection",
            vec![LocalizedText::new("One"), LocalizedText::new("Two")],
            0,
        )?;
        let canvas = view.canvas("canvas", Vec::new())?;
        let root = view.container(&[count, increment, name, selection, canvas])?;
        let view = view.finish(root, self.revision)?;
        Ok(OutputBuilder::new(context).view(view)?.finish())
    }
}

impl Widget for CounterWidget {
    fn init(&mut self, context: &mut WidgetContext) -> Result<GuestOutput, GuestError> {
        self.render(context)
    }

    fn handle(
        &mut self,
        event: HostEvent,
        context: &mut WidgetContext,
    ) -> Result<GuestOutput, GuestError> {
        self.handled += 1;
        if let HostEvent::Interaction(interaction) = event {
            match (interaction.element_id.as_str(), interaction.kind) {
                ("increment", InteractionKind::Clicked) => self.count += 1,
                ("name", InteractionKind::ValueChanged(value)) => self.name = value,
                _ if self.permissive => {}
                _ => return Err(GuestError::InvalidInput),
            }
        }
        self.render(context)
    }
}

fn interaction(element_id: &str, kind: InteractionKind) -> HostEvent {
    HostEvent::Interaction(Interaction {
        element_id: element_id.to_owned(),
        kind,
    })
}

#[test]
fn stable_element_ids_route_only_scoped_semantic_events() {
    let mut widget = CounterWidget::default();
    let locale = Locale::parse("en").expect("valid locale");
    let mut harness = WidgetHarness::new(&mut widget, locale).expect("widget init");

    assert_eq!(harness.text_at(0), Some("0"));
    harness
        .send(interaction("increment", InteractionKind::Clicked))
        .expect("semantic click");
    assert_eq!(harness.text_at(0), Some("1"));
    harness
        .send(interaction(
            "name",
            InteractionKind::ValueChanged("Ada".to_owned()),
        ))
        .expect("host-owned input value");
    assert_eq!(harness.text_input_value("name"), Some("Ada"));
    assert!(
        harness
            .send(interaction("name", InteractionKind::Clicked))
            .is_err()
    );
}

#[test]
fn passive_mode_delivers_no_interaction() {
    let mut widget = CounterWidget::default();
    let locale = Locale::parse("en").expect("valid locale");
    let mut harness = WidgetHarness::new(&mut widget, locale).expect("widget init");
    harness.set_mode(OverlayModeCode::Passive);

    assert!(
        harness
            .send(interaction("increment", InteractionKind::Clicked))
            .is_err()
    );
    assert_eq!(harness.text_at(0), Some("0"));
}

#[test]
fn generic_send_rejects_interactions_the_host_would_not_deliver() {
    let mut widget = CounterWidget {
        permissive: true,
        ..CounterWidget::default()
    };
    let locale = Locale::parse("en").expect("valid locale");
    let mut harness = WidgetHarness::new(&mut widget, locale).expect("widget init");

    harness.set_mode(OverlayModeCode::Passive);
    assert!(matches!(
        harness.send(interaction("increment", InteractionKind::Clicked)),
        Err(HarnessError::Passive)
    ));
    harness.set_mode(OverlayModeCode::Interactive);
    assert!(matches!(
        harness.send(interaction("missing", InteractionKind::Clicked)),
        Err(HarnessError::UnknownElement)
    ));
    assert!(matches!(
        harness.send(interaction("increment", InteractionKind::Toggled(true))),
        Err(HarnessError::ElementKind)
    ));
    for (id, kind) in [
        ("selection", InteractionKind::SelectionChanged(2)),
        ("name", InteractionKind::ValueChanged("x".repeat(4_097))),
        ("canvas", InteractionKind::Scrolled(1_000_001)),
        (
            "canvas",
            InteractionKind::Dragged((1_001, 0, DragPhase::Moved)),
        ),
    ] {
        assert!(matches!(
            harness.send(interaction(id, kind)),
            Err(HarnessError::ElementKind)
        ));
    }

    drop(harness);
    assert_eq!(widget.handled, 0, "invalid interactions reached the widget");
}

#[test]
fn locale_changes_use_exact_translation_then_default_fallback() {
    let mut widget = CounterWidget::default();
    let locale = Locale::parse("en").expect("valid locale");
    let mut harness = WidgetHarness::new(&mut widget, locale).expect("widget init");

    harness
        .send(HostEvent::LocaleChanged("fr".to_owned()))
        .expect("French locale");
    assert_eq!(harness.button_label("increment"), Some("Incrémenter"));
    harness
        .send(HostEvent::LocaleChanged("de".to_owned()))
        .expect("untranslated locale");
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
}

#[test]
fn extended_view_nodes_match_host_bounds_and_fail_transactionally() {
    let locale = Locale::parse("en").expect("valid locale");
    let mut view = ViewBuilder::new(&locale);

    assert_eq!(
        view.toggle("toggle", LocalizedText::new("x".repeat(4_097)), false),
        Err(BuildError::StringLimit)
    );
    let toggle = view
        .toggle("toggle", LocalizedText::new("Enabled"), false)
        .expect("failed toggle did not reserve its ID");
    assert_eq!(
        view.selection(
            "selection",
            vec![LocalizedText::new("One"), LocalizedText::new("Two")],
            2,
        ),
        Err(BuildError::Selection)
    );
    let selection = view
        .selection(
            "selection",
            vec![LocalizedText::new("One"), LocalizedText::new("Two")],
            1,
        )
        .expect("invalid selection did not reserve its ID");
    assert_eq!(
        view.progress(LocalizedText::new("Loading"), 1_001),
        Err(BuildError::Progress)
    );
    let progress = view
        .progress(LocalizedText::new("Loading"), 1_000)
        .expect("bounded progress");
    assert_eq!(
        view.canvas(
            "canvas",
            vec![CanvasPrimitive::Rect(CanvasRect {
                x_milli: 900,
                y_milli: 0,
                width_milli: 101,
                height_milli: 1_000,
            })],
        ),
        Err(BuildError::CanvasCoordinate)
    );
    let canvas = view
        .canvas(
            "canvas",
            vec![
                CanvasPrimitive::Line(CanvasLine {
                    start_x_milli: 0,
                    start_y_milli: 0,
                    end_x_milli: 1_000,
                    end_y_milli: 1_000,
                }),
                CanvasPrimitive::Text(CanvasText {
                    x_milli: 500,
                    y_milli: 500,
                    text: "Center".to_owned(),
                }),
            ],
        )
        .expect("invalid canvas did not reserve its ID");
    assert_eq!(view.image("Bad_Id"), Err(BuildError::ImageHandle));
    let image = view.image("icon").expect("valid asset handle");
    let root = view
        .container(&[toggle, selection, progress, canvas, image])
        .expect("root container");

    let finished = view.finish(root, 1).expect("complete valid view");
    assert_eq!(finished.nodes.len(), 6);

    let mut image_bounds = ViewBuilder::new(&locale);
    for _ in 0..64 {
        image_bounds.image("icon").expect("image within limit");
    }
    assert_eq!(
        image_bounds.image("icon"),
        Err(BuildError::ImageHandleLimit)
    );

    let mut collection_bounds = ViewBuilder::new(&locale);
    assert_eq!(
        collection_bounds.selection(
            "too-many-options",
            (0..129)
                .map(|index| LocalizedText::new(index.to_string()))
                .collect(),
            0,
        ),
        Err(BuildError::SelectionOptionLimit)
    );
    assert_eq!(
        collection_bounds.canvas(
            "too-many-primitives",
            (0..257)
                .map(|_| {
                    CanvasPrimitive::Line(CanvasLine {
                        start_x_milli: 0,
                        start_y_milli: 0,
                        end_x_milli: 1_000,
                        end_y_milli: 1_000,
                    })
                })
                .collect(),
        ),
        Err(BuildError::CanvasPrimitiveLimit)
    );
}

#[test]
fn harness_accepts_validated_init_input_and_exposes_current_output() {
    let mut widget = CounterWidget::default();
    let mut harness = WidgetHarness::from_init(
        &mut widget,
        InitInput {
            locale: "fr".to_owned(),
            granted_capabilities: GrantedCapabilities {
                http_hosts: Vec::new(),
                game_data: Vec::new(),
                storage: false,
                clipboard_write: false,
                provider: false,
            },
            settings: b"creator-settings".to_vec(),
            session_data: None,
        },
    )
    .expect("valid init input");

    assert!(harness.output().view.is_some());
    assert_eq!(harness.button_label("increment"), Some("Incrémenter"));
    harness
        .send(HostEvent::LocaleChanged("en".to_owned()))
        .expect("valid locale event");
    assert_eq!(harness.button_label("increment"), Some("Increment"));
}
