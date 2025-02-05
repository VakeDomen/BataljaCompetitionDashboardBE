use crate::{
    controllers::{
        elo::update_team_elo,
        matchmakers::{classic2v2::compile_team_bots, cleanup_matches, pairmakers::random},
    },
    db::{
        operations_competition::{get_competition_by_id, set_competition_round},
        operations_teams::{get_team_by_id, get_teams_by_competition_id},
        schema::competitions::games_per_round,
    },
    models::errors::MatchMakerError,
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

    for (team1, team2) in match_pairs {
        match run_match(&competition, &team1, &team2) {
            Ok(g) => {}
            Err(e) => eprintln!("Error: {}", e),
        }
    }

    if let Err(e) = update_team_elo(match_pairs) {
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
