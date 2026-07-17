//! `provider_availability`: report, per known provider, whether its launch
//! binary is present on this host so the new-session selector can disable an
//! un-installed provider with a reason instead of failing at spawn time.

use delta_model::{AgentProvider, ProviderAvailability};

use crate::interactor::InteractorCore;

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G> {
    /// Report launch availability for every known provider.
    ///
    /// Each provider's configured launch binary is probed through the injected
    /// [`BinaryDetector`](crate::ports::BinaryDetector): Claude via
    /// `launch.claude_bin`, Codex via the `codex_bin` handed to the adapter
    /// factory at the composition root — the *same* binaries a real spawn would
    /// use, so availability never diverges from what launch attempts. An absent
    /// binary yields `available: false` with a human-readable `detail`; the
    /// endpoint always answers (no 5xx on a host missing a provider).
    ///
    /// v1 reports binary presence only. A future version-compatibility verdict
    /// slots into [`ProviderAvailability::detail`] without reshaping this — see
    /// that type's docs (deferred to the real-Codex canary).
    pub async fn provider_availability(&self) -> Vec<ProviderAvailability> {
        // The known providers paired with the binary a spawn would launch.
        // Adding a provider is a new entry here plus its capability profile in
        // the gateway layer — the same shape the `AgentProvider` enum documents.
        let probes = [
            (AgentProvider::Claude, self.launch.claude_bin.as_str()),
            (AgentProvider::Codex, self.codex_bin.as_str()),
        ];

        let mut out = Vec::with_capacity(probes.len());
        for (provider, bin) in probes {
            let available = self.binary_detector.is_available(bin).await;
            let detail = (!available).then(|| unavailable_detail(provider, bin));
            out.push(ProviderAvailability {
                provider,
                available,
                detail,
            });
        }
        out
    }
}

/// The reason string shown when a provider's launch binary cannot be resolved.
fn unavailable_detail(provider: AgentProvider, bin: &str) -> String {
    format!(
        "The '{bin}' binary for {} was not found on PATH.",
        provider.as_str()
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use delta_model::AgentProvider;

    use crate::interactor::testing::{interactor, FakeBinaryDetector};
    use crate::ports::BinaryDetector;

    /// Both binaries present → both providers available, no detail.
    #[tokio::test]
    async fn both_present_reports_both_available() {
        let detector = Arc::new(FakeBinaryDetector::all_present());
        let ix = interactor().with_binary_detector(detector as Arc<dyn BinaryDetector>);

        let availability = ix.provider_availability().await;

        assert_eq!(availability.len(), 2);
        let claude = &availability[0];
        assert_eq!(claude.provider, AgentProvider::Claude);
        assert!(claude.available);
        assert_eq!(claude.detail, None);
        let codex = &availability[1];
        assert_eq!(codex.provider, AgentProvider::Codex);
        assert!(codex.available);
        assert_eq!(codex.detail, None);
    }

    /// Codex binary absent → Codex unavailable with a reason; Claude still
    /// available. This is the accident the endpoint prevents: picking a
    /// provider whose binary is missing.
    #[tokio::test]
    async fn codex_absent_reports_unavailable_with_detail() {
        // The default test interactor's Codex binary is `codex`; mark only that
        // one absent so the Claude probe still passes.
        let detector = Arc::new(FakeBinaryDetector::all_present().with_absent("codex"));
        let ix = interactor()
            .with_codex_bin("codex")
            .with_binary_detector(detector as Arc<dyn BinaryDetector>);

        let availability = ix.provider_availability().await;

        let claude = &availability[0];
        assert_eq!(claude.provider, AgentProvider::Claude);
        assert!(claude.available, "the Claude binary is still present");

        let codex = &availability[1];
        assert_eq!(codex.provider, AgentProvider::Codex);
        assert!(!codex.available);
        let detail = codex.detail.as_deref().expect("an unavailable reason");
        assert!(
            detail.contains("codex"),
            "reason names the binary: {detail}"
        );
    }

    /// The probed Codex binary follows `with_codex_bin`, so availability tracks
    /// the exact binary a Codex spawn would launch (e.g. a `DELTA_CODEX_BIN`
    /// override) rather than a hardcoded name.
    #[tokio::test]
    async fn probes_the_configured_codex_binary() {
        // Only `/opt/codex/bin/codex` is present; the default `codex` is not.
        let detector = Arc::new(FakeBinaryDetector::default().with_present("/opt/codex/bin/codex"));
        let ix = interactor()
            .with_codex_bin("/opt/codex/bin/codex")
            .with_binary_detector(detector as Arc<dyn BinaryDetector>);

        let availability = ix.provider_availability().await;

        let codex = &availability[1];
        assert_eq!(codex.provider, AgentProvider::Codex);
        assert!(
            codex.available,
            "the configured Codex path is probed, not the bare default"
        );
    }
}
