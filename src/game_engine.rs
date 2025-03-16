use crate::app_state::{
    CellType, Game, GameClients, GameFlow, Player, Point2d, Ship, ShipType, Wrapper,
};
use crate::dto::{
    CreateGameRequest, GameStatus, GridDTO, GridResponse, JoinRequest, NewGameResponse,
    PlayerAction, StateRequest, TurnRequest, WsEvent,
};
use std::collections::HashMap;
use GameStatus::{GameOver, Progress, WaitingPlayers};
/*
Explicitly store on the heap: Box

Shared ownership: Arc and Rc

Mutate something shared: Cell, RefCell, Mutex
https://www.reddit.com/r/rust/comments/llzewm/when_should_i_use_box_arc_rc_cell_and_refcell_can/
 */

pub fn game_new(
    wrapper: Wrapper,
    CreateGameRequest { username, ships }: CreateGameRequest,
) -> WsEvent {
    println!("game_new: {}", &username);

    {
        let mut state = wrapper.shared.state.read().unwrap();
        if let Some(game_id) = state.client_games.get(&username) {
            return WsEvent::CreateGameRs(NewGameResponse {
                id: game_id.clone(),
                status: Progress,
            });
        }
    } //drop lock

    let game_id = wrapper.create_game();
    let mut state = wrapper.shared.state.write().unwrap();

    // let mut ng = Game::new();
    let ng = state.games.get_mut(&game_id).unwrap();
    ng.join(username.clone(), ships);
    state.client_games.insert(username.clone(), game_id.clone());
    state.game_clients.insert(
        game_id.clone(),
        GameClients(username.clone(), String::new()),
    ); //tup????

    return WsEvent::CreateGameRs(NewGameResponse {
        id: game_id.clone(),
        status: WaitingPlayers,
    });
}

pub fn game_join(
    wrapper: Wrapper,
    JoinRequest {
        game_id,
        username,
        ships,
    }: JoinRequest,
) -> WsEvent {
    println!("game_join: {} {}", &game_id, &username);

    {
        let state = wrapper.shared.state.read().unwrap();

        if let None = state.games.get(&game_id) {
            println!("Joining nox-existing game: {} {}", &game_id, &username);
            return WsEvent::ServerAbort;
        }

        if let Some(g) = state.client_games.get(&username) {
            println!("P1 already in a game: {} {}", &g, &username);
            return WsEvent::ServerAbort;
        }

        let game = state.games.get(&game_id).unwrap();
        if game.p1.is_none() {
            println!(
                "P1 did not join at game creation (game logic error): {} {}",
                &game_id, &username
            );
            return WsEvent::ServerAbort;
        }
    } //drop lock

    let state = &mut wrapper.shared.state.write().unwrap();

    let x = state.games.get_mut(&game_id).unwrap();
    x.join(username.clone(), ships);
    let (p1_name, has_p1, has_p2, status) = (
        x.p1.as_ref().unwrap().name.clone(),
        x.p1.is_some(),
        x.p2.is_some(),
        x.status,
    );

    if has_p1 && has_p2 {
        // state.games.get_mut(&game_id).unwrap().p2 = Some(Player::new(username.clone().to_string(), arrange_p1()));
        state.client_games.insert(username.clone(), game_id.clone());
        state.game_clients.get_mut(&game_id).unwrap().1 = username.clone();
    }

    let g = state.games.get_mut(&game_id).unwrap();
    if status != GameOver {
        if let (Some(_), Some(_)) = (&g.p1, &g.p2) {
            g.status = Progress;
        } else {
            g.status = WaitingPlayers;
        }
    }

    // return game_state(state.clone(), StateRequest{game_id, username})
    return WsEvent::JoinRs(GridResponse::new(status.clone(), None), p1_name);
}

// #[get("/game/{id}/turn/{username}/{x}/{y}")]
pub fn game_turn(
    wrapper: Wrapper,
    TurnRequest {
        game_id,
        username,
        x,
        y,
    }: TurnRequest,
) -> WsEvent {
    println!("game_turn: {} {} {}:{}", &game_id, &username, x, y);

    if x < 0 && x > 9 || y < 0 || y > 9 {
        return WsEvent::BadRequestRs("Incorrect coordinates".to_string());
    }

    let p1_name;
    let p2_name;
    let status;
    {
        let mut state = wrapper.shared.state.read().unwrap();
        let mut game = state.games.get(&game_id).unwrap();
        // let game = &mut *data.game.lock().unwrap();
        if game.status == WaitingPlayers || game.status == GameOver {
            return WsEvent::TurnRs(GridDTO {
                me: vec![],
                enemy: vec![],
            });
        }

        // requester = if username == game.p1.as_ref().unwrap().name { P1 } else { P2 }; //bug always p2 turn if no such name
        let is_turning_player = username == game.current_turn; //bug always p2 turn if no such name
        if !is_turning_player {
            //game.current_turn == requester {
            return game_state(wrapper.clone(), StateRequest { game_id, username });
            // return WsEvent::TurnRs(grid_as_json(game.p1.as_ref().unwrap(), game.p2.as_ref().unwrap(), requester.clone()));
        }
    }

    {
        let mut state = wrapper.shared.state.write().unwrap();
        let game = state.games.get_mut(&game_id).unwrap();

        do_turn_user(Point2d { x, y }, username.clone(), game);

        status = game.status;

        p1_name = game.p1.as_ref().unwrap().name.clone();
        p2_name = game.p2.as_ref().unwrap().name.clone();
    }

    // if status == GameOver {
    //     println!("game over, remove {} and {} and {}", &p1_name, &p2_name, &game_id);
    //     //todo ?
    //     // let mut state = wrapper.shared.state.write().unwrap();
    //     // state.client_games.remove(&p1_name);
    //     // state.client_games.remove(&p2_name);
    //     // state.game_clients.remove(&game_id);
    //     // state.games.remove(&game_id);
    // }

    return game_state(wrapper.clone(), StateRequest { game_id, username });
    // return WsEvent::TurnRs(grid);
}

