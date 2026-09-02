//! Creator-facing Rust SDK for the OverCrow widget component contract.

#![cfg_attr(target_arch = "wasm32", no_std)]

#[doc(hidden)]
pub extern crate alloc;
#[cfg(target_arch = "wasm32")]
extern crate rlibc;

#[cfg(any(
    all(feature = "api-v1", feature = "api-v2"),
    not(any(feature = "api-v1", feature = "api-v2"))
))]
compile_error!("enable exactly one of `api-v1` or `api-v2`");

#[cfg(target_arch = "wasm32")]
#[global_allocator]
static GUEST_ALLOCATOR: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

#[cfg(target_arch = "wasm32")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    core::arch::wasm32::unreachable()
}

// wit-bindgen 0.53.1 expects the P2 standard library to export this canonical
// ABI allocator. This no_std guest cannot link that library because it would
// add WASI imports, so it provides the same private ABI boundary directly.
#[cfg(all(target_arch = "wasm32", target_env = "p2"))]
#[unsafe(export_name = "cabi_realloc")]
unsafe extern "C" fn cabi_realloc_p2(
    old_ptr: *mut u8,
    old_len: usize,
    align: usize,
    new_len: usize,
) -> *mut u8 {
    use alloc::alloc::{Layout, alloc, handle_alloc_error, realloc};

    if !align.is_power_of_two()
        || (old_len == 0 && !old_ptr.is_null())
        || (old_len != 0
            && (old_ptr.is_null() || new_len == 0 || !old_ptr.addr().is_multiple_of(align)))
    {
        core::arch::wasm32::unreachable();
    }
    if old_len == 0 && new_len == 0 {
        return align as *mut u8;
    }

    let new_layout = match Layout::from_size_align(new_len, align) {
        Ok(layout) => layout,
        Err(_) => core::arch::wasm32::unreachable(),
    };
    let ptr = if old_len == 0 {
        // SAFETY: new_layout was validated above and has nonzero size.
        unsafe { alloc(new_layout) }
    } else {
        let old_layout = match Layout::from_size_align(old_len, align) {
            Ok(layout) => layout,
            Err(_) => core::arch::wasm32::unreachable(),
        };
        // SAFETY: the canonical ABI caller owns old_ptr, passes its original
        // size/alignment, and new_len was validated through new_layout.
        unsafe { realloc(old_ptr, old_layout, new_len) }
    };
    if ptr.is_null() {
        handle_alloc_error(new_layout);
    }
    ptr
}

mod state;
mod view;

pub mod testing;

#[allow(clippy::too_many_arguments)] // The generated canonical ABI fixes the handle signature.
pub mod bindings {
    #[cfg(feature = "api-v1")]
    wit_bindgen::generate!({
        path: "../../../wit",
        world: "widget-v1",
        pub_export_macro: true,
    });
    #[cfg(feature = "api-v2")]
    wit_bindgen::generate!({
        path: "../../../wit",
        world: "widget-v2",
        pub_export_macro: true,
    });
}

pub use bindings::{
    CanvasLine, CanvasPrimitive, CanvasRect, CanvasText, DragPhase, GrantedCapabilities,
    GuestError, GuestOutput, HostCommand, HostEvent, HttpHeader, HttpResponseMetadata, InitInput,
    Interaction, InteractionKind, OverlayModeCode, SessionData, View, ViewNode,
};
#[cfg(feature = "api-v2")]
pub use bindings::{ContainerLayout, GridLayout, Layout, TextRole};
pub use state::{Locale, LocaleError, LocalizedText, WidgetContext};
pub use testing::{HarnessError, WidgetHarness};
pub use view::{BuildError, NodeId, OutputBuilder, ViewBuilder};

/// Stateful behavior implemented by one widget component instance.
pub trait Widget: Send {
    fn init(&mut self, context: &mut WidgetContext) -> Result<GuestOutput, GuestError>;

    fn handle(
        &mut self,
        event: HostEvent,
        context: &mut WidgetContext,
    ) -> Result<GuestOutput, GuestError>;

    #[cfg(feature = "api-v2")]
    fn http_response_start(
        &mut self,
        _request_id: u32,
        _status: Option<u16>,
        _body_length: u32,
        _metadata: HttpResponseMetadata,
        _context: &mut WidgetContext,
    ) -> Result<GuestOutput, GuestError> {
        Err(GuestError::InvalidState)
    }

    #[cfg(feature = "api-v2")]
    fn http_response_chunk(
        &mut self,
        _request_id: u32,
        _sequence: u8,
        _bytes: Vec<u8>,
        _context: &mut WidgetContext,
    ) -> Result<GuestOutput, GuestError> {
        Err(GuestError::InvalidState)
    }

    #[cfg(feature = "api-v2")]
    fn http_response_end(
        &mut self,
        _request_id: u32,
        _context: &mut WidgetContext,
    ) -> Result<GuestOutput, GuestError> {
        Err(GuestError::InvalidState)
    }

    fn stop(&mut self) {}
}

#[doc(hidden)]
pub mod __private {
    pub use spin::Mutex;

    use crate::{GuestError, HostEvent, WidgetContext};

    pub fn context(input: crate::InitInput) -> Result<WidgetContext, GuestError> {
        WidgetContext::from_init(input)
    }

    pub fn apply_event(context: &mut WidgetContext, event: &HostEvent) -> Result<(), GuestError> {
        context.apply_event(event)
    }

