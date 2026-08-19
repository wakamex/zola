use ahash::AHashMap;
use percent_encoding::percent_decode;

use errors::{Result, anyhow};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WikilinkTarget {
    pub source_path: String,
    pub permalink: String,
    pub aliases: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedWikilink {
    pub md_path: String,
    pub permalink: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WikilinkError {
    Missing,
    Ambiguous { candidates: Vec<String> },
}

/// Resolves wikilinks by content path, output alias, or bare stem.
///
/// For each target, the resolver indexes three forms that point back to the full source path:
///
/// 1. The content path without its `.md` extension, such as `docs/overview`.
/// 2. Each existing Zola alias, with surrounding slashes removed, such as `overview-old`.
/// 3. The bare stem, such as `overview`, when it differs from the full content path.
///
/// A stem that is identical to the full path is not indexed twice. Colliding aliases and stems are
/// retained so resolution can report every matching source path instead of choosing one
/// arbitrarily. Exact paths take precedence over aliases, and aliases take precedence over stems.
#[derive(Clone, Debug, Default)]
pub struct WikilinkResolver {
    targets: Vec<WikilinkTarget>,
    paths: AHashMap<String, Vec<usize>>,
    aliases: AHashMap<String, Vec<usize>>,
    stems: AHashMap<String, Vec<usize>>,
}

fn normalize_wikilink_key(value: &str) -> Option<String> {
    let normalized = value.trim_matches('/');
    let normalized = normalized.strip_suffix(".md").unwrap_or(normalized);
    (!normalized.is_empty()).then(|| normalized.to_string())
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
        let Some(identity) = normalize_wikilink_key(&target.source_path) else {
            return;
        };

        // Store the full content path without its Markdown extension.
        self.paths.entry(identity.clone()).or_default().push(index);

        // A bare stem is useful only when it differs from the full path. Keeping it in a separate
        // index lets resolution report collisions without overwriting an exact path.
        if let Some(stem) = identity.rsplit('/').next()
            && stem != identity
        {
            self.stems.entry(stem.to_string()).or_default().push(index);
        }

        // Aliases are existing Zola output paths, normalized to wikilink syntax.
        for alias in &target.aliases {
            if let Some(alias) = normalize_wikilink_key(alias) {
                self.aliases.entry(alias).or_default().push(index);
            }
        }
        self.targets.push(target);
    }

    fn select(&self, candidates: &[usize]) -> std::result::Result<ResolvedWikilink, WikilinkError> {
        let mut paths = candidates
            .iter()
            .map(|index| self.targets[*index].source_path.clone())
            .collect::<Vec<_>>();
        paths.sort();
        paths.dedup();

        match paths.as_slice() {
            [] => Err(WikilinkError::Missing),
            [path] => {
                let target = candidates
                    .iter()
                    .map(|index| &self.targets[*index])
                    .find(|target| target.source_path == *path)
                    .expect("wikilink candidate disappeared");
                Ok(ResolvedWikilink {
                    md_path: target.source_path.clone(),
                    permalink: target.permalink.clone(),
                })
            }
            _ => Err(WikilinkError::Ambiguous { candidates: paths }),
        }
    }

    pub fn resolve(&self, target: &str) -> std::result::Result<ResolvedWikilink, WikilinkError> {
        let Some(normalized) = normalize_wikilink_key(target) else {
            return Err(WikilinkError::Missing);
        };

        // Preserve the original lookup behavior: a full path always wins over a matching stem.
        if let Some(candidates) = self.paths.get(&normalized) {
            return self.select(candidates);
        }
        if let Some(candidates) = self.aliases.get(&normalized) {
            return self.select(candidates);
        }
        if !normalized.contains('/')
            && let Some(candidates) = self.stems.get(&normalized)
        {
            return self.select(candidates);
        }
        Err(WikilinkError::Missing)
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