pub fn game_state(
    wrapper: Wrapper,
    StateRequest {
        game_id,
        username: owner,
    }: StateRequest,
) -> WsEvent {
    // println!("game_state: {} {}", &game_id, &owner);

    let state = wrapper.shared.state.read().unwrap();
    if let None = state.games.get(&game_id) {
        return WsEvent::StateRs(GridResponse {
            status: GameOver,
            action: None,
            me: None,
            enemy: None,
            grid: HashMap::new(),
        });
    }

    let game = state.games.get(&game_id).unwrap();

    if game.status == WaitingPlayers {
        return WsEvent::StateRs(GridResponse::new(game.status.clone(), None));
    }

    let is_turning_player = owner == game.current_turn; //bug always p2 turn if no such name
    let action = if is_turning_player {
        PlayerAction::Shoot
    } else {
        PlayerAction::Wait
    }; //bug always p2 turn if no such name

    let players = vec![game.p1.as_ref().unwrap(), game.p2.as_ref().unwrap()];
    let mut players_grid = HashMap::new();
    for p in players {
        players_grid.insert(p.name.clone(), grid_as_json_single(p, owner != p.name));
    }

    return WsEvent::StateRs(GridResponse {
        status: game.status.clone(),
        action: Some(action),
        me: None,
        enemy: None,
        grid: players_grid,
    });
}

pub fn do_turn_user(hit: Point2d, requester: String, game: &mut Game) -> GameFlow {
    let players = vec![game.p1.as_mut().unwrap(), game.p2.as_mut().unwrap()];
    let mut enemy_opt = None;
    for p in players {
        if p.name != requester {
            enemy_opt = Some(p);
            break;
        }
    }
    // let mut enemy = if game.current_turn == requester { game.p2.as_mut().unwrap() } else { game.p1.as_mut().unwrap() };

    let enemy = enemy_opt.unwrap();
    match enemy.grid_state[hit.x][hit.y] {
        CellType::EmptyNoShip => {
            enemy.grid_state[hit.x][hit.y] = CellType::EmptyMissed;
            game.current_turn = enemy.name.clone();
        } //miss
        CellType::HasShip => {
            enemy.grid_state[hit.x][hit.y] = CellType::HasShipHit;
            enemy.destroyed_count += 1;
            //don't change current turn player
        }
        CellType::EmptyMissed => {} //already miss at prev turn, do nothing
        CellType::HasShipHit => {}  //already hit at prev turn, do nothing
    }

    return match enemy.destroyed_count >= 20 {
        true => {
            game.status = GameOver;
            GameFlow::GameOver
        }
        false => {
            game.status = Progress;
            GameFlow::NextTurn
        }
    };
}

pub fn grid_as_json_single(p: &Player, enemy: bool) -> Vec<Vec<String>> {
    let draw_cell_types = |p: &Player, grid: &mut Vec<Vec<String>>| {
        for (x, row) in p.grid_state.iter().enumerate() {
            for (y, cell) in row.iter().enumerate() {
                match cell {
                    CellType::EmptyNoShip => grid[x][y] = ".".to_string(),
                    CellType::HasShipHit => grid[x][y] = "x".to_string(),
                    CellType::HasShip => grid[x][y] = "#".to_string(),
                    CellType::EmptyMissed => grid[x][y] = "_".to_string(),
                }
            }
        }
    };

    let mut grid: Vec<Vec<String>> = vec![vec![String::new(); 10]; 10];
    draw_cell_types(p, &mut grid);

    if enemy {
        //hide enemy ships
        for (x, row) in grid.iter_mut().enumerate() {
            for (y, cell) in row.iter_mut().enumerate() {
                *cell = if cell == "#" {
                    ".".to_string()
                } else {
                    cell.clone()
                };
            }
        }
    }

    return grid;
}
