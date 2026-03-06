//! Popup-draft coordinator.
//!
//! `PopupDraftManager` keeps track of every open `CellPopup` and exposes a
//! single `flush_all()` method that saves all dirty-and-valid popups in one
//! shot.  Navigation-like actions (week nav, cache refresh, language switch,
//! `beforeunload`) call `flush_all()` before proceeding so the user never
//! loses work by accident.
//!
//! ## Usage
//!
//! 1. Call [`provide_popup_flush_context`] once (in `TimesheetView`).
//! 2. In each `CellPopup`, call [`use_popup_flush`] and then
//!    `mgr.register(…)` on mount / `mgr.unregister(…)` on close.
//! 3. Before any navigation action: `mgr.flush_all()` (fire-and-forget) or
//!    `mgr.flush_all_then(callback)` (run `callback` after all async saves
//!    have finished on the server).

use leptos::prelude::*;
use send_wrapper::SendWrapper;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Unique popup identifier (same `popup_id: u32` used elsewhere).
type PopupId = u32;

// ---------------------------------------------------------------------------
// Completion latch — shared counter that fires a callback when it hits zero
// ---------------------------------------------------------------------------

/// A simple reference-counted latch.  Each in-flight save holds a clone;
/// when the last one drops (or explicitly calls `arrive()`), the stored
/// callback fires exactly once.
#[derive(Clone)]
pub struct FlushLatch {
    inner: Rc<RefCell<LatchInner>>,
}

struct LatchInner {
    remaining: usize,
    on_complete: Option<Rc<dyn Fn()>>,
}

impl FlushLatch {
    /// Create a latch that expects `count` arrivals before firing `cb`.
    /// If `count == 0` the callback fires immediately.
    fn new(count: usize, cb: Rc<dyn Fn()>) -> Self {
        if count == 0 {
            cb();
            return Self {
                inner: Rc::new(RefCell::new(LatchInner {
                    remaining: 0,
                    on_complete: None,
                })),
            };
        }
        Self {
            inner: Rc::new(RefCell::new(LatchInner {
                remaining: count,
                on_complete: Some(cb),
            })),
        }
    }