    #[cfg(feature = "api-v2")]
    pub fn complete_http(context: &mut WidgetContext, request_id: u32) -> Result<(), GuestError> {
        context.complete_http(request_id)
    }
}

/// Export a `Default` widget as the no-import `widget-v1` component world.
#[macro_export]
#[cfg(feature = "api-v1")]
macro_rules! export_widget {
    ($widget:ty) => {
        mod __overcrow_widget_export {
            struct Component;

            static STATE: $crate::__private::Mutex<
                Option<($widget, $crate::WidgetContext)>,
            > = $crate::__private::Mutex::new(None);

            impl $crate::bindings::Guest for Component {
                fn init(
                    input: $crate::InitInput,
                ) -> Result<$crate::GuestOutput, $crate::GuestError> {
                    let mut state = STATE
                        .try_lock()
                        .ok_or($crate::GuestError::InvalidState)?;
                    if state.is_some() {
                        return Err($crate::GuestError::InvalidState);
                    }
                    let mut context = $crate::__private::context(input)?;
                    let mut widget = <$widget>::default();
                    let output = $crate::Widget::init(&mut widget, &mut context)?;
                    *state = Some((widget, context));
                    Ok(output)
                }

                fn handle(
                    event: $crate::HostEvent,
                ) -> Result<$crate::GuestOutput, $crate::GuestError> {
                    let mut state = STATE
                        .try_lock()
                        .ok_or($crate::GuestError::InvalidState)?;
                    let (widget, context) = state
                        .as_mut()
                        .ok_or($crate::GuestError::InvalidState)?;
                    $crate::__private::apply_event(context, &event)?;
                    $crate::Widget::handle(widget, event, context)
                }

                fn stop() {
                    if let Some(mut state) = STATE.try_lock() {
                        if let Some((mut widget, _)) = state.take() {
                            $crate::Widget::stop(&mut widget);
                        }
                    }
                }
            }

            $crate::bindings::export!(Component with_types_in $crate::bindings);
        }
    };
}

/// Export a `Default` widget as the no-import `widget-v2` component world.
#[macro_export]
#[cfg(feature = "api-v2")]
macro_rules! export_widget {
    ($widget:ty) => {
        mod __overcrow_widget_export {
            struct Component;

            static STATE: $crate::__private::Mutex<
                Option<($widget, $crate::WidgetContext)>,
            > = $crate::__private::Mutex::new(None);

            impl $crate::bindings::Guest for Component {
                fn init(
                    input: $crate::InitInput,
                ) -> Result<$crate::GuestOutput, $crate::GuestError> {
                    let mut state = STATE
                        .try_lock()
                        .ok_or($crate::GuestError::InvalidState)?;
                    if state.is_some() {
                        return Err($crate::GuestError::InvalidState);
                    }
                    let mut context = $crate::__private::context(input)?;
                    let mut widget = <$widget>::default();
                    let output = $crate::Widget::init(&mut widget, &mut context)?;
                    *state = Some((widget, context));
                    Ok(output)
                }

                fn handle(
                    event: $crate::HostEvent,
                ) -> Result<$crate::GuestOutput, $crate::GuestError> {
                    let mut state = STATE
                        .try_lock()
                        .ok_or($crate::GuestError::InvalidState)?;
                    let (widget, context) = state
                        .as_mut()
                        .ok_or($crate::GuestError::InvalidState)?;
                    $crate::__private::apply_event(context, &event)?;
                    $crate::Widget::handle(widget, event, context)
                }

                fn http_response_start(
                    request_id: u32,
                    status: Option<u16>,
                    body_length: u32,
                    metadata: $crate::HttpResponseMetadata,
                ) -> Result<$crate::GuestOutput, $crate::GuestError> {
                    let mut state = STATE
                        .try_lock()
                        .ok_or($crate::GuestError::InvalidState)?;
                    let (widget, context) = state
                        .as_mut()
                        .ok_or($crate::GuestError::InvalidState)?;
                    $crate::__private::complete_http(context, request_id)?;
                    $crate::Widget::http_response_start(
                        widget, request_id, status, body_length, metadata, context,
                    )
                }

                fn http_response_chunk(
                    request_id: u32,
                    sequence: u8,
                    bytes: $crate::alloc::vec::Vec<u8>,
                ) -> Result<$crate::GuestOutput, $crate::GuestError> {
                    let mut state = STATE
                        .try_lock()
                        .ok_or($crate::GuestError::InvalidState)?;
                    let (widget, context) = state
                        .as_mut()
                        .ok_or($crate::GuestError::InvalidState)?;
                    $crate::Widget::http_response_chunk(
                        widget, request_id, sequence, bytes, context,
                    )
                }

                fn http_response_end(
                    request_id: u32,
                ) -> Result<$crate::GuestOutput, $crate::GuestError> {
                    let mut state = STATE
                        .try_lock()
                        .ok_or($crate::GuestError::InvalidState)?;
                    let (widget, context) = state
                        .as_mut()
                        .ok_or($crate::GuestError::InvalidState)?;
                    $crate::Widget::http_response_end(widget, request_id, context)
                }

                fn stop() {
                    if let Some(mut state) = STATE.try_lock() {
                        if let Some((mut widget, _)) = state.take() {
                            $crate::Widget::stop(&mut widget);
                        }
                    }
                }
            }

            $crate::bindings::export!(Component with_types_in $crate::bindings);
        }
    };
}
