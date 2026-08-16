use std::fs;

use site::Site;
use tempfile::TempDir;

fn knowledge_site(extra_config: &str) -> TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("content/guides")).unwrap();
    fs::create_dir_all(root.path().join("content/bundles/example")).unwrap();
    fs::create_dir_all(root.path().join("templates")).unwrap();
    fs::write(root.path().join("templates/page.html"), "{{ page.content | safe }}").unwrap();
    fs::write(root.path().join("templates/section.html"), "{{ section.content | safe }}").unwrap();
    fs::write(root.path().join("templates/index.html"), "{{ section.content | safe }}").unwrap();
    fs::write(root.path().join("templates/404.html"), "Not found").unwrap();
    fs::write(root.path().join("content/index.md"), "# Home\n").unwrap();
    fs::write(root.path().join("content/guides/alpha.md"), "# Alpha\n").unwrap();
    fs::write(root.path().join("content/bundles/example/index.md"), "# Bundle\n").unwrap();
    fs::write(
        root.path().join("config.toml"),
        format!(
            r#"base_url = "https://example.com"
compile_sass = false
build_search_index = false
generate_sitemap = false
generate_robots_txt = false
skip_content_templating = ["raw/**"]

[content]
front_matter = "optional"
root_index = true
implicit_sections = true
{extra_config}

[markdown]
wikilinks = true
"#
        ),
    )
    .unwrap();
    root
}

#[test]
fn loads_root_index_and_implicit_sections_without_reinterpreting_page_bundles() {
    let root = knowledge_site("");
    let mut site = Site::new(root.path(), "config.toml").unwrap();
    site.load().unwrap();

    let content = root.path().join("content");
    let root_section = &site.library.sections[&content.join("index.md")];
    assert_eq!(root_section.path, "/");
    assert!(root_section.content.contains("<h1 id=\"home\">Home</h1>"));
    assert!(!root_section.implicit);

    let guides = &site.library.sections[&content.join("guides/_index.md")];
    assert!(guides.implicit);
    assert_eq!(guides.path, "/guides/");
    assert_eq!(guides.ancestors, ["index.md"]);

    assert!(site.library.sections.contains_key(&content.join("bundles/_index.md")));
    assert!(!site.library.sections.contains_key(&content.join("bundles/example/_index.md")));
    assert!(site.library.pages.contains_key(&content.join("bundles/example/index.md")));
}

#[test]
fn rejects_two_root_section_documents() {
    let root = knowledge_site("");
    fs::write(root.path().join("content/_index.md"), "+++\n+++\n").unwrap();
    let mut site = Site::new(root.path(), "config.toml").unwrap();
    let error = site.load().unwrap_err();
    assert!(format!("{error:#}").contains("defined by both `index.md` and `_index.md`"));
}

#[test]
fn resolves_source_relative_paths_aliases_and_backlinks() {
    let root = knowledge_site("");
    fs::write(root.path().join("content/index.md"), "[[guides/alpha|Alpha]]\n").unwrap();
    fs::write(
        root.path().join("content/guides/alpha.md"),
        "---\naliases: [/old-alpha/]\n---\n[[./beta#details|Details]]\n",
    )
    .unwrap();
    fs::write(
        root.path().join("content/guides/beta.md"),
        "# Beta\n\n[[old-alpha]]\n\n## Details\n",
    )
    .unwrap();

    let mut site = Site::new(root.path(), "config.toml").unwrap();
    site.load().unwrap();
    let content = root.path().join("content");
    let alpha = &site.library.pages[&content.join("guides/alpha.md")];
    assert_eq!(alpha.internal_links, [("guides/beta.md".to_string(), Some("details".to_string()))]);
    let beta = &site.library.pages[&content.join("guides/beta.md")];
    assert_eq!(beta.internal_links, [("guides/alpha.md".to_string(), None)]);

    let alpha_backlinks = &site.library.backlinks["guides/alpha.md"];
    assert!(alpha_backlinks.contains(&content.join("index.md")));
    assert!(alpha_backlinks.contains(&content.join("guides/beta.md")));
}

#[test]
fn reports_every_candidate_for_an_ambiguous_stem() {
    let root = knowledge_site("");
    fs::create_dir_all(root.path().join("content/archive")).unwrap();
    fs::write(root.path().join("content/guides/duplicate.md"), "# Guide\n").unwrap();
    fs::write(root.path().join("content/archive/duplicate.md"), "# Archive\n").unwrap();
    fs::write(root.path().join("content/index.md"), "[[duplicate]]\n").unwrap();

    let mut site = Site::new(root.path(), "config.toml").unwrap();
    let error = site.load().unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("target is ambiguous"));
    assert!(message.contains("archive/duplicate.md"));
    assert!(message.contains("guides/duplicate.md"));
}

#[test]
fn preserves_raw_html_and_literal_template_source() {
    let root = knowledge_site("");
    fs::create_dir_all(root.path().join("content/raw")).unwrap();
    fs::write(
        root.path().join("content/raw/transcript.md"),
        "<aside data-source=\"capture\">Raw HTML</aside>\n\nLiteral {{ captured.value }}\n",
    )
    .unwrap();

    let mut site = Site::new(root.path(), "config.toml").unwrap();
    site.load().unwrap();
    let transcript = &site.library.pages[&root.path().join("content/raw/transcript.md")];
    assert!(transcript.content.contains("<aside data-source=\"capture\">Raw HTML</aside>"));
    assert!(transcript.content.contains("{{ captured.value }}"));
}

#[test]
fn publishes_only_allowlisted_assets_and_reports_the_same_decisions() {
    let root = knowledge_site("asset_include = [\"**/*.pdf\"]");
    fs::write(root.path().join("content/guides/source.pdf"), "published").unwrap();
    fs::write(root.path().join("content/guides/rejected.db"), "excluded").unwrap();

    let mut site = Site::new(root.path(), "config.toml").unwrap();
    site.load().unwrap();
    let manifest = site.publication_manifest();
    let published = manifest
        .entries
        .iter()
        .find(|entry| entry.source_path == "content/guides/source.pdf")
        .unwrap();
    assert_eq!(published.state, "published");
    assert_eq!(published.rule, "asset_include");
    assert_eq!(published.output_path.as_deref(), Some("guides/source.pdf"));
    let rejected = manifest
        .entries
        .iter()
        .find(|entry| entry.source_path == "content/guides/rejected.db")
        .unwrap();
    assert_eq!(rejected.state, "excluded");
    assert_eq!(rejected.rule, "asset_include");

    site.build().unwrap();
    assert!(root.path().join("public/guides/source.pdf").exists());
    assert!(!root.path().join("public/guides/rejected.db").exists());
}

#[test]
fn rejects_an_output_file_over_the_configured_limit() {
    let root = knowledge_site("asset_include = [\"**/*.pdf\"]\nmax_file_size = 1024");
    fs::write(root.path().join("content/guides/source.pdf"), vec![b'x'; 2048]).unwrap();

    let mut site = Site::new(root.path(), "config.toml").unwrap();
    site.load().unwrap();
    let error = site.build().unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("source.pdf"));
    assert!(message.contains("exceeding content.max_file_size"));
}
