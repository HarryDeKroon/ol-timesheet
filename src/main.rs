#![recursion_limit = "512"]

use leptos::prelude::*;

cfg_if::cfg_if! {
    if #[cfg(feature = "ssr")] {

        fn shell(options: LeptosOptions) -> impl IntoView {
            use timesheet::app::App;

            view! {
                <!DOCTYPE html>
                <html lang="en">
                    <head>
                        <meta charset="utf-8" />
                        <meta name="viewport" content="width=device-width, initial-scale=1" />
                        <title>"Timesheet"</title>
                        <link rel="icon" type="image/x-icon" href="/favicon.ico" />
                        <link rel="stylesheet" href="/pkg/timesheet.css" />
                        <HydrationScripts options=options.clone() />
                    </head>
                    <body>
                        <App />
                    </body>
                </html>
            }
        }

        /// Simple WebSocket heartbeat endpoint.
        ///
        /// The connection staying open signals "online" to the client.  We send
        /// a periodic ping so proxies / browsers don't close the idle socket.
        /// Requires a valid authenticated session; unauthenticated upgrades are
        /// rejected with 401.
        async fn heartbeat_ws_handler(
            ws: axum::extract::ws::WebSocketUpgrade,
            headers: axum::http::HeaderMap,
        ) -> axum::response::Response {
            use axum::response::IntoResponse;
            if !timesheet::auth::is_authenticated(&headers) {
                return axum::http::StatusCode::UNAUTHORIZED.into_response();
            }
            ws.on_upgrade(handle_heartbeat_socket).into_response()
        }

        /// Handle the WebSocket connection for heartbeat.
        ///
        /// Sends a ping every 15 seconds to keep the connection alive and
        /// responds to incoming pings/pongs/close messages.
        async fn handle_heartbeat_socket(mut socket: axum::extract::ws::WebSocket) {
            use axum::extract::ws::Message;
            use futures::SinkExt;
            use std::time::Duration;

            let mut interval = tokio::time::interval(Duration::from_secs(15));

            loop {
                tokio::select! {
                    // Periodic ping to keep the connection alive.
                    _ = interval.tick() => {
                        if socket.send(Message::Ping(Default::default())).await.is_err() {
                            log::info!("Failed to send heartbeat ping, closing connection");
                            break;
                        }
                    }
                    // Handle incoming messages from the client.
                    msg = socket.recv() => {
                        match msg {
                            Some(Ok(Message::Ping(bytes))) => {
                                if socket.send(Message::Pong(bytes)).await.is_err() {
                                    log::info!("Failed to send pong, closing connection");
                                    break;
                                }
                            }
                            Some(Ok(Message::Pong(_))) => {
                                // Client responded to our ping — all good.
                            }
                            Some(Ok(Message::Close(_))) => {
                                log::info!("Client closed heartbeat connection");
                                let _ = SinkExt::<Message>::close(&mut socket).await;
                                break;
                            }
                            Some(Ok(Message::Text(_))) | Some(Ok(Message::Binary(_))) => {
                                // Ignore application-level messages.
                            }
                            Some(Err(e)) => {
                                log::warn!("WebSocket error: {}", e);
                                break;
                            }
                            None => {
                                log::info!("WebSocket stream ended");
                                break;
                            }
                        }
                    }
                }
            }
        }


        #[tokio::main]
        async fn main() {
            use axum::Router;
            use axum::routing::get;
            use leptos_axum::{LeptosRoutes, generate_route_list};

            dotenvy::dotenv().ok();
            env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

            // Initialise OAuth configuration from environment variables.
            timesheet::auth::init_oauth(timesheet::auth::OAuthConfig {
                client_id: std::env::var("JIRA_CLIENT_ID")
                    .expect("JIRA_CLIENT_ID env var not set"),
                client_secret: std::env::var("JIRA_CLIENT_SECRET")
                    .expect("JIRA_CLIENT_SECRET env var not set"),
                redirect_uri: std::env::var("OAUTH_REDIRECT_URI")
                    .unwrap_or_else(|_| "http://localhost:8081/auth/callback".to_string()),
            });

            let conf =
                leptos::config::get_configuration(None).expect("Failed to load Leptos configuration");
            let addr = conf.leptos_options.site_addr;
            let leptos_options = conf.leptos_options;
            let routes = generate_route_list(timesheet::app::App);

            // Build the router with AppState to carry LeptosOptions.
            let app: Router = Router::new()
                .route("/ws/heartbeat", get(heartbeat_ws_handler))
                .route("/auth/login", get(timesheet::auth::login_handler))
                .route("/auth/callback", get(timesheet::auth::callback_handler))
                .route("/auth/logout", get(timesheet::auth::logout_handler))
                .leptos_routes(&leptos_options, routes, {
                    let leptos_options = leptos_options.clone();
                    move || shell(leptos_options.clone())
                })
                .fallback(leptos_axum::file_and_error_handler(shell))
                .with_state(leptos_options);

            let listener = tokio::net::TcpListener::bind(&addr)
                .await
                .unwrap_or_else(|_| panic!("Failed to bind to {}", &addr));

            axum::serve(listener, app.into_make_service())
                .await
                .expect("Server failed to start");
        }

    } else {

        fn main() {
            use timesheet::app::App;
            _ = console_log::init_with_level(log::Level::Debug);
            console_error_panic_hook::set_once();
            mount_to_body(App);
        }
    }
}
