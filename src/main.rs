mod database;
mod config;
mod r#struct;
mod reset;

use crate::config::config;
use crate::database::connect_db;
use crate::r#struct::Notes;
use serde_json::json;
use meilisearch_sdk::{client::*};
use crossterm::{cursor::MoveToColumn, execute, style::{Color, Print, ResetColor, SetForegroundColor}, terminal::{Clear, ClearType}};
use std::{error::Error, io, sync::Mutex};
use crate::reset::reset;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = config().unwrap_or_else(|err| {
        println!("Configuration error: {}", err);
        std::process::exit(1);
    });
    let db = connect_db().await.unwrap();

    let protocol = if config.meili.ssl { "https" } else { "http" };
    let url = format!("{}://{}:{}", protocol, config.meili.host, config.meili.port);
    println!("Connecting to MeiliSearch at {}", url);

    let api_key = config.meili.apikey.clone();
    let client = Client::new(&url, if api_key.is_empty() { None } else { Some(api_key) });

    if config.meili.reset {
        reset(&client, &url, &config).await;
    } else {
        println!("Skipping reset");
    }

    let rows = db
        .query(
            "SELECT id, \"createdAt\", \"userId\", \"userHost\", \"channelId\", cw, text FROM note WHERE visibility NOT IN ('followers', 'specified') AND text IS NOT NULL ORDER BY \"createdAt\" DESC",
            &[],
        )
        .await?;
    let rows_len = rows.len();

    let mut data_vec = Vec::new();

    for row in rows {
        let notes = Notes {
            id: row.get("id"),
            created_at: row.get("createdAt"),
            user_id: row.get("userId"),
            user_host: row.get("userHost"),
            channel_id: row.get("channelId"),
            cw: row.get("cw"),
            text: row.get("text"),
        };

        let timestamp = notes.created_at.timestamp_millis();
        let data = json!({
        "id": notes.id,
        "createdAt": timestamp,
        "userId": notes.user_id,
        "userHost": notes.user_host,
        "channelId": notes.channel_id,
        "cw": notes.cw,
        "text": notes.text,
    });

        data_vec.push(data);
    }

    let mut stdout = io::stdout();
    let errors = Mutex::new(Vec::new());

    for (count, data) in data_vec.iter().enumerate() {
        let clear = Clear(ClearType::CurrentLine);
        let move_to_col = MoveToColumn(0);

        execute!(
            stdout,
            clear,
            move_to_col,
            SetForegroundColor(Color::Green),
            Print(format!("Count: {}, id: {}, createdAt: {}", count + 1, data["id"], data["createdAt"])),
            ResetColor,
        )?;

        let index = client.index("notes");
        let res = index.add_documents(&[data], Some("id")).await;

        match res {
            Ok(res) => {
                execute!(
                    stdout,
                    clear,
                    move_to_col,
                    SetForegroundColor(Color::Cyan),
                    Print(format!("Add documents: {:?}\n", res.enqueued_at)),
                    ResetColor,
                )?;
            }
            Err(e) => {
                execute!(
                    stdout,
                    clear,
                    move_to_col,
                    SetForegroundColor(Color::Red),
                    Print(format!("add_documents error: {:?}", e)),
                    ResetColor,
                )?;
                errors.lock().unwrap().push(e);
            }
        }
    }

    let errors = errors.into_inner().unwrap();
    if !errors.is_empty() {
        println!("{} errors occurred", errors.len());
        for error in errors {
            let timestamp = chrono::Utc::now().timestamp_millis();
            std::fs::write(format!("error-{}.log", timestamp), format!("{:?}", error)).unwrap();
            println!("{:?}", error);
        }
    }

    println!("Done: {}", rows_len);

    Ok(())
}
