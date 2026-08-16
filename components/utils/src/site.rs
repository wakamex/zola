use ahash::AHashMap;
use percent_encoding::percent_decode;

use errors::{Result, anyhow};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WikilinkTarget {
    pub source_path: String,
    pub identity: String,
    pub permalink: String,
    pub aliases: Vec<String>,
    pub lang: String,
    pub track_backlink: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedWikilink {
    pub md_path: String,
    pub permalink: String,
    pub track_backlink: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WikilinkError {
    Missing,
    Ambiguous { candidates: Vec<String> },
    InvalidTarget { reason: String },
}

#[derive(Clone, Debug, Default)]
pub struct WikilinkResolver {
    targets: Vec<WikilinkTarget>,
    paths: AHashMap<String, Vec<usize>>,
    aliases: AHashMap<String, Vec<usize>>,
    stems: AHashMap<String, Vec<usize>>,
}

fn normalize_path(path: &str) -> std::result::Result<String, WikilinkError> {
    if path.contains('\\') {
        return Err(WikilinkError::InvalidTarget {
            reason: "backslashes are not allowed; use forward slashes".to_string(),
        });
    }

    let mut normalized = Vec::new();
    for component in path.trim_matches('/').split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if normalized.pop().is_none() {
                    return Err(WikilinkError::InvalidTarget {
                        reason: "target escapes the content root".to_string(),
                    });
                }
            }
            component => normalized.push(component),
        }
    }
    let mut normalized = normalized.join("/");
    if let Some(without_extension) = normalized.strip_suffix(".md") {
        normalized = without_extension.to_string();
    }
    if normalized.is_empty() {
        return Err(WikilinkError::InvalidTarget { reason: "target is empty".to_string() });
    }
    Ok(normalized)
}

impl WikilinkResolver {
    pub fn from_targets(targets: impl IntoIterator<Item = WikilinkTarget>) -> Self {
        let mut resolver = Self::default();
        for target in targets {
            resolver.insert(target);
        }
        resolver
    }

    pub fn insert(&mut self, target: WikilinkTarget) {
        let index = self.targets.len();
        let identity = normalize_path(&target.identity)
            .expect("canonical wikilink identities must remain within the content root");
        self.paths.entry(identity.clone()).or_default().push(index);
        if let Some(stem) = identity.rsplit('/').next() {
            self.stems.entry(stem.to_string()).or_default().push(index);
        }
        for alias in &target.aliases {
            if let Ok(alias) = normalize_path(alias) {
                self.aliases.entry(alias).or_default().push(index);
            }
        }
        self.targets.push(target);
    }

    fn select(
        &self,
        candidates: &[usize],
        current_lang: &str,
        default_lang: &str,
    ) -> std::result::Result<ResolvedWikilink, WikilinkError> {
        let mut selected = candidates
            .iter()
            .copied()
            .filter(|index| self.targets[*index].lang == current_lang)
            .collect::<Vec<_>>();
        if selected.is_empty() && current_lang != default_lang {
            selected = candidates
                .iter()
                .copied()
                .filter(|index| self.targets[*index].lang == default_lang)
                .collect();
        }
        match selected.as_slice() {
            [] => Err(WikilinkError::Missing),
            [index] => {
                let target = &self.targets[*index];
                Ok(ResolvedWikilink {
                    md_path: target.source_path.clone(),
                    permalink: target.permalink.clone(),
                    track_backlink: target.track_backlink,
                })
            }
            _ => {
                let mut candidates = selected
                    .iter()
                    .map(|index| self.targets[*index].source_path.clone())
                    .collect::<Vec<_>>();
                candidates.sort();
                candidates.dedup();
                Err(WikilinkError::Ambiguous { candidates })
            }
        }
    }

    pub fn resolve(
        &self,
        source_path: &str,
        current_lang: &str,
        default_lang: &str,
        target: &str,
    ) -> std::result::Result<ResolvedWikilink, WikilinkError> {
        let (normalized, is_bare) = Self::normalize_target(source_path, target)?;

        if let Some(candidates) = self.paths.get(&normalized) {
            return self.select(candidates, current_lang, default_lang);
        }
        if let Some(candidates) = self.aliases.get(&normalized) {
            return self.select(candidates, current_lang, default_lang);
        }
        if is_bare && let Some(candidates) = self.stems.get(&normalized) {
            return self.select(candidates, current_lang, default_lang);
        }
        Err(WikilinkError::Missing)
    }

    pub fn normalize_target(
        source_path: &str,
        target: &str,
    ) -> std::result::Result<(String, bool), WikilinkError> {
        let is_relative = target.starts_with("./") || target.starts_with("../");
        let is_bare = !is_relative && !target.trim_matches('/').contains('/');
        let normalized = if is_relative {
            let source_parent = source_path.rsplit_once('/').map_or("", |(parent, _)| parent);
            normalize_path(&format!("{source_parent}/{target}"))?
        } else {
            normalize_path(target)?
        };
        Ok((normalized, is_bare))
    }
}

