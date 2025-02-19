use crate::{
    db::operations_competition::get_running_competitions, models::errors::MatchMakerError,
};

use super::matchmakers::{capped, classic2v2};

pub fn run_competitions_round() -> Result<(), MatchMakerError> {
    let competitions = match get_running_competitions() {
        Ok(c) => c,
        Err(e) => return Err(MatchMakerError::DatabaseError(e)),
    };

    for competition in competitions.into_iter() {
        match competition.type_.as_str() {
            "2v2" => classic2v2::run_round(competition.id)?,
            "capped" => capped::run_round(competition.id)?,
            _ => continue,
        }
    }
    Ok(())
}
