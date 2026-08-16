/// From reference/packages/opencode/src/session/session.ts
///
/// The Session service: create, list, fork, touch, patch operations over the
/// [`crate::store::SessionDb`] store. Pure orchestration — the store
/// implementation is provided by the runner/server.
use crate::session::{CreateInput, Info, SessionRow};
use crate::store::SessionDb;

pub struct SessionService<'a, D: SessionDb> {
    pub db: &'a D,
}

/// Stateless session mutations shared by callers that already own the
/// session projection. Keeping these operations in the session service
/// prevents HTTP/state adapters from reimplementing the session semantics.
#[derive(Debug, Clone, Copy, Default)]
pub struct SessionMutationService;

impl SessionMutationService {
    /// `Session.setTitle`.
    pub fn set_title(&self, info: &Info, title: &str) -> Info {
        let mut next = info.clone();
        next.title = title.to_string();
        next
    }
}

/// From reference `session.ts:createNext`.
pub fn create_next(
    project_id: &str,
    version: &str,
    directory: &str,
    input: &CreateInput,
    time: u64,
) -> Info {
    let parent = input.parent_id.clone();
    let title = input.title.clone().unwrap_or_else(|| {
        let prefix = if parent.is_some() {
            crate::session::CHILD_TITLE_PREFIX
        } else {
            crate::session::PARENT_TITLE_PREFIX
        };
        format!("{prefix}{}", iso_timestamp(time))
    });
    Info {
        id: crate::schema::create_session(None),
        slug: crate::util::slug::create(),
        project_id: project_id.to_string(),
        workspace_id: input.workspace_id.clone(),
        parent_id: parent,
        directory: directory.to_string(),
        path: None,
        summary: None,
        cost: Some(0.0),
        tokens: Some(crate::v1::SessionTokens {
            input: 0.0,
            output: 0.0,
            reasoning: 0.0,
            cache: crate::v1::CacheTokens {
                read: 0.0,
                write: 0.0,
            },
        }),
        share: None,
        title,
        agent: input.agent.clone(),
        model: input.model.clone().map(|m| crate::v1::SessionModel {
            id: m.id,
            provider_id: m.provider_id,
            variant: m.variant,
        }),
        version: version.to_string(),
        metadata: input.metadata.clone(),
        time: crate::v1::SessionTime {
            created: time,
            updated: time,
            compacting: None,
            archived: None,
        },
        permission: input.permission.clone(),
        revert: None,
    }
}

fn iso_timestamp(time: u64) -> String {
    // `new Date().toISOString()` — UTC `YYYY-MM-DDTHH:mm:ss.SSSZ`.
    let secs = time / 1000;
    let millis = time % 1000;
    let days = secs / 86_400;
    let remaining = secs % 86_400;
    let (hour, minute, second) = (remaining / 3600, (remaining % 3600) / 60, remaining % 60);
    let (year, month, day) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Convert days since 1970-01-01 to (year, month, day).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

impl<'a, D: SessionDb> SessionService<'a, D> {
    pub fn new(db: &'a D) -> Self {
        SessionService { db }
    }

    /// `Session.get` — raises the not-found message when absent.
    pub fn get(&self, id: &str) -> Result<Info, String> {
        self.db
            .session_row(id)
            .ok_or_else(|| format!("Session not found: {id}"))
    }

    /// `Session.touch` — bump `time.updated`.
    pub fn touch(&self, _id: &str, info: &Info, time: u64) -> Info {
        let mut next = info.clone();
        next.time.updated = time;
        next
    }

    /// `Session.setTitle`.
    pub fn set_title(&self, info: &Info, title: &str) -> Info {
        SessionMutationService.set_title(info, title)
    }

    /// `Session.setAgentModel`.
    pub fn set_agent_model(
        &self,
        info: &Info,
        agent: &str,
        model: &crate::v1::SessionModel,
        time: u64,
    ) -> Info {
        let mut next = info.clone();
        next.agent = Some(agent.to_string());
        next.model = Some(model.clone());
        next.time.updated = time;
        next
    }

    /// `Session.setSummary`.
    pub fn set_summary(&self, info: &Info, summary: crate::v1::SessionSummary, time: u64) -> Info {
        let mut next = info.clone();
        next.summary = Some(summary);
        next.time.updated = time;
        next
    }

    /// `Session.setRevert`.
    pub fn set_revert(
        &self,
        info: &Info,
        revert: Option<crate::v1::SessionRevert>,
        time: u64,
    ) -> Info {
        let mut next = info.clone();
        next.revert = revert;
        next.time.updated = time;
        next
    }

    /// `Session.fork` — clone messages up to (optionally) a message.
    pub fn fork(
        &self,
        original: &Info,
        directory: &str,
        worktree: &str,
        title: &str,
        time: u64,
    ) -> Info {
        let mut next = original.clone();
        next.id = crate::schema::create_session(None);
        next.parent_id = None;
        next.title = title.to_string();
        next.directory = directory.to_string();
        next.path = Some(crate::session::session_path(worktree, directory));
        next.time.created = time;
        next.time.updated = time;
        next
    }

    pub fn to_row(info: &Info) -> SessionRow {
        crate::session::to_row(info)
    }

    pub fn from_row(row: &SessionRow) -> Info {
        crate::session::from_row(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::MessageRow;
    use crate::v1::SessionInfo;

    struct MemDb;

    impl SessionDb for MemDb {
        fn context_epoch_baseline(&self, _: &str) -> Option<u64> {
            None
        }
        fn latest_compaction_seq(&self, _: &str) -> Option<u64> {
            None
        }
        fn message_rows(&self, _: &str) -> Vec<MessageRow> {
            vec![]
        }
        fn message_row(&self, _: &str) -> Option<MessageRow> {
            None
        }
        fn session_row(&self, id: &str) -> Option<SessionInfo> {
            if id == "ses_1" {
                Some(SessionInfo {
                    id: "ses_1".into(),
                    title: "Existing".into(),
                    ..SessionInfo::default()
                })
            } else {
                None
            }
        }
    }

    #[test]
    fn create_next_defaults_title_and_ids() {
        let info = create_next(
            "prj_1",
            "v1",
            "/work",
            &CreateInput::default(),
            1_700_000_000_000,
        );
        assert!(info.id.starts_with("ses_"));
        assert!(info.title.starts_with("New session - "));
        assert_eq!(info.cost, Some(0.0));
    }

    #[test]
    fn get_returns_not_found_message() {
        let service = SessionService::new(&MemDb);
        assert!(service.get("ses_missing").is_err());
        assert_eq!(service.get("ses_1").unwrap().title, "Existing");
    }

    #[test]
    fn title_mutation_changes_only_the_title() {
        let mut info = SessionInfo::default();
        info.title = "Before".into();
        info.time.updated = 42;

        let updated = SessionMutationService.set_title(&info, "After");

        assert_eq!(updated.title, "After");
        assert_eq!(updated.time.updated, 42);
    }

    #[test]
    fn iso_timestamp_matches_js_to_iso_string() {
        // 2024-06-01T12:00:00.000Z = 1717243200000 ms
        assert_eq!(iso_timestamp(1_717_243_200_000), "2024-06-01T12:00:00.000Z");
        assert_eq!(iso_timestamp(0), "1970-01-01T00:00:00.000Z");
    }
}
