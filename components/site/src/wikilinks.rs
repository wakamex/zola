use ahash::AHashMap;
use content::{Library, Taxonomy};
use markdown::{WikilinkResolver, WikilinkTarget};

/// Build wikilink targets from content, published colocated assets, and rendered taxonomy terms.
///
/// Render-disabled content remains addressable to preserve the lookup behavior of the original
/// permalink-based implementation. Asset targets require their exact qualified path and are only
/// included when their owner emits the asset. Taxonomy terms also require their exact qualified
/// output path and are included only when their taxonomy is rendered.
pub fn build_wikilinks(library: &Library, taxonomies: &[Taxonomy]) -> WikilinkResolver {
    let pages = library
        .pages
        .values()
        .map(|page| WikilinkTarget::content(page.file.relative.clone(), page.meta.aliases.clone()));
    let sections = library.sections.values().map(|section| {
        WikilinkTarget::content(section.file.relative.clone(), section.meta.aliases.clone())
    });
    let asset_owners = library
        .pages
        .values()
        .filter(|page| page.meta.render)
        .map(|page| (&page.file.relative, &page.permalink))
        .chain(
            library
                .sections
                .values()
                .filter(|section| section.meta.render && section.meta.redirect_to.is_none())
                .map(|section| (&section.file.relative, &section.permalink)),
        )
        .collect::<AHashMap<_, _>>();
    let assets = library.colocated_assets.iter().filter_map(|(path, (owner, relative))| {
        let permalink = asset_owners.get(owner)?;
        Some(WikilinkTarget::output(path.clone(), format!("{permalink}{relative}")))
    });
    let taxonomy_terms = taxonomies
        .iter()
        .filter(|taxonomy| taxonomy.kind.render)
        .flat_map(|taxonomy| &taxonomy.items)
        .map(|term| {
            WikilinkTarget::output(term.path.trim_matches('/').to_string(), term.permalink.clone())
        });
    WikilinkResolver::from_targets(pages.chain(sections).chain(assets).chain(taxonomy_terms))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use config::{Config, TaxonomyConfig};
    use content::{Library, Page, PageFrontMatter, Section, SectionFrontMatter, TaxonomyTerm};
    use markdown::{ResolvedWikilink, WikilinkError};

    use super::*;

    fn page(path: &str, aliases: &[&str], render: bool) -> Page {
        let mut page = Page::new(
            Path::new(&format!("content/{path}")),
            PageFrontMatter::default(),
            Path::new(""),
        );
        page.file.relative = path.to_string();
        page.meta.aliases = aliases.iter().map(|alias| alias.to_string()).collect();
        page.meta.render = render;
        page
    }

    fn section(path: &str, aliases: &[&str], redirect_to: Option<&str>) -> Section {
        let mut section = Section::new(
            Path::new(&format!("content/{path}")),
            SectionFrontMatter::default(),
            Path::new(""),
        );
        section.file.relative = path.to_string();
        section.meta.aliases = aliases.iter().map(|alias| alias.to_string()).collect();
        section.meta.redirect_to = redirect_to.map(str::to_string);
        section
    }

    fn asset(library: &mut Library, path: &str, owner: &str, relative: &str) {
        library
            .colocated_assets
            .insert(path.to_string(), (owner.to_string(), relative.to_string()));
    }

    fn taxonomy(slug: &str, term: &str, render: bool) -> Taxonomy {
        let path = format!("/{slug}/");
        let term_path = format!("{path}{term}/");
        Taxonomy {
            kind: TaxonomyConfig { render, ..Default::default() },
            lang: "en".to_string(),
            slug: slug.to_string(),
            path: path.clone(),
            permalink: path,
            items: vec![TaxonomyTerm {
                name: term.to_string(),
                slug: term.to_string(),
                path: term_path.clone(),
                permalink: term_path,
                pages: Vec::new(),
            }],
        }
    }

    #[test]
    fn includes_aliases_and_render_disabled_content() {
        let config = Config::default_for_test();
        let mut library = Library::new(&config);
        library.insert_page(page("guides/quickstart.md", &["/start/"], true));
        library.insert_page(page("notes/private.md", &["/private-note/"], false));
        library.insert_section(section("docs/_index.md", &["/documentation/"], None));

        let resolver = build_wikilinks(&library, &[]);
        assert_eq!(
            resolver.resolve("start"),
            Ok(ResolvedWikilink::Content("guides/quickstart.md"))
        );
        assert_eq!(
            resolver.resolve("notes/private"),
            Ok(ResolvedWikilink::Content("notes/private.md"))
        );
        assert_eq!(
            resolver.resolve("private-note"),
            Ok(ResolvedWikilink::Content("notes/private.md"))
        );
        assert_eq!(
            resolver.resolve("docs/_index"),
            Ok(ResolvedWikilink::Content("docs/_index.md"))
        );
        assert_eq!(
            resolver.resolve("documentation"),
            Ok(ResolvedWikilink::Content("docs/_index.md"))
        );
    }

    #[test]
    fn includes_only_published_assets_by_qualified_path() {
        let config = Config::default_for_test();
        let mut library = Library::new(&config);
        let mut guides = page("guides/index.md", &[], true);
        guides.permalink = "/guides/".to_string();
        library.insert_page(guides);
        library.insert_page(page("private/index.md", &[], false));
        let mut examples = section("examples/_index.md", &[], None);
        examples.permalink = "/examples/".to_string();
        library.insert_section(examples);
        let mut redirected = section("old/_index.md", &[], Some("/new/"));
        redirected.permalink = "/old/".to_string();
        library.insert_section(redirected);
        asset(&mut library, "guides/source.pdf", "guides/index.md", "source.pdf");
        asset(&mut library, "private/secret.pdf", "private/index.md", "secret.pdf");
        asset(&mut library, "examples/demo.zip", "examples/_index.md", "demo.zip");
        asset(&mut library, "old/archive.zip", "old/_index.md", "archive.zip");

        let resolver = build_wikilinks(&library, &[]);
        assert_eq!(
            resolver.resolve("guides/source.pdf"),
            Ok(ResolvedWikilink::Output("/guides/source.pdf"))
        );
        assert_eq!(resolver.resolve("source.pdf"), Err(WikilinkError::Missing));
        assert_eq!(resolver.resolve("private/secret.pdf"), Err(WikilinkError::Missing));
        assert_eq!(
            resolver.resolve("examples/demo.zip"),
            Ok(ResolvedWikilink::Output("/examples/demo.zip"))
        );
        assert_eq!(resolver.resolve("old/archive.zip"), Err(WikilinkError::Missing));
    }

    #[test]
    fn includes_only_rendered_taxonomy_terms_by_qualified_path() {
        let config = Config::default_for_test();
        let library = Library::new(&config);
        let taxonomies =
            [taxonomy("tags", "evidence", true), taxonomy("internal", "secret", false)];

        let resolver = build_wikilinks(&library, &taxonomies);
        assert_eq!(
            resolver.resolve("tags/evidence"),
            Ok(ResolvedWikilink::Output("/tags/evidence/"))
        );
        assert_eq!(resolver.resolve("evidence"), Err(WikilinkError::Missing));
        assert_eq!(resolver.resolve("internal/secret"), Err(WikilinkError::Missing));
    }
}
