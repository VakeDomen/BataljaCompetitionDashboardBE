use super::operations_db::establish_connection;
use crate::db::schema::competitions::dsl::*;
use crate::models::competition::{Competition, NewCompetition, SqlCompetition};
use chrono::Local;
use diesel::result::Error;
use diesel::{insert_into, prelude::*};

pub fn insert_competition(competition: NewCompetition) -> Result<Competition, Error> {
    let new_competition = SqlCompetition::from(competition);
    let mut conn = establish_connection().expect("Failed to get a DB connection from the pool");
    let _ = insert_into(competitions)
        .values(&new_competition)
        .execute(&mut conn)?;
    Ok(Competition::from(new_competition))
}

pub fn get_competition_by_id(uid: String) -> Result<Competition, Error> {
    let mut conn = establish_connection().expect("Failed to get a DB connection from the pool");
    match competitions
        .filter(id.eq(uid))
        .first::<SqlCompetition>(&mut conn)
    {
        Ok(u) => Ok(Competition::from(u)),
        Err(e) => Err(e),
    }
}

pub fn get_competitions_by_ids(ids: Vec<String>) -> Result<Vec<Competition>, Error> {
    let mut conn = establish_connection().expect("Failed to get a DB connection from the pool");
    let sql_competitions = competitions
        .filter(id.eq_any(ids))
        .load::<SqlCompetition>(&mut conn)?;
    let converted_competitions: Vec<Competition> = sql_competitions
        .into_iter()
        .map(|sql_competition| Competition::from(sql_competition))
        .collect();
    Ok(converted_competitions)
}

pub fn get_running_competitions() -> Result<Vec<Competition>, Error> {
    let mut conn = establish_connection().expect("Failed to get a DB connection from the pool");
    let current_time = Local::now().naive_utc();
    let sql_competitions = competitions
        .filter(start.le(current_time).and(end.ge(current_time)))
        .load::<SqlCompetition>(&mut conn)?;
    let converted_competitions: Vec<Competition> = sql_competitions
        .into_iter()
        .map(|sql_competition| Competition::from(sql_competition))
        .collect();
    Ok(converted_competitions)
}

pub fn set_competition_round(cid: &String, new_round: i32) -> Result<(), Error> {
    let mut conn = establish_connection().expect("Failed to get a DB connection from the pool");
    diesel::update(competitions.filter(id.eq(cid)))
        .set(round.eq(new_round.to_string()))
        .execute(&mut conn)?;
    Ok(())
}
