use anyhow::{Context as AnyhowContext, Result};
use poise::serenity_prelude::Role;
use poise::CreateReply;

use crate::commands::helpers::common::Visibility;
use crate::commands::helpers::courses;
use crate::commands::helpers::database::{self, MessageType};
use crate::commands::helpers::pagination::{PageRowRef, PageType, Paginator};
use crate::config::{EMOJI, ENTRIES_PER_PAGE};
use crate::data::course::Course;
use crate::database::DatabaseHandler;
use crate::Context;

/// Commands for managing courses
///
/// Commands to add, edit, list, or remove courses.
///
/// Requires `Administrator` permissions.
#[poise::command(
  slash_command,
  required_permissions = "ADMINISTRATOR",
  default_member_permissions = "ADMINISTRATOR",
  category = "Admin Commands",
  subcommands("add", "remove", "edit", "list"),
  subcommand_required,
  guild_only
)]
#[allow(clippy::unused_async)]
pub async fn courses(_: Context<'_>) -> Result<()> {
  Ok(())
}

/// Add a course and its associated graduate role to the database
///
/// Adds a course and its associated graduate role to the database.
#[poise::command(slash_command)]
async fn add(
  ctx: Context<'_>,
  #[description = "Name of the course"] course_name: String,
  #[description = "Role course participants are assumed to have"] participant_role: Role,
  #[description = "Role to be given to graduates"] graduate_role: Role,
) -> Result<()> {
  ctx.defer_ephemeral().await?;

  let guild_id = ctx
    .guild_id()
    .with_context(|| "Failed to retrieve guild ID from context")?;

  let mut transaction = ctx.data().db.start_transaction_with_retry(5).await?;
  if DatabaseHandler::course_exists(&mut transaction, &guild_id, course_name.as_str()).await? {
    ctx
      .say(format!("{} Course already exists.", EMOJI.mminfo))
      .await?;
    return Ok(());
  }

  // Verify that the roles are in the guild
  if !participant_role.guild_id.eq(&guild_id) {
    ctx
      .say(format!(
        "{} The participant role must be in the same guild as the command.",
        EMOJI.mminfo
      ))
      .await?;
    return Ok(());
  }
  if !graduate_role.guild_id.eq(&guild_id) {
    ctx
      .say(format!(
        "{} The graduate role must be in the same guild as the command.",
        EMOJI.mminfo
      ))
      .await?;
    return Ok(());
  }

  // Verify that the roles are not managed by an integration
  if participant_role.managed {
    ctx
      .say(format!(
        "{} The participant role must not be a bot role.",
        EMOJI.mminfo
      ))
      .await?;
    return Ok(());
  }
  if graduate_role.managed {
    ctx
      .say(format!(
        "{} The graduate role must not be a bot role.",
        EMOJI.mminfo
      ))
      .await?;
    return Ok(());
  }

  // Verify that the roles are not privileged
  if participant_role.permissions.administrator() {
    ctx
      .say(format!(
        "{} The participant role must not be an administrator role.",
        EMOJI.mminfo
      ))
      .await?;
    return Ok(());
  }
  if graduate_role.permissions.administrator() {
    ctx
      .say(format!(
        "{} The graduate role must not be an administrator role.",
        EMOJI.mminfo
      ))
      .await?;
    return Ok(());
  }

  if participant_role == graduate_role {
    ctx
      .say(format!(
        "{} The participant role and the graduate role must not be the same.",
        EMOJI.mminfo
      ))
      .await?;
    return Ok(());
  }

  let course = Course::new(course_name, participant_role.id, graduate_role.id, guild_id);

  DatabaseHandler::add_course(&mut transaction, &course).await?;

  database::commit_and_say(
    ctx,
    transaction,
    MessageType::TextOnly(format!("{} Course has been added.", EMOJI.mmcheck)),
    Visibility::Ephemeral,
  )
  .await?;

  Ok(())
}

