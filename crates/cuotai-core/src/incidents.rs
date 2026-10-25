//! Verified public incident sources (PRD 0011 RF1) and the opt-in status
//! client built on them (RF2/RF3).
//!
//! The registry is pure and allocation-free. It neither queries a source nor
//! asserts that a provider is operational. `None` means no reliable, scoped
//! source was verified, not "no incidents". Merely looking up a source never
//! enables polling: [`IncidentMonitor`] is off until a frontend turns it on,
//! and [`crate::Monitor`] then queries only the sources of configured
//! providers.

mod parse;
mod status;

pub(crate) use parse::plain_text;
pub(crate) use status::failure_notes;
pub use status::{
    Incident, IncidentMonitor, IncidentState, ProviderStatus, CACHE_TTL, MAX_BODY_BYTES,
    REQUEST_TIMEOUT,
};

use crate::Provider;

/// Wire format, including the dialect needed to interpret a non-Statuspage feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    StatuspageV2,
    AtomRss(FeedFormat),
    JsonCustom(JsonFormat),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedFormat {
    /// RSS 2.0; the GUID fragment starts with the service slug and region.
    AwsHealth,
    /// RSS 2.0; the title's leading `[component]` identifies the surface.
    Xai,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonFormat {
    /// Public, enabled Widget API, not the authenticated management API.
    IncidentIoWidget,
    /// Public incident history, scoped by stable `affected_products[].id`.
    GoogleCloud,
}

/// A machine selector and its human-readable, last-verified description.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Component {
    /// Statuspage/widget component ID, Google product ID, AWS service slug,
    /// or xAI feed component (without square brackets), depending on format.
    pub id: &'static str,
    pub name: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusSource {
    pub format: Format,
    /// Public, credential-free endpoint. Cache/coalesce by this URL, not provider.
    pub url: &'static str,
    /// Only these components are relevant. Never use the page-wide rollup or
    /// infer that an unscoped incident affects every component on the page.
    pub scope: &'static [Component],
    /// Sub-page slug of a multi-page source (one status URL, several regional
    /// pages); `None` for single-page sources. The reading of any other
    /// sub-page, or a response lacking this one, never counts.
    pub subpage: Option<&'static str>,
    /// ISO date of the endpoint, format, component and use-policy verification.
    pub verified_on: &'static str,
}

impl StatusSource {
    /// Exact membership after the format's component selector is extracted.
    /// Unknown/missing IDs must not match, even if their display names match.
    pub fn includes_component(self, id: &str) -> bool {
        self.scope.iter().any(|component| component.id == id)
    }

    /// Host of the endpoint (every registry URL is `https://host/path`):
    /// the domain a status query reveals the user's IP and interest to.
    pub fn host(self) -> &'static str {
        let rest = self.url.strip_prefix("https://").unwrap_or(self.url);
        rest.split('/').next().unwrap_or(rest)
    }
}

const OPENAI: &str = "https://status.openai.com/api/v1/summary";
const CLAUDE: &str = "https://status.claude.com/api/v2/summary.json";
const GOOGLE: &str = "https://status.cloud.google.com/incidents.json";
const VERIFIED_ON: &str = "2026-10-20";

const ELEVEN_LABS: &str = "https://status.elevenlabs.io/api/v1/summary";
const UNITED_STATES: &str = "united-states";

/// ElevenLabs: one incident.io summary for all regional sub-pages; the
/// *ElevenAPI -> Other APIs* component (the group that hosts the account
/// endpoints) has the same ID on every sub-page (verified 2026-10-25).
const fn eleven_labs(subpage: &'static str) -> StatusSource {
    const SCOPE: &[Component] = &[component("01M52E4R81RBEYD94QJACM67BQ", "Other APIs")];
    StatusSource {
        subpage: Some(subpage),
        verified_on: "2026-10-25",
        ..entry(
            Format::JsonCustom(JsonFormat::IncidentIoWidget),
            ELEVEN_LABS,
            SCOPE,
        )
    }
}

const fn component(id: &'static str, name: &'static str) -> Component {
    Component { id, name }
}

const fn entry(format: Format, url: &'static str, scope: &'static [Component]) -> StatusSource {
    StatusSource {
        format,
        url,
        scope,
        subpage: None,
        verified_on: VERIFIED_ON,
    }
}

macro_rules! source_entry {
    ($format:expr, $url:expr, $scope:expr) => {{
        const SCOPE: &[Component] = $scope;
        entry($format, $url, SCOPE)
    }};
}

