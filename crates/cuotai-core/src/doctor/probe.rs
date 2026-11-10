//! PRD 0046 RF4.3: `cuotai --doctor --probe`, the doctor's only network
//! checks, opt-in. One `HEAD` request per provider host the configuration
//! uses (and per public status page when status queries are on), without
//! any credential, header of the person or body: any HTTP answer means the
//! host is reachable; only a failed connection, TLS handshake or timeout
//! fails. Each answer's `Date` feeds the process clock observer (PRD 0052
//! RF7), so the doctor's clock check that follows has a server to compare.
//! Hosts are fixed per provider: an account's own base URL is never
//! probed, and local tools and loopback aggregators have nothing to probe.

use std::collections::BTreeSet;
use std::time::Duration;

use super::{Check, CheckStatus};
use crate::network::ClientPurpose;
use crate::text::Localized;
use crate::{Config, Provider};

/// How long one probe waits.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// The public API host of `provider` a probe contacts (the default region
/// where there are several); `None` for local tools, loopback aggregators,
/// extensions and providers whose host depends on the account.
pub fn host(provider: Provider) -> Option<&'static str> {
    Some(match provider {
        Provider::OpenAiApi => "api.openai.com",
        Provider::AnthropicApi => "api.anthropic.com",
        Provider::ChatGpt => "chatgpt.com",
        Provider::Claude => "api.anthropic.com",
        Provider::GeminiApi => "generativelanguage.googleapis.com",
        Provider::VertexAi => "monitoring.googleapis.com",
        Provider::AzureOpenAi => "management.azure.com",
        Provider::Cursor => "cursor.com",
        Provider::Copilot => "api.github.com",
        Provider::Windsurf => "server.codeium.com",
        Provider::MiniMax => "api.minimax.io",
        Provider::Kimi => "api.kimi.com",
        Provider::Qwen => "portal.qwen.ai",
        Provider::Groq => "api.groq.com",
        Provider::Perplexity => "api.perplexity.ai",
        Provider::Mistral => "api.mistral.ai",
        Provider::CloudflareAi => "api.cloudflare.com",
        Provider::OllamaCloud => "ollama.com",
        Provider::XiaomiMimo => "platform.xiaomimimo.com",
        Provider::OpenCodeGo => "opencode.ai",
        Provider::Qoder => "openapi.qoder.sh",
        Provider::GeminiAi => "cloudcode-pa.googleapis.com",
        Provider::Grok => "management-api.x.ai",
        Provider::NvidiaNim => "integrate.api.nvidia.com",
        Provider::Poolside => "inference.poolside.ai",
        Provider::Cline => "api.cline.bot",
        Provider::CodeBuddy => "www.codebuddy.ai",
        Provider::Zed => "cloud.zed.dev",
        Provider::AlibabaModelStudio => "dashscope-intl.aliyuncs.com",
        Provider::AlibabaCodingPlan => "coding-intl.dashscope.aliyuncs.com",
        Provider::AlibabaTokenPlan => "home.qwencloud.com",
        Provider::AlibabaCloud => "business.ap-southeast-1.aliyuncs.com",
        Provider::Antigravity => "cloudcode-pa.googleapis.com",
        Provider::Kimchi => "llm.kimchi.dev",
        Provider::ElevenLabs => "api.elevenlabs.io",
        Provider::Meta => "api.meta.ai",
        Provider::JetbrainsAi => "api.jetbrains.cloud",
        Provider::OpenRouter => "openrouter.ai",
        // Signed through the AWS CLI with the account's own region.
        Provider::AwsBedrock
        // An IDE's local sign-in, regional endpoints.
        | Provider::Kiro
        // Loopback aggregators and local programs.
        | Provider::NineRouter
        | Provider::LiteLlm
        | Provider::Extension => return None,
    })
}

/// One host to probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// `probe.<host>` or `status.<host>`.
    pub id: String,
    pub host: String,
    /// The URL requested (`https://<host>/`, or the status page's).
    pub url: String,
    /// A public status page (PRD 0011) rather than a provider API.
    pub status: bool,
}

/// The hosts `config` uses, each once, in account order; then, when
/// `status` is on, each public status page of those providers.
pub fn targets(config: &Config, status: bool) -> Vec<Target> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for account in &config.accounts {
        if let Some(host) = host(account.provider) {
            if seen.insert(("probe", host.to_owned())) {
                out.push(Target {
                    id: format!("probe.{host}"),
                    host: host.to_owned(),
                    url: format!("https://{host}/"),
                    status: false,
                });
            }
        }
    }
    if status {
        for account in &config.accounts {
            let region = account.params.get("region").map(String::as_str);
            let Some(source) = crate::incidents::source_for(account.provider, region) else {
                continue;
            };
            if seen.insert(("status", source.url.to_owned())) {
                out.push(Target {
                    id: format!("status.{}", source.host()),
                    host: source.host().to_owned(),
                    url: source.url.to_owned(),
                    status: true,
                });
            }
        }
    }
    out
}

