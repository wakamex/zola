use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use config::Config;
use content::{Library, Page, Section};
use ego_tree::iter::Edge;
use errors::{Result, bail};
use scraper::node::Element;
use scraper::{Html, Node, Selector};
use serde::Serialize;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u32 = 1;
const MANIFEST_NAME: &str = "search-content-manifest.json";
const DATA_PREFIX: &str = "search-content-";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ContentRecord {
    source: String,
    url: String,
    title: String,
    taxonomies: BTreeMap<String, Vec<String>>,
    sections: Vec<ContentSection>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ContentSection {
    #[serde(skip_serializing_if = "Option::is_none")]
    heading: Option<ContentHeading>,
    body: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ContentHeading {
    level: u32,
    id: String,
    ordinal: usize,
    title: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ContentManifest<'a> {
    schema_version: u32,
    record_count: usize,
    corpus_sha256: &'a str,
    data_file: &'a str,
}

#[derive(Debug)]
struct HeadingBuffer {
    level: u32,
    id: String,
    title: String,
}

#[derive(Default, Debug)]
struct TextCollector {
    sections: Vec<ContentSection>,
    current_heading: Option<ContentHeading>,
    heading: Option<HeadingBuffer>,
    body: String,
    heading_ordinals: HashMap<String, usize>,
}

impl TextCollector {
    fn start_element(&mut self, element: &Element) {
        let name = element.name();
        if let Some(level) = heading_level(name) {
            self.flush_section();
            self.heading = Some(HeadingBuffer {
                level,
                id: element.id().unwrap_or_default().to_string(),
                title: String::new(),
            });
            return;
        }

        if name == "img" {
            if let Some(alt) = element.attr("alt") {
                self.push_text(alt);
            }
            return;
        }

        if name == "br" || is_block_tag(name) {
            self.push_boundary('\n');
        }
    }

    fn end_tag(&mut self, name: &str) {
        if heading_level(name).is_some() {
            let Some(heading) = self.heading.take() else { return };
            let title = clean_inline(&heading.title);
            let ordinal = self.heading_ordinals.entry(heading.id.clone()).or_default();
            *ordinal += 1;
            self.current_heading = Some(ContentHeading {
                level: heading.level,
                id: heading.id,
                ordinal: *ordinal,
                title,
            });
            return;
        }

        match name {
            "td" | "th" => self.push_boundary('\t'),
            "tr" => self.push_boundary('\n'),
            _ if is_block_tag(name) => self.push_boundary('\n'),
            _ => {}
        }
    }

    fn push_text(&mut self, text: &str) {
        if let Some(ref mut heading) = self.heading {
            heading.title.push_str(text);
        } else {
            if !self.body.is_empty() {
                self.body.push(' ');
            }
            self.body.push_str(text);
        }
    }

    fn push_boundary(&mut self, boundary: char) {
        if self.heading.is_some() {
            return;
        }
        if !self.body.ends_with(boundary) {
            self.body.push(boundary);
        }
    }

    fn flush_section(&mut self) {
        let body = clean_body(&self.body);
        self.body.clear();
        if !body.is_empty() {
            self.sections.push(ContentSection { heading: self.current_heading.take(), body });
        } else {
            self.current_heading = None;
        }
    }

    fn finish(mut self) -> Vec<ContentSection> {
        self.flush_section();
        self.sections
    }
}

pub fn write_content_export(
    base_path: &Path,
    output_path: &Path,
    configured_path: &str,
    library: &Library,
    config: &Config,
) -> Result<()> {
    let configured_path = configured_path.trim();
    if configured_path.is_empty() {
        bail!("search.content_export cannot be empty");
    }

    let export_dir = resolve_export_dir(base_path, configured_path);
    if export_dir.starts_with(output_path) {
        bail!(
            "search.content_export `{}` must be outside the public output `{}`",
            export_dir.display(),
            output_path.display()
        );
    }
    fs::create_dir_all(&export_dir)?;

    let publications = collect_publications(library);
    let temporary_data = export_dir.join(format!(".{DATA_PREFIX}{}.tmp", std::process::id()));
    let mut writer = BufWriter::new(File::create(&temporary_data)?);
    let mut hasher = Sha256::new();
    let mut record_count = 0;
    for publication in publications {
        let Some(record) = publication.record(config) else { continue };
        let mut bytes = serde_json::to_vec(&record)?;
        bytes.push(b'\n');
        writer.write_all(&bytes)?;
        hasher.update(&bytes);
        record_count += 1;
    }
    writer.flush()?;
    writer.into_inner()?.sync_all()?;

    let corpus_sha256 = hex_digest(hasher.finalize().as_slice());
    let data_name = format!("{DATA_PREFIX}{corpus_sha256}.jsonl");
    let data_path = export_dir.join(&data_name);
    if data_path.exists() {
        fs::remove_file(&temporary_data)?;
    } else {
        fs::rename(&temporary_data, &data_path)?;
    }

    let manifest = ContentManifest {
        schema_version: SCHEMA_VERSION,
        record_count,
        corpus_sha256: &corpus_sha256,
        data_file: &data_name,
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    let temporary_manifest =
        export_dir.join(format!(".{MANIFEST_NAME}.{}.tmp", std::process::id()));
    let mut manifest_file = File::create(&temporary_manifest)?;
    manifest_file.write_all(&manifest_bytes)?;
    manifest_file.write_all(b"\n")?;
    manifest_file.sync_all()?;
    fs::rename(&temporary_manifest, export_dir.join(MANIFEST_NAME))?;

    remove_stale_data_files(&export_dir, &data_name)?;
    Ok(())
}

enum Publication<'a> {
    Page(&'a Page),
    Section(&'a Section),
}

impl Publication<'_> {
    fn source(&self) -> &str {
        match self {
            Self::Page(page) => &page.file.relative,
            Self::Section(section) => &section.file.relative,
        }
    }

    fn record(&self, config: &Config) -> Option<ContentRecord> {
        match self {
            Self::Page(page) => page_record(page),
            Self::Section(section) => section_record(section, config),
        }
    }
}

fn collect_publications(library: &Library) -> Vec<Publication<'_>> {
    let mut publications = Vec::new();
    for page in library.pages.values() {
        if page.meta.render && page.meta.in_search_index {
            publications.push(Publication::Page(page));
        }
    }
    for section in library.sections.values() {
        if section.meta.render
            && section.meta.in_search_index
            && section.meta.redirect_to.is_none()
            && !section.implicit
            && section.file.path.is_file()
        {
            publications.push(Publication::Section(section));
        }
    }
    publications.sort_by(|left, right| left.source().cmp(right.source()));
    publications
}

fn page_record(page: &Page) -> Option<ContentRecord> {
    let sections = parse_sections(&page.content);
    if sections.is_empty() {
        return None;
    }
    let title = page.meta.title.clone().unwrap_or_else(|| {
        page.toc
            .first()
            .filter(|heading| heading.level == 1 && !heading.title.is_empty())
            .map(|heading| heading.title.clone())
            .unwrap_or_else(|| display_slug(&page.slug))
    });
    Some(ContentRecord {
        source: page.file.relative.clone(),
        url: page.path.clone(),
        title,
        taxonomies: sorted_taxonomies(&page.meta.taxonomies),
        sections,
    })
}

fn section_record(section: &Section, config: &Config) -> Option<ContentRecord> {
    let sections = parse_sections(&section.content);
    if sections.is_empty() {
        return None;
    }
    let title = section.meta.title.clone().unwrap_or_else(|| {
        if section.is_index() {
            config.title.clone().unwrap_or_else(|| section.path.clone())
        } else {
            section.path.clone()
        }
    });
    Some(ContentRecord {
        source: section.file.relative.clone(),
        url: section.path.clone(),
        title,
        taxonomies: BTreeMap::new(),
        sections,
    })
}

fn parse_sections(html: &str) -> Vec<ContentSection> {
    let wrapped = format!(
        "<!doctype html><html><body><article data-zola-search-content><div class=prose>{html}</div></article></body></html>"
    );
    let document = Html::parse_document(&wrapped);
    let selector = Selector::parse("article[data-zola-search-content]")
        .expect("the content wrapper selector is valid");
    let Some(root) = document.select(&selector).next() else {
        return Vec::new();
    };
    let mut collector = TextCollector::default();
    let mut ignored_depth = 0;

    for edge in root.traverse() {
        match edge {
            Edge::Open(node) => match node.value() {
                Node::Element(_) if ignored_depth > 0 => ignored_depth += 1,
                Node::Element(element) if should_ignore(element) => ignored_depth = 1,
                Node::Element(element) => collector.start_element(element),
                Node::Text(text) if ignored_depth == 0 => collector.push_text(text),
                _ => {}
            },
            Edge::Close(node) => match node.value() {
                Node::Element(_) if ignored_depth > 0 => ignored_depth -= 1,
                Node::Element(element) => collector.end_tag(element.name()),
                _ => {}
            },
        }
    }

    collector.finish()
}

fn sorted_taxonomies(
    taxonomies: &std::collections::HashMap<String, Vec<String>>,
) -> BTreeMap<String, Vec<String>> {
    taxonomies.iter().map(|(name, terms)| (name.clone(), terms.clone())).collect()
}

fn display_slug(slug: &str) -> String {
    slug.replace(['-', '_'], " ")
}

fn resolve_export_dir(base_path: &Path, configured_path: &str) -> PathBuf {
    let configured = Path::new(configured_path);
    if configured.is_absolute() { configured.to_path_buf() } else { base_path.join(configured) }
}

fn remove_stale_data_files(export_dir: &Path, current_name: &str) -> Result<()> {
    for entry in fs::read_dir(export_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if entry.file_type()?.is_file()
            && name.starts_with(DATA_PREFIX)
            && name.ends_with(".jsonl")
            && name != current_name
        {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

fn should_ignore(element: &Element) -> bool {
    let name = element.name();
    if matches!(name, "script" | "style" | "nav" | "footer" | "form" | "template") {
        return true;
    }
    if element.attr("data-pagefind-ignore").is_some() || element.attr("aria-hidden") == Some("true")
    {
        return true;
    }
    if name == "sup" && element.classes().any(|item| item == "footnote-reference") {
        return true;
    }
    if name == "a" && element.classes().any(|item| item == "zola-anchor") {
        return true;
    }
    name == "a" && element.attr("href").is_some_and(|href| href.contains("#fr-"))
}

fn heading_level(name: &str) -> Option<u32> {
    match name {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

fn is_block_tag(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "dd"
            | "div"
            | "dl"
            | "dt"
            | "figcaption"
            | "figure"
            | "li"
            | "ol"
            | "p"
            | "pre"
            | "section"
            | "table"
            | "tbody"
            | "tfoot"
            | "thead"
            | "ul"
    )
}

fn clean_inline(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clean_body(value: &str) -> String {
    value
        .lines()
        .map(|line| {
            line.split('\t')
                .map(clean_inline)
                .filter(|cell| !cell.is_empty())
                .collect::<Vec<_>>()
                .join("\t")
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to a string cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_visible_structured_text() {
        let sections = parse_sections(
            r##"<p>Lead <img src="x" alt="evidence image"></p>
            <h2 id="finding">Finding <a class="zola-anchor">#</a></h2>
            <p>Visible <code>code()</code>.</p>
            <pre>first line
  second line</pre>
            <table><tr><th>A</th><th>B</th></tr><tr><td>C</td><td>D</td></tr></table>
            <sup class="footnote-reference"><a>[1]</a></sup>
            <p>Footnote text <a href="#fr-note-1">↩</a></p>
            <script>if (a < b) hidden()</script><style>.hidden > span {}</style>
            <p>Visible after raw text.</p>"##,
        );

        assert_eq!(sections.len(), 2);
        assert!(sections[0].heading.is_none());
        assert_eq!(sections[0].body, "Lead evidence image");
        let heading = sections[1].heading.as_ref().unwrap();
        assert_eq!(heading.level, 2);
        assert_eq!(heading.id, "finding");
        assert_eq!(heading.ordinal, 1);
        assert_eq!(heading.title, "Finding");
        assert_eq!(
            sections[1].body,
            "Visible code() .\nfirst line\nsecond line\nA\tB\nC\tD\nFootnote text\nVisible after raw text."
        );
    }

    #[test]
    fn assigns_duplicate_heading_ordinals() {
        let sections =
            parse_sections("<h2 id=repeat>First</h2><p>one</p><h2 id=repeat>Second</h2><p>two</p>");
        assert_eq!(sections[0].heading.as_ref().unwrap().ordinal, 1);
        assert_eq!(sections[1].heading.as_ref().unwrap().ordinal, 2);
    }
}
