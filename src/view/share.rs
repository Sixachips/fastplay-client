//! Clipboard + native share helpers. Both APIs only exist in secure
//! contexts (https or localhost), so plain-LAN-http dev sessions have to
//! fall back gracefully — hence the runtime feature detection.

use wasm_bindgen::{JsCast, JsValue};

fn has_property(obj: &JsValue, name: &str) -> bool {
    js_sys::Reflect::get(obj, &JsValue::from_str(name))
        .map(|value| !value.is_undefined())
        .unwrap_or(false)
}

/// Copy via the async Clipboard API. Fire and forget: returns false only
/// when the API is entirely missing (insecure context), in which case the
/// caller falls back to manual selection.
pub fn copy_text(text: &str) -> bool {
    let Some(window) = web_sys::window() else { return false };
    let navigator = window.navigator();
    if !has_property(&JsValue::from(&navigator), "clipboard") {
        return false;
    }
    // The generated `clipboard()` binding panics on an undefined attribute,
    // so go through Reflect — the presence check above makes this safe.
    let clipboard: web_sys::Clipboard = js_sys::Reflect::get(
        &JsValue::from(&navigator),
        &JsValue::from_str("clipboard"),
    )
    .map(|value| value.unchecked_into())
    .unwrap();
    let _ = clipboard.write_text(text);
    true
}

/// Hand the link to the OS share sheet (phones). False when unsupported.
pub fn share_link(url: &str) -> bool {
    let Some(window) = web_sys::window() else { return false };
    let navigator = window.navigator();
    if !has_property(&JsValue::from(&navigator), "share") {
        return false;
    }
    let data = web_sys::ShareData::new();
    data.set_url(url);
    data.set_title("fastPlay dominoes");
    if !navigator.can_share_with_data(&data) {
        return false;
    }
    // share() returns a promise; we ignore whether the user shared or
    // cancelled — either way there is nothing for the app to do.
    let _ = navigator.share_with_data(&data);
    true
}
