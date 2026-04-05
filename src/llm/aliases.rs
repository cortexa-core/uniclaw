/// Provider alias resolution — maps short names to backend config.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AuthStyle {
    Bearer,     // Authorization: Bearer <key>
    XApiKey,    // x-api-key: <key>
    QueryParam, // ?key=<key> in URL
    None,       // No auth header
}

#[derive(Debug, Clone)]
pub struct ProviderAlias {
    pub backend: &'static str,
    pub base_url: &'static str,
    #[allow(dead_code)]
    pub api_key_env: &'static str,
    pub auth_style: AuthStyle,
    pub extra_headers: &'static [(&'static str, &'static str)],
}

/// Resolve a provider short name to its alias config.
pub fn resolve(name: &str) -> Option<ProviderAlias> {
    let name = name.to_lowercase();
    let name = name.as_str();

    // Helper for the common openai-compatible, Bearer, no extra headers case.
    macro_rules! compat {
        ($url:expr, $env:expr) => {
            Some(ProviderAlias {
                backend: "openai_compatible",
                base_url: $url,
                api_key_env: $env,
                auth_style: AuthStyle::Bearer,
                extra_headers: &[],
            })
        };
    }

    match name {
        // ── Native providers ────────────────────────────────────────
        "anthropic" => Some(ProviderAlias {
            backend: "anthropic",
            base_url: "https://api.anthropic.com",
            api_key_env: "ANTHROPIC_API_KEY",
            auth_style: AuthStyle::XApiKey,
            extra_headers: &[],
        }),

        "gemini" | "google" | "google-gemini" => Some(ProviderAlias {
            backend: "gemini",
            base_url: "https://generativelanguage.googleapis.com",
            api_key_env: "GEMINI_API_KEY",
            auth_style: AuthStyle::QueryParam,
            extra_headers: &[],
        }),

        // ── OpenAI direct ───────────────────────────────────────────
        "openai" => compat!("https://api.openai.com", "OPENAI_API_KEY"),

        // ── Aggregators / fast inference ─────────────────────────────
        "openrouter" => Some(ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://openrouter.ai/api",
            api_key_env: "OPENROUTER_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[
                ("HTTP-Referer", "https://github.com/cortexa-core/uniclaw"),
                ("X-Title", "uniclaw"),
            ],
        }),

        "groq" => compat!("https://api.groq.com/openai", "GROQ_API_KEY"),
        "together" | "together-ai" => compat!("https://api.together.xyz", "TOGETHER_API_KEY"),
        "fireworks" | "fireworks-ai" => {
            compat!("https://api.fireworks.ai/inference", "FIREWORKS_API_KEY")
        }
        "cerebras" => compat!("https://api.cerebras.ai", "CEREBRAS_API_KEY"),
        "perplexity" => compat!("https://api.perplexity.ai", "PERPLEXITY_API_KEY"),
        "sambanova" => compat!("https://api.sambanova.ai", "SAMBANOVA_API_KEY"),

        // ── Specialized ─────────────────────────────────────────────
        "deepseek" => compat!("https://api.deepseek.com", "DEEPSEEK_API_KEY"),
        "mistral" => compat!("https://api.mistral.ai", "MISTRAL_API_KEY"),
        "xai" | "grok" => compat!("https://api.x.ai", "XAI_API_KEY"),
        "cohere" => compat!("https://api.cohere.com/compatibility", "COHERE_API_KEY"),

        // ── Local / self-hosted ─────────────────────────────────────
        "ollama" => Some(ProviderAlias {
            backend: "openai_compatible",
            base_url: "http://localhost:11434",
            api_key_env: "",
            auth_style: AuthStyle::None,
            extra_headers: &[],
        }),
        "lmstudio" | "lm-studio" => Some(ProviderAlias {
            backend: "openai_compatible",
            base_url: "http://localhost:1234",
            api_key_env: "",
            auth_style: AuthStyle::None,
            extra_headers: &[],
        }),
        "vllm" => Some(ProviderAlias {
            backend: "openai_compatible",
            base_url: "http://localhost:8000",
            api_key_env: "",
            auth_style: AuthStyle::None,
            extra_headers: &[],
        }),
        "litellm" => Some(ProviderAlias {
            backend: "openai_compatible",
            base_url: "http://localhost:4000",
            api_key_env: "",
            auth_style: AuthStyle::None,
            extra_headers: &[],
        }),
        "llamacpp" | "llama.cpp" => Some(ProviderAlias {
            backend: "openai_compatible",
            base_url: "http://localhost:8080",
            api_key_env: "",
            auth_style: AuthStyle::None,
            extra_headers: &[],
        }),

        // ── Chinese ecosystem ───────────────────────────────────────
        "qwen" | "dashscope" => compat!(
            "https://dashscope.aliyuncs.com/compatible-mode",
            "DASHSCOPE_API_KEY"
        ),
        "glm" | "zhipu" | "bigmodel" => {
            compat!("https://open.bigmodel.cn/api/paas", "ZHIPU_API_KEY")
        }
        "moonshot" | "kimi" => compat!("https://api.moonshot.cn", "MOONSHOT_API_KEY"),
        "minimax" => compat!("https://api.minimax.chat", "MINIMAX_API_KEY"),
        "doubao" | "volcengine" => compat!("https://ark.cn-beijing.volces.com/api", "ARK_API_KEY"),
        "stepfun" => compat!("https://api.stepfun.com", "STEPFUN_API_KEY"),
        "baichuan" => compat!("https://api.baichuan-ai.com", "BAICHUAN_API_KEY"),
        "yi" | "01ai" => compat!("https://api.01.ai", "YI_API_KEY"),

        // ── Hosting / gateways ──────────────────────────────────────
        "deepinfra" => compat!("https://api.deepinfra.com/v1/openai", "DEEPINFRA_API_KEY"),
        "huggingface" | "hf" => {
            compat!("https://api-inference.huggingface.co", "HF_API_TOKEN")
        }
        "venice" => compat!("https://api.venice.ai", "VENICE_API_KEY"),
        "nvidia" | "nim" => compat!("https://integrate.api.nvidia.com", "NVIDIA_API_KEY"),
        "hyperbolic" => compat!("https://api.hyperbolic.xyz", "HYPERBOLIC_API_KEY"),

        _ => None,
    }
}

