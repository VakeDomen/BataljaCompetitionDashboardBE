use diesel::prelude::{Insertable, Queryable};
use serde::{Serialize, Deserialize};
use chrono::{NaiveDateTime, Local};
use uuid::Uuid;
use crate::db::schema::competitions::{self};

#[derive(Debug, Deserialize)]
pub struct NewCompetition {
    name: String,
    start: NaiveDateTime,
    end: NaiveDateTime,
    type_: String,
}

#[derive(Debug)]
pub struct Competition {
    pub id: String,
    pub name: String,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub allowed_submissions: bool,
    pub round: i32,
    pub type_: String,
    pub games_per_round: i32,
    pub game_pack: String,
    pub created: NaiveDateTime,
}   

#[derive(Queryable, Debug, Insertable)]
#[diesel(table_name = competitions)]
pub struct SqlCompetition {
    pub id: String,
    pub name: String,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub allowed_submissions: String,
    pub round: String,
    pub type_: String,
    pub games_per_round: i32,
    pub game_pack: String,
    pub created: NaiveDateTime,
}

#[derive(Debug, Serialize, Clone)]
pub struct PublicCompetition {
    pub id: String,
    pub name: String,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub allowed_submissions: bool,
    pub round: i32,
    pub type_: String,
    created: NaiveDateTime,
}

impl From<SqlCompetition> for Competition {
    fn from(sql_competition: SqlCompetition) -> Self {
        Self {
            id: sql_competition.id,
            name: sql_competition.name,
            start: sql_competition.start.into(),
            end: sql_competition.end.into(),
            allowed_submissions: sql_competition.allowed_submissions.parse().unwrap(),
            round: sql_competition.round.parse().unwrap(),
            type_: sql_competition.type_,
            games_per_round: sql_competition.games_per_round,
            game_pack: sql_competition.game_pack,
            created: sql_competition.created,
        }
    }
}

impl From<Competition> for PublicCompetition {
    fn from(competition: Competition) -> Self {
        Self { 
            id: competition.id,
            name: competition.name,
            start: competition.start,
            end: competition.end,
            allowed_submissions: competition.allowed_submissions,
            round: competition.round,
            type_: competition.type_,
            created: competition.created,
        }
    }
}

impl From<NewCompetition> for SqlCompetition {
    fn from(new_competition: NewCompetition) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: new_competition.name,
            start: new_competition.start,
            end: new_competition.end,
            allowed_submissions: true.to_string(),
            round: 0.to_string(),
            type_: new_competition.type_.clone(),
            games_per_round: 6,
            game_pack: format!("./resources/packs/Batalja{}Pack.zip", new_competition.type_),
            created: Local::now().naive_utc(),
        }
    }
}