/// Result of a successful resolution of an internal link.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ResolvedInternalLink {
    /// Resolved link target, as absolute URL address.
    pub permalink: String,
    /// Internal path to the .md file, without the leading `@/`.
    pub md_path: String,
    /// Optional anchor target.
    /// We can check whether it exists only after all the markdown markdown is done.
    pub anchor: Option<String>,
}

/// Resolves an internal link (of the `@/posts/something.md#hey` sort) to its absolute link and
/// returns the path + anchor as well
pub fn resolve_internal_link(
    link: &str,
    permalinks: &AHashMap<String, String>,
) -> Result<ResolvedInternalLink> {
    // First we remove the @/ since that's zola specific
    let clean_link = link.replacen("@/", "", 1);
    // Then we remove any potential anchor
    // parts[0] will be the file path and parts[1] the anchor if present
    let parts = clean_link.split('#').collect::<Vec<_>>();
    // If we have slugification turned off, we might end up with some escaped characters so we need
    // to decode them first
    let decoded = percent_decode(parts[0].as_bytes()).decode_utf8_lossy().to_string();
    let target =
        permalinks.get(&decoded).ok_or_else(|| anyhow!("Relative link {} not found.", link))?;
    if parts.len() > 1 {
        Ok(ResolvedInternalLink {
            permalink: format!("{}#{}", target, parts[1]),
            md_path: decoded,
            anchor: Some(parts[1].to_string()),
        })
    } else {
        Ok(ResolvedInternalLink { permalink: target.to_string(), md_path: decoded, anchor: None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::resolve_internal_link;

    fn resolver() -> WikilinkResolver {
        WikilinkResolver::from_targets([
            WikilinkTarget {
                source_path: "guides/alpha.md".to_string(),
                identity: "guides/alpha".to_string(),
                permalink: "/guides/alpha/".to_string(),
                aliases: vec!["legacy-alpha".to_string()],
                lang: "en".to_string(),
                track_backlink: true,
            },
            WikilinkTarget {
                source_path: "old-alpha.md".to_string(),
                identity: "old-alpha".to_string(),
                permalink: "/old-alpha/".to_string(),
                aliases: Vec::new(),
                lang: "en".to_string(),
                track_backlink: true,
            },
            WikilinkTarget {
                source_path: "guides/beta.md".to_string(),
                identity: "guides/beta".to_string(),
                permalink: "/guides/beta/".to_string(),
                aliases: Vec::new(),
                lang: "en".to_string(),
                track_backlink: true,
            },
            WikilinkTarget {
                source_path: "guides/duplicate.md".to_string(),
                identity: "guides/duplicate".to_string(),
                permalink: "/guides/duplicate/".to_string(),
                aliases: Vec::new(),
                lang: "en".to_string(),
                track_backlink: true,
            },
            WikilinkTarget {
                source_path: "archive/duplicate.md".to_string(),
                identity: "archive/duplicate".to_string(),
                permalink: "/archive/duplicate/".to_string(),
                aliases: Vec::new(),
                lang: "en".to_string(),
                track_backlink: true,
            },
            WikilinkTarget {
                source_path: "guides/alpha.fr.md".to_string(),
                identity: "guides/alpha".to_string(),
                permalink: "/fr/guides/alpha/".to_string(),
                aliases: vec!["ancien-alpha".to_string()],
                lang: "fr".to_string(),
                track_backlink: true,
            },
        ])
    }

    #[test]
    fn wikilinks_follow_path_alias_and_unique_stem_precedence() {
        let resolver = resolver();
        assert_eq!(
            resolver.resolve("index.md", "en", "en", "guides/alpha.md").unwrap().md_path,
            "guides/alpha.md"
        );
        assert_eq!(
            resolver.resolve("guides/alpha.md", "en", "en", "./beta").unwrap().md_path,
            "guides/beta.md"
        );
        assert_eq!(
            resolver.resolve("guides/nested/page.md", "en", "en", "../alpha").unwrap().md_path,
            "guides/alpha.md"
        );
        assert_eq!(
            resolver.resolve("index.md", "en", "en", "legacy-alpha").unwrap().md_path,
            "guides/alpha.md"
        );
        assert_eq!(
            resolver.resolve("index.md", "en", "en", "old-alpha").unwrap().md_path,
            "old-alpha.md"
        );
        assert_eq!(
            resolver.resolve("index.md", "en", "en", "beta").unwrap().md_path,
            "guides/beta.md"
        );
    }

    #[test]
    fn wikilinks_preserve_ambiguity_and_exact_case() {
        let resolver = resolver();
        assert_eq!(
            resolver.resolve("index.md", "en", "en", "duplicate"),
            Err(WikilinkError::Ambiguous {
                candidates: vec![
                    "archive/duplicate.md".to_string(),
                    "guides/duplicate.md".to_string(),
                ]
            })
        );
        assert_eq!(resolver.resolve("index.md", "en", "en", "Beta"), Err(WikilinkError::Missing));
    }

    #[test]
    fn wikilinks_resolve_in_current_language_then_default_language() {
        let resolver = resolver();
        assert_eq!(
            resolver.resolve("index.fr.md", "fr", "en", "guides/alpha").unwrap().md_path,
            "guides/alpha.fr.md"
        );
        assert_eq!(
            resolver.resolve("index.fr.md", "fr", "en", "beta").unwrap().md_path,
            "guides/beta.md"
        );
    }

    #[test]
    fn wikilinks_reject_host_dependent_or_escaping_paths() {
        let resolver = resolver();
        assert!(matches!(
            resolver.resolve("guides/page.md", "en", "en", "..\\alpha"),
            Err(WikilinkError::InvalidTarget { .. })
        ));
        assert!(matches!(
            resolver.resolve("guides/page.md", "en", "en", "../../outside"),
            Err(WikilinkError::InvalidTarget { .. })
        ));
    }

    #[test]
    fn wikilink_path_normalization_holds_for_generated_component_sequences() {
        let components = ["alpha", "beta", "δelta", ".", "", ".."];

        for mut state in 0_u64..4096 {
            let mut path = String::new();
            let mut expected = Vec::new();
            let mut escaped = false;

            for _ in 0..12 {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                let component = components[(state as usize) % components.len()];
                if !path.is_empty() {
                    path.push('/');
                }
                path.push_str(component);

                match component {
                    "" | "." => {}
                    ".." if expected.pop().is_none() => escaped = true,
                    ".." => {}
                    value => expected.push(value),
                }
            }

            let result = normalize_path(&path);
            if escaped || expected.is_empty() {
                assert!(matches!(result, Err(WikilinkError::InvalidTarget { .. })), "{path}");
            } else {
                assert_eq!(result.unwrap(), expected.join("/"), "{path}");
            }
        }

        for target in ["alpha", "δelta/path", "../outside", "alpha.md"] {
            for byte_index in target.char_indices().map(|(index, _)| index).chain([target.len()]) {
                let mut with_backslash = target.to_string();
                with_backslash.insert(byte_index, '\\');
                assert!(matches!(
                    normalize_path(&with_backslash),
                    Err(WikilinkError::InvalidTarget { .. })
                ));
            }
        }

        for parent_depth in 0..64 {
            let path = format!("{}outside", "../".repeat(parent_depth));
            let result = normalize_path(&path);
            if parent_depth == 0 {
                assert_eq!(result.unwrap(), "outside");
            } else {
                assert!(matches!(result, Err(WikilinkError::InvalidTarget { .. })), "{path}");
            }
        }
    }

    #[test]
    fn can_resolve_valid_internal_link() {
        let mut permalinks = AHashMap::new();
        permalinks.insert("pages/about.md".to_string(), "https://vincent.is/about".to_string());
        let res = resolve_internal_link("@/pages/about.md", &permalinks).unwrap();
        assert_eq!(res.permalink, "https://vincent.is/about");
    }

    #[test]
    fn can_resolve_valid_root_internal_link() {
        let mut permalinks = AHashMap::new();
        permalinks.insert("about.md".to_string(), "https://vincent.is/about".to_string());
        let res = resolve_internal_link("@/about.md", &permalinks).unwrap();
        assert_eq!(res.permalink, "https://vincent.is/about");
    }

    #[test]
    fn can_resolve_internal_links_with_anchors() {
        let mut permalinks = AHashMap::new();
        permalinks.insert("pages/about.md".to_string(), "https://vincent.is/about".to_string());
        let res = resolve_internal_link("@/pages/about.md#hello", &permalinks).unwrap();
        assert_eq!(res.permalink, "https://vincent.is/about#hello");
        assert_eq!(res.md_path, "pages/about.md".to_string());
        assert_eq!(res.anchor, Some("hello".to_string()));
    }

    #[test]
    fn can_resolve_escaped_internal_links() {
        let mut permalinks = AHashMap::new();
        permalinks.insert(
            "pages/about space.md".to_string(),
            "https://vincent.is/about%20space/".to_string(),
        );
        let res = resolve_internal_link("@/pages/about%20space.md#hello", &permalinks).unwrap();
        assert_eq!(res.permalink, "https://vincent.is/about%20space/#hello");
        assert_eq!(res.md_path, "pages/about space.md".to_string());
        assert_eq!(res.anchor, Some("hello".to_string()));
    }

    #[test]
    fn errors_resolve_inexistent_internal_link() {
        let res = resolve_internal_link("@/pages/about.md#hello", &AHashMap::new());
        assert!(res.is_err());
    }
}
