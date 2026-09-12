use serde_json::json;
use shprd_agent::{Attachment, Command};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = shprd_agent::default_directory()?;
    let listed = shprd_agent::list(&directory).await?;
    let sessions = listed["sessions"].as_array().ok_or("invalid catalog")?;
    if sessions.len() != 1 {
        return Err("fixture requires exactly one session".into());
    }
    let id = sessions[0]["id"].as_str().ok_or("missing session id")?;
    let mut attachment = Attachment::connect(&directory, id).await?;
    let messages = attachment.request(&Command::GetMessages, |_| {}).await?;
    if messages["messages"][0]["content"] != "fixture history" {
        return Err("wrong history".into());
    }
    let models = attachment
        .request(&Command::GetAvailableModels, |_| {})
        .await?;
    if models["models"][0]["modelId"] != "free" {
        return Err("wrong model".into());
    }
    let result = attachment
        .request(
            &Command::Prompt {
                message: "fixture-only".into(),
            },
            |_| {},
        )
        .await?;
    if result["accepted"] != true {
        return Err("prompt not admitted".into());
    }
    let mut events = Vec::new();
    attachment
        .request(&Command::Abort, |event| events.push(event))
        .await?;
    if events[0]["agent_event"]["event"]["type"] != "abort_requested" {
        return Err("missing abort event".into());
    }
    println!("{}", json!({"probe":"ok","session_id":id}));
    Ok(())
}