/// Update the roles for an existing course
///
/// Updates the roles for an existing course.
#[poise::command(slash_command)]
async fn edit(
  ctx: Context<'_>,
  #[description = "Name of the course"] course_name: String,
  #[description = "Role course participants are assumed to have"] participant_role: Option<Role>,
  #[description = "Role to be given to graduates"] graduate_role: Option<Role>,
) -> Result<()> {
  ctx.defer_ephemeral().await?;

  if participant_role.is_none() && graduate_role.is_none() {
    ctx
      .send(
        CreateReply::default()
          .content(format!("{} No changes were provided.", EMOJI.mminfo))
          .ephemeral(true),
      )
      .await?;
    return Ok(());
  }

  let guild_id = ctx
    .guild_id()
    .with_context(|| "Failed to retrieve guild ID from context")?;

  let mut transaction = ctx.data().db.start_transaction_with_retry(5).await?;
  let course =
    DatabaseHandler::get_course(&mut transaction, &guild_id, course_name.as_str()).await?;

  // Verify that the course exists
  if course.is_none() {
    courses::course_not_found(ctx, &mut transaction, guild_id, course_name).await?;
    return Ok(());
  }

  let course = course.with_context(|| "Failed to assign CourseData to course")?;

  let participant_role = match participant_role {
    Some(participant_role) => {
      if !participant_role.guild_id.eq(&guild_id) {
        ctx
          .say(format!(
            "{} The participant role must be in the same guild as the command.",
            EMOJI.mminfo
          ))
          .await?;
        return Ok(());
      }
      if participant_role.managed {
        ctx
          .say(format!(
            "{} The participant role must not be a bot role.",
            EMOJI.mminfo
          ))
          .await?;
        return Ok(());
      }
      if participant_role.permissions.administrator() {
        ctx
          .say(format!(
            "{} The participant role must not be an administrator role.",
            EMOJI.mminfo
          ))
          .await?;
        return Ok(());
      }
      participant_role.id
    }
    None => course.participant_role,
  };

  let graduate_role = match graduate_role {
    Some(graduate_role) => {
      if !graduate_role.guild_id.eq(&guild_id) {
        ctx
          .say(format!(
            "{} The graduate role must be in the same guild as the command.",
            EMOJI.mminfo
          ))
          .await?;
        return Ok(());
      }
      if graduate_role.managed {
        ctx
          .say(format!(
            "{} The graduate role must not be a bot role.",
            EMOJI.mminfo
          ))
          .await?;
        return Ok(());
      }
      if graduate_role.permissions.administrator() {
        ctx
          .say(format!(
            "{} The graduate role must not be an administrator role.",
            EMOJI.mminfo
          ))
          .await?;
        return Ok(());
      }
      graduate_role.id
    }
    None => course.graduate_role,
  };

  // Verify that the roles are not the same
  if participant_role == graduate_role {
    ctx
      .say(format!(
        "{} The participant role and the graduate role must not be the same.",
        EMOJI.mminfo
      ))
      .await?;
    return Ok(());
  }

  let course = Course::new(course_name, participant_role, graduate_role, guild_id);

  DatabaseHandler::update_course(&mut transaction, &course).await?;

  database::commit_and_say(
    ctx,
    transaction,
    MessageType::TextOnly(format!("{} Course roles have been updated.", EMOJI.mmcheck)),
    Visibility::Ephemeral,
  )
  .await?;

  Ok(())
}

/// List all courses
///
/// Lists all courses in the database.
#[poise::command(slash_command)]
async fn list(
  ctx: Context<'_>,
  #[description = "The page to show"] page: Option<usize>,
) -> Result<()> {
  let guild_id = ctx
    .guild_id()
    .with_context(|| "Failed to retrieve guild ID from context")?;

  let mut transaction = ctx.data().db.start_transaction_with_retry(5).await?;

  let courses = DatabaseHandler::get_all_courses(&mut transaction, &guild_id).await?;
  let courses: Vec<PageRowRef> = courses.iter().map(|course| course as PageRowRef).collect();

  drop(transaction);

  Paginator::new("Courses", &courses, ENTRIES_PER_PAGE.default)
    .paginate(ctx, page, PageType::Standard, Visibility::Ephemeral)
    .await?;

  Ok(())
}

/// Remove a course from the database
///
/// Removes a course from the database.
#[poise::command(slash_command)]
async fn remove(
  ctx: Context<'_>,
  #[description = "Name of the course"] course_name: String,
) -> Result<()> {
  ctx.defer_ephemeral().await?;

  let guild_id = ctx
    .guild_id()
    .with_context(|| "Failed to retrieve guild ID from context")?;

  let mut transaction = ctx.data().db.start_transaction_with_retry(5).await?;
  if !DatabaseHandler::course_exists(&mut transaction, &guild_id, course_name.as_str()).await? {
    ctx
      .say(format!("{} Course does not exist.", EMOJI.mminfo))
      .await?;
    return Ok(());
  }

  DatabaseHandler::remove_course(&mut transaction, &guild_id, course_name.as_str()).await?;

  database::commit_and_say(
    ctx,
    transaction,
    MessageType::TextOnly(format!("{} Course has been removed.", EMOJI.mmcheck)),
    Visibility::Ephemeral,
  )
  .await?;

  Ok(())
}
