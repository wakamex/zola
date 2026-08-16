use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ahash::AHashMap;
use serde::Serialize;
use walkdir::WalkDir;

use crate::Site;
use content::Section;
use utils::fs::is_temp_file;

#[derive(Clone, Debug, Serialize)]
pub struct PublicationEntry {
    pub source_path: String,
    pub kind: &'static str,
    pub state: &'static str,
    pub rule: &'static str,
    pub route: Option<String>,
    pub output_path: Option<String>,
    pub aliases: Vec<String>,
    pub draft: bool,
    pub bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct PublicationSummary {
    pub entries: usize,
    pub bytes: u64,
    pub published_bytes: u64,
    pub by_state: BTreeMap<&'static str, usize>,
    pub by_kind: BTreeMap<&'static str, usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PublicationManifest {
    pub version: u8,
    pub entries: Vec<PublicationEntry>,
    pub summary: PublicationSummary,
}

fn display_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn content_source(relative: &Path) -> String {
    format!("content/{}", display_path(relative))
}

fn section_output_base(site: &Site, lang: &str, components: &[String]) -> PathBuf {
    let mut path = PathBuf::new();
    if lang != site.config.default_language {
        path.push(lang);
    }
    for component in components {
        path.push(component);
    }
    path
}

pub fn build(site: &Site) -> PublicationManifest {
    let mut asset_outputs = AHashMap::new();
    for page in site.library.pages.values() {
        let base = PathBuf::from(page.path.trim_start_matches('/'));
        for asset in &page.assets {
            if let Ok(relative) = asset.strip_prefix(page.file.path.parent().unwrap()) {
                asset_outputs.insert(asset.clone(), display_path(&base.join(relative)));
            }
        }
    }
    for section in site.library.sections.values() {
        let base = section_output_base(site, &section.lang, &section.file.components);
        for asset in &section.assets {
            if let Ok(relative) = asset.strip_prefix(section.file.path.parent().unwrap()) {
                asset_outputs.insert(asset.clone(), display_path(&base.join(relative)));
            }
        }
    }

    let content = site.base_path.join("content");
    let mut draft_section_sources = Vec::new();
    let mut draft_roots = Vec::new();
    for entry in
        WalkDir::new(&content).follow_links(true).into_iter().filter_map(|entry| entry.ok())
    {
        let path = entry.path();
        let filename = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        if !path.is_file()
            || !(filename == "_index.md" || filename.starts_with("_index."))
            || site.library.sections.contains_key(path)
            || site
                .config
                .ignored_content_globset
                .as_ref()
                .is_some_and(|globset| globset.is_match(path))
        {
            continue;
        }
        if let Ok(section) = Section::from_file(path, &site.config, &site.base_path)
            && section.meta.draft
        {
            draft_section_sources.push(path.to_path_buf());
            draft_roots.push(path.parent().unwrap().to_path_buf());
        }
    }
    let mut entries = Vec::new();
    for entry in
        WalkDir::new(&content).follow_links(true).into_iter().filter_map(|entry| entry.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Ok(relative) = path.strip_prefix(&content) else { continue };
        let bytes = entry.metadata().map(|metadata| metadata.len()).unwrap_or(0);
        let filename = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        let ignored = site
            .config
            .ignored_content_globset
            .as_ref()
            .is_some_and(|globset| globset.is_match(path));
        if ignored {
            entries.push(PublicationEntry {
                source_path: content_source(relative),
                kind: if path.extension().is_some_and(|extension| extension == "md") {
                    "markdown"
                } else {
                    "asset"
                },
                state: "excluded",
                rule: "ignored_content",
                route: None,
                output_path: None,
                aliases: Vec::new(),
                draft: false,
                bytes,
            });
            continue;
        }

        if filename.starts_with('.') || is_temp_file(path) {
            entries.push(PublicationEntry {
                source_path: content_source(relative),
                kind: if path.extension().is_some_and(|extension| extension == "md") {
                    "markdown"
                } else {
                    "asset"
                },
                state: "excluded",
                rule: if filename.starts_with('.') { "hidden_file" } else { "temporary_file" },
                route: None,
                output_path: None,
                aliases: Vec::new(),
                draft: false,
                bytes,
            });
            continue;
        }

        let draft_section_source = draft_section_sources.iter().any(|source| source == path);
        let in_draft_section = draft_roots.iter().any(|root| path.starts_with(root));

        if path.extension().is_some_and(|extension| extension == "md") {
            if let Some(page) = site.library.pages.get(path) {
                entries.push(PublicationEntry {
                    source_path: content_source(relative),
                    kind: "page",
                    state: "published",
                    rule: "markdown",
                    route: Some(page.path.clone()),
                    output_path: Some(format!("{}index.html", page.path.trim_start_matches('/'))),
                    aliases: page.meta.aliases.clone(),
                    draft: page.meta.draft,
                    bytes,
                });
            } else if let Some(section) = site.library.sections.get(path) {
                let rule = if section.file.components.is_empty()
                    && site.config.content.root_index
                    && section.file.name == "index"
                {
                    "root_index"
                } else {
                    "explicit_section"
                };
                entries.push(PublicationEntry {
                    source_path: content_source(relative),
                    kind: "section",
                    state: "published",
                    rule,
                    route: Some(section.path.clone()),
                    output_path: Some(format!(
                        "{}index.html",
                        section.path.trim_start_matches('/')
                    )),
                    aliases: section.meta.aliases.clone(),
                    draft: section.meta.draft,
                    bytes,
                });
            } else {
                entries.push(PublicationEntry {
                    source_path: content_source(relative),
                    kind: "markdown",
                    state: "draft",
                    rule: if draft_section_source || !in_draft_section {
                        "draft"
                    } else {
                        "draft_section"
                    },
                    route: None,
                    output_path: None,
                    aliases: Vec::new(),
                    draft: true,
                    bytes,
                });
            }
        } else if in_draft_section {
            entries.push(PublicationEntry {
                source_path: content_source(relative),
                kind: "asset",
                state: "draft",
                rule: "draft_section",
                route: None,
                output_path: None,
                aliases: Vec::new(),
                draft: true,
                bytes,
            });
        } else if let Some(output_path) = asset_outputs.get(path) {
            entries.push(PublicationEntry {
                source_path: content_source(relative),
                kind: "asset",
                state: "published",
                rule: if site.config.content.asset_include.is_some() {
                    "asset_include"
                } else {
                    "upstream_asset_default"
                },
                route: None,
                output_path: Some(output_path.clone()),
                aliases: Vec::new(),
                draft: false,
                bytes,
            });
        } else {
            entries.push(PublicationEntry {
                source_path: content_source(relative),
                kind: "asset",
                state: "excluded",
                rule: if site.config.content.is_asset_allowed(relative) {
                    "unpublished_content"
                } else {
                    "asset_include"
                },
                route: None,
                output_path: None,
                aliases: Vec::new(),
                draft: false,
                bytes,
            });
        }
    }

    for section in site.library.sections.values().filter(|section| section.implicit) {
        let relative = section.file.parent.strip_prefix(&content).unwrap_or(&section.file.parent);
        entries.push(PublicationEntry {
            source_path: format!("{}/", content_source(relative)),
            kind: "implicit_section",
            state: "published",
            rule: "implicit_sections",
            route: Some(section.path.clone()),
            output_path: Some(format!("{}index.html", section.path.trim_start_matches('/'))),
            aliases: Vec::new(),
            draft: false,
            bytes: 0,
        });
    }

    entries.sort_by(|left, right| {
        left.source_path.cmp(&right.source_path).then(left.kind.cmp(right.kind))
    });
    let mut by_state = BTreeMap::new();
    let mut by_kind = BTreeMap::new();
    for entry in &entries {
        *by_state.entry(entry.state).or_insert(0) += 1;
        *by_kind.entry(entry.kind).or_insert(0) += 1;
    }
    let summary = PublicationSummary {
        entries: entries.len(),
        bytes: entries.iter().map(|entry| entry.bytes).sum(),
        published_bytes: entries
            .iter()
            .filter(|entry| entry.state == "published")
            .map(|entry| entry.bytes)
            .sum(),
        by_state,
        by_kind,
    };
    PublicationManifest { version: 1, entries, summary }
}
