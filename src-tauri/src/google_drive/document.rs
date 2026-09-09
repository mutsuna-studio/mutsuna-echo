use crate::transcript_store::SummaryTranscriptSnapshot;
use serde_json::{json, Value};

pub(super) fn transcript(title: &str, snapshot: Option<&SummaryTranscriptSnapshot>) -> String {
    let mut text = format!("{title}\n文字起こし\n\n");
    match snapshot {
        Some(snapshot) => {
            for segment in &snapshot.segments {
                let seconds = segment.start_ms / 1000;
                text.push_str(&format!(
                    "[{:02}:{:02}:{:02}] {}\n{}\n\n",
                    seconds / 3600,
                    seconds / 60 % 60,
                    seconds % 60,
                    segment.speaker,
                    segment.text
                ));
            }
        }
        None => text.push_str("文字起こしはまだ作成されていません。\n"),
    }
    text
}

pub(super) fn notes(title: &str, note: Option<&Value>) -> String {
    let mut text = format!("{title}\n会議ノート\n\n");
    let Some(note) = note else {
        text.push_str("会議ノートはまだ作成されていません。\n");
        return text;
    };
    for key in ["oneLine", "overview"] {
        if let Some(s) = note["summary"][key].as_str().filter(|s| !s.is_empty()) {
            text.push_str(s);
            text.push_str("\n\n");
        }
    }
    for (heading, items, fields) in [
        ("要点", &note["summary"]["keyPoints"], &["text"][..]),
        (
            "参加者",
            &note["participants"],
            &["displayName", "role", "organization"],
        ),
        ("議題", &note["topics"], &["title", "summary"]),
        ("決定事項", &note["decisions"], &["statement", "rationale"]),
        (
            "アクション項目",
            &note["actionItems"],
            &["title", "description", "status"],
        ),
        ("未解決事項", &note["openIssues"], &["title", "description"]),
        ("質問", &note["questions"], &["text"]),
        ("メモ", &note["notes"], &["title", "body"]),
    ] {
        let Some(items) = items.as_array() else {
            continue;
        };
        let active: Vec<_> = items
            .iter()
            .filter(|v| v["recordMeta"]["lifecycle"] != "deleted")
            .collect();
        if active.is_empty() {
            continue;
        }
        text.push_str(heading);
        text.push('\n');
        for item in active {
            let parts: Vec<_> = fields
                .iter()
                .filter_map(|key| item[key].as_str())
                .filter(|s| !s.is_empty())
                .collect();
            text.push_str("• ");
            text.push_str(&parts.join(" — "));
            text.push('\n');
            if let Some(assignees) = item["assigneeParticipantIds"].as_array() {
                let names: Vec<_> = assignees
                    .iter()
                    .filter_map(|id| {
                        note["participants"]
                            .as_array()?
                            .iter()
                            .find(|p| &p["participantId"] == id)?["displayName"]
                            .as_str()
                    })
                    .collect();
                if !names.is_empty() {
                    text.push_str(&format!("  担当: {}\n", names.join("、")));
                }
            }
            if let Some(due) = item["due"]["raw"]
                .as_str()
                .or_else(|| item["due"]["rawText"].as_str())
            {
                text.push_str(&format!("  期限: {due}\n"));
            }
            if let Some(answer) = item["answer"]["text"].as_str() {
                text.push_str(&format!("  回答: {answer}\n"));
            }
        }
        text.push('\n');
    }
    text
}

