use content::Library;
use utils::site::{WikilinkResolver, WikilinkTarget};

/// Build a wikilink resolver from every renderable page and section in the library.
///
/// The resolver preserves each content path and also reuses the aliases already declared in front
/// matter. Bare stems are resolved only when they identify one content file.
pub fn build_wikilinks(library: &Library) -> WikilinkResolver {
    let pages = library.pages.values().filter(|page| page.meta.render).map(|page| WikilinkTarget {
        source_path: page.file.relative.clone(),
        permalink: page.permalink.clone(),
        aliases: page.meta.aliases.clone(),
    });
    let sections = library.sections.values().filter(|section| section.meta.render).map(|section| {
        WikilinkTarget {
            source_path: section.file.relative.clone(),
            permalink: section.permalink.clone(),
            aliases: section.meta.aliases.clone(),
        }
    });
    WikilinkResolver::from_targets(pages.chain(sections))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use config::Config;
    use content::{Library, Page, PageFrontMatter};
    use utils::site::WikilinkError;

    use super::*;

    fn assert_resolves(resolver: &WikilinkResolver, target: &str, expected: &str) {
        assert_eq!(resolver.resolve(target).unwrap().md_path, expected);
    }

    fn page(path: &str, permalink: &str, aliases: &[&str]) -> Page {
        let mut page = Page::new(
            Path::new(&format!("content/{path}")),
            PageFrontMatter::default(),
            Path::new(""),
        );
        page.file.relative = path.to_string();
        page.permalink = permalink.to_string();
        page.meta.aliases = aliases.iter().map(|alias| alias.to_string()).collect();
        page
    }

    #[test]
    fn build_wikilinks_lookups() {
        let config = Config::default_for_test();
        let mut library = Library::new(&config);
        library.insert_page(page("blog/overview.md", "/blog/overview/", &[]));
        library.insert_page(page("docs/overview.md", "/docs/overview/", &[]));
        library.insert_page(page("about.md", "/about/", &[]));
        library.insert_page(page("blog/_index.md", "/blog/", &[]));
        library.insert_page(page("_index.md", "/", &[]));
        library.insert_page(page("guides/quickstart.md", "/guides/quickstart/", &["/start/"]));

        let resolver = build_wikilinks(&library);

        // Full paths always resolve.
        assert_resolves(&resolver, "blog/overview", "blog/overview.md");
        assert_resolves(&resolver, "docs/overview", "docs/overview.md");
        assert_resolves(&resolver, "about", "about.md");
        assert_resolves(&resolver, "blog/_index", "blog/_index.md");
        assert_resolves(&resolver, "guides/quickstart", "guides/quickstart.md");

        // Unique stems and aliases resolve to the same content path.
        assert_resolves(&resolver, "quickstart", "guides/quickstart.md");
        assert_resolves(&resolver, "start", "guides/quickstart.md");

        // An exact path takes precedence over the colliding blog/_index.md stem.
        assert_resolves(&resolver, "_index", "_index.md");

        // A path whose stem is the whole path remains a single, unambiguous target.
        assert_resolves(&resolver, "about", "about.md");

        // Colliding bare stems report every path rather than resolving arbitrarily.
        assert_eq!(
            resolver.resolve("overview"),
            Err(WikilinkError::Ambiguous {
                candidates: vec!["blog/overview.md".to_string(), "docs/overview.md".to_string(),],
            })
        );
    }

    #[test]
    fn reports_every_candidate_for_an_ambiguous_stem() {
        let config = Config::default_for_test();
        let mut library = Library::new(&config);
        library.insert_page(page("guides/duplicate.md", "/guides/duplicate/", &[]));
        library.insert_page(page("archive/duplicate.md", "/archive/duplicate/", &[]));

        assert_eq!(
            build_wikilinks(&library).resolve("duplicate"),
            Err(WikilinkError::Ambiguous {
                candidates: vec![
                    "archive/duplicate.md".to_string(),
                    "guides/duplicate.md".to_string(),
                ],
            })
        );
    }
}
