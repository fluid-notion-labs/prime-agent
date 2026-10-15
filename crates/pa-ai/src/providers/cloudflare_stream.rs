//! Cloudflare endpoint placeholder resolution: the compiled Workers AI and
//! AI Gateway base URLs are templates (`{CLOUDFLARE_ACCOUNT_ID}` /
//! `{CLOUDFLARE_GATEWAY_ID}`) that materialize from the ambient environment
//! before dispatch (TS `providers/cloudflare-stream.ts`).
//!
//! The storage side of the TS `cloudflare-auth.ts` credential env is not
//! ported: the Rust API-key credential carries no env map, so the ambient
//! environment is the only source, exactly like TS's per-field fallback.

use std::borrow::Cow;

use crate::types::Model;

const CLOUDFLARE_ACCOUNT_ID: &str = "CLOUDFLARE_ACCOUNT_ID";
const CLOUDFLARE_GATEWAY_ID: &str = "CLOUDFLARE_GATEWAY_ID";

const ACCOUNT_ID_PLACEHOLDER: &str = "{CLOUDFLARE_ACCOUNT_ID}";
const GATEWAY_ID_PLACEHOLDER: &str = "{CLOUDFLARE_GATEWAY_ID}";

/// The Cloudflare built-in providers share the `cloudflare-` id prefix, the
/// same discrimination TS's stream wrapper reaches through per-provider
/// registration.
#[must_use]
pub fn is_cloudflare_provider(provider: &str) -> bool {
    provider.starts_with("cloudflare-")
}

/// Whether the endpoint placeholders can resolve from the ambient
/// environment for `provider`: the account id for every Cloudflare
/// provider, the gateway id additionally for the AI Gateway (the
/// endpoint-env half of TS `resolveCloudflareEnv`; the API-key half is
/// owned by the auth storage).
#[must_use]
pub fn cloudflare_endpoint_env_resolved(provider: &str) -> bool {
    let non_empty = |name: &str| std::env::var(name).is_ok_and(|value| !value.is_empty());
    non_empty(CLOUDFLARE_ACCOUNT_ID)
        && (provider != "cloudflare-ai-gateway" || non_empty(CLOUDFLARE_GATEWAY_ID))
}

/// Whether the model's base URL still carries a Cloudflare endpoint
/// placeholder (`hasResolvedCloudflareAuth`'s URL-side counterpart).
#[must_use]
pub fn has_cloudflare_placeholders(model: &Model) -> bool {
    is_cloudflare_provider(&model.provider)
        && (model.base_url.contains(ACCOUNT_ID_PLACEHOLDER)
            || model.base_url.contains(GATEWAY_ID_PLACEHOLDER))
}

/// Substitute one `{NAME}` placeholder from the ambient environment; an
/// unset or empty value keeps the placeholder text, so a missing account id
/// fails the request visibly instead of silently retargeting it.
fn substitute(base_url: &str, placeholder: &str, name: &str) -> String {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => base_url.replace(placeholder, &value),
        _ => base_url.to_owned(),
    }
}

/// The model with Cloudflare endpoint placeholders resolved from the ambient
/// environment (TS `resolveCloudflareModel`); the borrowed model when
/// nothing needs substituting.
#[must_use]
pub fn resolve_cloudflare_model(model: &Model) -> Cow<'_, Model> {
    if !has_cloudflare_placeholders(model) {
        return Cow::Borrowed(model);
    }
    let mut resolved = model.clone();
    resolved.base_url = substitute(
        &substitute(
            &model.base_url,
            ACCOUNT_ID_PLACEHOLDER,
            CLOUDFLARE_ACCOUNT_ID,
        ),
        GATEWAY_ID_PLACEHOLDER,
        CLOUDFLARE_GATEWAY_ID,
    );
    Cow::Owned(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cloudflare_model(provider: &str, base_url: &str) -> Model {
        serde_json::from_value(serde_json::json!({
            "id": "@cf/test/model", "name": "Test", "api": "openai-completions",
            "provider": provider, "baseUrl": base_url, "reasoning": false,
            "input": ["text"],
            "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0},
            "contextWindow": 1000, "maxTokens": 100,
        }))
        .expect("test model")
    }

    /// Sets the env var for the test's scope; the drop removes it again so
    /// sibling tests never see it.
    struct EnvGuard(&'static str);

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            std::env::remove_var(self.0);
        }
    }

    fn env_guard(name: &'static str, value: &str) -> EnvGuard {
        std::env::set_var(name, value);
        EnvGuard(name)
    }

    #[test]
    fn resolves_account_and_gateway_placeholders_from_ambient_env() {
        let model = cloudflare_model(
            "cloudflare-workers-ai",
            "https://api.cloudflare.com/client/v4/accounts/{CLOUDFLARE_ACCOUNT_ID}/ai/v1",
        );
        {
            let _guard = env_guard("CLOUDFLARE_ACCOUNT_ID", "acct123");
            let resolved = resolve_cloudflare_model(&model);
            assert_eq!(
                resolved.base_url,
                "https://api.cloudflare.com/client/v4/accounts/acct123/ai/v1"
            );
            // A fully resolved URL no longer carries placeholders.
            assert!(!has_cloudflare_placeholders(&resolved));
        }
        // Without the env value the placeholder text survives (TS parity:
        // `env[NAME] ?? '{NAME}'`), and the model flags as unresolved.
        let unresolved = resolve_cloudflare_model(&model);
        assert_eq!(unresolved.base_url, model.base_url);
        assert!(has_cloudflare_placeholders(&unresolved));
    }

    #[test]
    fn non_cloudflare_models_pass_through_untouched() {
        let model = cloudflare_model(
            "openai",
            "https://api.example.com/v1/{CLOUDFLARE_ACCOUNT_ID}",
        );
        let resolved = resolve_cloudflare_model(&model);
        let _guard = env_guard("CLOUDFLARE_ACCOUNT_ID", "acct123");
        assert!(resolved.base_url.contains("{CLOUDFLARE_ACCOUNT_ID}"));
    }

    #[test]
    fn endpoint_env_gate_needs_the_gateway_id_only_for_the_gateway() {
        let _account = env_guard("CLOUDFLARE_ACCOUNT_ID", "acct123");
        assert!(cloudflare_endpoint_env_resolved("cloudflare-workers-ai"));
        assert!(!cloudflare_endpoint_env_resolved("cloudflare-ai-gateway"));
        let _gateway = env_guard("CLOUDFLARE_GATEWAY_ID", "gw456");
        assert!(cloudflare_endpoint_env_resolved("cloudflare-ai-gateway"));
    }

    #[test]
    fn empty_env_values_do_not_resolve() {
        let _guard = env_guard("CLOUDFLARE_ACCOUNT_ID", "");
        assert!(!cloudflare_endpoint_env_resolved("cloudflare-workers-ai"));
        let model = cloudflare_model(
            "cloudflare-workers-ai",
            "https://api.cloudflare.com/client/v4/accounts/{CLOUDFLARE_ACCOUNT_ID}/ai/v1",
        );
        assert_eq!(resolve_cloudflare_model(&model).base_url, model.base_url);
    }
}