/// Verified public source, if any, for this provider. Exhaustive intentionally:
/// adding a provider requires an explicit decision and verification evidence in
/// `docs/providers.md`. Coverage is product-level, not an account/region SLA.
pub const fn source(provider: Provider) -> Option<StatusSource> {
    use Format::*;
    use JsonFormat::*;
    Some(match provider {
        Provider::OpenAiApi => source_entry!(
            JsonCustom(IncidentIoWidget),
            OPENAI,
            &[
                component("01JMXBRMFE6N2NNT7DG6XZQ6PW", "Chat Completions"),
                component("01JP8CD9JR3HR6Y7G4Q75N4DVW", "Responses"),
                component("01JMXBRMFEMZK0HPK19RYET250", "Fine-tuning"),
                component("01JMXBRMFEV0AJ0VVS68N9CD6R", "Embeddings"),
                component("01JMXBRMFE4MAP2BHSJNZ787WX", "Images"),
                component("01JMXBRMFE5ESNNV8JDHVCGSRD", "Batch"),
                component("01JMXBRMFEKVBWKK82B44QFMCE", "Audio"),
                component("01JMXBRMFEVZ7E0X9GD9FWR9WX", "Moderations"),
                component("01JMXBRMFEQW613TFE89F45035", "Realtime"),
                component("01JMXBRMFESJCBGJR10PDD3WCQ", "Files"),
                component("01JSM5RTJWHRWDTS6Q604VEW3B", "Login (API)"),
            ]
        ),
        Provider::ChatGpt => source_entry!(
            JsonCustom(IncidentIoWidget),
            OPENAI,
            &[
                component("01KMKFAMWKQ81YWSE1Z18R6VHR", "Codex in ChatGPT Desktop"),
                component("01JMXBNJXG1S2D9V65P1ZZTD94", "Login (ChatGPT)"),
                component("01JVCV8YSWZFRSM1G5CVP253SK", "Codex Web"),
                component("01KMP3KP5MGE23B80K1EK4S8PV", "Codex API"),
                component("01KMKFAMWKNQ84Z1766MV08ZDE", "CLI"),
                component("01KMP3KP5M8X0EBTVW6KN327EE", "VS Code extension"),
            ]
        ),
        Provider::ElevenLabs => eleven_labs(UNITED_STATES),
        Provider::AnthropicApi => source_entry!(
            StatuspageV2,
            CLAUDE,
            &[
                component("k8w3r06qmzrp", "Claude API (api.anthropic.com)"),
                component("0qbwn08sd68x", "Claude Console (platform.claude.com)"),
            ]
        ),
        Provider::Claude => source_entry!(
            StatuspageV2,
            CLAUDE,
            &[
                component("rwppv331jlwc", "claude.ai"),
                component("yyzkbfz2thpt", "Claude Code"),
                component("bpp5gb3hpjcl", "Claude Cowork"),
            ]
        ),
        Provider::VertexAi => source_entry!(
            JsonCustom(GoogleCloud),
            GOOGLE,
            &[
                component("Z0FZJAMvEB4j3NbCJs6B", "Vertex Gemini API"),
                component("sdXM79fz1FS6ekNpu37K", "Vertex AI Online Prediction"),
                component("3zaaDb7antc73BM1UAVT", "Cloud Monitoring"),
            ]
        ),
        Provider::AwsBedrock => source_entry!(
            AtomRss(FeedFormat::AwsHealth),
            "https://status.aws.amazon.com/rss/all.rss",
            &[component(
                "bedrock",
                "Amazon Bedrock (all regions in the feed)"
            ),]
        ),
        Provider::Cursor => source_entry!(
            StatuspageV2,
            "https://status.cursor.com/api/v2/summary.json",
            &[
                component("jh0714rgjgt4", "cursor.com"),
                component("rflc60xp5jp2", "IDE"),
                component("vsny1qv7v86c", "CLI"),
                component("mwv1g9sc7kdh", "Cloud Agents"),
            ]
        ),
        Provider::Copilot => source_entry!(
            StatuspageV2,
            "https://www.githubstatus.com/api/v2/summary.json",
            &[
                component("pjmpxvq2cmr2", "Copilot"),
                component("cnnb39dkkk82", "Copilot AI Model Providers"),
                component("brv1bkgrwx7q", "API Requests"),
            ]
        ),
        Provider::Windsurf => source_entry!(
            StatuspageV2,
            "https://status.windsurf.com/api/v2/summary.json",
            &[
                component("r5wf1ykd7y1m", "Cascade"),
                component("8q19cygxvshj", "Windsurf Tab"),
            ]
        ),
        Provider::MiniMax => source_entry!(
            StatuspageV2,
            "https://status.minimax.io/api/v2/summary.json",
            &[component("pr0d8qr59svt", "Large Language Models (LLM)"),]
        ),
        Provider::Kimi => source_entry!(
            StatuspageV2,
            "https://status.moonshot.cn/api/v2/summary.json",
            &[
                component("8psr5dfdld0s", "Open API"),
                component("cyrtlddz0q8c", "Kimi Code"),
                component("1l34rxbvx0gt", "K3 (Kimi Code)"),
                component("bs3h7057mc6g", "K2.8 Preview (Kimi Code)"),
                component("qrqn4ss6d252", "K2.7 Code Highspeed (Kimi Code)"),
            ]
        ),
        Provider::Groq => source_entry!(
            JsonCustom(IncidentIoWidget),
            "https://groqstatus.com/api/v1/summary",
            &[component("01K053E2FAKWKEYHXEV7WAHJBM", "API"),]
        ),
        Provider::Perplexity => source_entry!(
            JsonCustom(IncidentIoWidget),
            "https://status.perplexity.com/api/v1/summary",
            &[
                component("01M2R6E5FPASTEW3A3V8TSYHJ0", "API"),
                component("01M0TRMC3ED1PG4GRRXXMVNNZ6", "Computer"),
            ]
        ),
        Provider::CloudflareAi => source_entry!(
            StatuspageV2,
            "https://www.cloudflarestatus.com/api/v2/summary.json",
            &[component("zvm7mqn92xzc", "Workers AI"),]
        ),
        Provider::Grok => source_entry!(
            AtomRss(FeedFormat::Xai),
            "https://status.x.ai/feed.xml",
            &[
                component("Global (api.x.ai)", "Global API"),
                component("US (us.api.x.ai)", "US API"),
                component("grok.com", "grok.com"),
                component("Grok (Build)", "Grok Build"),
            ]
        ),
        Provider::NvidiaNim => source_entry!(
            StatuspageV2,
            "https://status.ngc.nvidia.com/api/v2/summary.json",
            &[
                component("fd9jj5gvm96f", "NVIDIA Build"),
                component("yxl7m35pcbfd", "Cloud Functions"),
            ]
        ),
        // Explicitly unverified, never a synthetic operational reading. The
        // dated exclusion audit in docs/providers.md records the reasons.
        Provider::AzureOpenAi
        | Provider::GeminiApi
        | Provider::Qwen
        | Provider::Mistral
        | Provider::OllamaCloud
        | Provider::XiaomiMimo
        | Provider::OpenCodeGo
        | Provider::Qoder
        | Provider::GeminiAi
        | Provider::Poolside
        | Provider::Kiro
        | Provider::Cline
        | Provider::CodeBuddy
        | Provider::Zed
        | Provider::AlibabaModelStudio
        | Provider::AlibabaCodingPlan
        | Provider::AlibabaTokenPlan
        | Provider::AlibabaCloud
        | Provider::Antigravity
        | Provider::Kimchi
        | Provider::NineRouter
        | Provider::OpenRouter
        | Provider::Extension => return None,
    })
}

