use serde::Deserialize;
use std::path::Path;

/// A loaded, validated skill
pub struct Skill {
    pub name: String,
    #[allow(dead_code)] // kept for future skill listing/search
    pub description: String,
    pub content: String,
}

/// YAML frontmatter parsed from skill markdown files
#[derive(Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
    #[serde(default)]
    requires: SkillRequires,
}

#[derive(Deserialize, Default)]
struct SkillRequires {
    #[serde(default)]
    tools: Vec<String>,
    #[serde(default)]
    env: Vec<String>,
}

/// Manages skill loading and gating
pub struct SkillManager {
    skills: Vec<Skill>,
}

impl SkillManager {
    /// Load skills from directory, filter by tool/env requirements
    pub fn load(skills_dir: &Path, available_tools: &[String]) -> Self {
        let mut skills = Vec::new();

        let entries = match std::fs::read_dir(skills_dir) {
            Ok(e) => e,
            Err(_) => return Self { skills },
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.extension().is_some_and(|ext| ext == "md") {
                continue;
            }

            match Self::load_skill(&path, available_tools) {
                Ok(Some(skill)) => {
                    tracing::info!("Loaded skill: {}", skill.name);
                    skills.push(skill);
                }
                Ok(None) => {} // gated out, already logged
                Err(e) => {
                    tracing::warn!("Failed to load skill {}: {e}", path.display());
                }
            }
        }

        tracing::info!("Loaded {} skills from {}", skills.len(), skills_dir.display());
        Self { skills }
    }

    /// Get all skill content for system prompt injection.
    /// Warns if total size exceeds 16KB but does not truncate.
    pub fn prompt_content(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }

        let parts: Vec<String> = self.skills
            .iter()
            .map(|s| format!("### {}\n\n{}", s.name, s.content))
            .collect();

        let result = parts.join("\n\n---\n\n");

        if result.len() > 16384 {
            tracing::warn!(
                "Total skills content is {}B (>16KB). Consider reducing skill count.",
                result.len()
            );
        }

        result
    }

    fn load_skill(path: &Path, available_tools: &[String]) -> anyhow::Result<Option<Skill>> {
        let raw = std::fs::read_to_string(path)?;
        let (frontmatter, body) = Self::parse_frontmatter(&raw)?;
        let meta: SkillFrontmatter = parse_yaml_frontmatter(&frontmatter)?;

        // Gate: required tools
        for tool in &meta.requires.tools {
            if !available_tools.iter().any(|t| t == tool) {
                tracing::info!("Skill '{}' gated: requires tool '{tool}'", meta.name);
                return Ok(None);
            }
        }

        // Gate: required env vars
        for env_var in &meta.requires.env {
            if std::env::var(env_var).is_err() {
                tracing::info!("Skill '{}' gated: requires env var '{env_var}'", meta.name);
                return Ok(None);
            }
        }

        let content = body.trim().to_string();
        if content.is_empty() {
            tracing::warn!("Skill '{}' has empty content, skipping", meta.name);
            return Ok(None);
        }

        Ok(Some(Skill {
            name: meta.name,
            description: meta.description,
            content,
        }))
    }

    fn parse_frontmatter(raw: &str) -> anyhow::Result<(String, String)> {
        let trimmed = raw.trim_start();
        if !trimmed.starts_with("---") {
            return Err(anyhow::anyhow!("No YAML frontmatter (must start with ---)"));
        }
        let after_first = &trimmed[3..];
        let end = after_first.find("\n---")
            .ok_or_else(|| anyhow::anyhow!("No closing --- for frontmatter"))?;
        Ok((after_first[..end].trim().to_string(), after_first[end + 4..].to_string()))
    }
}

/// Convert simple YAML frontmatter to TOML for parsing (avoids yaml crate dep)
fn parse_yaml_frontmatter(frontmatter: &str) -> anyhow::Result<SkillFrontmatter> {
    let mut toml_lines = Vec::new();
    let mut in_requires = false;

    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed == "requires:" {
            in_requires = true;
            toml_lines.push("[requires]".to_string());
            continue;
        }

        let is_indented = line.starts_with("  ") || line.starts_with('\t');
        if !is_indented && in_requires {
            in_requires = false;
        }

        if let Some((key, value)) = trimmed.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            toml_lines.push(format!("{key} = {}", yaml_value_to_toml(value)));
        }
    }

    let toml_str = toml_lines.join("\n");
    toml::from_str(&toml_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse skill frontmatter: {e}"))
}

