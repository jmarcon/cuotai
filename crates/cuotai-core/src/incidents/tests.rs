use super::*;
use serde_json::Value;

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

/// Each provider makes an explicit decision; fixture-backed positive rows catch
/// accidentally pointing a provider at another vendor's page/component catalog.
#[test]
fn every_provider_has_a_verified_scope_or_an_explicit_none() {
    use Provider::*;
    let cases = [
        (
            OpenAiApi,
            Some(include_str!("fixtures/openai-components.json")),
        ),
        (AnthropicApi, Some(include_str!("fixtures/claude.json"))),
        (
            ChatGpt,
            Some(include_str!("fixtures/openai-components.json")),
        ),
        (Claude, Some(include_str!("fixtures/claude.json"))),
        (GeminiApi, None),
        (
            VertexAi,
            Some(include_str!("fixtures/google-products.json")),
        ),
        (AwsBedrock, Some("aws")),
        (AzureOpenAi, None),
        (Cursor, Some(include_str!("fixtures/cursor.json"))),
        (Copilot, Some(include_str!("fixtures/github.json"))),
        (Windsurf, Some(include_str!("fixtures/windsurf.json"))),
        (MiniMax, Some(include_str!("fixtures/minimax.json"))),
        (Kimi, Some(include_str!("fixtures/moonshot.json"))),
        (Qwen, None),
        (Groq, Some(include_str!("fixtures/groq-components.json"))),
        (
            Perplexity,
            Some(include_str!("fixtures/perplexity-components.json")),
        ),
        (Mistral, None),
        (CloudflareAi, Some(include_str!("fixtures/cloudflare.json"))),
        (OllamaCloud, None),
        (XiaomiMimo, None),
        (OpenCodeGo, None),
        (Qoder, None),
        (GeminiAi, None),
        (Grok, Some("xai")),
        (NvidiaNim, Some(include_str!("fixtures/nvidia.json"))),
        (Poolside, None),
        (Kiro, None),
        (Cline, None),
        (CodeBuddy, None),
        (Zed, None),
        (AlibabaModelStudio, None),
        (AlibabaCodingPlan, None),
        (AlibabaTokenPlan, None),
        (AlibabaCloud, None),
        (Antigravity, None),
        (Kimchi, None),
        (
            ElevenLabs,
            Some(include_str!("fixtures/eleven-labs-components.json")),
        ),
        (Meta, None),
        (NineRouter, None),
        (OpenRouter, None),
        (LiteLlm, None),
    ];
    assert_eq!(cases.map(|(p, _)| p), Provider::ALL);
    for (provider, fixture) in cases {
        let Some(fixture) = fixture else {
            assert_eq!(source(provider), None, "{provider:?}");
            continue;
        };
        let source = source(provider).expect("verified provider");
        let url = url::Url::parse(source.url).unwrap();
        assert_eq!(url.scheme(), "https");
        assert!(url.username().is_empty() && url.password().is_none());
        assert!(url.query().is_none() && url.fragment().is_none());
        chrono::NaiveDate::parse_from_str(source.verified_on, "%Y-%m-%d").unwrap();
        assert!(!source.scope.is_empty());
        assert!(!source.includes_component(""));
        assert!(!source.includes_component("unknown-component"));
        if fixture == "aws" {
            assert_eq!(source.format, Format::AtomRss(FeedFormat::AwsHealth));
            assert!(source.includes_component("bedrock"));
            assert!(!source.includes_component("multipleservices"));
            continue;
        }
        if fixture == "xai" {
            assert_eq!(source.format, Format::AtomRss(FeedFormat::Xai));
            continue;
        }
        let catalog = json(fixture);
        let components = catalog[if source.format == Format::JsonCustom(JsonFormat::GoogleCloud) {
            "products"
        } else {
            "components"
        }]
        .as_array()
        .unwrap();
        for component in source.scope {
            assert_eq!(
                components
                    .iter()
                    .filter(|c| c["id"] == component.id)
                    .count(),
                1,
                "{provider:?}: {} must exist exactly once in the verified catalog",
                component.id
            );
            assert!(source.includes_component(component.id));
            assert!(!source.includes_component(&format!("{}-unrelated", component.id)));
        }
    }
}

#[test]
fn gemini_api_does_not_borrow_google_cloud_component_health() {
    assert_eq!(source(Provider::GeminiApi), None);

    let vertex = source(Provider::VertexAi).expect("verified Vertex AI source");
    assert!(vertex.includes_component("Z0FZJAMvEB4j3NbCJs6B"));
    assert!(vertex.includes_component("3zaaDb7antc73BM1UAVT"));
}

#[test]
fn shared_platforms_do_not_leak_unrelated_components() {
    for (a, b) in [
        (Provider::OpenAiApi, Provider::ChatGpt),
        (Provider::AnthropicApi, Provider::Claude),
    ] {
        let a = source(a).unwrap();
        let b = source(b).unwrap();
        assert_eq!((a.url, a.format), (b.url, b.format));
        for c in a.scope {
            assert!(!b.includes_component(c.id));
        }
        for c in b.scope {
            assert!(!a.includes_component(c.id));
        }
    }
    for (provider, unrelated) in [
        (Provider::Copilot, "br0l2tvcx85d"),      // Actions
        (Provider::Windsurf, "k1sy38n3ktcq"),     // Netlify API
        (Provider::CloudflareAi, "ks6zgqry9c1p"), // A city, not Workers AI
        (Provider::MiniMax, "zvwy77wxk251"),      // Video
        (Provider::NvidiaNim, "672k1lfkl6fl"),    // Fleet Command
        (Provider::Kimi, "sttxgsd2bps7"),         // Chat website
        (Provider::Grok, "Grok in X"),
    ] {
        assert!(!source(provider).unwrap().includes_component(unrelated));
    }
}

