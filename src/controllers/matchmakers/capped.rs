use crate::{db::operations_competition::get_competition_by_id, models::errors::MatchMakerError};

pub fn run_game(competition_id: String) -> Result<(), MatchMakerError> {
    println("Running Capped competition: {}", competition_id);

    let competition = match get_competition_by_id(competition_id) {
        Ok(c) => c,
        Err(e) => return Err(MatchMakerError::DatabaseError(e)),
    };

    Ok(())
}
