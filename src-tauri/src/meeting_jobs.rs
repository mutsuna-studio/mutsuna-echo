use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
};

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum MeetingJobKind {
    Transcription,
    Diarization,
    Summary,
    Formatting,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActiveMeetingJob {
    pub(crate) meeting_id: String,
    pub(crate) kind: MeetingJobKind,
}

static ACTIVE_MEETING_JOBS: LazyLock<Mutex<HashMap<String, MeetingJobKind>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) struct MeetingJobGuard {
    meeting_id: String,
    kind: MeetingJobKind,
}

impl MeetingJobGuard {
    pub(crate) fn begin(meeting_id: &str, kind: MeetingJobKind) -> Result<Self, String> {
        crate::meeting_store::validate_meeting_id(meeting_id)?;
        let mut active = ACTIVE_MEETING_JOBS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if active.contains_key(meeting_id) {
            return Err(
                "この会議では別の処理を実行中です。完了してからもう一度お試しください。".into(),
            );
        }
        active.insert(meeting_id.to_string(), kind);
        Ok(Self {
            meeting_id: meeting_id.to_string(),
            kind,
        })
    }
}

impl Drop for MeetingJobGuard {
    fn drop(&mut self) {
        let mut active = ACTIVE_MEETING_JOBS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if active.get(&self.meeting_id) == Some(&self.kind) {
            active.remove(&self.meeting_id);
        }
    }
}

pub(crate) fn active_kind(meeting_id: &str) -> Option<MeetingJobKind> {
    ACTIVE_MEETING_JOBS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(meeting_id)
        .copied()
}

#[tauri::command]
pub(crate) fn list_active_meeting_jobs() -> Vec<ActiveMeetingJob> {
    ACTIVE_MEETING_JOBS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .map(|(meeting_id, kind)| ActiveMeetingJob {
            meeting_id: meeting_id.clone(),
            kind: *kind,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_rejects_overlapping_jobs_for_the_same_meeting() {
        let meeting_id = uuid::Uuid::now_v7().to_string();
        let first = MeetingJobGuard::begin(&meeting_id, MeetingJobKind::Summary)
            .expect("first meeting job");
        assert!(MeetingJobGuard::begin(&meeting_id, MeetingJobKind::Diarization).is_err());
        drop(first);
        assert!(MeetingJobGuard::begin(&meeting_id, MeetingJobKind::Diarization).is_ok());
    }

    #[test]
    fn guard_allows_jobs_for_different_meetings() {
        let first_id = uuid::Uuid::now_v7().to_string();
        let second_id = uuid::Uuid::now_v7().to_string();
        let _first =
            MeetingJobGuard::begin(&first_id, MeetingJobKind::Summary).expect("first meeting job");
        assert!(MeetingJobGuard::begin(&second_id, MeetingJobKind::Transcription).is_ok());
    }
}
