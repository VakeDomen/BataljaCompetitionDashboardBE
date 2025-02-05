use crate::{
    db::{
        operations_competition::get_competition_by_id,
        operations_teams::{get_team_by_id, get_teams_by_competition_id},
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

    Ok(())
}
