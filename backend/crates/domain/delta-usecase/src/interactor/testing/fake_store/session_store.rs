//! The [`SessionStore`] impl for [`FakeStore`].
//!
//! Rust requires a trait to be implemented in a single `impl` block, so this
//! is the one place the trait is wired up: every method forwards to the
//! inherent method of the same name (inherent methods take precedence in
//! method resolution), and those live in the per-area sibling modules.

use std::collections::BTreeMap;

use async_trait::async_trait;
use delta_attribution::SubagentLaunch;
use delta_model::{
    AgentProvider, LaunchOption, Message, MessageUuid, PermissionRequest, PromptTemplate, Send,
    Session, SessionId, Thread, ThreadId,
};

use crate::error::Result;
use crate::ports::{
    CloneRoot, NewSession, RepositoryCloneRow, SessionPageRow, SessionStore, SpawningSession,
};
use crate::SessionPageCursor;

use super::FakeStore;

#[async_trait]
impl SessionStore for FakeStore {
    async fn register_session(&self, new: NewSession) -> Result<(Session, ThreadId)> {
        self.register_session(new).await
    }

    async fn insert_spawning_session(
        &self,
        spawning: SpawningSession<'_>,
    ) -> Result<(Session, ThreadId)> {
        self.insert_spawning_session(spawning).await
    }

    async fn set_provider_ids(
        &self,
        id: &SessionId,
        provider_session_id: Option<&str>,
        provider_thread_id: Option<&str>,
    ) -> Result<()> {
        self.set_provider_ids(id, provider_session_id, provider_thread_id)
            .await
    }

    async fn delete_session(&self, id: &SessionId) -> Result<()> {
        self.delete_session(id).await
    }

    async fn mark_session_failed(&self, id: &SessionId, reason: Option<&str>) -> Result<()> {
        self.mark_session_failed(id, reason).await
    }

    async fn list_sessions_page(
        &self,
        cursor: Option<SessionPageCursor>,
        limit: u32,
    ) -> Result<Vec<SessionPageRow>> {
        self.list_sessions_page(cursor, limit).await
    }

    async fn list_sessions_by_ids(&self, ids: &[SessionId]) -> Result<Vec<SessionPageRow>> {
        self.list_sessions_by_ids(ids).await
    }

    async fn session(&self, id: &SessionId) -> Result<Option<Session>> {
        self.session(id).await
    }

    async fn last_activity_at(&self, session_id: &SessionId) -> Result<Option<String>> {
        self.last_activity_at(session_id).await
    }

    async fn main_thread_id(&self, session_id: &SessionId) -> Result<ThreadId> {
        self.main_thread_id(session_id).await
    }

    async fn recent_workdirs(&self, limit: u32) -> Result<Vec<crate::ports::RecentWorkdir>> {
        self.recent_workdirs(limit).await
    }

    async fn cwd_exists(&self, path: &str) -> Result<bool> {
        self.cwd_exists(path).await
    }

    async fn repository_clone_rows(
        &self,
        worktree_base: &str,
        active_repo_limit: i64,
        user_clone_limit: i64,
        generated_clone_limit: i64,
    ) -> Result<Vec<RepositoryCloneRow>> {
        self.repository_clone_rows(
            worktree_base,
            active_repo_limit,
            user_clone_limit,
            generated_clone_limit,
        )
        .await
    }

    async fn thread(&self, id: ThreadId) -> Result<Option<Thread>> {
        self.thread(id).await
    }

    async fn list_threads(&self, session_id: &SessionId) -> Result<Vec<Thread>> {
        self.list_threads(session_id).await
    }

    async fn create_thread(
        &self,
        session_id: &SessionId,
        title: &str,
        parent_thread_id: Option<ThreadId>,
    ) -> Result<Thread> {
        self.create_thread(session_id, title, parent_thread_id)
            .await
    }

    async fn enqueue_send(
        &self,
        session_id: &SessionId,
        thread_id: ThreadId,
        semantic_parent_uuid: Option<&MessageUuid>,
        text: &str,
        locator_quote: Option<&str>,
    ) -> Result<Send> {
        self.enqueue_send(
            session_id,
            thread_id,
            semantic_parent_uuid,
            text,
            locator_quote,
        )
        .await
    }

    async fn enqueue_queued_send(
        &self,
        session_id: &SessionId,
        thread_id: ThreadId,
        semantic_parent_uuid: Option<&MessageUuid>,
        text: &str,
        locator_quote: Option<&str>,
    ) -> Result<Send> {
        self.enqueue_queued_send(
            session_id,
            thread_id,
            semantic_parent_uuid,
            text,
            locator_quote,
        )
        .await
    }

    async fn send(&self, id: i64) -> Result<Option<Send>> {
        self.send(id).await
    }

    async fn next_queued_send(&self, session_id: &SessionId) -> Result<Option<Send>> {
        self.next_queued_send(session_id).await
    }

    async fn open_sends(&self, session_id: &SessionId) -> Result<Vec<Send>> {
        self.open_sends(session_id).await
    }

    async fn promote_queued_send(&self, id: i64) -> Result<()> {
        self.promote_queued_send(id).await
    }

    async fn requeue_send(&self, id: i64) -> Result<()> {
        self.requeue_send(id).await
    }

    async fn restore_all_dispatched(&self) -> Result<usize> {
        self.restore_all_dispatched().await
    }

    async fn hold_send_for_release(&self, id: i64) -> Result<bool> {
        self.hold_send_for_release(id).await
    }

    async fn release_held_send(&self, id: i64) -> Result<bool> {
        self.release_held_send(id).await
    }

    async fn head_dispatched_send(&self, session_id: &SessionId) -> Result<Option<Send>> {
        self.head_dispatched_send(session_id).await
    }