fn yaml_value_to_toml(value: &str) -> String {
    if value == "true" || value == "false" || value.parse::<u64>().is_ok() {
        return value.to_string();
    }
    if value.starts_with('[') && value.ends_with(']') {
        let inner = &value[1..value.len() - 1];
        let elements: Vec<String> = inner.split(',')
            .map(|e| {
                let e = e.trim().trim_matches('"').trim_matches('\'');
                if e.parse::<u64>().is_ok() || e == "true" || e == "false" {
                    e.to_string()
                } else {
                    format!("\"{e}\"")
                }
            })
            .collect();
        return format!("[{}]", elements.join(", "));
    }
    let unquoted = value.trim_matches('"').trim_matches('\'');
    format!("\"{unquoted}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter() {
        let raw = "---\nname: test\ndescription: A test\n---\n\n## Body\n\nContent here.";
        let (fm, body) = SkillManager::parse_frontmatter(raw).unwrap();
        assert!(fm.contains("name: test"));
        assert!(body.contains("Content here"));
    }

    #[test]
    fn test_parse_frontmatter_no_yaml() {
        assert!(SkillManager::parse_frontmatter("Just markdown").is_err());
    }

    #[test]
    fn test_parse_yaml_simple() {
        let fm = "name: my-skill\ndescription: Does things";
        let parsed = parse_yaml_frontmatter(fm).unwrap();
        assert_eq!(parsed.name, "my-skill");
        assert_eq!(parsed.description, "Does things");
    }

    #[test]
    fn test_parse_yaml_with_requires() {
        let fm = "name: smart-home\ndescription: Control devices\nrequires:\n  tools: [shell_exec]\n  env: [HUE_IP]";
        let parsed = parse_yaml_frontmatter(fm).unwrap();
        assert_eq!(parsed.requires.tools, vec!["shell_exec"]);
        assert_eq!(parsed.requires.env, vec!["HUE_IP"]);
    }

    #[test]
    fn test_load_and_gate_by_tools() {
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(dir.path().join("valid.md"),
            "---\nname: valid\ndescription: Works\n---\n\nDo stuff.").unwrap();

        std::fs::write(dir.path().join("gated.md"),
            "---\nname: gated\ndescription: Needs special\nrequires:\n  tools: [nonexistent]\n---\n\nWon't load.").unwrap();

        let mgr = SkillManager::load(dir.path(), &["get_time".into()]);
        assert_eq!(mgr.skills.len(), 1);
        assert_eq!(mgr.skills[0].name, "valid");
    }

    #[test]
    fn test_load_and_gate_by_env() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("env.md"),
            "---\nname: env-skill\ndescription: Needs env\nrequires:\n  env: [UNICLAW_TEST_98765]\n---\n\nContent.").unwrap();

        let mgr = SkillManager::load(dir.path(), &[]);
        assert_eq!(mgr.skills.len(), 0);

        std::env::set_var("UNICLAW_TEST_98765", "1");
        let mgr = SkillManager::load(dir.path(), &[]);
        assert_eq!(mgr.skills.len(), 1);
        std::env::remove_var("UNICLAW_TEST_98765");
    }

    #[test]
    fn test_prompt_content_all_injected() {
        let mgr = SkillManager {
            skills: vec![
                Skill { name: "a".into(), description: "".into(), content: "Skill A content.".into() },
                Skill { name: "b".into(), description: "".into(), content: "Skill B content.".into() },
            ],
        };
        let result = mgr.prompt_content();
        assert!(result.contains("Skill A content"));
        assert!(result.contains("Skill B content"));
    }

    #[test]
    fn test_empty_skills() {
        let mgr = SkillManager { skills: vec![] };
        assert!(mgr.prompt_content().is_empty());
    }

    #[test]
    fn test_empty_body_skipped() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("empty.md"),
            "---\nname: empty\ndescription: No body\n---\n\n").unwrap();
        let mgr = SkillManager::load(dir.path(), &[]);
        assert_eq!(mgr.skills.len(), 0);
    }
}
