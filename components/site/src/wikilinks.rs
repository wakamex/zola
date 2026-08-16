use config::Config;
use content::Library;
use utils::site::{WikilinkResolver, WikilinkTarget};

fn identity(source_path: &str, lang: &str, default_lang: &str) -> String {
    let without_extension = source_path.strip_suffix(".md").unwrap_or(source_path);
    if lang != default_lang {
        without_extension.strip_suffix(&format!(".{lang}")).unwrap_or(without_extension).to_string()
    } else {
        without_extension.to_string()
    }
}

pub fn build_wikilinks(library: &Library, config: &Config) -> WikilinkResolver {
    let pages = library.pages.values().filter(|page| page.meta.render).map(|page| WikilinkTarget {
        source_path: page.file.relative.clone(),
        identity: identity(&page.file.relative, &page.lang, &config.default_language),
        permalink: page.permalink.clone(),
        aliases: page.meta.aliases.clone(),
        lang: page.lang.clone(),
    });
    let sections = library.sections.values().filter(|section| section.meta.render).map(|section| {
        WikilinkTarget {
            source_path: section.file.relative.clone(),
            identity: identity(&section.file.relative, &section.lang, &config.default_language),
            permalink: section.permalink.clone(),
            aliases: section.meta.aliases.clone(),
            lang: section.lang.clone(),
        }
    });
    WikilinkResolver::from_targets(pages.chain(sections))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use content::{Library, Page, PageFrontMatter};

    use super::*;

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

        let resolver = build_wikilinks(&library, &config);
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
