use ahash::AHashMap;

#[derive(Clone, Debug)]
pub enum WikilinkTarget {
    Content { source_path: String, aliases: Vec<String> },
    Output { path: String, permalink: String },
}

impl WikilinkTarget {
    pub fn content(source_path: impl Into<String>, aliases: Vec<String>) -> Self {
        Self::Content { source_path: source_path.into(), aliases }
    }

    pub fn output(path: impl Into<String>, permalink: impl Into<String>) -> Self {
        Self::Output { path: path.into(), permalink: permalink.into() }
    }

    fn key(&self) -> Option<String> {
        match self {
            Self::Content { source_path, .. } => normalize_source_path(source_path),
            Self::Output { path, .. } => normalize_lookup(path).map(str::to_string),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ResolvedWikilink<'a> {
    Content(&'a str),
    Output(&'a str),
}

#[derive(Debug, PartialEq, Eq)]
pub enum WikilinkError {
    Missing,
    Ambiguous { candidates: Vec<String> },
}

/// Resolves content and exact output-path wikilinks.
///
/// Content targets are indexed in three forms that point back to the full source path:
///
/// 1. The source path without its `.md` extension, such as `docs/overview`.
/// 2. Each existing Zola alias, with surrounding slashes removed, such as `overview-old`.
/// 3. The bare stem, such as `overview`, when it differs from the full source path.
///
/// A stem that is identical to the full path is not indexed twice. Colliding aliases and stems are
/// retained so resolution can suggest a qualified key for every matching source path instead of
/// choosing one arbitrarily. Exact paths take precedence over aliases, and aliases take precedence
/// over stems.
///
/// Output targets are indexed only by their exact qualified path and never create stem aliases.
#[derive(Clone, Debug, Default)]
pub struct WikilinkResolver {
    targets: Vec<WikilinkTarget>,
    paths: AHashMap<String, Vec<usize>>,
    aliases: AHashMap<String, Vec<usize>>,
    stems: AHashMap<String, Vec<usize>>,
}

fn normalize_source_path(value: &str) -> Option<String> {
    let normalized = value.trim_matches('/');
    let normalized = normalized.strip_suffix(".md").unwrap_or(normalized);
    (!normalized.is_empty()).then(|| normalized.to_string())
}

fn normalize_lookup(value: &str) -> Option<&str> {
    let normalized = value.trim_matches('/');
    (!normalized.is_empty()).then_some(normalized)
}

impl WikilinkResolver {
    pub fn from_targets(targets: impl IntoIterator<Item = WikilinkTarget>) -> Self {
        let mut resolver = Self::default();
        for target in targets {
            resolver.insert(target);
        }
        resolver
    }

    fn insert(&mut self, target: WikilinkTarget) {
        let Some(identity) = target.key() else { return };
        let index = self.targets.len();

        // Store content paths without their Markdown extension and output paths exactly as written.
        self.paths.entry(identity.clone()).or_default().push(index);

        if let WikilinkTarget::Content { aliases, .. } = &target {
            // Content retains the original bare-stem shorthand. Output targets require their exact,
            // qualified path so they cannot introduce global shorthand or content-stem collisions.
            if let Some(stem) = identity.rsplit('/').next()
                && stem != identity
            {
                self.stems.entry(stem.to_string()).or_default().push(index);
            }

            // Aliases are existing Zola output paths, normalized to wikilink syntax.
            for alias in aliases {
                if let Some(alias) = normalize_lookup(alias) {
                    let candidates = self.aliases.entry(alias.to_string()).or_default();
                    if !candidates.contains(&index) {
                        candidates.push(index);
                    }
                }
            }
        }
        self.targets.push(target);
    }

    fn resolved(&self, index: usize) -> ResolvedWikilink<'_> {
        match &self.targets[index] {
            WikilinkTarget::Content { source_path, .. } => ResolvedWikilink::Content(source_path),
            WikilinkTarget::Output { permalink, .. } => ResolvedWikilink::Output(permalink),
        }
    }

    fn select(
        &self,
        candidates: &[usize],
    ) -> std::result::Result<ResolvedWikilink<'_>, WikilinkError> {
        if let [index] = candidates {
            return Ok(self.resolved(*index));
        }

        let mut paths = candidates
            .iter()
            .map(|index| self.targets[*index].key().expect("indexed targets have normalized paths"))
            .collect::<Vec<_>>();
        paths.sort_unstable();
        paths.dedup();

        match paths.as_slice() {
            [] => Err(WikilinkError::Missing),
            [_] => Ok(self.resolved(candidates[0])),
            _ => Err(WikilinkError::Ambiguous { candidates: paths }),
        }
    }

    pub fn resolve(
        &self,
        target: &str,
    ) -> std::result::Result<ResolvedWikilink<'_>, WikilinkError> {
        let Some(normalized) = normalize_lookup(target) else {
            return Err(WikilinkError::Missing);
        };

        if let Some(candidates) = self.paths.get(normalized) {
            return self.select(candidates);
        }
        if let Some(candidates) = self.aliases.get(normalized) {
            return self.select(candidates);
        }
        if !normalized.contains('/')
            && let Some(candidates) = self.stems.get(normalized)
        {
            return self.select(candidates);
        }
        Err(WikilinkError::Missing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(path: &str) -> WikilinkTarget {
        WikilinkTarget::content(path, Vec::new())
    }

    fn assert_content(resolver: &WikilinkResolver, target: &str, expected: &str) {
        assert_eq!(resolver.resolve(target), Ok(ResolvedWikilink::Content(expected)));
    }

    #[test]
    fn resolves_paths_aliases_and_unique_stems() {
        let resolver = WikilinkResolver::from_targets([
            target("blog/overview.md"),
            target("docs/overview.md"),
            target("about.md"),
            target("blog/_index.md"),
            target("_index.md"),
            WikilinkTarget::content("guides/quickstart.md", vec!["/start/".to_string()]),
        ]);

        // Full paths always resolve.
        assert_content(&resolver, "blog/overview", "blog/overview.md");
        assert_content(&resolver, "docs/overview", "docs/overview.md");
        assert_content(&resolver, "about", "about.md");
        assert_content(&resolver, "blog/_index", "blog/_index.md");
        assert_content(&resolver, "guides/quickstart", "guides/quickstart.md");

        // Unique stems and aliases resolve to the same source path.
        assert_content(&resolver, "quickstart", "guides/quickstart.md");
        assert_content(&resolver, "start", "guides/quickstart.md");

        // The exact root path takes precedence over the colliding blog/_index.md stem.
        assert_content(&resolver, "_index", "_index.md");

        // A stem identical to its full path is not indexed separately.
        assert!(!resolver.stems.contains_key("about"));

        // Colliding bare stems suggest valid qualified keys for an actionable error.
        assert_eq!(
            resolver.resolve("overview"),
            Err(WikilinkError::Ambiguous {
                candidates: vec!["blog/overview".to_string(), "docs/overview".to_string()],
            })
        );

        // Accepting Markdown extensions would be an unrelated syntax expansion.
        assert_eq!(resolver.resolve("about.md"), Err(WikilinkError::Missing));
    }
}
