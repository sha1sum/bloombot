use std::time::{Duration, Instant};

use anyhow::{Context as AnyhowContext, Result};
use log::info;
use pgvector::Vector;
use poise::serenity_prelude::{builder::*, ChannelId, ComponentInteractionCollector};
use poise::CreateReply;

use crate::config::{BloomBotEmbed, CHANNELS, ENTRIES_PER_PAGE};
use crate::database::DatabaseHandler;
// use crate::pagination::{PageRowRef, Pagination};
use crate::Context;

/// Glossary commands
///
/// Commands for interacting with the glossary.
///
/// Get `info` on a glossary entry, see a `list` of entries, `search` for a relevant entry, or `suggest` a term for addition.
#[poise::command(
  slash_command,
  category = "Informational",
  subcommands("list", "info", "search", "suggest"),
  subcommand_required,
  guild_only
)]
#[allow(clippy::unused_async)]
pub async fn glossary(_: Context<'_>) -> Result<()> {
  Ok(())
}

/// See a list of all glossary entries
///
/// Shows a list of all glossary entries.
#[poise::command(slash_command)]
async fn list(
  ctx: Context<'_>,
  #[description = "The page to show"] page: Option<usize>,
) -> Result<()> {
  let data = ctx.data();

  let guild_id = ctx
    .guild_id()
    .with_context(|| "Failed to retrieve guild ID from context")?;

  let mut transaction = data.db.start_transaction_with_retry(5).await?;
  let term_names = DatabaseHandler::get_term_list(&mut transaction, &guild_id).await?;

  let term_count = term_names.len();
  let mut sorted_terms = Vec::<(String, String)>::with_capacity(term_count);

  for term in term_names {
    let char = match term.name.chars().next() {
      Some(char) => char.to_string(),
      None => String::new(),
    };
    let mut full_term = term.name.clone();
    let aliases = term.aliases.clone().unwrap_or(Vec::new());
    if !aliases.is_empty() {
      full_term.push_str(" (");
      let alias_count = aliases.len();
      for (i, alias) in aliases.iter().enumerate() {
        full_term.push_str(alias);
        if i < (alias_count - 1) {
          full_term.push_str(", ");
        }
      }
      full_term.push(')');
    }
    sorted_terms.push((char, full_term));
  }

  let terms_per_page = ENTRIES_PER_PAGE.glossary;
  let mut pages: Vec<Vec<(String, String)>> = vec![];
  while !sorted_terms.is_empty() {
    let mut page = vec![];
    for _i in 1..=terms_per_page {
      if sorted_terms.is_empty() {
        break;
      }
      if let Some(term) = sorted_terms.pop() {
        page.push(term);
      }
    }
    pages.push(page);
  }

  let mut letter: &str;
  let mut page_text: String;
  let mut all_pages = vec![];
  let mut total_pages = 0;

  for page in pages {
    letter = &page[0].0;
    page_text = format!(
      "-# Terms in parentheses are aliases for the preceding term. Use </glossary info:1135659962308243479> with any term or alias to read the full entry.\n\n-# {letter}\n"
    );
    for entry in &page {
      if entry.0 == letter {
        page_text.push_str(format!("- {}\n", entry.1).as_str());
      } else {
        page_text.push_str(format!("-# {}\n- {}\n", entry.0, entry.1).as_str());
        letter = &entry.0;
      }
    }
    page_text.push_str("** **\n\n");
    all_pages.push(page_text);
    total_pages += 1;
  }

  let ctx_id = ctx.id();
  let prev_button_id = format!("{ctx_id}prev");
  let next_button_id = format!("{ctx_id}next");

  let mut current_page = page.unwrap_or(0).saturating_sub(1);

  // Send the embed with the first page as content
  let reply = {
    let components = CreateActionRow::Buttons(vec![
      CreateButton::new(&prev_button_id).label("Previous"),
      CreateButton::new(&next_button_id).label("Next"),
    ]);

    CreateReply::default()
      .embed(
        BloomBotEmbed::new()
          .title("List of Glossary Terms")
          .description(&all_pages[current_page])
          .footer(CreateEmbedFooter::new(format!(
            "Page {} of {total_pages}・Terms {}-{}・Total Terms: {term_count}",
            current_page + 1,
            current_page * terms_per_page + 1,
            if (term_count / ((current_page + 1) * terms_per_page)) > 0 {
              (current_page + 1) * terms_per_page
            } else {
              term_count
            },
          ))),
      )
      .components(vec![components])
  };

  ctx.send(reply).await?;

  // Loop through incoming interactions with the navigation buttons
  while let Some(press) = ComponentInteractionCollector::new(ctx)
    // We defined our button IDs to start with `ctx_id`. If they don't, some other command's
    // button was pressed
    .filter(move |press| press.data.custom_id.starts_with(&ctx_id.to_string()))
    // Timeout when no navigation button has been pressed for 24 hours
    .timeout(Duration::from_secs(3600 * 24))
    .await
  {
    // Depending on which button was pressed, go to next or previous page
    if press.data.custom_id == next_button_id {
      current_page += 1;
      if current_page >= all_pages.len() {
        current_page = 0;
      }
    } else if press.data.custom_id == prev_button_id {
      current_page = current_page.checked_sub(1).unwrap_or(all_pages.len() - 1);
    } else {
      // This is an unrelated button interaction
      continue;
    }

    // Update the message with the new page contents
    press
      .create_response(
        ctx.serenity_context(),
        CreateInteractionResponse::UpdateMessage(
          CreateInteractionResponseMessage::new().embed(
            BloomBotEmbed::new()
              .title("List of Glossary Terms")
              .description(&all_pages[current_page])
              .footer(CreateEmbedFooter::new(format!(
                "Page {} of {total_pages}・Terms {}-{}・Total Terms: {term_count}",
                current_page + 1,
                current_page * terms_per_page + 1,
                if (term_count / ((current_page + 1) * terms_per_page)) > 0 {
                  (current_page + 1) * terms_per_page
                } else {
                  term_count
                },
              ))),
          ),
        ),
      )
      .await?;
  }

  Ok(())
}

