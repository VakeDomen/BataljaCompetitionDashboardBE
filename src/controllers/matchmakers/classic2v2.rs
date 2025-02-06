use num_cpus;
use rayon::prelude::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, BufRead, BufReader},
    path::Path,
    process::{Command, ExitStatus, Output, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use wait_timeout::ChildExt;

use crate::{
    controllers::{
        command_executor::{execute_command, recursive_copy},
        elo::{calc_elo_changes, update_team_elo},
        file_handler::save_to_zip,
        matchmakers::{cleanup_matches, pairmakers::random::create_match_pairs},
    },
    db::{
        operations_bot::{get_bot_by_id, set_bot_error},
        operations_competition::{get_competition_by_id, set_competition_round},
        operations_game2v2::insert_game,
        operations_teams::get_teams_by_competition_id,
    },
    models::{
        bot::Bot,
        competition::Competition,
        errors::MatchMakerError,
        game_2v2::{Game2v2, NewGame2v2},
        game_player_stats::{GameError, GamePlayerStats},
        team::Team,
    },
};

/// Runs a 2v2 round for a specified competition.
///
/// This function manages the execution of a single 2v2 round for a competition, which includes:
/// 1. Fetching the competition details from the database.
/// 2. Retrieving all the teams participating in the competition.
/// 3. Compiling the bots for each team.
/// 4. Creating match pairs for the round.
/// 5. Running each match in parallel.
/// 6. Cleaning up the match directory after all games have been executed.
/// 7. Incrementing the competition round for the next set of matches.
///
/// # Arguments
///
/// * `competition_id` - A string representing the ID of the competition for which the round is to be run.
///
/// # Returns
///
/// A `Result` containing a `Vec` of tuples, where each tuple contains two teams that played against each other in the round.
/// If successful, or a `MatchMakerError` if there's an error.
///
/// # Errors
///
/// This function will return an error if:
/// - The competition cannot be fetched from the database.
/// - The teams for the specified competition cannot be retrieved.
/// - There's an issue compiling the bots for any team.
/// - There's an error running any of the matches.
/// - The cleanup process fails.
/// - There's a problem updating the competition's round in the database.
///
pub fn run_round(competition_id: String) -> Result<(), MatchMakerError> {
    println!("Running 2v2 competition: {}", competition_id);
    let competition = match get_competition_by_id(competition_id) {
        Ok(c) => c,
        Err(e) => return Err(MatchMakerError::DatabaseError(e)),
    };

    let teams = match get_teams_by_competition_id(&competition.id) {
        Ok(teams) => teams,
        Err(e) => return Err(MatchMakerError::DatabaseError(e)),
    };

    let compiled_teams = compile_team_bots(teams);
    let match_pairs = create_match_pairs(competition.games_per_round, compiled_teams);

    // Get the number of available logical cores
    let num_cores = num_cpus::get();

    // Calculate the number of threads to use (one less than the number of cores)
    let num_threads = num_cores - 1;

    // Create a custom thread pool with a specified number of threads
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .build()
        .unwrap();

    // Create a thread-safe vector using Arc and Mutex
    let games: Arc<Mutex<Vec<Game2v2>>> = Arc::new(Mutex::new(Vec::new()));

    // Execute the parallel operation with the custom thread pool
    pool.install(|| {
        match_pairs.par_iter().for_each(|match_pair| {
            match run_match(&competition, &match_pair.0, &match_pair.1) {
                Ok(g) => {
                    let mut games_lock = games.lock().unwrap();
                    games_lock.push(g)
                }
                Err(e) => eprintln!("Error: {}", e),
            }
        });
    });

    // Attempt to take ownership of the Mutex
    let games_mutex = Arc::try_unwrap(games)
        .expect("Arc::try_unwrap failed, there are multiple owners of the Arc");

    // Lock the Mutex to access the vector
    let games_vec = games_mutex
        .into_inner()
        .expect("Mutex::into_inner failed, the mutex is poisoned");

    if let Err(e) = update_team_elo(games_vec) {
        return Err(MatchMakerError::DatabaseError(e.into()));
    };

    // Cleanup: Remove the match directory
    cleanup_matches()?;

    // increment competition round
    let new_round = competition.round + 1;
    if let Err(e) = set_competition_round(&competition.id, new_round) {
        return Err(MatchMakerError::DatabaseError(e));
    }
    println!("Competition done!");
    Ok(())
}

/// Kill all processes running with the command "java Player."
pub fn kill_java_player_processes() -> Result<(), std::io::Error> {
    // Get a list of all processes with "java Player" in their command line
    let ps_output = Command::new("ps").arg("ax").output()?;

    // Convert the output to a string
    let ps_output_str = String::from_utf8_lossy(&ps_output.stdout);

    // Split the output into lines
    let process_lines: Vec<&str> = ps_output_str.lines().collect();

    // Iterate through the lines and find processes with "java Player"
    for process_line in process_lines {
        if process_line.contains("java Player") {
            // Extract the process ID (PID)
            let pid_str = process_line.split_whitespace().next().unwrap_or_default();

            // Parse the PID as an integer
            if let Ok(pid) = pid_str.parse::<i32>() {
                // Kill the process using the "kill" command
                let kill_result = Command::new("kill")
                    .arg("-9") // Use SIGKILL to forcefully terminate the process
                    .arg(pid.to_string())
                    .output();

                match kill_result {
                    Ok(Output {
                        status,
                        stdout,
                        stderr,
                    }) => {
                        if status.success() {
                            println!(
                                "Killed process with PID {}: {:?}",
                                pid,
                                String::from_utf8_lossy(&stdout)
                            );
                        } else {
                            eprintln!(
                                "Failed to kill process with PID {}: {:?}",
                                pid,
                                String::from_utf8_lossy(&stderr)
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("Error killing process with PID {}: {:?}", pid, e);
                    }
                }
            }
        }
    }

    Ok(())
}

/// Runs a game match between two teams in a given competition.
///
/// This function manages the preparation, execution, and cleanup of a game match between two teams.
/// The steps include:
///
/// 1. Initializing a new 2v2 game instance based on the teams and competition details.
/// 2. Creating a unique directory for the match within the `./resources/matches` folder.
/// 3. Copying the bots of both teams to the match directory.
/// 4. Running the game using the Evaluator JAR, ensuring the game and its spawned bot processes
///    are grouped together for easy management.
/// 5. Saving the game's output to a file within the `./resources/games` folder.
/// 6. Cleaning up by terminating any lingering processes related to the game to prevent zombies.
/// 7. Parsing the game output to produce a structured representation of the game results.
/// 8. Cleaning up by removing the match directory created in step 2.
///
/// # Arguments
///
/// * `competition` - A reference to the competition in which the teams are participating.
/// * `team1` - The first team participating in the match.
/// * `team2` - The second team participating in the match.
///
/// # Returns
///
/// A `Result` containing the structured game results (`Game2v2`) if successful. If there are any
/// issues during the preparation, execution, or cleanup, a `MatchMakerError` will be returned.
///
/// # Errors
///
/// This function may return one of the following errors:
///
/// - `MatchMakerError::IOError` if there is an I/O error during file operations.
/// - `MatchMakerError::TimeoutError` if the game process exceeds the specified timeout.
/// - `MatchMakerError::GameProcessFailed` if the game process exits with an error.
///
/// # Notes
///
/// - This function assumes that the necessary external tools and JAR files for game evaluation are
///   available and correctly configured.
///
fn run_match(
    competition: &Competition,
    team1: &Team,
    team2: &Team,
) -> Result<Game2v2, MatchMakerError> {
    // Initialize a new 2v2 game with details from the provided teams and competition
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

    // Create a directory to store match-related files
    let match_folder = Path::new("./resources/matches").join(match_game.id.to_string());
    if let Err(e) = fs::create_dir_all(&match_folder) {
        return Err(MatchMakerError::IOError(e));
    }

    // create a round directory (if doesn't exist) to later store game replays
    let output_dir = format!("./resources/games/{}", competition.round);
    if let Err(e) = fs::create_dir_all(&output_dir) {
        return Err(MatchMakerError::IOError(e));
    }

    // Copy each bot from the work directory to the match directory
    let bots = vec![&team1.bot1, &team1.bot2, &team2.bot1, &team2.bot2];
    for bot_id in &bots {
        let source = Path::new("./resources/workdir/bots").join(bot_id);
        let destination = match_folder.join(bot_id);

        if let Err(e) = recursive_copy(&source, &destination) {
            return Err(MatchMakerError::IOError(e));
        }
    }

    // Execute the game using the Evaluator JAR and collect the paths of each bot
    let mut bot_paths: Vec<String> = bots
        .iter()
        .map(|bot_id| {
            format!(
                "-p_{} {}",
                i,
                match_folder.join(bot_id).to_string_lossy().to_string()
            )
        })
        .collect();
    let output_file = format!(
        "./resources/games/{}/{}.zip",
        competition.round,
        match_game.id.to_string()
    );
    let mut command_args = vec![
        "-jar".to_string(),
        "resources/gamefiles/Evaluator.jar".to_string(),
        "--gui=false".to_string(),
    ];
    command_args.append(&mut bot_paths);

    // Spawn the child process
    let mut child = Command::new("java")
        .args(&command_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| MatchMakerError::IOError(e))?;

    // Set up asynchronous reading of stdout and stderr
    let stdout = child.stdout.take().expect("Failed to take stdout");
    let stderr = child.stderr.take().expect("Failed to take stderr");

    let stdout_reader = BufReader::new(stdout);
    let stderr_reader = BufReader::new(stderr);

    // Spawn threads to handle stdout and stderr
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

    // Wait for the process to finish or timeout
    let timeout_result: Option<ExitStatus> = child
        .wait_timeout(Duration::from_secs(120))
        .map_err(|e| MatchMakerError::IOError(e))?;
    // Initialize flags for success and timeout
    // let mut timeout_occurred = false;
    // let mut success = true;
    // Check if the process has finished
    if let None = timeout_result {
        // Timeout occurred
        // timeout_occurred = true;
        // success = false;
        // Attempt to kill the child process
        let _ = child.kill();
        let st = child.wait();
        println!("Game timed out, killed and exited with status: {:#?}", st);
    }

    // Join the threads and collect the output
    let output: Vec<String> = stdout_handle.join().expect("Failed to join stdout thread");
    let errors: Vec<String> = stderr_handle.join().expect("Failed to join stderr thread");

    // if timeout_occurred {
    //     // Process did not finish in time
    //     return Err(MatchMakerError::TimeoutError);
    // } else if !success {
    //     // Process finished but was not successful
    //     return Err(MatchMakerError::GameProcessFailed);
    // }

    // Save the game's output to the specified file
    let output_string = output.join("\n");
    if let Err(e) = save_to_zip(output_string, &output_file) {
        return Err(e);
    } else {
        match_game.log_file_path = output_file;
    }

    // Save any errors to a separate file
    if !errors.concat().trim().eq("...") {
        let error_string = errors.join("\n");
        let error_file = format!(
            "./resources/games/{}/{}_error.txt",
            competition.round,
            match_game.id.to_string()
        );
        if let Err(e) = fs::write(&error_file, &error_string) {
            // Log error output to help diagnose problems
            log::error!("Error output from child process: {}", error_string);
            return Err(MatchMakerError::IOError(e));
        }
    }

    // Parse the game using the provided function and return the result
    parse_game(output, errors, match_game)
}

/// Parses game output to determine match results and constructs a `Game2v2` object.
///
/// This function processes the output lines from a game match to extract relevant information
/// such as which bots survived and the scores of each bot. Based on this information, it
/// determines the winner of the match and constructs a `Game2v2` object that encapsulates
/// these details.
///
/// The function expects lines in the format `R <score> <color>` to determine scores of each bot.
/// Colors (`red`, `blue`, `green`, `yellow`) are associated with bots from both teams.
///
/// # Arguments
///
/// * `lines` - A vector of strings representing the game's output lines.
/// * `match_game` - A mutable `NewGame2v2` object that contains initial game details and will be
///                  updated with the parsed results.
///
/// # Returns
///
/// A `Result` containing a `Game2v2` object if successful, or a `MatchMakerError` if there's an error.
///
fn parse_game(
    lines: Vec<String>,
    errors: Vec<String>,
    mut match_game: NewGame2v2,
) -> Result<Game2v2, MatchMakerError> {
    if errors.len() > 1 {
        // always at least 1 because of first "..." row
        parse_bugged_game(lines, errors, &mut match_game);
    } else {
        parse_healthy_game(lines, errors, &mut match_game);
    }

    if let Err(e) = calc_elo_changes(&mut match_game) {
        return Err(MatchMakerError::DatabaseError(e.into()));
    }

    match insert_game(match_game) {
        Ok(g) => Ok(g),
        Err(e) => Err(MatchMakerError::DatabaseError(e)),
    }
}

fn parse_bugged_game(_lines: Vec<String>, errors: Vec<String>, match_game: &mut NewGame2v2) -> () {
    // find bot id
    let bot_ids = [
        match_game.team1bot1_id.clone(),
        match_game.team1bot2_id.clone(),
        match_game.team2bot1_id.clone(),
        match_game.team2bot2_id.clone(),
    ];
    let mut bugged_bot_id_option = None;
    for row in errors.iter() {
        for bot_id in bot_ids.iter() {
            if row.contains(bot_id) {
                bugged_bot_id_option = Some(bot_id);
                break;
            }
        }
    }
    match_game.team1bot1_survived = true;
    match_game.team1bot2_survived = true;
    match_game.team2bot1_survived = true;
    match_game.team2bot2_survived = true;

    if let Some(bugged_bot_id) = bugged_bot_id_option {
        if &match_game.team1bot1_id == bugged_bot_id {
            match_game.team1bot1_survived = false;
            match_game.winner_id = match_game.team2_id.clone();
        }

        if &match_game.team1bot2_id == bugged_bot_id {
            match_game.team1bot2_survived = false;
            match_game.winner_id = match_game.team2_id.clone();
        }

        if &match_game.team2bot1_id == bugged_bot_id {
            match_game.team2bot1_survived = false;
            match_game.winner_id = match_game.team1_id.clone();
        }

        if &match_game.team2bot2_id == bugged_bot_id {
            match_game.team2bot2_survived = false;
            match_game.winner_id = match_game.team1_id.clone();
        }
    }

    let trimmed_lines: String = errors.join("\n").replace("\\", "\\\\");

    // Remove backslashes from the formatted string
    let additional_data_error = GameError {
        error: trimmed_lines,
        blame_id: bugged_bot_id_option
            .unwrap_or(&"Unknown".to_string())
            .to_string(),
    };

    match_game.additional_data = serde_json::to_string(&additional_data_error)
        .unwrap_or(String::from("{ \"error\": \"Error serializing\"}"));
}

fn parse_healthy_game(lines: Vec<String>, _errors: Vec<String>, match_game: &mut NewGame2v2) -> () {
    let mut r_green = 0;
    let mut r_blue = 0;
    let mut r_yellow = 0;
    let mut r_cyan = 0;
    let mut current_bot: Option<String> = None;
    let mut last_L: Option<String> = None;
    let mut stats: HashMap<String, GamePlayerStats> = HashMap::new();
    let mut stats_keys = vec!["team2bot2", "team1bot2", "team2bot1", "team1bot1"];

    for line in lines.into_iter() {
        // track score through the game
        // the last score is the final score of the game
        // needed to determine the winner (if timeout still both teams are alive
        // and the "survived"/"winnwe" stat at the end is not enough)
        if line.contains("R ") {
            let parts: Vec<&str> = line.split(" ").collect();
            if parts.len() == 3 {
                match parts[2] {
                    "green" => r_green = parts[1].parse().unwrap_or(0),
                    "blue" => r_blue = parts[1].parse().unwrap_or(0),
                    "yellow" => r_yellow = parts[1].parse().unwrap_or(0),
                    "cyan" => r_cyan = parts[1].parse().unwrap_or(0),
                    _ => (),
                }
            }
        }

        if line.contains("L ") {
            last_L = Some(line.to_owned());
        }

        if line.contains("STAT: ") {
            // try to extract a bot name
            // also init a stat object for the player (untill next player id there is going to
            // be a sequence of stats in form of <key>: <value> for this player)
            let next_key_option = stats_keys.pop();
            if let Some(next_key) = next_key_option {
                current_bot = Some(next_key.to_string());
                stats.insert(next_key.into(), GamePlayerStats::default());
            }
        }

        let parts: Vec<&str> = line.split(" ").collect();

        // if collecting player stats
        if let Some(bot_key) = &current_bot {
            if parts.len() == 2 {
                let stat = match stats.get_mut(bot_key) {
                    Some(s) => s,
                    None => continue,
                };

                match parts[0] {
                    "turnsPlayed:" => stat.turns_played = parts[1].parse().unwrap_or(0),
                    "survive:" => stat.survived = parts[1].parse().unwrap_or(false),
                    "fleetGenerated:" => stat.fleet_generated = parts[1].parse().unwrap_or(0),
                    "fleetLost:" => stat.fleet_lost = parts[1].parse().unwrap_or(0),
                    "fleetReinforced:" => stat.fleet_reinforced = parts[1].parse().unwrap_or(0),
                    "largestAttack:" => stat.largest_attack = parts[1].parse().unwrap_or(0),
                    "largestLoss:" => stat.largest_loss = parts[1].parse().unwrap_or(0),
                    "largestReinforcement:" => {
                        stat.largest_reinforcement = parts[1].parse().unwrap_or(0)
                    }
                    "planetsLost:" => stat.planets_lost = parts[1].parse().unwrap_or(0),
                    "planetsConquered:" => stat.planets_conquered = parts[1].parse().unwrap_or(0),
                    "planetsDefended:" => stat.planets_defended = parts[1].parse().unwrap_or(0),
                    "planetsAttacked:" => stat.planets_attacked = parts[1].parse().unwrap_or(0),
                    "numFleetLost:" => stat.num_fleet_lost = parts[1].parse().unwrap_or(0),
                    "numFleetReinforced:" => {
                        stat.num_fleet_reinforced = parts[1].parse().unwrap_or(0)
                    }
                    "numFleetGenerated:" => {
                        stat.num_fleet_generated = parts[1].parse().unwrap_or(0)
                    }
                    "totalTroopsGenerated:" => {
                        stat.total_troops_generated = parts[1].parse().unwrap_or(0)
                    }
                    _ => (),
                }
            }
        }
    }

    // check if bots survived
    match_game.team1bot1_survived = if let Some(stat) = stats.get("team1bot1") {
        stat.survived
    } else {
        false
    };
    match_game.team1bot2_survived = if let Some(stat) = stats.get("team1bot2") {
        stat.survived
    } else {
        false
    };
    match_game.team2bot1_survived = if let Some(stat) = stats.get("team2bot1") {
        stat.survived
    } else {
        false
    };
    match_game.team2bot2_survived = if let Some(stat) = stats.get("team2bot2") {
        stat.survived
    } else {
        false
    };

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
        (false, false, true, false) => match_game.winner_id = match_game.team2_id.clone(),
        (false, false, false, true) => match_game.winner_id = match_game.team2_id.clone(),
        (_, _, _, _) => match_game.winner_id = "".to_string(),
    }

    // if multiple teams alive at the end (timeout) check who won by score
    if match_game.winner_id.eq("") {
        let t1_score = r_yellow + r_green;
        let t2_score = r_blue + r_cyan;

        if t1_score > t2_score {
            match_game.winner_id = match_game.team1_id.clone();
        } else {
            match_game.winner_id = match_game.team2_id.clone();
        }
    }
    if stats.is_empty() && last_L.is_some() {
        parse_bugged_game(vec![], vec![last_L.unwrap()], match_game)
    } else {
        match_game.additional_data = serde_json::to_string(&stats)
            .unwrap_or(String::from("{ \"error\": \"Error serializing\"}"));
    }
}

/// Attempts to compile the bots associated with each team in parallel.
///
/// This function performs the following steps for each team:
/// 1. If a team doesn't have both bot1 and bot2, the team is skipped.
/// 2. Retrieves the details of bot1 and bot2. If there's an error fetching the details, the team is skipped.
/// 3. Tries to compile bot1 and bot2. If there's a compilation error, an error is set for the respective bot.
/// 4. Teams with successful bot compilations are collected and returned.
///
/// # Arguments
///
/// * `teams` - A vector of `Team` objects for which bots need to be compiled.
///
/// # Returns
///
/// * A vector of `Team` objects for which both bots were successfully compiled.
///
/// # Notes
///
/// This function uses parallel processing for improved performance. Each team's bots are compiled in a separate thread.
///
pub fn compile_team_bots(teams: Vec<Team>) -> Vec<Team> {
    // Parallel processing of each team to compile associated bots
    let results: Vec<Team> = teams
        .into_par_iter()
        .filter_map(|team| {
            // Skip teams without both bot1 and bot2
            if team.bot1.eq("") || team.bot2.eq("") {
                return None;
            }

            // Retrieve bot details
            let bot1 = match get_bot_by_id(team.bot1.clone()) {
                Ok(b) => b,
                Err(_) => return None,
            };

            let bot2 = match get_bot_by_id(team.bot2.clone()) {
                Ok(b) => b,
                // Err(e) => return Some(Err(MatchMakerError::DatabaseError(e))),
                Err(_) => return None,
            };

            // Attempt to compile bot1
            if let Err(e) = compile_bot(&bot1) {
                if let Err(_) = set_bot_error(bot1, e.to_string()) {
                    // return Some(Err(MatchMakerError::DatabaseError(e)));
                    return None;
                }
                // return Some(Err(e))
                return None;
            }

            // Attempt to compile bot2
            if let Err(e) = compile_bot(&bot2) {
                if let Err(_) = set_bot_error(bot2, e.to_string()) {
                    // return Some(Err(MatchMakerError::DatabaseError(e)));
                    return None;
                }
                // return Some(Err(e))
                return None;
            }

            // Return the team if both bots compiled successfully
            Some(team)
        })
        .collect();

    results
    // // Extract teams with successful bot compilations
    // let compiled_teams: Vec<Team> = results.into_iter().filter_map(|res| {
    //     match res {
    //         Ok(team) => Some(team),
    //         Err(_) => None,
    //     }
    // }).collect();

    // compiled_teams
}

/// Check if a file contains the string "public static void main("
fn contains_main_method(file_path: &str) -> io::Result<bool> {
    let file = File::open(file_path)?;

    for line in io::BufReader::new(file).lines() {
        let line = line?;
        // Check if the line contains the desired string
        if line.contains("public static void main(") {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Compiles the provided bot's source code.
///
/// This function performs the following tasks:
/// 1. Creates a working directory specific to the bot.
/// 2. Copies the bot's ZIP file to the working directory.
/// 3. Unzips the bot's ZIP file.
/// 4. Finds any Java files inside the unzipped directory.
/// 5. Compiles the Java files using the `javac` command.
///
/// # Arguments
///
/// * `bot` - A `Bot` instance containing the bot's details, including the source path.
///
/// # Returns
///
/// * `Ok(())` if the bot's source code was compiled successfully.
/// * `Err(MatchMakerError)` if any step in the process fails.
///
/// # Errors
///
/// This function will return an error if:
/// * The working directory cannot be created.
/// * The ZIP file cannot be copied or unzipped.
/// * No Java files are found in the unzipped directory.
/// * The Java files cannot be compiled.
///
pub fn compile_bot(bot: &Bot) -> Result<(), MatchMakerError> {
    let workdir = Path::new("./resources/workdir/bots").join(bot.id.clone());
    let source_path = Path::new(&bot.source_path);

    // Create a dedicated working directory for the bot.
    if let Err(e) = fs::create_dir_all(&workdir) {
        return Err(MatchMakerError::IOError(e));
    }

    // Convert the paths to string representations for command execution.
    let workdir_str = match workdir.as_os_str().to_str() {
        Some(s) => s,
        None => return Err(MatchMakerError::InvalidPath(workdir.into())),
    };
    let source_path_str = match source_path.as_os_str().to_str() {
        Some(s) => s,
        None => return Err(MatchMakerError::InvalidPath(source_path.into())),
    };

    // Copy the bot's ZIP file to its working directory.
    if let Err(e) = execute_command("cp".to_string(), vec![source_path_str, workdir_str]) {
        return Err(MatchMakerError::IOError(e));
    };

    // Extract the file name from the source path.
    let file_name_osstr = match source_path.file_name() {
        Some(n) => n,
        None => return Err(MatchMakerError::InvalidPath(source_path.into())),
    };

    let file_name_str = match file_name_osstr.to_str() {
        Some(s) => s,
        None => return Err(MatchMakerError::InvalidPath(source_path.into())),
    };

    // Unzip the bot's ZIP file in the working directory.
    let unzip_target = workdir.join(file_name_str);
    let unzip_target_str = match unzip_target.as_os_str().to_str() {
        Some(s) => s,
        None => return Err(MatchMakerError::InvalidPath(unzip_target.into())),
    };

    if let Err(e) = execute_command(
        "unzip".to_string(),
        vec!["-o", unzip_target_str, "-d", workdir_str],
    ) {
        return Err(MatchMakerError::IOError(e));
    }

    // Retrieve a list of Java files from the unzipped directory.
    let java_files: Vec<String> = match fs::read_dir(&workdir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension() == Some(std::ffi::OsStr::new("java")))
            .map(|entry| entry.path().display().to_string())
            .collect(),
        Err(e) => return Err(MatchMakerError::IOError(e)),
    };

    if java_files.is_empty() {
        return Err(MatchMakerError::IOError(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No Java files found",
        )));
    }

    // Check if "Player.java" exists in the list of Java files
    if !java_files.iter().any(|file| file.ends_with("Player.java")) {
        return Err(MatchMakerError::PlayerFileMissing);
    }

    // Convert the list of file paths to a format suitable for the `javac` command.
    let java_files_str: Vec<&str> = java_files.iter().map(AsRef::as_ref).collect();

    // Player.java path
    let player_java_path = java_files
        .iter()
        .find(|&file| file.contains("Player.java"))
        .cloned()
        .unwrap();
    let contains_main_method_option = contains_main_method(&player_java_path);
    if let Ok(has_main_function) = contains_main_method_option {
        if !has_main_function {
            return Err(MatchMakerError::MainMethodNotInPlayerFile);
        }
    } else {
        return Err(MatchMakerError::MainMethodNotInPlayerFile);
    }

    // Compile the Java files.
    if let Err(e) = execute_command("javac".to_string(), java_files_str) {
        return Err(MatchMakerError::IOError(e));
    }

    Ok(())
}
