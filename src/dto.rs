use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type GameId = String;
pub type ClientId = String;
pub type ShipsRaw = Vec<Vec<(usize, usize)>>;
pub type Grid2D = Vec<Vec<String>>;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum WsEvent {
    ConnectRq { player_id: ClientId },
    ConnectRs { player_id: ClientId },
    CreateGameRq(CreateGameRequest),
    CreateGameRs { game_id: GameId, status: GameStatus },
    GameStart { game_id: GameId },
    GameOver,
    QueueRq(QueueRequest),
    QueueRs { player_id: ClientId },
    JoinRq(JoinGameRequest),
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
    pub me: Grid2D,
    pub enemy: Grid2D,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GridResponse {
    pub me: Option<Grid2D>,
    pub enemy: Option<Grid2D>,
    pub status: GameStatus,
    pub action: Option<PlayerAction>,
    pub grid: HashMap<String, Grid2D>,
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
pub struct TurnRequest {
    pub game_id: String,
    pub username: String,
    pub x: usize,
    pub y: usize,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct JoinGameRequest {
    pub game_id: String,
    pub username: String,
    pub ships: ShipsRaw,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QueueRequest {
    pub username: ClientId,
    pub ships: ShipsRaw,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreateGameRequest {
    pub username: ClientId,
    pub ships: ShipsRaw,
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
