mod elasticlunr;
mod export;
mod fuse;

use content::{Library, Page, Section};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use time::OffsetDateTime;

pub use elasticlunr::{ELASTICLUNR_JS, build_index as build_elasticlunr};
pub use export::write_content_export;
pub use fuse::build_index as build_fuse;

static AMMONIA: LazyLock<ammonia::Builder<'static>> = LazyLock::new(|| {
    let mut clean_content = HashSet::new();
    clean_content.insert("script");
    clean_content.insert("style");
    clean_content.insert("pre");
    let mut builder = ammonia::Builder::new();
    builder
        .tags(HashSet::new())
        .tag_attributes(HashMap::new())
        .generic_attributes(HashSet::new())
        .link_rel(None)
        .allowed_classes(HashMap::new())
        .clean_content_tags(clean_content);
    builder
});

/// Uses ammonia to clean the body, and truncates it to `truncate_content_length`.
/// Removes extra whitespace and blank lines.
pub fn clean_and_truncate_body(truncate_content_length: Option<usize>, body: &str) -> String {
    let mut clean = AMMONIA
        .clean(body)
        .to_string()
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if let Some(new_len) = truncate_content_length {
        clean.truncate(clean.char_indices().nth(new_len).map(|(i, _)| i).unwrap_or(clean.len()))
    }
    clean
}

/// A single page or section that should be included in the search index
/// for a specific language.
struct IndexItem<'a> {
    url: &'a str,
    title: &'a Option<String>,
    description: &'a Option<String>,
    content: &'a String,
    datetime: &'a Option<OffsetDateTime>,
    path: &'a String,
}

/// A rendered page or authored section whose parent section permits search indexing.
pub(crate) enum SearchablePublication<'a> {
    Page(&'a Page),
    Section(&'a Section),
}

impl SearchablePublication<'_> {
    pub(crate) fn source(&self) -> &str {
        match self {
            Self::Page(page) => &page.file.relative,
            Self::Section(section) => &section.file.relative,
        }
    }
}

/// Collect the canonical set of published content eligible for search.
///
/// Pages are reached through their direct parent section so a section with
/// `in_search_index = false` also excludes its direct pages. The section's
/// computed page roster already excludes hidden and unrendered pages.
pub(crate) fn collect_searchable_publications<'a>(
    lang: Option<&str>,
    library: &'a Library,
) -> Vec<SearchablePublication<'a>> {
    let mut publications = Vec::new();
    let mut seen_pages = HashSet::new();

    for section in library.sections.values() {
        if lang.is_some_and(|lang| section.lang != lang) || !section.meta.in_search_index {
            continue;
        }

        if section.meta.render
            && section.meta.redirect_to.is_none()
            && !section.hidden
            && !section.implicit
            && section.file.path.is_file()
        {
            publications.push(SearchablePublication::Section(section));
        }

        for page_path in &section.pages {
            if !seen_pages.insert(page_path.clone()) {
                continue;
            }
            let page = &library.pages[page_path];
            if page.meta.in_search_index && lang.map_or(true, |lang| page.lang == lang) {
                publications.push(SearchablePublication::Page(page));
            }
        }
    }

    publications.sort_by(|left, right| left.source().cmp(right.source()));
    publications
}

/// Collect all pages and sections which should be included in the search index
/// of a given language.
fn collect_index_items<'a>(lang: &str, library: &'a Library) -> Vec<IndexItem<'a>> {
    collect_searchable_publications(Some(lang), library)
        .into_iter()
        .map(|publication| match publication {
            SearchablePublication::Section(section) => IndexItem {
                url: &section.permalink,
                title: &section.meta.title,
                datetime: &None,
                description: &section.meta.description,
                content: &section.content,
                path: &section.path,
            },
            SearchablePublication::Page(page) => IndexItem {
                url: &page.permalink,
                title: &page.meta.title,
                datetime: &page.meta.datetime,
                description: &page.meta.description,
                content: &page.content,
                path: &page.path,
            },
        })
        .collect()
}

#[cfg(test)]
#[test]
fn clean_and_truncate_body_test() {
    assert_eq!(clean_and_truncate_body(None, "hello world"), "hello world");
    assert_eq!(
        clean_and_truncate_body(None, "hello <script>alert('xss')</script> world"),
        "hello world"
    );
    assert_eq!(clean_and_truncate_body(Some(100), "hello"), "hello");
    assert_eq!(clean_and_truncate_body(Some(2), "hello"), "he");
    assert_eq!(clean_and_truncate_body(Some(6), "hello \u{202E} world"), "hello ");
    assert_eq!(clean_and_truncate_body(Some(7), "hello \u{202E} world"), "hello \u{202e}");
    assert_eq!(clean_and_truncate_body(None, "hello        world"), "hello world");
    assert_eq!(clean_and_truncate_body(None, "hello    \n    world"), "hello\nworld");
    assert_eq!(clean_and_truncate_body(None, "\n hello  \n \n \n   world\n   "), "hello\nworld");
}
