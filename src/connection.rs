use crate::model::ConnectionStatus;
use leptos::prelude::*;

/// Connection state provided via Leptos context.
///
/// Tracks whether the WebSocket to the server is open and how many API
/// requests are currently in flight.  The derived `status()` method
/// implements the three-state model described in the heartbeat story:
///
/// - **Online** (green) — socket open, no in-flight requests
/// - **Waiting** (orange) — socket open, at least one in-flight request
/// - **Offline** (red) — socket closed, regardless of request count
#[derive(Clone, Copy)]
pub struct ConnectionState {
    pub socket_open: RwSignal<bool>,
    pub in_flight: RwSignal<i32>,
}

impl ConnectionState {
    pub fn new() -> Self {
        Self {
            socket_open: RwSignal::new(false),
            in_flight: RwSignal::new(0),
        }
    }

    /// Derived connection status.
    pub fn status(&self) -> ConnectionStatus {
        if !self.socket_open.get() {
            ConnectionStatus::Offline
        } else if self.in_flight.get() > 0 {
            ConnectionStatus::Waiting
        } else {
            ConnectionStatus::Online
        }
    }

    /// Whether the connection is available (socket open). Use this to
    /// guard user interactions.
    pub fn is_available(&self) -> bool {
        self.socket_open.get()
    }

    /// Increment in-flight request counter. Call before issuing an API
    /// request.
    pub fn request_started(&self) {
        self.in_flight.update(|n| *n += 1);
    }

    /// Decrement in-flight request counter. Call in the `finally`
    /// equivalent after an API request completes (success or error).
    /// Clamps at zero to guard against drift.
    pub fn request_finished(&self) {
        self.in_flight.update(|n| {
            if *n <= 0 {
                log::warn!(
                    "ConnectionState: request_finished called with in_flight={}, clamping to 0",
                    *n
                );
                *n = 0;
            } else {
                *n -= 1;
            }
        });
    }
}

/// Provide the [`ConnectionState`] as a Leptos context and start the
/// WebSocket heartbeat on the client side.
///
/// Call this once at the top of your app (or at least before any
/// component that needs the connection state).
pub fn provide_connection_context() -> ConnectionState {
    let state = ConnectionState::new();
    provide_context(state);

    #[cfg(feature = "hydrate")]
    {
        start_websocket(state);
    }

    // On the server side we leave socket_open as false — the indicator
    // is only meaningful in the browser.
    #[cfg(feature = "ssr")]
    {
        // Mark as online on the server so SSR-rendered HTML isn't red.
        state.socket_open.set(true);
    }

    state
}

/// Obtain the [`ConnectionState`] from context.
pub fn use_connection() -> ConnectionState {
    use_context::<ConnectionState>().expect("ConnectionState context not provided")
}

// ── Client-side WebSocket logic ─────────────────────────────────────────────