#[test]
fn provider_wide_outage_does_not_override_the_scope() {
    let cloudflare = json(include_str!("fixtures/cloudflare.json"));
    assert_eq!(cloudflare["status"]["indicator"], "major");
    let source = source(Provider::CloudflareAi).unwrap();
    let selected: Vec<_> = cloudflare["components"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| source.includes_component(c["id"].as_str().unwrap()))
        .collect();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0]["name"], "Workers AI");
    assert_eq!(selected[0]["status"], "operational");
}

#[test]
fn native_json_product_ids_survive_renames_and_exclude_other_services() {
    let incidents = json(include_str!("fixtures/google-cloud.json"));
    let source = source(Provider::VertexAi).unwrap();
    let products = incidents[0]["affected_products"].as_array().unwrap();
    let vertex = products
        .iter()
        .find(|p| source.includes_component(p["id"].as_str().unwrap()))
        .unwrap();
    assert!(source.includes_component(vertex["id"].as_str().unwrap()));
    // Names are descriptive only; IDs, not unstable marketing names, select products.
    assert!(!source.includes_component(vertex["title"].as_str().unwrap()));
    assert!(!source.includes_component("fPovtKbaWN9UTepMm3kJ"));
}

#[test]
fn widget_and_feed_component_selectors_are_not_page_rollups() {
    let empty = json(include_str!("fixtures/widget-empty.json"));
    assert_eq!(empty["ongoing_incidents"], serde_json::json!([]));
    let widget = json(include_str!("fixtures/widget-maintenance.json"));
    for component in widget["scheduled_maintenances"][0]["affected_components"]
        .as_array()
        .unwrap()
    {
        assert!(!source(Provider::OpenAiApi)
            .unwrap()
            .includes_component(component["id"].as_str().unwrap()));
    }
    let aws = include_str!("fixtures/aws-health.rss");
    let guid = aws
        .split("<guid isPermaLink=\"false\">")
        .nth(1)
        .unwrap()
        .split("</guid>")
        .next()
        .unwrap();
    let service = guid.split('#').nth(1).unwrap().split('-').next().unwrap();
    assert!(!source(Provider::AwsBedrock)
        .unwrap()
        .includes_component(service));
    let xai = include_str!("fixtures/xai.rss");
    for title in xai.split("<title>[").skip(1) {
        let component = title.split(']').next().unwrap();
        assert!(source(Provider::Grok)
            .unwrap()
            .includes_component(component));
    }
}

/// RF4.1: only plain `https` pages on a verified source's host can be
/// opened; look-alike hosts, other schemes and decorated URLs cannot.
#[test]
fn status_links_are_limited_to_plain_pages_on_verified_hosts() {
    for url in [
        "https://status.claude.com/",
        "https://status.claude.com/incidents/abc123xyz",
        "https://status.aws.amazon.com/",
        "https://status.x.ai/grok-com/ps_1",
    ] {
        assert!(is_status_link(url), "{url}");
    }
    for url in [
        "",
        "status.claude.com",
        "http://status.claude.com/",
        "https://status.claude.com",
        "HTTPS://STATUS.CLAUDE.COM/",
        "https://status.claude.com:8443/",
        "https://user@status.claude.com/",
        "https://status.claude.com/?next=https://example.com",
        "https://status.claude.com/#x",
        "https://status.claude.com.example.com/",
        "https://example.com/status.claude.com/",
        "https://status.mistral.ai/",
        "javascript:alert(1)",
        "file:///etc/passwd",
    ] {
        assert!(!is_status_link(url), "{url}");
    }
}

/// PRD 0037 RF8: the region picks the sub-page of the one shared summary;
/// an unknown region has no source instead of a guessed one.
#[test]
fn eleven_labs_regions_select_a_subpage_of_one_shared_url() {
    let subpage = |region| source_for(Provider::ElevenLabs, region).and_then(|s| s.subpage);
    assert_eq!(subpage(None), Some("united-states"));
    assert_eq!(subpage(Some("default")), Some("united-states"));
    assert_eq!(subpage(Some("us")), Some("united-states"));
    assert_eq!(subpage(Some("eu")), Some("eu"));
    assert_eq!(subpage(Some("in")), Some("india"));
    assert_eq!(subpage(Some("sg")), Some("singapore"));
    assert_eq!(source_for(Provider::ElevenLabs, Some("mars")), None);
    let urls: std::collections::HashSet<_> = [None, Some("eu"), Some("in"), Some("sg")]
        .into_iter()
        .map(|r| source_for(Provider::ElevenLabs, r).unwrap().url)
        .collect();
    assert_eq!(urls.len(), 1);
    // Captured response: the four regional sub-pages exist.
    let summary = json(include_str!("fixtures/eleven-labs-summary.json"));
    let slugs: Vec<_> = summary["subpages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["subpage"].as_str().unwrap())
        .collect();
    assert_eq!(slugs, ["united-states", "eu", "india", "singapore"]);
    // Only the account-endpoint component is in scope, not TTS/STT/Agents.
    let source = source(Provider::ElevenLabs).unwrap();
    assert!(source.includes_component("01M52E4R81RBEYD94QJACM67BQ"));
    assert!(!source.includes_component("01M52E23077QGJZHKBM2FBBPFM")); // Text to Speech
    assert!(!source.includes_component("01M52E2F2T3TJB9ADTV9MXEQBF")); // Speech to Text
}