/// See information about a glossary entry
///
/// Shows information about a glossary entry.
#[poise::command(slash_command)]
async fn info(
  ctx: Context<'_>,
  #[description = "The term to show information about"] term: String,
) -> Result<()> {
  let guild_id = ctx
    .guild_id()
    .with_context(|| "Failed to retrieve guild ID from context")?;

  let mut transaction = ctx.data().db.start_transaction_with_retry(5).await?;

  let term_info = DatabaseHandler::get_term(&mut transaction, &guild_id, term.as_str()).await?;
  let mut embed = BloomBotEmbed::new();

  if let Some(term_info) = term_info {
    embed = embed.title(term_info.name).description(term_info.meaning);
    let usage = term_info.usage.unwrap_or(String::new());
    if !usage.is_empty() {
      embed = embed.field("Example of Usage:", usage, false);
    }
    let links = term_info.links.unwrap_or(Vec::new());
    if !links.is_empty() {
      embed = embed.field(
        "Related Resources:",
        {
          let mut field = String::new();
          let mut count = 1;

          for link in links {
            field.push_str(&format!("{count}. {link}\n"));
            count += 1;
          }

          field
        },
        false,
      );
    }
    let aliases = term_info.aliases.clone().unwrap_or(Vec::new());
    if !aliases.is_empty() {
      embed = embed.field(
        "Aliases:",
        {
          let mut field = String::new();
          let alias_count = aliases.len();

          for (i, alias) in aliases.iter().enumerate() {
            field.push_str(alias);
            if i < (alias_count - 1) {
              field.push_str(", ");
            }
          }

          field
        },
        false,
      );
    }
    let category = term_info.category.unwrap_or(String::new());
    if !category.is_empty() {
      embed = embed.footer(CreateEmbedFooter::new(format!("Categories: {category}")));
    }
  } else {
    let possible_terms =
      DatabaseHandler::get_possible_terms(&mut transaction, &guild_id, term.as_str(), 0.7).await?;

    if possible_terms.len() == 1 {
      let possible_term = possible_terms
        .first()
        .with_context(|| "Failed to retrieve first element of possible_terms")?;

      embed = embed
        .title(&possible_term.name)
        .description(&possible_term.meaning);
      let usage = possible_term.usage.clone().unwrap_or(String::new());
      if !usage.is_empty() {
        embed = embed.field("Example of Usage:", usage, false);
      }
      let links = possible_term.links.clone().unwrap_or(Vec::new());
      if !links.is_empty() {
        embed = embed.field(
          "Related Resources:",
          {
            let mut field = String::new();
            let mut count = 1;

            for link in links {
              field.push_str(&format!("{count}. {link}\n"));
              count += 1;
            }

            field
          },
          false,
        );
      }
      let aliases = possible_term.aliases.clone().unwrap_or(Vec::new());
      if !aliases.is_empty() {
        embed = embed.field(
          "Aliases:",
          {
            let mut field = String::new();
            let alias_count = aliases.len();

            for (i, alias) in aliases.iter().enumerate() {
              field.push_str(alias);
              if i < (alias_count - 1) {
                field.push_str(", ");
              }
            }

            field
          },
          false,
        );
      }
      let category = possible_term.category.clone().unwrap_or(String::new());
      if category.is_empty() {
        embed = embed.footer(CreateEmbedFooter::new(format!(
          "*You searched for '{}'. The closest term available was '{}'.",
          term, possible_term.name
        )));
      } else {
        embed = embed.footer(CreateEmbedFooter::new(format!(
          "Categories: {}\n\n*You searched for '{}'. The closest term available was '{}'.",
          category, term, possible_term.name
        )));
      }
    } else if possible_terms.is_empty() {
      embed = embed
        .title("Term not found")
        .description(format!("The term `{term}` was not found in the glossary."));
    } else {
      embed = embed
        .title("Term not found")
        .description(format!("The term `{term}` was not found in the glossary."));

      embed = embed.field(
        "Did you mean one of these?",
        {
          let mut field = String::new();

          for possible_term in possible_terms.iter().take(3) {
            field.push_str(&format!("`{}`\n", possible_term.name));
          }

          field.push_str(
            "\n\n*Try using </glossary search:1135659962308243479> to take advantage of a more powerful search.*",
          );

          field
        },
        false,
      );
    }
  }

  ctx.send(CreateReply::default().embed(embed)).await?;

  Ok(())
}

