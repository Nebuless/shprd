use shprd_history::{Agent, FileMeta, HistoryService, HostConfig, PaneMetadata, Update};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = concat!(
        "{\"type\":\"session\",\"id\":\"example-session\"}\n",
        "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"inspect\"}}\n",
        "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"toolCall\",\"id\":\"read-1\",\"name\":\"read\",\"arguments\":{\"path\":\"README.md\"}}]}}\n",
        "{\"type\":\"message\",\"message\":{\"role\":\"toolResult\",\"toolCallId\":\"read-1\",\"toolName\":\"read\",\"content\":\"contents\"}}\n",
    );
    let service = HistoryService::new(HostConfig::default());
    let file = FileMeta::fixture("/fixture/example.jsonl", source, 1);
    let pane = PaneMetadata::new("pane", "workspace", "tab", Agent::Pi, &file.path);
    let projection = service.project_text(&pane, file, source)?;
    let export = service.atif_json(&projection)?;
    assert_eq!(export["schema_version"], "ATIF-v1.7");
    assert_eq!(export["session_id"], "example-session");
    assert_eq!(export["agent"]["name"], "pi");
    let Update::Snapshot { entries, .. } = service.history_snapshot(&projection) else {
        return Err("expected history snapshot".into());
    };
    let tool = entries
        .iter()
        .find(|entry| entry.kind == "tool_result")
        .ok_or("expected tool result")?;
    assert_eq!(tool.text, "");
    assert_eq!(tool.text_bytes, Some("contents".len()));
    println!(
        "ATIF-v1.7 pi example-session {}",
        projection.v1_messages[0].text
    );
    Ok(())
}
