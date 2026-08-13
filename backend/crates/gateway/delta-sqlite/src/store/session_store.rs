//! The [`SessionStore`] impl for [`SqliteStore`].
//!
//! Rust requires a trait to be implemented in a single `impl` block, so this
//! is the one place the trait is wired up: every method forwards to the
//! inherent method of the same name (inherent methods take precedence in
//! method resolution), and those live in the per-aggregate sibling modules.

use std::collections::BTreeMap;

use async_trait::async_trait;

use delta_attribution::SubagentLaunch;
use delta_model::{
    AgentProvider, LaunchOption, Message, MessageUuid, PermissionRequest, Send, Session, SessionId,
    Thread, ThreadId,
};
use delta_usecase::{
    NewSession, RecentWorkdir, RepositoryCloneRow, RepositoryScanRoot, SessionPageCursor,
    SessionPageRow, SessionStore,
};

use super::SqliteStore;

#[async_trait]
impl SessionStore for SqliteStore {
    async fn register_session(
        &self,
        new: NewSession,
    ) -> std::result::Result<(Session, ThreadId), delta_usecase::Error> {
        self.register_session(new).await
    }

    #[allow(clippy::too_many_arguments)]
    async fn insert_spawning_session(
        &self,
        id: &SessionId,
        cwd: &str,
        branch_at_launch: Option<&str>,
        repo_root: Option<&str>,
        requested_workdir: Option<&str>,
        repository_display_name: Option<&str>,
        provider: AgentProvider,
    ) -> std::result::Result<(Session, ThreadId), delta_usecase::Error> {
        self.insert_spawning_session(
            id,
            cwd,
            branch_at_launch,
            repo_root,
            requested_workdir,
            repository_display_name,
            provider,
        )
        .await
    }

    async fn set_provider_ids(
        &self,
        id: &SessionId,
        provider_session_id: Option<&str>,
        provider_thread_id: Option<&str>,
    ) -> std::result::Result<(), delta_usecase::Error> {
        self.set_provider_ids(id, provider_session_id, provider_thread_id)
            .await
    }

    async fn delete_session(
        &self,
        id: &SessionId,
    ) -> std::result::Result<(), delta_usecase::Error> {
        self.delete_session(id).await
    }

    async fn mark_session_failed(
        &self,
        id: &SessionId,
    ) -> std::result::Result<(), delta_usecase::Error> {
        self.mark_session_failed(id).await
    }

    async fn list_sessions_page(
        &self,
        cursor: Option<SessionPageCursor>,
        limit: u32,
    ) -> std::result::Result<Vec<SessionPageRow>, delta_usecase::Error> {
        self.list_sessions_page(cursor, limit).await
    }

    async fn session(
        &self,
        id: &SessionId,
    ) -> std::result::Result<Option<Session>, delta_usecase::Error> {
        self.session(id).await
    }

