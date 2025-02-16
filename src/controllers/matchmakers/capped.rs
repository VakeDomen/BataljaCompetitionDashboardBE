use std::{
    collections::HashMap,
    fs::{self, create_dir_all},
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use serde::{Deserialize, Deserializer, Serialize};
use wait_timeout::ChildExt;

use crate::{
    controllers::{
        command_executor::recursive_copy,
        elo::{calc_elo_changes, update_team_elo},
        file_handler::save_to_zip,
        matchmakers::{classic2v2::compile_team_bots, cleanup_matches, pairmakers::random},
    },
    db::{
        operations_competition::{get_competition_by_id, set_competition_round},
        operations_game2v2::insert_game,
        operations_teams::get_teams_by_competition_id,
    },
    models::{
        competition::Competition,
        errors::MatchMakerError,
        game_2v2::{Game2v2, NewGame2v2},
        team::Team,
    },
};

pub fn run_round(competition_id: String) -> Result<(), MatchMakerError> {
    println!("Running Capped competition: {}", competition_id);

    let competition = match get_competition_by_id(competition_id) {
        Ok(c) => c,
        Err(e) => return Err(MatchMakerError::DatabaseError(e)),
    };

    let teams = match get_teams_by_competition_id(&competition.id) {
        Ok(teams) => teams,
        Err(e) => return Err(MatchMakerError::DatabaseError(e)),
    };

    let compiled_teams = compile_team_bots(teams);
    let match_pairs = random::create_match_pairs(competition.games_per_round, compiled_teams);

    let mut games_vec = Vec::new();

    for (team1, team2) in match_pairs {
        match run_match(&competition, &team1, &team2) {
            Ok(g) => games_vec.push(g),
            Err(e) => eprintln!("Error: {}", e),
        }
    }

    if let Err(e) = update_team_elo(games_vec) {
        return Err(MatchMakerError::DatabaseError(e));
    }

    cleanup_matches()?;

    let new_round = competition.round + 1;
    if let Err(e) = set_competition_round(&competition.id, new_round) {
        return Err(MatchMakerError::DatabaseError(e));
    }

    println!("Competition done!");
    Ok(())
}

/// Runs a match between two teams in a given competition
///
/// Runs similar to classic2v2::run_match
fn run_match(
    competition: &Competition,
    team1: &Team,
    team2: &Team,
) -> Result<Game2v2, MatchMakerError> {
    let mut match_game = NewGame2v2::new(
        competition.id.clone(),
        competition.round,
        team1.id.clone(),
        team2.id.clone(),
        team1.bot1.clone(),
        team1.bot2.clone(),
        team2.bot1.clone(),
        team2.bot2.clone(),
    );

    let match_folder = Path::new("./resources/matches").join(match_game.id.to_string());
    if let Err(e) = create_dir_all(&match_folder) {
        return Err(MatchMakerError::IOError(e));
    }

    let output_dir = format!("./resources/games/{}", competition.round);
    if let Err(e) = fs::create_dir_all(&output_dir) {
        return Err(MatchMakerError::IOError(e));
    }

    let bots = vec![&team1.bot1, &team1.bot2, &team2.bot1, &team2.bot2];
    for bot_id in &bots {
        let source = Path::new("./resources/workdir/bots").join(bot_id);
        let destination = match_folder.join(bot_id);

        if let Err(e) = recursive_copy(&source, &destination) {
            return Err(MatchMakerError::IOError(e));
        }
    }

    let mut bot_paths = bots
        .iter()
        .map(|bot_id| match_folder.join(bot_id).to_string_lossy().to_string())
        .collect();

    let output_file = format!(
        "./resources/games/{}/{}.zip",
        competition.round,
        match_game.id.to_string()
    );
    let mut command_args = vec![
        "-jar".to_string(),
        "resources/gamefiles/Capped.jar".to_string(),
        "--no-gui".to_string(),
        "-j".to_string(),
        "-t".to_string(),
        "200".to_string(),
        "-tn_0".to_string(),
        format!(" {}",team1.name.clone()),
        "-tn_1".to_string(),
        team2.name.clone(),
    ];

    command_args.append(&mut bot_paths);

    let mut child = Command::new("java")
        .args(&command_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| MatchMakerError::IOError(e))?;

    let stdout = child.stdout.take().expect("Failed to take stdout");
    let stderr = child.stderr.take().expect("Failed to take stderr");

    let stdout_reader = BufReader::new(stdout);
    let stderr_reader = BufReader::new(stderr);

    let stdout_handle = thread::spawn(move || {
        stdout_reader
            .lines()
            .filter_map(Result::ok)
            .collect::<Vec<String>>()
    });
    let stderr_handle = thread::spawn(move || {
        stderr_reader
            .lines()
            .filter_map(Result::ok)
            .collect::<Vec<String>>()
    });

    let timeout_result = child
        .wait_timeout(Duration::from_secs(120))
        .map_err(|e| MatchMakerError::IOError(e))?;

    if let None = timeout_result {
        let _ = child.kill();
        let st = child.wait();
        println!("Game timed out, killed and exited with status: {:#?}", st);
    }

    let output = stdout_handle.join().expect("Failed to join stdout thread");
    let errors = stderr_handle.join().expect("Failed to join stderr thread");

    let output_string = output.join("\n");

    if let Err(e) = save_to_zip(output_string, &output_file) {
        return Err(e);
    } else {
        match_game.log_file_path = output_file;
    }

    if !errors.is_empty() {
        let error_string = errors.join("\n");
        let error_file = format!(
            "./resources/games/{}/{}_error.txt",
            competition.round,
            match_game.id.to_string()
        );

        if let Err(e) = fs::write(&error_file, &error_string) {
            log::error!("Error output from child process: {}", error_string);
            return Err(MatchMakerError::IOError(e));
        }
    }

    parse_game(output, errors, match_game)
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum BotStatus {
    None,
    Winner,
    Lost,
    Time,
    Error,
}

impl BotStatus {
    fn did_survive(&self) -> bool {
        match self {
            BotStatus::Winner => true,
            BotStatus::None => true,
            _ => false,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct BotStats {
    num_fleet_generated: i32,
    color: String,
    largest_loss: i32,
    planets_lost: i32,
    num_fleet_lost: i32,
    fleet_generated: i32,
    fleet_lost: i32,
    team: String,
    fleet_reinforced: i32,
    #[serde(deserialize_with = "option_none")]
    error: Option<String>,
    planets_conquered: i32,
    num_fleet_reinforced: i32,
    owning_planets: i32,
    planets_defended: i32,
    score: i32,
    reason_for_death: BotStatus,
    turns_played: i32,
    largest_attack: i32,
    largest_reinforcement: i32,
    owning_fleet: i32,
}

fn option_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    if s.is_empty() {
        Ok(None)
    } else {
        Ok(Some(s))
    }
}

fn parse_game(
    mut lines: Vec<String>,
    errors: Vec<String>,
    mut match_game: NewGame2v2,
) -> Result<Game2v2, MatchMakerError> {
    let mut stats: HashMap<String, BotStats> = HashMap::new();

    // Check for error, this is unneseseary players wise, but is better to be kept in for any unusual runs of the game executor.
    if !errors.is_empty() {
        log::error!(
            "ERROR Running game [{}]\nError: {:?}",
            match_game.id,
            errors
        );
    }

    if let None = lines.pop() {
        log::warn!("No lines found");
    }

    for bot_str in vec!["team2bot2", "team2bot1", "team1bot2", "team1bot1"] {
        let bot_json = lines.pop().unwrap_or_else(|| {
            log::warn!("No {} JSON", bot_str);
            return "".to_owned();
        });

        let bot: BotStats = match serde_json::from_str(&bot_json) {
            Ok(b) => b,
            Err(e) => return Err(MatchMakerError::IOError(e.into())),
        };

        stats.insert(bot_str.to_owned(), bot);

        match bot_str {
            "team1bot1" => {
                if let Some(stat) = stats.get(bot_str) {
                    match_game.team1bot1_survived = stat.reason_for_death.did_survive();
                }
            }
            "team1bot2" => {
                if let Some(stat) = stats.get(bot_str) {
                    match_game.team1bot2_survived = stat.reason_for_death.did_survive();
                }
            }
            "team2bot1" => {
                if let Some(stat) = stats.get(bot_str) {
                    match_game.team2bot1_survived = stat.reason_for_death.did_survive();
                }
            }
            "team2bot2" => {
                if let Some(stat) = stats.get(bot_str) {
                    match_game.team2bot2_survived = stat.reason_for_death.did_survive();
                }
            }
            _ => {
                log::warn!("This shouldn't happen: {}", bot_str);
            }
        }
    }

    match (
        &match_game.team1bot1_survived,
        &match_game.team1bot2_survived,
        &match_game.team2bot1_survived,
        &match_game.team2bot2_survived,
    ) {
        (true, true, false, false) => match_game.winner_id = match_game.team1_id.clone(),
        (true, false, false, false) => match_game.winner_id = match_game.team1_id.clone(),
        (false, true, false, false) => match_game.winner_id = match_game.team1_id.clone(),
        (false, false, true, true) => match_game.winner_id = match_game.team2_id.clone(),
        (false, false, false, true) => match_game.winner_id = match_game.team2_id.clone(),
        (false, false, true, false) => match_game.winner_id = match_game.team2_id.clone(),
        (t1a, t1b, t2a, t2b) if (*t1a || *t1b) && (*t2a || *t2b) => {
            let t1b1 = stats
                .get("team1bot1")
                .and_then(|x| Some(x.score))
                .unwrap_or_default();
            let t1b2 = stats
                .get("team1bot2")
                .and_then(|x| Some(x.score))
                .unwrap_or_default();
            let t2b1 = stats
                .get("team2bot1")
                .and_then(|x| Some(x.score))
                .unwrap_or_default();
            let t2b2 = stats
                .get("team2bot2")
                .and_then(|x| Some(x.score))
                .unwrap_or_default();

            let first = t1b1 + t1b2;
            let second = t2b1 + t2b2;

            match_game.winner_id = if first > second {
                match_game.team1_id.clone()
            } else {
                match_game.team2_id.clone()
            }
        }
        // unclear game end conditions
        _ => {
            log::error!("UNEXPECTED game ending for :[{}]", match_game.id);
        }
    }

    // TODO: IS Error parsing neceserry? adititonal data has everyting inside.

    if !stats.is_empty() {
        match_game.additional_data = serde_json::to_string(&stats)
            .unwrap_or_else(|_| "{ \"error\": \"Error serializing stats\"}".to_string());
    }

    if let Err(e) = calc_elo_changes(&mut match_game) {
        return Err(MatchMakerError::DatabaseError(e.into()));
    }

    match insert_game(match_game) {
        Ok(g) => Ok(g),
        Err(e) => Err(MatchMakerError::DatabaseError(e)),
    }
}
