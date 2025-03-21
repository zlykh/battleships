use std::collections::{HashMap, VecDeque};
use std::env;
use std::net::SocketAddr;

use crate::app_state::{Client, MyState, Shared, Wrapper};
use crate::dto::{StateRequest, WsEvent};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::Router;
use futures::{sink::SinkExt, stream::StreamExt};
use serde_json::json;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::broadcast::Receiver;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;
use tower_http::services::ServeDir;
use tower_http::trace::{self, TraceLayer};
use tracing::Level;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

mod app_state;
mod dto;
mod game_engine;

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!(
                    "{}=trace,axum::rejection=trace", //,tower_http=debug,
                    env!("CARGO_CRATE_NAME")
                )
                .into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let app_state = Wrapper {
        shared: Arc::new(Shared {
            state: RwLock::new(MyState {
                games: HashMap::new(),
                client_games: HashMap::new(),
                game_clients: HashMap::new(),
                queue: VecDeque::with_capacity(100),
            }),
        }),
    };

    start_matchmaker(app_state.clone());

    let app = Router::new()
        .route(
            "/",
            get(Html(std::fs::read_to_string("static/main.html").unwrap())),
        )
        .nest_service("/static", ServeDir::new("static"))
        .route("/ws", get(ws_handler))
        .with_state(app_state)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
                .on_response(trace::DefaultOnResponse::new().level(Level::INFO)),
        );

    let host = "[::]:8080";
    let listener = tokio::net::TcpListener::bind(host).await.unwrap();
    println!("Starting at {}", host);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

async fn ws_handler(ws: WebSocketUpgrade, State(wrapper): State<Wrapper>) -> impl IntoResponse {
    ws.on_upgrade(|socket| websocket(socket, wrapper))
}