pub(super) fn replace_tabs(
    doc: &Value,
    note_id: &str,
    note: &str,
    transcript_id: &str,
    transcript: &str,
) -> Result<Value, String> {
    if note_id == transcript_id {
        return Err("保存先のタブが重複しています。別版として保存してください。".into());
    }
    let revision = doc["revisionId"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("ドキュメントの更新状態を確認できません。")?;
    let tabs = doc["tabs"]
        .as_array()
        .ok_or("保存先のタブを確認できません。")?;
    let mut requests = vec![];
    for (id, text) in [(note_id, note), (transcript_id, transcript)] {
        let tab = tabs
            .iter()
            .find(|t| t["tabProperties"]["tabId"] == id)
            .ok_or("保存先のタブが移動・削除されています。別版として保存してください。")?;
        let end = tab["documentTab"]["body"]["content"]
            .as_array()
            .and_then(|items| items.last())
            .and_then(|item| item["endIndex"].as_u64())
            .filter(|n| *n >= 2)
            .ok_or("保存先の本文を確認できません。")?;
        if end > 2 {
            requests.push(json!({"deleteContentRange":{"range":{"tabId":id,"startIndex":1,"endIndex":end - 1}}}));
        }
        // Google Docs indexes are UTF-16 code units. Body replacement leaves the final newline.
        let text: String = text
            .chars()
            .filter(|c| *c == '\n' || *c == '\t' || !c.is_control())
            .collect();
        requests.push(json!({"insertText":{"location":{"tabId":id,"index":1},"text":text}}));
        let title_end = text
            .lines()
            .next()
            .unwrap_or_default()
            .encode_utf16()
            .count()
            + 1;
        if title_end > 1 {
            requests.push(json!({"updateParagraphStyle":{"range":{"tabId":id,"startIndex":1,"endIndex":title_end},"paragraphStyle":{"namedStyleType":"TITLE"},"fields":"namedStyleType"}}));
        }
    }
    for id in [note_id, transcript_id] {
        requests.push(json!({"updateDocumentStyle":{
            "tabId":id,"documentStyle":{"documentFormat":{"documentMode":"PAGELESS"}},
            "fields":"documentFormat.documentMode"
        }}));
    }
    Ok(json!({"requests":requests,"writeControl":{"requiredRevisionId":revision}}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn two_tabs_updated_atomically_with_revision_and_utf16_indices() {
        let doc = json!({"revisionId":"rev1","tabs":[
            {"tabProperties":{"tabId":"n"},"documentTab":{"body":{"content":[{"endIndex":20}]}}},
            {"tabProperties":{"tabId":"t"},"documentTab":{"body":{"content":[{"endIndex":2}]}}}
        ]});
        let body = replace_tabs(&doc, "n", "😀会議\n本文", "t", "文字起こし\n").unwrap();
        assert_eq!(body["writeControl"]["requiredRevisionId"], "rev1");
        assert_eq!(
            body["requests"][0]["deleteContentRange"]["range"]["endIndex"],
            19
        );
        assert_eq!(
            body["requests"][2]["updateParagraphStyle"]["range"]["endIndex"],
            5
        );
        assert_eq!(body["requests"][3]["insertText"]["location"]["tabId"], "t");
        let styles: Vec<_> = body["requests"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r.get("updateDocumentStyle"))
            .collect();
        assert_eq!(styles.len(), 2);
        for (style, id) in styles.iter().zip(["n", "t"]) {
            assert_eq!(style["tabId"], id);
            assert_eq!(
                style["documentStyle"]["documentFormat"]["documentMode"],
                "PAGELESS"
            );
            assert_eq!(style["fields"], "documentFormat.documentMode");
        }
        assert!(replace_tabs(&doc, "missing", "x", "t", "y").is_err());
        assert!(replace_tabs(&doc, "n", "x", "n", "y").is_err());
        let mut no_revision = doc;
        no_revision["revisionId"] = Value::Null;
        assert!(replace_tabs(&no_revision, "n", "x", "t", "y").is_err());
    }
    #[test]
    fn notes_preserve_actions_and_exclude_deleted_records() {
        let note = json!({"summary":{"overview":"概要"},"participants":[{"participantId":"p","displayName":"田中"}],
            "actionItems":[{"title":"確認する","assigneeParticipantIds":["p"],"due":{"rawText":"明日"}},{"title":"削除済み","recordMeta":{"lifecycle":"deleted"}}]});
        let text = notes("会議", Some(&note));
        assert!(text.contains("確認する"));
        assert!(text.contains("担当: 田中"));
        assert!(text.contains("期限: 明日"));
        assert!(!text.contains("削除済み"));
    }
}
