use super::operations_db::establish_connection;
use crate::db::schema::teams::dsl::*;
use crate::models::team::{BotSelector, NewTeam, SqlTeam, Team};
use crate::models::user::User;
use diesel::result::Error;
use diesel::{insert_into, prelude::*};

pub fn create_team(team: NewTeam) -> Result<Team, Error> {
    let new_team = SqlTeam::from(team);
    let mut conn = establish_connection().expect("Failed to get a DB connection from the pool");
    let _ = insert_into(teams).values(&new_team).execute(&mut conn)?;
    Ok(Team::from(new_team))
}

pub fn join_team(team: Team, user: User) -> Result<(), Error> {
    let mut conn = establish_connection().expect("Failed to get a DB connection from the pool");
    diesel::update(teams.filter(id.eq(team.id.clone())))
        .set(partner.eq(user.id))
        .execute(&mut conn)?;
    Ok(())
}

pub fn get_team_by_id(uid: String) -> Result<Team, Error> {
    let mut conn = establish_connection().expect("Failed to get a DB connection from the pool");
    match teams.filter(id.eq(uid)).first::<SqlTeam>(&mut conn) {
        Ok(t) => Ok(Team::from(t)),
        Err(e) => Err(e),
    }
}

pub fn get_team_by_student_for_competition(user: User, comp_id: String) -> Result<Team, Error> {
    let mut conn = establish_connection().expect("Failed to get a DB connection from the pool");
    match teams
        .filter(competition_id.eq(comp_id))
        .filter(owner.eq(user.id.clone()).or(partner.eq(user.id.clone())))
        .first::<SqlTeam>(&mut conn)
    {
        Ok(t) => Ok(Team::from(t)),
        Err(e) => Err(e),
    }
}

pub fn get_team_by_student(user: User) -> Result<Vec<Team>, Error> {
    let mut conn = establish_connection().expect("Failed to get a DB connection from the pool");
    match teams
        .filter(owner.eq(user.id.clone()).or(partner.eq(user.id.clone())))
        .load::<SqlTeam>(&mut conn)
    {
        Ok(t) => Ok(t.into_iter().map(Team::from).collect::<Vec<Team>>()),
        Err(e) => Err(e),
    }
}

pub fn get_teams_by_competition_id(com_id: &String) -> Result<Vec<Team>, Error> {
    let mut conn = establish_connection().expect("Failed to get a DB connection from the pool");
    match teams
        .filter(competition_id.eq(com_id))
        .load::<SqlTeam>(&mut conn)
    {
        Ok(t) => Ok(t.into_iter().map(Team::from).collect::<Vec<Team>>()),
        Err(e) => Err(e),
    }
}

pub fn leave_team(team: Team, user: User) -> Result<(), Error> {
    let mut conn = establish_connection().expect("Failed to get a DB connection from the pool");
    diesel::update(teams.filter(id.eq(team.id).and(partner.eq(user.id))))
        .set(partner.eq(""))
        .execute(&mut conn)?;
    Ok(())
}

pub fn kick_partner(team: Team, user: User) -> Result<(), Error> {
    let mut conn = establish_connection().expect("Failed to get a DB connection from the pool");
    diesel::update(teams.filter(id.eq(team.id).and(owner.eq(user.id))))
        .set(partner.eq(""))
        .execute(&mut conn)?;
    Ok(())
}

pub fn disband_team(team: Team, user: User) -> Result<(), Error> {
    let mut conn = establish_connection().expect("Failed to get a DB connection from the pool");
    diesel::delete(teams.filter(id.eq(team.id).and(owner.eq(user.id)))).execute(&mut conn)?;
    Ok(())
}

pub fn is_member_of_a_team(user: User) -> bool {
    let mut conn = establish_connection().expect("Failed to get a DB connection from the pool");
    match teams
        .filter(owner.eq(user.id.clone()).or(partner.eq(user.id.clone())))
        .first::<SqlTeam>(&mut conn)
    {
        Ok(_) => true,
        Err(_) => false,
    }
}

pub fn is_member_of_a_team_on_competition(user: User, comp_id: String) -> bool {
    let mut conn = establish_connection().expect("Failed to get a DB connection from the pool");
    match teams
        .filter(competition_id.eq(comp_id))
        .filter(owner.eq(user.id.clone()).or(partner.eq(user.id.clone())))
        .first::<SqlTeam>(&mut conn)
    {
        Ok(_) => true,
        Err(_) => false,
    }
}

pub fn set_team_name(team: &Team, new_name: String) -> Result<Team, Error> {
    let mut conn = establish_connection().expect("Failed to get a DB connection from the pool");
    let _ = diesel::update(teams.filter(id.eq(team.id.clone())))
        .set(name.eq(new_name))
        .execute(&mut conn)?;
    get_team_by_id(team.id.clone())
}

pub fn set_team_bot(team: &Team, bot: BotSelector, bot_id: String) -> Result<(), Error> {
    let mut conn = establish_connection().expect("Failed to get a DB connection from the pool");
    let builder = diesel::update(teams.filter(id.eq(team.id.clone())));
    match bot {
        BotSelector::First => builder.set(bot1.eq(bot_id)).execute(&mut conn)?,
        BotSelector::Second => builder.set(bot2.eq(bot_id)).execute(&mut conn)?,
    };
    Ok(())
}

pub fn add_elo_change(tid: String, elo_change: i32) -> Result<(), Error> {
    let mut conn = establish_connection().expect("Failed to get a DB connection from the pool");
    let team: SqlTeam = teams.find(tid.clone()).first(&mut conn)?;
    // Calculate the new ELO
    let new_elo = team.elo + elo_change;
    // Update the team's ELO in the database
    diesel::update(teams.find(tid))
        .set(elo.eq(new_elo))
        .execute(&mut conn)?;
    Ok(())
}
