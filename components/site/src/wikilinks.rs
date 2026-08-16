use ahash::AHashMap;
use content::{Library, Taxonomy};
use utils::site::{WikilinkResolver, WikilinkTarget};

fn identity(source_path: &str, lang: &str) -> String {
    let without_extension = source_path.strip_suffix(".md").unwrap_or(source_path);
    without_extension.strip_suffix(&format!(".{lang}")).unwrap_or(without_extension).to_string()
}

pub fn build_wikilinks(library: &Library, taxonomies: &[Taxonomy]) -> WikilinkResolver {
    let pages = library.pages.values().filter(|page| page.meta.render).map(|page| WikilinkTarget {
        source_path: page.file.relative.clone(),
        identity: identity(&page.file.relative, &page.lang),
        permalink: page.permalink.clone(),
        aliases: page.meta.aliases.clone(),
        lang: page.lang.clone(),
        track_backlink: true,
    });
    let sections = library.sections.values().filter(|section| section.meta.render).map(|section| {
        WikilinkTarget {
            source_path: section.file.relative.clone(),
            identity: identity(&section.file.relative, &section.lang),
            permalink: section.permalink.clone(),
            aliases: section.meta.aliases.clone(),
            lang: section.lang.clone(),
            track_backlink: true,
        }
    });
    let owners = library
        .pages
        .values()
        .map(|page| (&page.file.relative, (&page.permalink, &page.lang)))
        .chain(
            library
                .sections
                .values()
                .map(|section| (&section.file.relative, (&section.permalink, &section.lang))),
        )
        .collect::<AHashMap<_, _>>();
    let assets = library.colocated_assets.iter().filter_map(|(path, (owner, relative))| {
        let (permalink, lang) = owners.get(owner)?;
        Some(WikilinkTarget {
            source_path: path.clone(),
            identity: path.clone(),
            permalink: format!("{permalink}{relative}"),
            aliases: Vec::new(),
            lang: (*lang).clone(),
            track_backlink: false,
        })
    });
    let taxonomy_terms = taxonomies.iter().flat_map(|taxonomy| {
        taxonomy.items.iter().map(|term| {
            let identity = term.path.trim_matches('/').to_string();
            WikilinkTarget {
                source_path: identity.clone(),
                identity,
                permalink: term.permalink.clone(),
                aliases: Vec::new(),
                lang: taxonomy.lang.clone(),
                track_backlink: false,
            }
        })
    });
    WikilinkResolver::from_targets(pages.chain(sections).chain(assets).chain(taxonomy_terms))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use config::Config;
    use content::{Library, Page, PageFrontMatter};

    use super::*;

    #[test]
    fn removes_explicit_language_suffixes_from_target_identities() {
        assert_eq!(identity("guides/page.en.md", "en"), "guides/page");
        assert_eq!(identity("guides/page.fr.md", "fr"), "guides/page");
        assert_eq!(identity("guides/page.md", "en"), "guides/page");
    }

    #[test]
    fn builds_records_from_published_library_content() {
        let config = Config::default_for_test();
        let mut library = Library::new(&config);
        let mut page = Page::new(
            Path::new("content/guides/quickstart.md"),
            PageFrontMatter::default(),
            Path::new(""),
        );
        page.file.relative = "guides/quickstart.md".to_string();
        page.lang = "en".to_string();
        page.permalink = "/guides/quickstart/".to_string();
        page.meta.aliases = vec!["start".to_string()];
        library.insert_page(page);

        let resolver = build_wikilinks(&library, &[]);
        assert_eq!(
            resolver.resolve("index.md", "en", "en", "quickstart").unwrap().md_path,
            "guides/quickstart.md"
        );
        assert_eq!(
            resolver.resolve("index.md", "en", "en", "start").unwrap().md_path,
            "guides/quickstart.md"
        );
    }
}
