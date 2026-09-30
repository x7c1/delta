//! Outstanding background-task (subagent) launches.

use std::collections::BTreeMap;

use delta_attribution::SubagentLaunch;
use delta_model::{SessionId, ThreadId};

use crate::error::{Error, Result};

use super::FakeStore;

impl FakeStore {
    pub(super) async fn record_subagent_launch(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        thread_id: ThreadId,
    ) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        // Mirrors the SQL UPSERT: the `task_id` of an already-upgraded row is
        // preserved across a re-record (the launching thread refreshes; the
        // separately-learned task id does not). A brand-new row starts with
        // `task_id: None` until `upgrade_subagent_task_id` runs.
        let key = (session_id.clone(), tool_use_id.to_owned());
        let task_id = g
            .subagent_launches
            .get(&key)
            .and_then(|l| l.task_id.clone());
        g.subagent_launches
            .insert(key, SubagentLaunch { thread_id, task_id });
        Ok(())
    }

    pub(super) async fn upgrade_subagent_task_id(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        task_id: &str,
    ) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        if let Some(launch) = g
            .subagent_launches
            .get_mut(&(session_id.clone(), tool_use_id.to_owned()))
        {
            launch.task_id = Some(task_id.to_owned());
        }
        Ok(())
    }

    pub(super) async fn clear_subagent_launch(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
    ) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        if g.fail_clear_subagent_launch_for.as_deref() == Some(tool_use_id) {
            return Err(Error::Store(
                "injected clear_subagent_launch failure".into(),
            ));
        }
        g.subagent_launches
            .remove(&(session_id.clone(), tool_use_id.to_owned()));
        Ok(())
    }

    pub(super) async fn outstanding_subagent_launches(
        &self,
        session_id: &SessionId,
    ) -> Result<BTreeMap<String, SubagentLaunch>> {
        let g = self.inner.lock().unwrap();
        Ok(g.subagent_launches
            .iter()
            .filter(|((sid, _), _)| sid == session_id)
            .map(|((_, tool_use_id), launch)| (tool_use_id.clone(), launch.clone()))
            .collect())
    }
}