/// Search glossary entries using keywords or phrases
///
/// Searches glossary entries using keywords or phrases, leveraging AI to find the closest matches.
#[poise::command(slash_command)]
async fn search(
  ctx: Context<'_>,
  #[description = "The term to search for"] search: String,
) -> Result<()> {
  ctx.defer().await?;

  let data = ctx.data();

  let guild_id = ctx
    .guild_id()
    .with_context(|| "Failed to retrieve guild ID from context")?;

  let start_time = Instant::now();
  let mut transaction = data.db.start_transaction_with_retry(5).await?;
  let vector = Vector::from(
    data
      .embeddings
      .create_embedding(search.clone(), ctx.author().id)
      .await?,
  );
  let possible_terms =
    DatabaseHandler::search_terms_by_vector(&mut transaction, &guild_id, &vector, 3).await?;
  let search_time = start_time.elapsed();

  let mut embed = BloomBotEmbed::new();
  let mut terms_returned = 0;
  embed = embed.title(format!("Search results for `{search}`"));

  if possible_terms.is_empty() {
    embed =
      embed.description("No terms were found. Try browsing the glossary with `/glossary list`.");
  } else {
    for (index, possible_term) in possible_terms.iter().enumerate() {
      // Set threshold for terms to include
      if possible_term.distance_score.unwrap_or(1.0) > 0.3 {
        continue;
      }
      let relevance_description = match possible_term.distance_score {
        Some(score) => {
          let similarity_score = (1.0 - score) * 100.0;
          info!(
            "Term {} has a similarity score of {}",
            index + 1,
            similarity_score
          );
          match similarity_score.round() {
            100.0..=f64::MAX => "Exact match",
            // Adjust for cosine similarity
            90.0..=99.0 => "High",
            80.0..=89.0 => "Medium",
            70.0..=79.0 => "Low",
            // 80..=99 => "Very similar",
            // 60..=79 => "Similar",
            // 40..=59 => "Somewhat similar",
            // 20..=39 => "Not very similar",
            // 0..=19 => "Not similar",
            _ => "Unknown",
          }
        }
        None => "Unknown",
      };

      // If longer than 1024 (embed field max) - 45 (relevance message),
      // truncate to 979 - 3 for "..."
      let meaning = if possible_term.meaning.len() > 979 {
        format!(
          "{}...",
          possible_term.meaning.chars().take(976).collect::<String>()
        )
      } else {
        possible_term.meaning.clone()
      };

      embed = embed.field(
        format!("Term {}: `{}`", index + 1, &possible_term.term_name),
        format!(
          // "```{meaning}```\n> Estimated relevance: *{relevance_description}*"
          "{meaning}\n```Estimated relevance: {relevance_description}```\n** **"
        ),
        false,
      );

      terms_returned += 1;
    }
  }

  embed = embed.footer(CreateEmbedFooter::new(format!(
    "Search took {}ms",
    search_time.as_millis()
  )));

  if terms_returned == 0 {
    embed =
      embed.description("No terms were found. Try browsing the glossary with `/glossary list`.");
  }

  ctx.send(CreateReply::default().embed(embed)).await?;

  Ok(())
}

/// Suggest a term for the glossary
///
/// Suggest a term for addition to the glossary.
#[poise::command(slash_command)]
async fn suggest(
  ctx: Context<'_>,
  #[description = "Term you wish to suggest"] suggestion: String,
) -> Result<()> {
  let log_embed = BloomBotEmbed::new()
    .title("Term Suggestion")
    .description(format!("**Suggestion**: {suggestion}"))
    .footer(
      CreateEmbedFooter::new(format!(
        "Suggested by {} ({})",
        ctx.author().name,
        ctx.author().id
      ))
      .icon_url(ctx.author().avatar_url().unwrap_or_default()),
    )
    .clone();

  let log_channel = ChannelId::new(CHANNELS.bloomlogs);

  log_channel
    .send_message(ctx, CreateMessage::new().embed(log_embed))
    .await?;

  ctx
    .send(
      CreateReply::default()
        .content("Your suggestion has been submitted. Thank you!")
        .ephemeral(true),
    )
    .await?;

  Ok(())
}
