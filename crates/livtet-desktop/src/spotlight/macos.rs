//! CoreSpotlight indexing and Spotlight-result activation (macOS only).
//!
//! All Objective-C work is contained here. Index writes are synchronous and
//! happen on the blocking pool; activation is a method swizzle on tao's
//! `NSApplicationDelegate`, because tao forwards only browsing activities and
//! drops `CSSearchableItemActionType` on the floor.

use std::sync::{Mutex, OnceLock};

use objc2::runtime::{AnyObject, Bool, Imp, Method, Sel};
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::NSApplication;
use objc2_core_spotlight::{CSSearchableIndex, CSSearchableItem, CSSearchableItemAttributeSet};
use objc2_foundation::{NSArray, NSData, NSString, NSUserActivity};
use tauri::{AppHandle, Emitter, Manager};

use super::{DOMAIN, OPEN_EDITION_EVENT, SpotlightError, SpotlightRecord};

/// Raw values of `CSSearchableItemActionType` and
/// `CSSearchableItemActivityIdentifier` from CoreSpotlight.
const SEARCHABLE_ITEM_ACTION_TYPE: &str = "com.apple.corespotlight.searchableItemActionType";
const SEARCHABLE_ITEM_ACTIVITY_IDENTIFIER: &str = "kCSSearchableItemActivityIdentifier";

/// Unique-identifier prefix; everything after it is the edition id.
const IDENTIFIER_PREFIX: &str = "livtet:edition:";

/// Editions per `indexSearchableItems:` call.
const BATCH: usize = 200;

/// Longest edge of the cover thumbnail stored in the index.
const THUMBNAIL_EDGE: u32 = 128;

type ContinueUserActivity =
    unsafe extern "C-unwind" fn(&AnyObject, Sel, &NSUserActivity, *mut AnyObject) -> Bool;

static ORIGINAL: OnceLock<Imp> = OnceLock::new();
static APP: OnceLock<AppHandle> = OnceLock::new();
static PENDING: Mutex<Option<String>> = Mutex::new(None);

pub(super) fn is_available() -> bool {
    // SAFETY: a class method with no arguments.
    unsafe { CSSearchableIndex::isIndexingAvailable() }
}

pub(super) fn clear_index() -> Result<(), SpotlightError> {
    // SAFETY: the default index is process-wide and the call takes no pointers.
    unsafe {
        let index = CSSearchableIndex::defaultSearchableIndex();
        let domain = NSString::from_str(DOMAIN);
        let domains = NSArray::from_retained_slice(&[domain]);
        index.deleteSearchableItemsWithDomainIdentifiers_completionHandler(&domains, None);
    }
    Ok(())
}

pub(super) fn index_records(records: &[SpotlightRecord]) -> Result<usize, SpotlightError> {
    // SAFETY: every Objective-C object below is retained by its Rust owner until
    // the batch call returns, and the index methods copy their inputs.
    unsafe {
        let index = CSSearchableIndex::defaultSearchableIndex();

        let domain = NSString::from_str(DOMAIN);
        let domains = NSArray::from_retained_slice(&[domain]);
        index.deleteSearchableItemsWithDomainIdentifiers_completionHandler(&domains, None);

        for chunk in records.chunks(BATCH) {
            let items = chunk
                .iter()
                .map(|record| item_for(record))
                .collect::<Vec<_>>();
            let array = NSArray::from_retained_slice(&items);
            index.indexSearchableItems_completionHandler(&array, None);
        }
    }

    Ok(records.len())
}

unsafe fn item_for(record: &SpotlightRecord) -> objc2::rc::Retained<CSSearchableItem> {
    // SAFETY: all values are copied by the attribute set and item.
    unsafe {
        let attributes = CSSearchableItemAttributeSet::new();
        let title = NSString::from_str(&record.title);
        attributes.setTitle(Some(&title));
        attributes.setDisplayName(Some(&title));

        if let Some(description) = &record.description {
            attributes.setContentDescription(Some(&NSString::from_str(description)));
        }
        if !record.authors.is_empty() {
            attributes.setAuthorNames(Some(&string_array(&record.authors)));
        }
        if !record.keywords.is_empty() {
            attributes.setKeywords(Some(&string_array(&record.keywords)));
        }
        if let Some(bytes) = thumbnail_data(record.cover_path.as_deref()) {
            attributes.setThumbnailData(Some(&NSData::from_vec(bytes)));
        }

        let identifier = NSString::from_str(&format!("{IDENTIFIER_PREFIX}{}", record.edition_id));
        let domain = NSString::from_str(DOMAIN);
        let item = CSSearchableItem::new();
        item.setUniqueIdentifier(&identifier);
        item.setDomainIdentifier(Some(&domain));
        item.setAttributeSet(&attributes);
        item
    }
}

