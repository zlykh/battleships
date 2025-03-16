use crate::app_state::{GameId, Point2d};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum GameStatus {
    WaitingPlayers,
    GameOver,
    Progress,
}

#[derive(Debug, Copy, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum PlayerAction {
    Shoot,
    Wait,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GridDTO {
    pub me: Vec<Vec<String>>,
    pub enemy: Vec<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GridResponse {
    pub me: Option<Vec<Vec<String>>>,
    pub enemy: Option<Vec<Vec<String>>>,
    pub status: GameStatus,
    pub action: Option<PlayerAction>,
    pub grid: HashMap<String, Vec<Vec<String>>>,
}

impl GridResponse {
    pub fn new(status: GameStatus, action: Option<PlayerAction>) -> Self {
        Self {
            me: None,
            enemy: None,
            status,
            action,
            grid: HashMap::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NewGameResponse {
    pub id: String,
    pub status: GameStatus,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedResponse {
    pub player_id: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum WsEvent {
    ConnectRq(String),
    ConnectRs(ConnectedResponse),
    CreateGameRq(CreateGameRequest),
    CreateGameRs(NewGameResponse),
    GameStart,
    GameOver,
    JoinRq(JoinRequest),
    JoinRs(GridResponse, String),
    TurnRq(TurnRequest),
    TurnRs(GridDTO),
    StateRq(StateRequest),
    StateRs(GridResponse),
    BadRequestRs(String),
    ServerAbort,
    Disconnect,
    Debug(String),
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TurnRequest {
    pub game_id: String,
    pub username: String,
    pub x: usize,
    pub y: usize,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct JoinRequest {
    pub game_id: String,
    pub username: String,
    pub ships: Vec<Vec<(usize, usize)>>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StateRequest {
    pub game_id: String,
    pub username: String,
}

impl StateRequest {
    pub fn new(game_id: GameId, username: String) -> Self {
        Self { game_id, username }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreateGameRequest {
    pub username: String,
    pub ships: Vec<Vec<(usize, usize)>>,
}