/// All recognized alias strings.
#[allow(dead_code)]
pub fn all_aliases() -> &'static [&'static str] {
    &[
        // Native
        "anthropic",
        "gemini",
        "google",
        "google-gemini",
        // OpenAI direct
        "openai",
        // Aggregators / fast inference
        "openrouter",
        "groq",
        "together",
        "together-ai",
        "fireworks",
        "fireworks-ai",
        "cerebras",
        "perplexity",
        "sambanova",
        // Specialized
        "deepseek",
        "mistral",
        "xai",
        "grok",
        "cohere",
        // Local / self-hosted
        "ollama",
        "lmstudio",
        "lm-studio",
        "vllm",
        "litellm",
        "llamacpp",
        "llama.cpp",
        // Chinese ecosystem
        "qwen",
        "dashscope",
        "glm",
        "zhipu",
        "bigmodel",
        "moonshot",
        "kimi",
        "minimax",
        "doubao",
        "volcengine",
        "stepfun",
        "baichuan",
        "yi",
        "01ai",
        // Hosting / gateways
        "deepinfra",
        "huggingface",
        "hf",
        "venice",
        "nvidia",
        "nim",
        "hyperbolic",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_aliases_resolve() {
        for alias in all_aliases() {
            assert!(
                resolve(alias).is_some(),
                "alias {alias:?} should resolve to Some"
            );
        }
    }

    #[test]
    fn test_groq_resolves_to_compatible() {
        let a = resolve("groq").unwrap();
        assert_eq!(a.backend, "openai_compatible");
        assert_eq!(a.base_url, "https://api.groq.com/openai");
        assert_eq!(a.api_key_env, "GROQ_API_KEY");
        assert_eq!(a.auth_style, AuthStyle::Bearer);
    }

    #[test]
    fn test_gemini_resolves_to_native() {
        let a = resolve("gemini").unwrap();
        assert_eq!(a.backend, "gemini");
    }

    #[test]
    fn test_ollama_has_no_auth() {
        let a = resolve("ollama").unwrap();
        assert_eq!(a.auth_style, AuthStyle::None);
        assert_eq!(a.api_key_env, "");
    }

    #[test]
    fn test_openrouter_has_extra_headers() {
        let a = resolve("openrouter").unwrap();
        assert!(!a.extra_headers.is_empty());
        assert_eq!(a.extra_headers.len(), 2);
    }

    #[test]
    fn test_unknown_returns_none() {
        assert!(resolve("nonexistent").is_none());
    }

    #[test]
    fn test_multi_aliases_same_backend() {
        let t1 = resolve("together").unwrap();
        let t2 = resolve("together-ai").unwrap();
        assert_eq!(t1.base_url, t2.base_url);
        assert_eq!(t1.backend, t2.backend);
    }
}
