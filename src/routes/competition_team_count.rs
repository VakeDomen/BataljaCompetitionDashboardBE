use std::collections::HashMap;

use crate::db::operations_competition::get_running_competitions;
use crate::db::operations_teams::get_teams_by_competition_id;
use actix_web::{get, HttpResponse};

#[get("/competition/team/count")]
pub async fn competition_team_count() -> HttpResponse {
    let competitions = match get_running_competitions() {
        Ok(competitions) => competitions,
        Err(e) => return HttpResponse::InternalServerError().json(e.to_string()),
    };

    let mut hm: HashMap<String, usize> = HashMap::new();

    for competition in competitions.into_iter() {
        let id = competition.id.clone();
        let teams = match get_teams_by_competition_id(&id) {
            Ok(v) => v,
            Err(_) => continue,
        };
        hm.insert(competition.id, teams.len());
    }

    HttpResponse::Ok().json(hm)
}