fn string_array(values: &[String]) -> objc2::rc::Retained<NSArray<NSString>> {
    let values = values
        .iter()
        .map(|value| NSString::from_str(value))
        .collect::<Vec<_>>();
    NSArray::from_retained_slice(&values)
}

/// Decode the cover and re-encode a small PNG for `thumbnailData`, skipping the
/// image entirely when it cannot be read.
fn thumbnail_data(path: Option<&str>) -> Option<Vec<u8>> {
    let image = image::ImageReader::open(path?)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let thumbnail = image.thumbnail(THUMBNAIL_EDGE, THUMBNAIL_EDGE);
    let mut cursor = std::io::Cursor::new(Vec::new());
    thumbnail
        .write_to(&mut cursor, image::ImageFormat::Png)
        .ok()?;
    Some(cursor.into_inner())
}

/// Install the delegate hook and record the app handle. Call once, from setup.
pub fn install_activation(app: &AppHandle) {
    let _ = APP.set(app.clone());

    if let Ok(mut pending) = PENDING.lock()
        && let Some(edition_id) = pending.take()
    {
        drop(pending);
        dispatch(&edition_id);
    }

    // SAFETY: called on the main thread during setup, before any activation.
    unsafe { swizzle() }
}

unsafe fn swizzle() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };

    let application = NSApplication::sharedApplication(mtm);
    let Some(delegate) = application.delegate() else {
        return;
    };

    let object: &AnyObject = delegate.as_ref();
    let class = object.class();
    let selector = sel!(application:continueUserActivity:restorationHandler:);
    let Some(method) = class.instance_method(selector) else {
        return;
    };

    if ORIGINAL.set(method.implementation()).is_err() {
        return;
    }

    let replacement: Imp =
        unsafe { std::mem::transmute(continue_activity as ContinueUserActivity) };
    unsafe { objc2::ffi::method_setImplementation(method as *const Method, replacement) };
}

unsafe extern "C-unwind" fn continue_activity(
    this: &AnyObject,
    selector: Sel,
    activity: &NSUserActivity,
    restoration_handler: *mut AnyObject,
) -> Bool {
    // A panic must not unwind into AppKit.
    let edition_id = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: the activity was passed by AppKit and is valid for this call.
        unsafe { searchable_item_identifier(activity) }
    }))
    .ok()
    .flatten();

    if let Some(edition_id) = edition_id {
        dispatch(&edition_id);
        return Bool::YES;
    }

    let Some(original) = ORIGINAL.get().copied() else {
        return Bool::NO;
    };
    let original: ContinueUserActivity = unsafe { std::mem::transmute(original) };
    // SAFETY: forwarding the exact arguments AppKit supplied.
    unsafe { original(this, selector, activity, restoration_handler) }
}

unsafe fn searchable_item_identifier(activity: &NSUserActivity) -> Option<String> {
    if activity.activityType().to_string() != SEARCHABLE_ITEM_ACTION_TYPE {
        return None;
    }

    let info = activity.userInfo()?;
    let key = NSString::from_str(SEARCHABLE_ITEM_ACTIVITY_IDENTIFIER);
    let value = info.objectForKey(&key)?;
    let identifier = value.downcast::<NSString>().ok()?;
    identifier
        .to_string()
        .strip_prefix(IDENTIFIER_PREFIX)
        .map(str::to_owned)
}

/// Reveal the main window and ask the web layer to open the edition drawer.
fn dispatch(edition_id: &str) {
    let Some(app) = APP.get() else {
        if let Ok(mut pending) = PENDING.lock() {
            *pending = Some(edition_id.to_owned());
        }
        return;
    };

    crate::tray::focus_main_window(app);

    if let Some(window) = app.get_webview_window(crate::tray::MAIN_WINDOW)
        && let Err(error) = window.emit_to(
            tauri::EventTarget::webview_window(crate::tray::MAIN_WINDOW),
            OPEN_EDITION_EVENT,
            edition_id.to_owned(),
        )
    {
        tracing::warn!(%error, "could not deliver the Spotlight selection");
    }
}
