//! Outstanding background-task (subagent) launches.

use std::collections::BTreeMap;

use delta_attribution::SubagentLaunch;
use delta_model::{SessionId, ThreadId};

use crate::error::{Error, Result};

use crate::ports::RunningSubagentLaunch;

use super::{FakeStore, FakeSubagentIndicator};

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
        g.subagent_indicators
            .retain(|i| !(&i.session_id == session_id && i.tool_use_id == tool_use_id));
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

    pub(super) async fn record_subagent_indicator(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        subagent_type: Option<&str>,
        description: Option<&str>,
    ) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        // Mirrors the SQL targeted UPDATE: an unknown launch is a no-op, and
        // re-marking one refreshes its details in place (keeping its order).
        if !g
            .subagent_launches
            .contains_key(&(session_id.clone(), tool_use_id.to_owned()))
        {
            return Ok(());
        }
        let marked = FakeSubagentIndicator {
            session_id: session_id.clone(),
            tool_use_id: tool_use_id.to_owned(),
            subagent_type: subagent_type.map(str::to_owned),
            description: description.map(str::to_owned),
        };
        match g
            .subagent_indicators
            .iter_mut()
            .find(|i| &i.session_id == session_id && i.tool_use_id == tool_use_id)
        {
            Some(existing) => *existing = marked,
            None => g.subagent_indicators.push(marked),
        }
        Ok(())
    }

    pub(super) async fn running_subagent_launches(
        &self,
        session_id: &SessionId,
    ) -> Result<Vec<RunningSubagentLaunch>> {
        let g = self.inner.lock().unwrap();
        Ok(g.subagent_indicators
            .iter()
            .filter(|i| &i.session_id == session_id)
            .filter_map(|i| {
                let launch = g
                    .subagent_launches
                    .get(&(i.session_id.clone(), i.tool_use_id.clone()))?;
                Some(RunningSubagentLaunch {
                    tool_use_id: i.tool_use_id.clone(),
                    thread_id: launch.thread_id,
                    task_id: launch.task_id.clone(),
                    subagent_type: i.subagent_type.clone(),
                    description: i.description.clone(),
                })
            })
            .collect())
    }
}
