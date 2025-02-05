use std::{fs::File, io::Read};

use crate::{
    controllers::jwt::exchange_token_for_user,
    db::{
        operations_game2v2::get_game_by_id, operations_teams::get_team_by_student_for_competition,
    },
    models::user::Role,
};
use actix_web::{get, web, HttpResponse};
use actix_web_httpauth::extractors::bearer::BearerAuth;
use serde::Serialize;
use zip::ZipArchive;

#[derive(Debug, Serialize)]
struct GameLogResponse {
    log_file_contents: String,
}

#[get("/game/log/{id}")]
pub async fn game_log(auth: Option<BearerAuth>, id: web::Path<String>) -> HttpResponse {
    let game = match get_game_by_id(id.clone()) {
        Ok(game) => game,
        Err(_) => return HttpResponse::NotFound().finish(),
    };

    if !game.public {
        let auth_token = match auth {
            Some(token) => token,
            None => return HttpResponse::Forbidden().finish(),
        };
        let requesting_user_option = exchange_token_for_user(auth_token);
        let requesting_user = match requesting_user_option {
            Some(u) => u,
            None => return HttpResponse::Forbidden().finish(),
        };

        if requesting_user.role != Role::Admin {
            let team = match get_team_by_student_for_competition(
                requesting_user,
                game.competition_id.clone(),
            ) {
                Ok(t) => t,
                Err(_) => return HttpResponse::Unauthorized().finish(),
            };
            if !team.id.eq(&game.team1_id) && !team.id.eq(&game.team2_id) {
                return HttpResponse::Forbidden().finish();
            }
        }
    }

    let log_file_path = game.log_file_path;

    // Open the ZIP file
    let file = match File::open(&log_file_path) {
        Ok(file) => file,
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    // Create a new ZIP archive from the file
    let mut zip = match ZipArchive::new(file) {
        Ok(zip) => zip,
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    // Assuming the log file is the first file in the archive
    let mut log_file = match zip.by_index(0) {
        Ok(file) => file,
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    // Read the contents of the log file
    let mut log_file_contents = String::new();
    if let Err(_) = log_file.read_to_string(&mut log_file_contents) {
        return HttpResponse::InternalServerError().finish();
    }

    // Return the JSON response with a 200 OK status
    HttpResponse::Ok()
        .content_type("application/text; charset=utf-8")
        .body(log_file_contents)
}