/// The source for one configured account: like [`source`], but the region of
/// a regional provider selects the sub-page (`default`/`us` -> United States,
/// `eu`, `in` -> India, `sg` -> Singapore). An unknown region has no source
/// (it is a configuration error elsewhere), never a guessed one.
pub fn source_for(provider: Provider, region: Option<&str>) -> Option<StatusSource> {
    if provider != Provider::ElevenLabs {
        return source(provider);
    }
    Some(eleven_labs(match region {
        None | Some("default" | "us") => UNITED_STATES,
        Some("eu") => "eu",
        Some("in") => "india",
        Some("sg") => "singapore",
        Some(_) => return None,
    }))
}

/// Whether `url` is a link the status client can report: a plain `https`
/// URL on the host of a verified source, in normalized form, without port,
/// credentials, query or fragment. Lets a frontend open a reported status
/// or incident page while refusing any other address.
pub fn is_status_link(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    parsed.scheme() == "https"
        && parsed.as_str() == url
        && parsed.port().is_none()
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.query().is_none()
        && parsed.fragment().is_none()
        && parsed.host_str().is_some_and(|host| {
            Provider::ALL
                .into_iter()
                .filter_map(source)
                .any(|source| source.host() == host)
        })
}

#[cfg(test)]
mod tests;
