use crate::app_state::{CellType, Client, Game, GameClients, GameFlow, Player, Point2d, Wrapper};
use crate::dto::{CreateGameRequest, GameStatus, Grid2D, GridDTO, GridResponse, JoinGameRequest, PlayerAction, QueueRequest, StateRequest, TurnRequest, WsEvent};
use std::collections::HashMap;
use tokio::sync::mpsc::Sender;
use GameStatus::{GameOver, Progress, WaitingPlayers};

pub fn game_new(
    wrapper: Wrapper,
    CreateGameRequest { username, ships }: CreateGameRequest,
) -> WsEvent {
    println!("game_new: {}", &username);

    {
        let state = wrapper.shared.state.read().unwrap();
        if let Some(game_id) = state.client_games.get(&username) {
            return WsEvent::CreateGameRs {
                game_id: game_id.clone(),
                status: Progress,
            };
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
    );

    return WsEvent::CreateGameRs {
        game_id: game_id.clone(),
        status: WaitingPlayers,
    };
}

pub fn game_join(
    wrapper: Wrapper,
    JoinGameRequest {
        game_id,
        username,
        ships,
    }: JoinGameRequest,
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

    return WsEvent::JoinRs(GridResponse::new(status.clone(), None), p1_name);
}

pub fn enqueue(
    wrapper: Wrapper,
    QueueRequest { username, ships }: QueueRequest,
    sender: Sender<WsEvent>,
) -> WsEvent {
    println!("queue: {}", &username);

    let state = &mut wrapper.shared.state.write().unwrap();
    state.queue.push_back((username.clone(), sender, ships));

    return WsEvent::QueueRs {
        player_id: username,
    };
}

pub async fn match_players(wrapper: Wrapper) {
    {
        let state = &mut wrapper.shared.state.read().unwrap();
        // println!("Queue len: {}", state.queue.len());
        if state.queue.len() < 2 {
            return;
        }
    }

    let (p1, p2);
    {
        let state = &mut wrapper.shared.state.write().unwrap();
        p1 = state.queue.pop_front();
        p2 = state.queue.pop_front();
    }

    //assume queue doesn't contain dangling players (removed on disconnect)
    if let (Some((c1, sender1, ships1)), Some((c2, sender2, ships2))) = (p1, p2) {
        let rs = game_new(
            wrapper.clone(),
            CreateGameRequest {
                username: c1.clone(),
                ships: ships1,
            },
        );
        if let WsEvent::CreateGameRs { game_id, .. } = rs {
            game_join(
                wrapper.clone(),
                JoinGameRequest {
                    game_id: game_id.clone(),
                    username: c2.clone(),
                    ships: ships2,
                },
            );

            wrapper.attach_client(&game_id, Client::new(c1.clone(), sender1));

            wrapper.attach_client(&game_id, Client::new(c2.clone(), sender2));

            let (me, opponent) = wrapper.get_clients(&game_id);
            let my_state = game_state(
                wrapper.clone(),
                StateRequest::new(game_id.clone(), me.id),
            );
            let opponent_state = game_state(
                wrapper.clone(),
                StateRequest::new(game_id.clone(), opponent.id),
            );

            println!("Matched {} vs {} in game {}", &c1, &c2, &game_id);

            let _ = me
                .sender
                .send(WsEvent::GameStart {
                    game_id: game_id.clone(),
                })
                .await
                .unwrap();
            let _ = opponent
                .sender
                .send(WsEvent::GameStart {
                    game_id: game_id.clone(),
                })
                .await
                .unwrap();

            let _ = me.sender.send(my_state).await.unwrap();
            let _ = opponent.sender.send(opponent_state).await.unwrap();
        };
    };
}

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

    {
        let state = wrapper.shared.state.read().unwrap();
        let game = state.games.get(&game_id).unwrap();

        if game.status == WaitingPlayers || game.status == GameOver {
            return WsEvent::TurnRs(GridDTO {
                me: vec![],
                enemy: vec![],
            });
        }

        let is_turning_player = username == game.current_turn; //bug always p2 turn if no such name
        if !is_turning_player {
            return game_state(wrapper.clone(), StateRequest::new(game_id, username));
        }
    }

    {
        let mut state = wrapper.shared.state.write().unwrap();
        let game = state.games.get_mut(&game_id).unwrap();

        do_turn_user(Point2d { x, y }, username.clone(), game);
    }

    return game_state(wrapper.clone(), StateRequest::new(game_id, username));
}

pub fn game_state(wrapper: Wrapper, StateRequest { game_id, username }: StateRequest) -> WsEvent {
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

    let is_turning_player = username == game.current_turn;
    let action = if is_turning_player {
        PlayerAction::Shoot
    } else {
        PlayerAction::Wait
    }; //bug always p2 turn if no such name

    let players = vec![game.p1.as_ref().unwrap(), game.p2.as_ref().unwrap()];
    let mut players_grid = HashMap::new();
    for p in players {
        players_grid.insert(p.name.clone(), grid_as_json_single(p, username != p.name));
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

pub fn grid_as_json_single(p: &Player, enemy: bool) -> Grid2D {
    let draw_cell_types = |p: &Player, grid: &mut Grid2D| {
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

    let mut grid: Grid2D = vec![vec![String::new(); 10]; 10];
    draw_cell_types(p, &mut grid);

    if enemy {
        //hide enemy ships
        for (_, row) in grid.iter_mut().enumerate() {
            for (_, cell) in row.iter_mut().enumerate() {
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