    async fn main_thread_id(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<ThreadId, delta_usecase::Error> {
        self.main_thread_id(session_id).await
    }

    async fn recent_workdirs(
        &self,
        limit: u32,
    ) -> std::result::Result<Vec<RecentWorkdir>, delta_usecase::Error> {
        self.recent_workdirs(limit).await
    }

    async fn cwd_exists(&self, path: &str) -> std::result::Result<bool, delta_usecase::Error> {
        self.cwd_exists(path).await
    }

    async fn repository_clone_rows(
        &self,
        worktree_base: &str,
        active_repo_limit: i64,
        user_clone_limit: i64,
        generated_clone_limit: i64,
    ) -> std::result::Result<Vec<RepositoryCloneRow>, delta_usecase::Error> {
        self.repository_clone_rows(
            worktree_base,
            active_repo_limit,
            user_clone_limit,
            generated_clone_limit,
        )
        .await
    }

    async fn thread(
        &self,
        id: ThreadId,
    ) -> std::result::Result<Option<Thread>, delta_usecase::Error> {
        self.thread(id).await
    }

    async fn list_threads(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<Vec<Thread>, delta_usecase::Error> {
        self.list_threads(session_id).await
    }

    async fn create_thread(
        &self,
        session_id: &SessionId,
        title: &str,
        parent_thread_id: Option<ThreadId>,
    ) -> std::result::Result<Thread, delta_usecase::Error> {
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
    ) -> std::result::Result<Send, delta_usecase::Error> {
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
    ) -> std::result::Result<Send, delta_usecase::Error> {
        self.enqueue_queued_send(
            session_id,
            thread_id,
            semantic_parent_uuid,
            text,
            locator_quote,
        )
        .await
    }

    async fn send(&self, id: i64) -> std::result::Result<Option<Send>, delta_usecase::Error> {
        self.send(id).await
    }

    async fn next_queued_send(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<Option<Send>, delta_usecase::Error> {
        self.next_queued_send(session_id).await
    }

    async fn open_sends(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<Vec<Send>, delta_usecase::Error> {
        self.open_sends(session_id).await
    }

    async fn promote_queued_send(&self, id: i64) -> std::result::Result<(), delta_usecase::Error> {
        self.promote_queued_send(id).await
    }

    async fn requeue_send(&self, id: i64) -> std::result::Result<(), delta_usecase::Error> {
        self.requeue_send(id).await
    }

    async fn restore_all_dispatched(&self) -> std::result::Result<usize, delta_usecase::Error> {
        self.restore_all_dispatched().await
    }

    async fn release_restored_send(
        &self,
        id: i64,
    ) -> std::result::Result<bool, delta_usecase::Error> {
        self.release_restored_send(id).await
    }

    async fn head_dispatched_send(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<Option<Send>, delta_usecase::Error> {
        self.head_dispatched_send(session_id).await
    }

    async fn dispatched_sends(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<Vec<Send>, delta_usecase::Error> {
        self.dispatched_sends(session_id).await
    }

    async fn mark_send_matched(
        &self,
        id: i64,
        matched_uuid: &MessageUuid,
    ) -> std::result::Result<(), delta_usecase::Error> {
        self.mark_send_matched(id, matched_uuid).await
    }

    async fn latest_user_thread(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<Option<ThreadId>, delta_usecase::Error> {
        self.latest_user_thread(session_id).await
    }

    async fn cancel_send(&self, id: i64) -> std::result::Result<(), delta_usecase::Error> {
        self.cancel_send(id).await
    }

    async fn cancel_queued_send(&self, id: i64) -> std::result::Result<bool, delta_usecase::Error> {
        self.cancel_queued_send(id).await
    }

    async fn upsert_messages(
        &self,
        messages: &[Message],
    ) -> std::result::Result<(), delta_usecase::Error> {
        self.upsert_messages(messages).await
    }

    async fn last_activity_at(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<Option<String>, delta_usecase::Error> {
        self.last_activity_at(session_id).await
    }

    async fn message_count(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<usize, delta_usecase::Error> {
        self.message_count(session_id).await
    }

    async fn transcript_lines_read(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<usize, delta_usecase::Error> {
        self.transcript_lines_read(session_id).await
    }

    async fn set_transcript_lines_read(
        &self,
        session_id: &SessionId,
        lines: usize,
    ) -> std::result::Result<(), delta_usecase::Error> {
        self.set_transcript_lines_read(session_id, lines).await
    }

    async fn thread_messages(
        &self,
        thread_id: ThreadId,
    ) -> std::result::Result<Vec<Message>, delta_usecase::Error> {
        self.thread_messages(thread_id).await
    }

    async fn record_permission_request(
        &self,
        session_id: &SessionId,
        tool_name: &str,
        tool_input_json: &str,
        tool_use_id: Option<&str>,
    ) -> std::result::Result<PermissionRequest, delta_usecase::Error> {
        self.record_permission_request(session_id, tool_name, tool_input_json, tool_use_id)
            .await
    }

    async fn decide_permission_request(
        &self,
        request_id: i64,
        allowed: bool,
    ) -> std::result::Result<Option<PermissionRequest>, delta_usecase::Error> {
        self.decide_permission_request(request_id, allowed).await
    }

    async fn resolve_permission_by_tool_use_id(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        allowed: bool,
    ) -> std::result::Result<Vec<i64>, delta_usecase::Error> {
        self.resolve_permission_by_tool_use_id(session_id, tool_use_id, allowed)
            .await
    }

    async fn deny_pending_permission_requests(
        &self,
        session_id: &SessionId,
        reason: &str,
    ) -> std::result::Result<Vec<i64>, delta_usecase::Error> {
        self.deny_pending_permission_requests(session_id, reason)
            .await
    }

    async fn record_subagent_launch(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        thread_id: ThreadId,
    ) -> std::result::Result<(), delta_usecase::Error> {
        self.record_subagent_launch(session_id, tool_use_id, thread_id)
            .await
    }

    async fn upgrade_subagent_task_id(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
        task_id: &str,
    ) -> std::result::Result<(), delta_usecase::Error> {
        self.upgrade_subagent_task_id(session_id, tool_use_id, task_id)
            .await
    }

    async fn clear_subagent_launch(
        &self,
        session_id: &SessionId,
        tool_use_id: &str,
    ) -> std::result::Result<(), delta_usecase::Error> {
        self.clear_subagent_launch(session_id, tool_use_id).await
    }

    async fn outstanding_subagent_launches(
        &self,
        session_id: &SessionId,
    ) -> std::result::Result<BTreeMap<String, SubagentLaunch>, delta_usecase::Error> {
        self.outstanding_subagent_launches(session_id).await
    }

    async fn list_launch_options(
        &self,
    ) -> std::result::Result<Vec<LaunchOption>, delta_usecase::Error> {
        self.list_launch_options().await
    }

    async fn create_launch_option(
        &self,
        label: Option<&str>,
        name: &str,
        value: Option<&str>,
        default_enabled: bool,
        provider: AgentProvider,
    ) -> std::result::Result<LaunchOption, delta_usecase::Error> {
        self.create_launch_option(label, name, value, default_enabled, provider)
            .await
    }

    async fn set_launch_option_default_enabled(
        &self,
        id: i64,
        default_enabled: bool,
    ) -> std::result::Result<Option<LaunchOption>, delta_usecase::Error> {
        self.set_launch_option_default_enabled(id, default_enabled)
            .await
    }

    async fn delete_launch_option(&self, id: i64) -> std::result::Result<(), delta_usecase::Error> {
        self.delete_launch_option(id).await
    }

    async fn list_repository_scan_roots(
        &self,
    ) -> std::result::Result<Vec<RepositoryScanRoot>, delta_usecase::Error> {
        self.list_repository_scan_roots().await
    }

    async fn insert_repository_scan_root(
        &self,
        path: &str,
    ) -> std::result::Result<RepositoryScanRoot, delta_usecase::Error> {
        self.insert_repository_scan_root(path).await
    }

    async fn delete_repository_scan_root(
        &self,
        path: &str,
    ) -> std::result::Result<(), delta_usecase::Error> {
        self.delete_repository_scan_root(path).await
    }
}