#[cfg(feature = "hydrate")]
fn start_websocket(state: ConnectionState) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;
    use web_sys::{CloseEvent, Event, MessageEvent, WebSocket};

    /// Create and connect a WebSocket, wiring up open/close/error
    /// handlers that update the connection state.  Returns the
    /// WebSocket instance.
    fn connect(state: ConnectionState) -> Result<WebSocket, JsValue> {
        // Build the ws:// or wss:// URL from the current page location.
        let location = web_sys::window().expect("no window").location();
        let protocol = location.protocol().unwrap_or_else(|_| "http:".into());
        let ws_protocol = if protocol == "https:" { "wss:" } else { "ws:" };
        let host = location.host().unwrap_or_else(|_| "localhost:3093".into());
        let url = format!("{}//{}/ws/heartbeat", ws_protocol, host);

        let ws = WebSocket::new(&url)?;

        // ── onopen ──
        {
            let state = state;
            let onopen = Closure::<dyn Fn(Event)>::new(move |_: Event| {
                log::info!("Heartbeat WebSocket opened");
                state.socket_open.set(true);
            });
            ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
            onopen.forget();
        }

        // ── onclose ──
        {
            let state = state;
            let onclose = Closure::<dyn Fn(CloseEvent)>::new(move |ev: CloseEvent| {
                log::info!(
                    "Heartbeat WebSocket closed (code={}, reason={})",
                    ev.code(),
                    ev.reason()
                );
                state.socket_open.set(false);
                schedule_reconnect(state);
            });
            ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
            onclose.forget();
        }

        // ── onerror ──
        {
            let onerror = Closure::<dyn Fn(Event)>::new(move |_: Event| {
                log::warn!("Heartbeat WebSocket error");
                // The close event always follows, so we handle state there.
            });
            ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
            onerror.forget();
        }

        // ── onmessage — keep-alive pong; nothing to do ──
        {
            let onmessage = Closure::<dyn Fn(MessageEvent)>::new(move |_: MessageEvent| {
                // Server may send "ping" frames; we ignore them.
            });
            ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
            onmessage.forget();
        }

        Ok(ws)
    }

    /// Schedule a reconnection attempt with exponential back-off.
    ///
    /// Uses a simple strategy: 1s, 2s, 4s, 8s, …, capped at 30s.
    /// The delay is stored in a thread-local so it can be reset on
    /// successful open.
    fn schedule_reconnect(state: ConnectionState) {
        use std::cell::Cell;
        thread_local! {
            static DELAY_MS: Cell<u32> = const { Cell::new(1_000) };
        }

        let delay = DELAY_MS.with(|d| {
            let current = d.get();
            // Increase for next attempt, capped at 30 s.
            d.set((current * 2).min(30_000));
            current
        });

        log::info!("Reconnecting heartbeat WebSocket in {}ms", delay);

        let cb = Closure::wrap(Box::new(move || {
            match connect(state) {
                Ok(ws) => {
                    // Reset back-off on successful *construction* (the
                    // open handler will fire later).  We reset here so
                    // that if it immediately fails the back-off still
                    // applies.
                    let state_for_open = state;
                    let onopen_reset = Closure::<dyn Fn(Event)>::new(move |_: Event| {
                        log::info!("Heartbeat WebSocket reconnected");
                        state_for_open.socket_open.set(true);
                        DELAY_MS.with(|d| d.set(1_000));
                    });
                    ws.set_onopen(Some(onopen_reset.as_ref().unchecked_ref()));
                    onopen_reset.forget();

                    // Re-wire onclose for further reconnections.
                    let state_for_close = state;
                    let onclose = Closure::<dyn Fn(CloseEvent)>::new(move |ev: CloseEvent| {
                        log::info!(
                            "Heartbeat WebSocket closed (code={}, reason={})",
                            ev.code(),
                            ev.reason()
                        );
                        state_for_close.socket_open.set(false);
                        schedule_reconnect(state_for_close);
                    });
                    ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
                    onclose.forget();

                    let onerror = Closure::<dyn Fn(Event)>::new(move |_: Event| {
                        log::warn!("Heartbeat WebSocket error");
                    });
                    ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
                    onerror.forget();

                    let onmessage = Closure::<dyn Fn(MessageEvent)>::new(move |_: MessageEvent| {});
                    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
                    onmessage.forget();
                }
                Err(e) => {
                    log::error!("Failed to create WebSocket: {:?}", e);
                    // Schedule another attempt.
                    schedule_reconnect(state);
                }
            }
        }) as Box<dyn FnMut()>);

        web_sys::window()
            .expect("no window")
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                delay as i32,
            )
            .expect("setTimeout failed");
        cb.forget();
    }

    // Kick off the initial connection.
    match connect(state) {
        Ok(_ws) => {
            // The WebSocket is self-managing via the closures above.
            // We intentionally do NOT store it — it lives as long as
            // the closures (which are `.forget()`-ed).
        }
        Err(e) => {
            log::error!("Failed to create initial heartbeat WebSocket: {:?}", e);
            schedule_reconnect(state);
        }
    }
}
