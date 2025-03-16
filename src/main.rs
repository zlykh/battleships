use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::ops::ControlFlow;
use std::{env, thread};

use crate::app_state::{Client, MyState, Shared, Wrapper};
use crate::dto::{ConnectedResponse, StateRequest, TurnRequest, WsEvent};
use axum::body::Bytes;
use axum::extract::ws::{CloseFrame, Message, Utf8Bytes, WebSocket};
use axum::extract::{ConnectInfo, State, WebSocketUpgrade};
use axum::middleware::AddExtension;
use axum::response::{Html, IntoResponse};
use axum::routing::{any, get};
use axum::Router;
use axum_extra::{headers, TypedHeader};
use futures::{sink::SinkExt, stream::StreamExt};
use rand::distr::{Alphanumeric, SampleString};
use rand::Rng;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::sync::broadcast::Receiver;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;
use tower_http::services::ServeDir;
use uuid::Uuid;

mod app_state;
mod dto;
mod game_engine;

#[tokio::main]
async fn main() {
    let app_state = Wrapper {
        shared: Arc::new(Shared {
            state: RwLock::new(MyState {
                games: HashMap::new(),
                client_games: HashMap::new(),
                game_clients: HashMap::new(),
            }),
        }),
    };

    let app = Router::new()
        .nest_service("/static", ServeDir::new("static"))
        .route("/ws", get(ws_handler))
        .with_state(app_state);

    let host = "[::]:8080";
    let listener = tokio::net::TcpListener::bind(host)
        .await
        .unwrap();
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
                    WsEvent::ConnectRq(_) => {
                        let _ = self_chan_sender
                            .send(
                                (WsEvent::ConnectRs(ConnectedResponse {
                                    player_id: connection_id.clone(),
                                })),
                            )
                            .await
                            .unwrap();
                    }
                    WsEvent::CreateGameRq(mut rq) => {
                        rq.username = connection_id.clone();

                        let response = game_engine::game_new(wrapper.clone(), rq);
                        let game_id = match &response {
                            WsEvent::CreateGameRs(ng) => ng.id.clone(),
                            _ => String::new(),
                        };

                        // set up channels
                        wrapper.attach_client(
                            &game_id,
                            Client::new(connection_id.clone(), self_chan_sender.clone()),
                        );

                        let room_receiver = wrapper.get_room_sender(&game_id).subscribe();
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

                        let room_receiver = wrapper.get_room_sender(&game_id).subscribe();
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

    //loop msg after joining... and use BREAk if needed!
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
                            //todo get game if from state
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
                            let response = game_engine::game_turn(wrapper.clone(), rq);

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
    // let mut asdf = broadband_handle.unwrap();
    tokio::select! {
        _ = &mut send_self_ws_task => {
            // asdf.abort();
            recv_task.abort();
        },
        _ = &mut recv_task => {
            send_self_ws_task.abort();
            // asdf.abort();
        },
        // _ = &mut asdf => {
        //     send_self_ws_task.abort();
        //     recv_task.abort();
        // },
    }

    println!("Client disconnected: {}", &connection_id);
    //handle disconnected
    let state = &mut wrapper.shared.state.write().unwrap();
    if let Some(game_id) = state.client_games.remove(&connection_id) {
        state.game_clients.remove(&game_id);
        state.games.remove(&game_id);
    }
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
