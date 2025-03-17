use crate::app_state::ShipType::OneDeck1;
use crate::dto::{ClientId, GameId, GameStatus, ShipsRaw, WsEvent};
use rand::distr::{Alphanumeric, SampleString};
use rand::Rng;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;
use tokio::sync::mpsc::Sender;

#[derive(Debug)]
pub struct GameClients(pub(crate) ClientId, pub(crate) ClientId);

#[derive(Clone)]
pub struct Wrapper {
    pub shared: Arc<Shared>,
}

impl Wrapper {
    pub fn create_game(&self) -> String {
        let mut state = self.shared.state.write().unwrap();

        let game = Game::new();
        let game_id = game.id.clone();

        state.games.insert(game_id.clone(), game);

        game_id
    }

    pub fn is_game_over(&self, game_id: &str) -> bool {
        let state = self.shared.state.read().unwrap();
        if let Some(g) = state.games.get(game_id) {
            return g.status == GameStatus::GameOver;
        }

        return false;
    }

    pub fn attach_client(&self, game_id: &str, client: Client) {
        let state = &mut self.shared.state.write().unwrap();
        if let Some(r) = state.games.get_mut(game_id) {
            if r.client1.is_none() {
                r.client1 = Some(client);
                return;
            }

            if r.client2.is_none() {
                r.client2 = Some(client);
                return;
            }
        }
    }

    pub fn get_clients(&self, game_id: &str) -> (Client, Client) {
        let state = self.shared.state.read().unwrap();
        let g = state.games.get(game_id).unwrap();

        (g.client1.clone().unwrap(), g.client2.clone().unwrap())
    }

    pub fn get_room_sender(&self, game_id: &str) -> broadcast::Sender<String> {
        let state = self.shared.state.read().unwrap();
        let r = state.games.get(game_id).unwrap();
        r.room_sender.clone()
    }
}

#[derive(Debug)]
pub struct Shared {
    pub state: RwLock<MyState>,
}

#[derive(Debug)]
pub struct MyState {
    pub games: HashMap<GameId, Game>,
    pub game_clients: HashMap<GameId, GameClients>,
    pub client_games: HashMap<ClientId, GameId>,
    pub queue: VecDeque<(ClientId, Sender<WsEvent>, ShipsRaw)>,
}

#[derive(Debug)]
pub struct Game {
    pub id: String,
    pub p1: Option<Player>,
    pub p2: Option<Player>,
    pub current_turn: String,
    pub status: GameStatus,
    pub room_sender: broadcast::Sender<String>,
    pub client1: Option<Client>,
    pub client2: Option<Client>,
}

impl Game {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(16);
        Self {
            p1: None,
            p2: None,
            client1: None,
            client2: None,
            room_sender: tx,
            current_turn: String::new(),
            status: GameStatus::WaitingPlayers,
            id: Alphanumeric.sample_string(&mut rand::rng(), 6),
        }
    }

    pub fn join(&mut self, name: String, ships: ShipsRaw) {
        let mut server_ships = Vec::new();
        for ship in ships {
            let mut ship_coords = vec![];
            let mut ship_size = 0;
            for coords in ship.into_iter() {
                let p = Point2d::new(coords.0, coords.1);
                ship_coords.push(p);
                ship_size += 1;
            }
            server_ships.push(Ship::new(
                ship_coords,
                match ship_size {
                    1 => ShipType::OneDeck1,
                    2 => ShipType::TwoDeck1,
                    3 => ShipType::ThreeDeck1,
                    4 => ShipType::FourDeck1,
                    _ => OneDeck1,
                },
            ))
        }

        let (mut cnt1, mut cnt2, mut cnt3, mut cnt4, mut err) = (0, 0, 0, 0, 0);
        for ship in server_ships.iter() {
            match ship.coords.len() {
                1 => cnt1 += 1,
                2 => cnt2 += 1,
                3 => cnt3 += 1,
                4 => cnt4 += 1,
                _ => err += 1,
            }
        }

        if err != 0 || cnt1 != 4 || cnt2 != 3 || cnt3 != 2 || cnt4 != 1 {
            println!("Error! Incorrect numbr fo ships");
            return;
        }

        if self.p1.is_none() {
            self.p1 = Some(Player::new(name, server_ships));
            return;
        }

        if self.p2.is_none() {
            self.p2 = Some(Player::new(name, server_ships));
            let total_players = 2;
            let first_turn_idx = rand::rng().random_range(0..total_players);
            self.current_turn = match first_turn_idx {
                0 => self.p1.as_ref().unwrap().name.clone(),
                _ => self.p2.as_ref().unwrap().name.clone(),
            };
            self.status = GameStatus::Progress;
            return;
        }
    }
}

#[derive(Debug, Clone)]
pub struct Client {
    pub id: String,
    pub sender: Sender<WsEvent>,
}

impl Client {
    pub fn new(id: String, sender: Sender<WsEvent>) -> Self {
        Self { id, sender }
    }
}

#[derive(Debug)]
pub struct Player {
    pub name: String,
    pub grid_state: Vec<Vec<CellType>>,
    pub destroyed_count: usize,
}

impl Player {
    pub fn new(name: String, ships: Vec<Ship>) -> Self {
        let ship_xy_cells: Vec<Point2d> = ships
            .iter()
            .flat_map(|nested| nested.coords.iter())
            .map(|p| Point2d { x: p.x, y: p.y })
            .collect();

        Self {
            name,
            grid_state: {
                let mut state = vec![vec![CellType::EmptyNoShip; 10]; 10];
                for point in ship_xy_cells.iter() {
                    state[point.x][point.y] = CellType::HasShip
                }
                state
            },
            destroyed_count: 0,
        }
    }
}

#[derive(PartialEq, Debug)]
pub enum GameFlow {
    NextTurn,
    GameOver,
}

#[derive(Debug, Default, Clone, Copy)]
pub enum CellType {
    #[default]
    EmptyNoShip,
    HasShip,
    EmptyMissed, // miss
    HasShipHit,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Point2d {
    pub x: usize,
    pub y: usize,
}

impl Point2d {
    pub fn new(x: usize, y: usize) -> Self {
        Self { x, y }
    }
}

pub fn gen_hit_grid() -> Vec<Point2d> {
    let mut flat_grid: Vec<Point2d> = vec![];
    for i in 0..10 {
        for j in 0..10 {
            flat_grid.push(Point2d { x: i, y: j })
        }
    }
    return flat_grid;
}

#[derive(Clone, PartialEq, Debug, Copy)]
pub enum ShipType {
    OneDeck1,
    OneDeck2,
    OneDeck3,
    OneDeck4,
    TwoDeck1,
    TwoDeck2,
    TwoDeck3,
    ThreeDeck1,
    ThreeDeck2,
    FourDeck1,
}

#[derive(Debug)]
pub struct Ship {
    pub coords: Vec<Point2d>,
    pub ship_type: ShipType,
}

impl Ship {
    pub fn new(coords: Vec<Point2d>, ship_type: ShipType) -> Self {
        Self { coords, ship_type }
    }
}
