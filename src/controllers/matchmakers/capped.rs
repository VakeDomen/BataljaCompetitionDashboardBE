use crate::{
    controllers::{
        elo::update_team_elo,
        matchmakers::{classic2v2::compile_team_bots, cleanup_matches, pairmakers::random},
    },
    db::{
        operations_competition::{get_competition_by_id, set_competition_round},
        operations_teams::get_teams_by_competition_id,
    },
    models::{competition::Competition, errors::MatchMakerError, game_2v2::Game2v2, team::Team},
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
    Ok(Game2v2::default())
}
