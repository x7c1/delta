//! The launch-option registry, including the shipped (built-in) rows.

use delta_model::{AgentProvider, LaunchOption};

use crate::error::Result;

use super::{FakeStore, FAKE_CREATED_AT};

impl FakeStore {
    pub(super) async fn list_launch_options(&self) -> Result<Vec<LaunchOption>> {
        let g = self.inner.lock().unwrap();
        // Delta-shipped rows first (ascending id), then the user's own newest
        // first, mirroring the SQL store's ordering.
        let mut out = g.launch_options.clone();
        out.sort_by_key(|o| match &o.builtin_key {
            Some(_) => (0, o.id),
            None => (1, -o.id),
        });
        Ok(out)
    }

    pub(super) async fn launch_option(&self, id: i64) -> Result<Option<LaunchOption>> {
        let g = self.inner.lock().unwrap();
        Ok(g.launch_options.iter().find(|o| o.id == id).cloned())
    }

    pub(super) async fn create_launch_option(
        &self,
        label: Option<&str>,
        name: &str,
        value: Option<&str>,
        default_enabled: bool,
        provider: AgentProvider,
    ) -> Result<LaunchOption> {
        let mut g = self.inner.lock().unwrap();
        g.next_launch_option_id += 1;
        let option = LaunchOption {
            id: g.next_launch_option_id,
            label: label.map(str::to_owned),
            name: name.to_owned(),
            value: value.map(str::to_owned),
            default_enabled,
            created_at: FAKE_CREATED_AT.into(),
            provider,
            builtin_key: None,
        };
        g.launch_options.push(option.clone());
        Ok(option)
    }

    pub(super) async fn set_launch_option_default_enabled(
        &self,
        id: i64,
        default_enabled: bool,
    ) -> Result<Option<LaunchOption>> {
        let mut g = self.inner.lock().unwrap();
        match g.launch_options.iter_mut().find(|o| o.id == id) {
            Some(option) => {
                option.default_enabled = default_enabled;
                Ok(Some(option.clone()))
            }
            None => Ok(None),
        }
    }

    pub(super) async fn delete_launch_option(&self, id: i64) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        g.launch_options.retain(|o| o.id != id);
        Ok(())
    }

    pub(super) async fn upsert_builtin_launch_option(
        &self,
        builtin_key: &str,
        label: &str,
        name: &str,
        value: Option<&str>,
        provider: AgentProvider,
    ) -> Result<LaunchOption> {
        let mut g = self.inner.lock().unwrap();
        if let Some(existing) = g
            .launch_options
            .iter_mut()
            .find(|o| o.builtin_key.as_deref() == Some(builtin_key))
        {
            // The declared catalog owns these three; `default_enabled`,
            // `created_at` and the id stay exactly as they are.
            existing.label = Some(label.to_owned());
            existing.name = name.to_owned();
            existing.value = value.map(str::to_owned);
            existing.provider = provider;
            return Ok(existing.clone());
        }
        g.next_launch_option_id += 1;
        let option = LaunchOption {
            id: g.next_launch_option_id,
            label: Some(label.to_owned()),
            name: name.to_owned(),
            value: value.map(str::to_owned),
            // Offered, never imposed: a freshly materialized preset starts off.
            default_enabled: false,
            created_at: FAKE_CREATED_AT.into(),
            provider,
            builtin_key: Some(builtin_key.to_owned()),
        };
        g.launch_options.push(option.clone());
        Ok(option)
    }

    pub(super) async fn delete_builtin_launch_options_except(
        &self,
        keys: &[&str],
    ) -> Result<usize> {
        let mut g = self.inner.lock().unwrap();
        let before = g.launch_options.len();
        g.launch_options.retain(|o| match &o.builtin_key {
            // The user's own rows are never in scope of a catalog change.
            None => true,
            Some(key) => keys.contains(&key.as_str()),
        });
        Ok(before - g.launch_options.len())
    }
}