    /// Signal that one save has completed.  When the last arrival happens
    /// the `on_complete` callback fires.
    pub fn arrive(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.remaining = inner.remaining.saturating_sub(1);
        if inner.remaining == 0 {
            if let Some(cb) = inner.on_complete.take() {
                // Drop the borrow before calling the callback to avoid
                // borrow conflicts if the callback touches us.
                drop(inner);
                cb();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Popup draft registry
// ---------------------------------------------------------------------------

/// A registered popup draft.
///
/// All closures capture the popup's own reactive signals so the manager
/// never needs to know about `CellPopup` internals.
struct PopupDraft {
    /// Returns `true` when the popup content passes validation (all hours
    /// parse, comments are non-empty when required, etc.).
    is_valid: Rc<dyn Fn() -> bool>,
    /// Returns `true` when the user has changed *anything* relative to the
    /// initial server state (edits, new rows, deletions).
    is_dirty: Rc<dyn Fn() -> bool>,
    /// Trigger the save-and-close logic.  The optional [`FlushLatch`] must
    /// have `arrive()` called once the async server calls complete (inside
    /// the `spawn_local` task).  When `None`, the save behaves identically
    /// to the regular Save button (fire-and-forget).
    save: Rc<dyn Fn(Option<FlushLatch>)>,
}

/// The non-Send inner state that lives behind a `SendWrapper`.
struct Inner {
    drafts: RefCell<HashMap<PopupId, PopupDraft>>,
    /// Guard flag to prevent re-entrant flushes.
    flushing: RefCell<bool>,
}

/// Shared, cloneable handle to the draft registry.
///
/// The real state is `!Send` (it contains `Rc<dyn Fn…>` closures) so we
/// wrap it in [`SendWrapper`] which satisfies the `Send + Sync` bounds
/// that Leptos context requires while ensuring it is only ever accessed
/// on the originating (main/wasm) thread.
#[derive(Clone)]
pub struct PopupDraftManager {
    inner: Rc<SendWrapper<Inner>>,
}

// SAFETY: SendWrapper already provides the Send + Sync impls we need,
// and Rc is only accessed on the single wasm thread.  We need these
// impls so the type can live inside Leptos context which requires
// Send + Sync.
unsafe impl Send for PopupDraftManager {}
unsafe impl Sync for PopupDraftManager {}

impl PopupDraftManager {
    fn new() -> Self {
        Self {
            inner: Rc::new(SendWrapper::new(Inner {
                drafts: RefCell::new(HashMap::new()),
                flushing: RefCell::new(false),
            })),
        }
    }

    /// Register a popup so it participates in `flush_all`.
    ///
    /// `save_fn` receives an `Option<FlushLatch>`.  When `Some`, the popup
    /// **must** call `latch.arrive()` after its async server work finishes.
    /// When `None` (normal Save-button path), it can ignore it.
    pub fn register(
        &self,
        id: PopupId,
        is_valid: impl Fn() -> bool + 'static,
        is_dirty: impl Fn() -> bool + 'static,
        save_fn: impl Fn(Option<FlushLatch>) + 'static,
    ) {
        self.inner.drafts.borrow_mut().insert(
            id,
            PopupDraft {
                is_valid: Rc::new(is_valid),
                is_dirty: Rc::new(is_dirty),
                save: Rc::new(save_fn),
            },
        );
    }

    /// Remove a popup from the registry (called on close / unmount).
    pub fn unregister(&self, id: PopupId) {
        self.inner.drafts.borrow_mut().remove(&id);
    }

    /// Returns `true` when at least one popup is registered.
    pub fn has_open_popups(&self) -> bool {
        !self.inner.drafts.borrow().is_empty()
    }

    /// Save every open popup that is both **dirty** and **valid** (fire-
    /// and-forget).  Invalid popups are silently skipped.
    ///
    /// Returns `true` if the flush ran (was not blocked by the re-entrancy
    /// guard).
    pub fn flush_all(&self) -> bool {
        self.flush_inner(None)
    }

    /// Like [`flush_all`](Self::flush_all), but `on_complete` is called
    /// once **all** triggered saves have finished their async server work.
    /// If no popups needed saving, `on_complete` fires immediately.
    ///
    /// Returns `true` if the flush ran.
    pub fn flush_all_then(&self, on_complete: impl Fn() + 'static) -> bool {
        self.flush_inner(Some(Rc::new(on_complete)))
    }

    // Shared implementation for both flush variants.
    fn flush_inner(&self, on_complete: Option<Rc<dyn Fn()>>) -> bool {
        // Re-entrancy guard
        {
            let mut guard = self.inner.flushing.borrow_mut();
            if *guard {
                // If we can't flush, still fire the callback so callers
                // aren't left hanging.
                if let Some(cb) = on_complete {
                    cb();
                }
                return false;
            }
            *guard = true;
        }

        // Collect the save closures we need to fire *before* we mutably
        // borrow `drafts` again (the save_fn will call `unregister` which
        // also borrows `drafts`).
        let to_save: Vec<Rc<dyn Fn(Option<FlushLatch>)>> = {
            let drafts = self.inner.drafts.borrow();
            drafts
                .values()
                .filter(|d| (d.is_dirty)() && (d.is_valid)())
                .map(|d| Rc::clone(&d.save))
                .collect()
        };

        let count = to_save.len();

        // Build a latch only when the caller wants a completion callback.
        let latch = on_complete.map(|cb| FlushLatch::new(count, cb));

        for save in &to_save {
            save(latch.clone());
        }

        // If no saves were triggered and there was no latch (fire-and-forget
        // path), this is a no-op.  If there *was* a latch with count == 0
        // the callback already fired inside `FlushLatch::new`.

        *self.inner.flushing.borrow_mut() = false;
        true
    }

    /// Close (without saving) every open popup whose `is_dirty` returns
    /// `false`.  This is intentionally **not** used by the standard flush
    /// path but can be handy for "discard clean popups" scenarios.
    #[allow(dead_code)]
    pub fn close_clean_popups(&self) {
        let to_close: Vec<Rc<dyn Fn(Option<FlushLatch>)>> = {
            let drafts = self.inner.drafts.borrow();
            drafts
                .values()
                .filter(|d| !(d.is_dirty)())
                .map(|d| Rc::clone(&d.save))
                .collect()
        };
        for save in &to_close {
            save(None);
        }
    }
}

/// Provide the [`PopupDraftManager`] context.  Call once from the top-level
/// view that owns the popup layer (e.g. `TimesheetView`).
pub fn provide_popup_flush_context() {
    provide_context(PopupDraftManager::new());
}

/// Retrieve the [`PopupDraftManager`] from Leptos context.
pub fn use_popup_flush() -> PopupDraftManager {
    use_context::<PopupDraftManager>().expect("PopupDraftManager context must be provided")
}