async fn websocket(stream: WebSocket, wrapper: Wrapper) {
    let connection_id = Uuid::new_v4().to_string();
    println!("Client connected: {}", connection_id);

    let (mut self_ws_out, mut self_ws_in) = stream.split();
    let (self_chan_sender, mut self_chan_receiver) = mpsc::channel(10);
    // Таск перенаправляет сообщения из канала клиента в клиентский вебсокет
    // Внешняя ф-ция держит переменные только для одного клиента (self_...)
    let mut send_self_ws_task = tokio::spawn(async move {
        while let Some(msg) = self_chan_receiver.recv().await {
            match msg {
                WsEvent::Disconnect => {
                    println!("Disconnecting ? by server");
                    if self_ws_out.send(Message::Close(None)).await.is_err() {
                        break;
                    }
                }
                _ => {
                    let p = json!(msg).to_string();
                    // println!("sending {} ", &p);
                    if self_ws_out.send(Message::text(p)).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    //not async, before spawing async loops
    // let mut broadband_handle = None;
    while let Some(Ok(msg)) = self_ws_in.next().await {
        match msg {
            Message::Text(text) => {
                println!("received: {}", text);
                let v: WsEvent = serde_json::from_str(text.as_str()).unwrap();
                match v {
                    WsEvent::ConnectRq { .. } => {
                        let _ = self_chan_sender
                            .send(WsEvent::ConnectRs {
                                player_id: connection_id.clone(),
                            })
                            .await
                            .unwrap();
                    }
                    WsEvent::CreateGameRq(mut rq) => {
                        rq.username = connection_id.clone();

                        let response = game_engine::game_new(wrapper.clone(), rq);
                        let game_id = match &response {
                            WsEvent::CreateGameRs { game_id, .. } => game_id.clone(),
                            _ => String::new(),
                        };

                        // set up channels
                        wrapper.attach_client(
                            &game_id,
                            Client::new(connection_id.clone(), self_chan_sender.clone()),
                        );

                        // let room_receiver = wrapper.get_room_sender(&game_id).subscribe();
                        // broadband_handle = Some(broadband_consumer(room_receiver, self_chan_sender.clone()));

                        let _ = self_chan_sender.send(response).await.unwrap();
                        break;
                    }
                    WsEvent::JoinRq(mut rq) => {
                        let username = connection_id.clone();
                        let game_id = rq.game_id.clone();
                        rq.username = username.clone();

                        // set up channels
                        wrapper.attach_client(
                            &game_id,
                            Client::new(username.clone(), self_chan_sender.clone()),
                        );

                        // let room_receiver = wrapper.get_room_sender(&game_id).subscribe();
                        // broadband_handle = Some(broadband_consumer(room_receiver, self_chan_sender.clone()));

                        let response = game_engine::game_join(wrapper.clone(), rq);
                        let _ = self_chan_sender.send(response).await.unwrap();

                        //this not delivered to p1... blocked fpr some reason and order break
                        // let _ = wrapper.get_room_sender(&game_id).send(json!(WsEvent::GameStart).to_string()).unwrap();

                        // get senders for me & opponent
                        // send initial state for me & opponent
                        let (me, opponent) = wrapper.get_clients(&game_id);
                        let my_state = game_engine::game_state(
                            wrapper.clone(),
                            StateRequest::new(game_id.clone(), me.id),
                        );
                        let opponent_state = game_engine::game_state(
                            wrapper.clone(),
                            StateRequest::new(game_id.clone(), opponent.id),
                        );
                        let _ = me.sender.send(my_state).await.unwrap();
                        let _ = opponent.sender.send(opponent_state).await.unwrap();

                        break;
                    }
                    WsEvent::QueueRq(mut rq) => {
                        let username = connection_id.clone();
                        rq.username = username.clone();

                        let response =
                            game_engine::enqueue(wrapper.clone(), rq, self_chan_sender.clone());
                        let _ = self_chan_sender.send(response).await.unwrap();

                        break;
                    }
                    _ => {}
                }
            }
            _ => {
                println!("unsupported ws message type");
                break;
            }
        }
    }
    //--end not async

    //loop msg after joining... and use BREAK if needed!
    let connection_id_copy = connection_id.clone();
    let wrapper_copy = wrapper.clone();
    let mut recv_task = tokio::spawn(async move {
        let connection_id = connection_id_copy;
        let wrapper = wrapper_copy;
        while let Some(Ok(msg)) = self_ws_in.next().await {
            match msg {
                Message::Text(text) => {
                    println!("received: {}", text);
                    let v: WsEvent = serde_json::from_str(text.as_str()).unwrap();
                    match v {
                        WsEvent::TurnRq(mut rq) => {
                            let username = connection_id.clone();
                            {
                                if let None = wrapper
                                    .shared
                                    .state
                                    .read()
                                    .unwrap()
                                    .client_games
                                    .get(&username)
                                {
                                    break;
                                }
                            }
                            let game_id = rq.game_id.clone();
                            rq.username = connection_id.clone();
                            game_engine::game_turn(wrapper.clone(), rq);

                            // get senders for me & opponent
                            // send initial state for me & opponent
                            let (me, opponent) = wrapper.get_clients(&game_id);
                            let my_state = game_engine::game_state(
                                wrapper.clone(),
                                StateRequest::new(game_id.clone(), me.id),
                            );
                            let opponent_state = game_engine::game_state(
                                wrapper.clone(),
                                StateRequest::new(game_id.clone(), opponent.id),
                            );
                            let _ = me.sender.send(my_state).await.unwrap();
                            let _ = opponent.sender.send(opponent_state).await.unwrap();

                            if wrapper.is_game_over(&game_id) {
                                let _ = me.sender.send(WsEvent::GameOver).await.unwrap();
                                let _ = opponent.sender.send(WsEvent::GameOver).await.unwrap();

                                let _ = me.sender.send(WsEvent::Disconnect).await.unwrap();
                                let _ = opponent.sender.send(WsEvent::Disconnect).await.unwrap();
                            }
                        }
                        _ => {}
                    }
                }
                Message::Close(_) => {
                    println!("Disconnecting ? by web");
                }
                _ => {}
            }
        }
    });

    // Заблочится на этих тасках, пока один из них не сдохнет и не выключит остальные
    // Тогда перейдет к дальше к блоку "дисконект"
    tokio::select! {
        _ = &mut send_self_ws_task => {
            recv_task.abort();
        },
        _ = &mut recv_task => {
            send_self_ws_task.abort();
        },

    }

    println!("Client disconnected: {}", &connection_id);
    //handle disconnected
    let state = &mut wrapper.shared.state.write().unwrap();
    if let Some(game_id) = state.client_games.remove(&connection_id) {
        state.game_clients.remove(&game_id);
        state.games.remove(&game_id);

        if let Some(idx) = state.queue.iter().position(|p| &p.0 == &connection_id) {
            state.queue.remove(idx);
        }
    }
}

fn start_matchmaker(wrapper: Wrapper) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));

    tokio::spawn(async move {
        let wrapper_clone = wrapper.clone();
        interval.tick().await;
        loop {
            interval.tick().await;
            game_engine::match_players(wrapper_clone.clone()).await;
        }
    });
}

fn broadband_consumer(
    mut room_receiver: Receiver<String>,
    self_channel_sender: Sender<String>,
) -> JoinHandle<()> {
    return tokio::spawn(async move {
        while let Ok(msg) = room_receiver.recv().await {
            if self_channel_sender.send(msg).await.is_err() {
                break;
            }
        }
    });
}
