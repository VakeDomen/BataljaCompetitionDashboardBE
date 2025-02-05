use std::{fs::File, io::Read};

use crate::db::operations_competition::get_competition_by_id;
use actix_web::{get, web, HttpResponse};

#[get("/competition/pack/{comp_id}")]
pub async fn competition_pack(comp_id: web::Path<String>) -> HttpResponse {
    let competition = match get_competition_by_id(comp_id.into_inner()) {
        Ok(competition) => competition,
        Err(_e) => return HttpResponse::NotFound().finish(),
    };

    let path = competition.game_pack;
    let mut file = match File::open(&path) {
        Ok(file) => file,
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };

    // Read the file contents into a buffer
    let mut buffer = Vec::new();
    if file.read_to_end(&mut buffer).is_err() {
        return HttpResponse::InternalServerError().finish();
    }

    // Get the filename for use in the Content-Disposition header
    let filename = path.split("/").last().unwrap_or("download.zip");

    // Return the response
    HttpResponse::Ok()
        .content_type("application/zip")
        .append_header((
            "Content-Disposition",
            format!("attachment; filename=\"{}\"", filename),
        ))
        .body(buffer)
}