    async fn dispatched_sends(&self, session_id: &SessionId) -> Result<Vec<Send>> {
        self.dispatched_sends(session_id).await
    }

    async fn mark_send_matched(&self, id: i64, matched_uuid: &MessageUuid) -> Result<()> {
        self.mark_send_matched(id, matched_uuid).await
    }

    async fn settle_send_delivered(&self, id: i64) -> Result<bool> {
        self.settle_send_delivered(id).await
    }

    async fn latest_user_thread(&self, session_id: &SessionId) -> Result<Option<ThreadId>> {
        self.latest_user_thread(session_id).await
    }

    async fn cancel_send(&self, id: i64) -> Result<()> {
        self.cancel_send(id).await
    }

    async fn cancel_queued_send(&self, id: i64) -> Result<bool> {
        self.cancel_queued_send(id).await
    }

    async fn upsert_messages(&self, messages: &[Message]) -> Result<()> {
        self.upsert_messages(messages).await
    }

    async fn message_count(&self, session_id: &SessionId) -> Result<usize> {
        self.message_count(session_id).await
    }

    async fn transcript_lines_read(&self, session_id: &SessionId) -> Result<usize> {
        self.transcript_lines_read(session_id).await
    }

    async fn set_transcript_lines_read(&self, session_id: &SessionId, lines: usize) -> Result<()> {
        self.set_transcript_lines_read(session_id, lines).await
    }

    async fn thread_messages(&self, thread_id: ThreadId) -> Result<Vec<Message>> {
        self.thread_messages(thread_id).await
    }

    async fn record_permission_request(
        &self,
        session_id: &SessionId,
        tool_name: &str,
        tool_input_json: &str,
        tool_use_id: Option<&str>,
    ) -> Result<PermissionRequest> {
        self.record_permission_request(session_id, tool_name, tool_input_json, tool_use_id)
            .await
    }

    async fn decide_permission_request(
        &self,
        request_id: i64,
        allowed: bool,
    ) -> Result<Option<PermissionRequest>> {
        self.decide_permission_request(request_id, allowed).await
    }

    async fn resolve_permission_by_tool_use_id(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        allowed: bool,
    ) -> Result<Vec<i64>> {
        self.resolve_permission_by_tool_use_id(session_id, tool_use_id, allowed)
            .await
    }

    async fn deny_pending_permission_requests(
        &self,
        session_id: &SessionId,
        reason: &str,
    ) -> Result<Vec<i64>> {
        self.deny_pending_permission_requests(session_id, reason)
            .await
    }

    async fn record_subagent_launch(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        thread_id: ThreadId,
    ) -> Result<()> {
        self.record_subagent_launch(session_id, tool_use_id, thread_id)
            .await
    }

    async fn upgrade_subagent_task_id(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        task_id: &str,
    ) -> Result<()> {
        self.upgrade_subagent_task_id(session_id, tool_use_id, task_id)
            .await
    }

    async fn clear_subagent_launch(&self, session_id: &SessionId, tool_use_id: &str) -> Result<()> {
        self.clear_subagent_launch(session_id, tool_use_id).await
    }

    async fn outstanding_subagent_launches(
        &self,
        session_id: &SessionId,
    ) -> Result<BTreeMap<String, SubagentLaunch>> {
        self.outstanding_subagent_launches(session_id).await
    }

    async fn list_launch_options(&self) -> Result<Vec<LaunchOption>> {
        self.list_launch_options().await
    }

    async fn launch_option(&self, id: i64) -> Result<Option<LaunchOption>> {
        self.launch_option(id).await
    }

    async fn create_launch_option(
        &self,
        label: Option<&str>,
        name: &str,
        value: Option<&str>,
        default_enabled: bool,
        provider: AgentProvider,
    ) -> Result<LaunchOption> {
        self.create_launch_option(label, name, value, default_enabled, provider)
            .await
    }

    async fn set_launch_option_default_enabled(
        &self,
        id: i64,
        default_enabled: bool,
    ) -> Result<Option<LaunchOption>> {
        self.set_launch_option_default_enabled(id, default_enabled)
            .await
    }

    async fn delete_launch_option(&self, id: i64) -> Result<()> {
        self.delete_launch_option(id).await
    }

    async fn upsert_builtin_launch_option(
        &self,
        builtin_key: &str,
        label: &str,
        name: &str,
        value: Option<&str>,
        provider: AgentProvider,
    ) -> Result<LaunchOption> {
        self.upsert_builtin_launch_option(builtin_key, label, name, value, provider)
            .await
    }

    async fn delete_builtin_launch_options_except(&self, keys: &[&str]) -> Result<usize> {
        self.delete_builtin_launch_options_except(keys).await
    }

    async fn list_prompt_templates(&self) -> Result<Vec<PromptTemplate>> {
        self.list_prompt_templates().await
    }

    async fn create_prompt_template(&self, label: &str, text: &str) -> Result<PromptTemplate> {
        self.create_prompt_template(label, text).await
    }

    async fn update_prompt_template(
        &self,
        id: i64,
        label: &str,
        text: &str,
    ) -> Result<Option<PromptTemplate>> {
        self.update_prompt_template(id, label, text).await
    }

    async fn delete_prompt_template(&self, id: i64) -> Result<()> {
        self.delete_prompt_template(id).await
    }

    async fn list_clone_roots(&self) -> Result<Vec<CloneRoot>> {
        self.list_clone_roots().await
    }

    async fn insert_clone_root(&self, path: &str) -> Result<CloneRoot> {
        self.insert_clone_root(path).await
    }

    async fn delete_clone_root(&self, path: &str) -> Result<()> {
        self.delete_clone_root(path).await
    }
}
