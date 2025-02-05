use std::iter;

use rand::{thread_rng, Rng};

use crate::models::team::Team;

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
pub fn create_match_pairs(match_num: i32, teams: Vec<Team>) -> Vec<(Team, Team)> {
    let mut pairs = Vec::new();
    let games_to_play = ((teams.len() as f32 * match_num as f32) / 2.).ceil() as i32;

    let mut players: Vec<usize> = iter::repeat(0..teams.len())
        .take(match_num as usize)
        .flatten()
        .collect();

    while (pairs.len() as i32) < games_to_play {
        let random_index = thread_rng().gen_range(0..players.len());
        let first_team_index = players.swap_remove(random_index);

        if players.len() < 1 {
            break;
        }

        let random_index = thread_rng().gen_range(0..players.len());
        let second_team_index = players.swap_remove(random_index);

        pairs.push((
            teams[first_team_index].clone(),
            teams[second_team_index].clone(),
        ));
    }

    pairs
}
