use std::{path::Path, fs, process::Command};
use rand::Rng;
use rayon::prelude::{IntoParallelIterator, ParallelIterator, IntoParallelRefIterator};

use crate::{
    db::{
        operations_competition::get_competition_by_id, 
        operations_teams::get_teams_by_competition_id, 
        operations_bot::{get_bot_by_id, set_bot_error},
    }, 
    models::{
        team::Team, 
        errors::MatchMakerError, 
        bot::Bot, 
        game_2v2::NewGame2v2, 
        competition::Competition
    }
};

use super::command_executor::{execute_command, recursive_copy};

pub fn run_2v2_round(competition_id: String) -> Result<Vec<(Team, Team)>, MatchMakerError> {
    let competition = match get_competition_by_id(competition_id) {
        Ok(c) => c,
        Err(e) => return Err(MatchMakerError::DatabaseError(e))
    };

    let teams = match get_teams_by_competition_id(competition.id.clone()) {
        Ok(teams) => teams,
        Err(e) => return Err(MatchMakerError::DatabaseError(e))
    };

    let compiled_teams = compile_team_bots(teams);
    let match_pairs = create_match_pairs(competition.games_per_round, compiled_teams);

    match_pairs.par_iter().for_each(|match_pair| {
        let result = run_match(&competition, &match_pair.0, &match_pair.1);
        
        match result {
            Ok(out) => println!("{:#?}", out),
            Err(e) => eprintln!("Error: {}", e), // Handle the error here or log it
        }
    });
  
    
    Ok(match_pairs)
}


/// Runs a game match between two teams in a given competition.
///
/// This function handles the entire lifecycle of a match:
/// 1. Setting up a new game.
/// 2. Creating a unique directory for the match.
/// 3. Copying the bots of both teams to the match directory.
/// 4. Executing the game using the Evaluator JAR.
/// 5. Saving the game output to a file and returning it.
///
/// # Arguments
///
/// * `competition` - A reference to the competition the teams are participating in.
/// * `team1` - The first team participating in the match.
/// * `team2` - The second team participating in the match.
///
/// # Returns
///
/// A `Result` containing a `Vec<String>` of the game's output lines if successful, or a `MatchMakerError` if there's an error.
fn run_match(competition: &Competition, team1: &Team, team2: &Team) -> Result<Vec<String>, MatchMakerError> {
    // Initialize a new 2v2 game with details from the provided teams and competition
    let match_game = NewGame2v2::new(
        competition.id.clone(),
        competition.round.to_string(),
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
    let mut bot_paths: Vec<String> = bots.iter().map(|bot_id| match_folder.join(bot_id).to_string_lossy().to_string()).collect();
    let output_file = format!("./resources/games/{}.txt", match_game.id.to_string());
    let mut command_args = vec![
        "-jar".to_string(),
        "resources/gamefiles/Evaluator.jar".to_string(),
    ];
    command_args.append(&mut bot_paths);

    // Run the game command and capture its output
    let result = Command::new("java")
        .args(&command_args)
        .output()
        .map_err(|e| MatchMakerError::IOError(e))?;

    // Save the game's output to the specified file
    if let Err(e) = fs::write(&output_file, result.stdout.clone()) {
        return Err(MatchMakerError::IOError(e))
    }

    // Convert the game's output into a vector of strings and return it
    let lines: Vec<String> = String::from_utf8_lossy(&result.stdout).lines().map(String::from).collect();
    Ok(lines)
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
fn compile_team_bots(teams: Vec<Team>) -> Vec<Team> {
    // Parallel processing of each team to compile associated bots
    let results: Vec<Result<Team, MatchMakerError>> = teams.into_par_iter().filter_map(|team| {
        // Skip teams without both bot1 and bot2
        if team.bot1.eq("") || team.bot2.eq("") {
            return None
        }

        // Retrieve bot details
        let bot1 = match get_bot_by_id(team.bot1.clone()) {
            Ok(b) => b,
            Err(e) => return Some(Err(MatchMakerError::DatabaseError(e))),
        };

        let bot2 = match get_bot_by_id(team.bot2.clone()) {
            Ok(b) => b,
            Err(e) => return Some(Err(MatchMakerError::DatabaseError(e))),
        };
        
        // Attempt to compile bot1
        if let Err(e) = compile_bot(&bot1) {
            if let Err(e) = set_bot_error(bot1, e.to_string()) {
                return Some(Err(MatchMakerError::DatabaseError(e)));
            }
            return Some(Err(e))
        }

        // Attempt to compile bot2
        if let Err(e) = compile_bot(&bot2) {
            if let Err(e) = set_bot_error(bot2, e.to_string()) {
                return Some(Err(MatchMakerError::DatabaseError(e)));
            }
            return Some(Err(e))
        }

        // Return the team if both bots compiled successfully
        Some(Ok(team))
    }).collect();

    // Extract teams with successful bot compilations
    let compiled_teams: Vec<Team> = results.into_iter().filter_map(|res| {
        match res {
            Ok(team) => Some(team),
            Err(_) => None,
        }
    }).collect();

    compiled_teams
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
fn compile_bot(bot: &Bot) -> Result<(), MatchMakerError> {
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
    if let Err(e) = execute_command(
        "cp".to_string(), 
        vec![source_path_str, workdir_str]
    ) {
        return Err(MatchMakerError::IOError(e))
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
        vec!["-o", unzip_target_str, "-d", workdir_str]
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
        Err(e) => return Err(MatchMakerError::IOError(e))
    };
    
    if java_files.is_empty() {
        return Err(MatchMakerError::IOError(std::io::Error::new(std::io::ErrorKind::NotFound, "No Java files found")));
    }
    
    // Convert the list of file paths to a format suitable for the `javac` command.
    let java_files_str: Vec<&str> = java_files
        .iter()
        .map(AsRef::as_ref)
        .collect();

    // Compile the Java files.
    if let Err(e) = execute_command(
        "javac".to_string(),
        java_files_str
    ) {
        return Err(MatchMakerError::IOError(e));
    }

    Ok(())
}

/// Creates match pairs for a set of teams.
///
/// # Arguments
///
/// * `match_num` - The number of matches each team should play.
/// * `teams` - A vector containing all the teams.
///
/// # Returns
///
/// A vector containing tuples, where each tuple represents a match between two teams.
///
/// # Panics
///
/// The function may panic if the random number generation fails.
/// 
fn create_match_pairs(match_num: i32, teams: Vec<Team>) -> Vec<(Team, Team)> {
    let mut pairs = Vec::new();
    let games_to_play = ((teams.len() as f32 * match_num as f32) / 2.).ceil() as i32;

    let mut players: Vec<usize> = std::iter::repeat(0..teams.len())
        .take(match_num as usize)
        .flatten()
        .collect();

    while (pairs.len() as i32) < games_to_play {
        let random_index = rand::thread_rng().gen_range(0..players.len());
        let first_team_index = players.swap_remove(random_index);
    
        if players.len() < 1 {
            break
        }

        let random_index = rand::thread_rng().gen_range(0..players.len());
        let second_team_index = players.swap_remove(random_index);
    
        pairs.push((
            teams[first_team_index].clone(),
            teams[second_team_index].clone(),
        ));
    }

    pairs
}
