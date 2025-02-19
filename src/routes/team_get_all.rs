use crate::{
    controllers::jwt::exchange_token_for_user,
    db::operations_teams::get_teams_by_competition_id,
    models::{team::PublicTeam, user::Role},
};
use actix_web::{get, web, HttpResponse};
use actix_web_httpauth::extractors::bearer::BearerAuth;

#[get("/team/all/{comp_id}")]
pub async fn team_get_all(auth: BearerAuth, comp_id: web::Path<String>) -> HttpResponse {
    let requesting_user = match exchange_token_for_user(auth) {
        Some(u) => u,
        None => return HttpResponse::Unauthorized().finish(),
    };

    if requesting_user.role != Role::Admin {
        return HttpResponse::Forbidden().finish();
    }

    let competition_id = comp_id.into_inner();
    match get_teams_by_competition_id(&competition_id) {
        Ok(teams) => HttpResponse::Ok().json(
            teams
                .into_iter()
                .map(PublicTeam::from)
                .collect::<Vec<PublicTeam>>(),
        ),
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}