/// Probe every target at once with `client` (built without credentials);
/// each answer's `Date` goes to `skew`. One check per target, in order.
pub async fn run(
    targets: &[Target],
    client: &reqwest::Client,
    skew: &crate::network::ClockSkew,
) -> Vec<Check> {
    let requests = targets.iter().map(|target| async move {
        let sent = client.head(&target.url).timeout(TIMEOUT).send().await;
        if let Ok(response) = &sent {
            skew.observe(
                response.headers(),
                chrono::Utc::now(),
                if target.status {
                    ClientPurpose::PublicStatus
                } else {
                    ClientPurpose::Provider
                },
            );
        }
        (target, sent.is_ok())
    });
    let results = futures_util::future::join_all(requests).await;
    results
        .into_iter()
        .map(|(target, reached)| {
            let title = Localized::with_param(
                match target.status {
                    true => "doctor.title.probe_status",
                    false => "doctor.title.probe",
                },
                "host",
                target.host.as_str(),
                match target.status {
                    true => "Status page {host}",
                    false => "Connection to {host}",
                },
            );
            if reached {
                Check::new(target.id.clone(), CheckStatus::Ok, title).detail(Localized::fixed(
                    "doctor.probe.ok",
                    "It answers over HTTPS (no credential was sent).",
                ))
            } else {
                Check::new(target.id.clone(), CheckStatus::Fail, title)
                    .detail(Localized::fixed(
                        "doctor.probe.failed",
                        "It could not be reached: no connection, a certificate refused, or no answer within 10 seconds.",
                    ))
                    .fix(Localized::fixed(
                        "doctor.fix.probe",
                        "Check the network, proxy and firewall of this computer, then try again.",
                    ))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn hosts_are_fixed_unique_and_skip_local_providers() {
        let config = Config::from_toml(
            "[[accounts]]\nname = \"a\"\nprovider = \"groq\"\ncredential_env = \"A\"\n\
             [[accounts]]\nname = \"b\"\nprovider = \"groq\"\ncredential_env = \"B\"\n\
             [[accounts]]\nname = \"c\"\nprovider = \"litellm\"\ncredential_env = \"C\"\n\
             [[accounts]]\nname = \"d\"\nprovider = \"open_ai_api\"\ncredential_env = \"D\"\n",
        )
        .unwrap();
        let hosts: Vec<_> = targets(&config, false).into_iter().map(|t| t.url).collect();
        assert_eq!(hosts, ["https://api.groq.com/", "https://api.openai.com/"]);
        for provider in Provider::ALL {
            if let Some(host) = host(provider) {
                assert!(
                    !host.contains(['/', ':']) && host.contains('.'),
                    "{provider:?}: {host}"
                );
            }
        }
        // Status pages are added only when status queries are on.
        assert!(targets(&config, true).len() >= targets(&config, false).len());
    }

    #[tokio::test]
    async fn any_answer_is_reachable_and_a_closed_port_fails_without_credentials() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .respond_with(
                ResponseTemplate::new(401).insert_header("date", "Mon, 01 Jan 2001 00:00:00 GMT"),
            )
            .mount(&server)
            .await;
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let closed_url = format!("http://{}/", closed.local_addr().unwrap());
        drop(closed);
        let targets = [
            Target {
                id: "probe.ok".into(),
                host: "ok".into(),
                url: format!("{}/", server.uri()),
                status: false,
            },
            Target {
                id: "probe.down".into(),
                host: "down".into(),
                url: closed_url,
                status: false,
            },
        ];
        let skew = crate::network::ClockSkew::new();
        let checks = run(&targets, &reqwest::Client::new(), &skew).await;
        assert_eq!(checks[0].status, CheckStatus::Ok);
        assert_eq!(checks[1].status, CheckStatus::Fail);
        assert!(checks[1].fix.is_some());
        // The answer's date reached the clock observer: years off.
        assert!(skew.observed() && skew.warning().is_some());
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert!(request.body.is_empty());
        assert!(request.headers.get("authorization").is_none());
        assert!(request.headers.get("cookie").is_none());
    }
}
