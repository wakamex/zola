use std::fs;

use serde_json::Value;
use sha2::{Digest, Sha256};
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
    fs::write(root.path().join("templates/taxonomy_list.html"), "{{ taxonomy.name }}").unwrap();
    fs::write(root.path().join("templates/taxonomy_single.html"), "{{ term.name }}").unwrap();
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
taxonomies = [{{ name = "tags", feed = false }}]
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
    let root = knowledge_site(
        "asset_include = [\"guides/source.pdf\", \"guides/content/*.pdf\", \"guides/.secret.pdf\", \"unrendered-page/*.pdf\", \"unrendered-section/*.pdf\"]",
    );
    fs::write(root.path().join("content/guides/source.pdf"), "published").unwrap();
    fs::write(root.path().join("content/guides/.secret.pdf"), "hidden").unwrap();
    fs::write(root.path().join("content/guides/rejected.db"), "excluded").unwrap();
    fs::create_dir_all(root.path().join("content/guides/content")).unwrap();
    fs::write(root.path().join("content/guides/content/nested.pdf"), "nested").unwrap();
    fs::write(
        root.path().join("content/guides/unrendered.md"),
        "+++\nrender = false\n+++\n# Not rendered\n",
    )
    .unwrap();
    fs::create_dir_all(root.path().join("content/unrendered-section")).unwrap();
    fs::write(
        root.path().join("content/unrendered-section/_index.md"),
        "+++\nrender = false\n+++\n# Not rendered\n",
    )
    .unwrap();
    fs::write(
        root.path().join("content/unrendered-section/section.pdf"),
        "published with section assets",
    )
    .unwrap();
    fs::create_dir_all(root.path().join("content/unrendered-page")).unwrap();
    fs::write(
        root.path().join("content/unrendered-page/index.md"),
        "+++\nrender = false\n+++\n# Not rendered\n",
    )
    .unwrap();
    fs::write(
        root.path().join("content/unrendered-page/page.pdf"),
        "not published without page output",
    )
    .unwrap();
    fs::write(
        root.path().join("content/guides/alpha.md"),
        "+++\n[taxonomies]\ntags = [\"evidence\"]\n+++\n# Alpha\n",
    )
    .unwrap();

    let mut site = Site::new(root.path(), "config.toml").unwrap();
    site.load().unwrap();
    let asset = site.wikilinks.resolve("index.md", "en", "en", "guides/source.pdf").unwrap();
    assert_eq!(asset.permalink, "https://example.com/guides/source.pdf");
    assert!(!asset.track_backlink);
    let tag = site.wikilinks.resolve("index.md", "en", "en", "tags/evidence").unwrap();
    assert_eq!(tag.permalink, "https://example.com/tags/evidence/");
    assert!(!tag.track_backlink);
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
    let hidden = manifest
        .entries
        .iter()
        .find(|entry| entry.source_path == "content/guides/.secret.pdf")
        .unwrap();
    assert_eq!(hidden.state, "excluded");
    assert_eq!(hidden.rule, "hidden_file");
    let nested = manifest
        .entries
        .iter()
        .find(|entry| entry.source_path == "content/guides/content/nested.pdf")
        .unwrap();
    assert_eq!(nested.state, "published");
    assert_eq!(nested.output_path.as_deref(), Some("guides/content/nested.pdf"));
    for source_path in [
        "content/guides/unrendered.md",
        "content/unrendered-page/index.md",
        "content/unrendered-section/_index.md",
    ] {
        let unrendered =
            manifest.entries.iter().find(|entry| entry.source_path == source_path).unwrap();
        assert_eq!(unrendered.state, "excluded");
        assert_eq!(unrendered.rule, "render_false");
        assert_eq!(unrendered.route, None);
        assert_eq!(unrendered.output_path, None);
    }
    let unrendered_page_asset = manifest
        .entries
        .iter()
        .find(|entry| entry.source_path == "content/unrendered-page/page.pdf")
        .unwrap();
    assert_eq!(unrendered_page_asset.state, "excluded");
    assert_eq!(unrendered_page_asset.rule, "unpublished_content");
    let unrendered_section_asset = manifest
        .entries
        .iter()
        .find(|entry| entry.source_path == "content/unrendered-section/section.pdf")
        .unwrap();
    assert_eq!(unrendered_section_asset.state, "published");
    assert_eq!(
        unrendered_section_asset.output_path.as_deref(),
        Some("unrendered-section/section.pdf")
    );

    site.build().unwrap();
    assert!(root.path().join("public/guides/source.pdf").exists());
    assert!(root.path().join("public/guides/content/nested.pdf").exists());
    assert!(!root.path().join("public/guides/.secret.pdf").exists());
    assert!(!root.path().join("public/guides/rejected.db").exists());
    assert!(!root.path().join("public/guides/unrendered/index.html").exists());
    assert!(!root.path().join("public/unrendered-page/index.html").exists());
    assert!(!root.path().join("public/unrendered-page/page.pdf").exists());
    assert!(!root.path().join("public/unrendered-section/index.html").exists());
    assert!(root.path().join("public/unrendered-section/section.pdf").exists());
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

#[test]
fn writes_a_complete_deterministic_search_content_export() {
    let root = knowledge_site("");
    fs::create_dir_all(root.path().join("content/private")).unwrap();
    fs::create_dir_all(root.path().join("content/hidden-section")).unwrap();
    let config_path = root.path().join("config.toml");
    let mut config = fs::read_to_string(&config_path).unwrap();
    config.push_str("\n[search]\ncontent_export = \"build/search\"\n");
    fs::write(&config_path, config).unwrap();
    fs::write(
        root.path().join("templates/components.html"),
        "{% component evidence() %}<strong>Expanded shortcode.</strong>{% endcomponent %}",
    )
    .unwrap();
    fs::write(
        root.path().join("content/guides/alpha.md"),
        "+++\n[taxonomies]\ntags = [\"evidence\"]\n+++\n# Alpha\n\nLead. {{ <evidence /> }}\n\n## Finding\n\nBody.\n",
    )
    .unwrap();
    fs::write(
        root.path().join("content/guides/unrendered.md"),
        "+++\nrender = false\n+++\n# Excluded\n",
    )
    .unwrap();
    fs::write(
        root.path().join("content/guides/hidden.md"),
        "+++\nhidden = true\n+++\n# Hidden page\n",
    )
    .unwrap();
    fs::write(
        root.path().join("content/private/_index.md"),
        "+++\nin_search_index = false\n+++\n# Private section\n",
    )
    .unwrap();
    fs::write(root.path().join("content/private/child.md"), "# Child excluded by parent\n")
        .unwrap();
    fs::write(
        root.path().join("content/hidden-section/_index.md"),
        "+++\nhidden = true\n+++\n# Hidden section\n",
    )
    .unwrap();

    let mut site = Site::new(root.path(), "config.toml").unwrap();
    site.load().unwrap();
    site.build().unwrap();

    let export_dir = root.path().join("build/search");
    let manifest_path = export_dir.join("search-content-manifest.json");
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["schemaVersion"], 1);
    let data_name = manifest["dataFile"].as_str().unwrap();
    let data = fs::read(export_dir.join(data_name)).unwrap();
    assert_eq!(
        manifest["recordCount"],
        data.split(|byte| *byte == b'\n').filter(|line| !line.is_empty()).count()
    );
    let digest = Sha256::digest(&data).iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    assert_eq!(manifest["corpusSha256"], digest);
    assert!(data_name.contains(&digest));
    assert!(!root.path().join("public/build/search").exists());

    let records: Vec<Value> = data
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert!(records.iter().any(|record| {
        record["source"] == "guides/alpha.md"
            && record["url"] == "/guides/alpha/"
            && record["title"] == "Alpha"
            && record["taxonomies"]["tags"][0] == "evidence"
            && record["sections"][0]["body"] == "Lead. Expanded shortcode."
            && record["sections"][1]["heading"]["id"] == "finding"
            && record["sections"][1]["body"] == "Body."
    }));
    assert!(!records.iter().any(|record| record["source"] == "guides/unrendered.md"));
    assert!(!records.iter().any(|record| record["source"] == "guides/hidden.md"));
    assert!(!records.iter().any(|record| record["source"] == "private/_index.md"));
    assert!(!records.iter().any(|record| record["source"] == "private/child.md"));
    assert!(!records.iter().any(|record| record["source"] == "hidden-section/_index.md"));
    assert!(!records.iter().any(|record| record["source"] == "guides/_index.md"));

    site.build().unwrap();
    let second_manifest = fs::read_to_string(&manifest_path).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&second_manifest).unwrap(), manifest);
    assert_eq!(
        fs::read_dir(&export_dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".jsonl"))
            .count(),
        1
    );
}

#[test]
fn rejects_search_content_export_inside_public_output() {
    assert_export_path_rejected("public/search", "must be outside the public output");
    assert_export_path_rejected("public/../public/search", "must be outside the public output");
}

#[test]
fn rejects_search_content_export_inside_public_source_roots() {
    assert_export_path_rejected("content/search", "must be outside public source root");
    assert_export_path_rejected("static/search", "must be outside public source root");
}

#[cfg(unix)]
#[test]
fn rejects_search_content_export_through_a_symlink_to_public_output() {
    use std::os::unix::fs::symlink;

    let root = knowledge_site("");
    fs::create_dir_all(root.path().join("public")).unwrap();
    symlink(root.path().join("public"), root.path().join("export-link")).unwrap();
    let config_path = root.path().join("config.toml");
    let mut config = fs::read_to_string(&config_path).unwrap();
    config.push_str("\n[search]\ncontent_export = \"export-link/search\"\n");
    fs::write(&config_path, config).unwrap();

    let mut site = Site::new(root.path(), "config.toml").unwrap();
    site.load().unwrap();
    let error = site.build().unwrap_err();
    assert!(format!("{error:#}").contains("must be outside the public output"));
}

fn assert_export_path_rejected(export_path: &str, expected: &str) {
    let root = knowledge_site("");
    let config_path = root.path().join("config.toml");
    let mut config = fs::read_to_string(&config_path).unwrap();
    config.push_str(&format!("\n[search]\ncontent_export = \"{export_path}\"\n"));
    fs::write(&config_path, config).unwrap();

    let mut site = Site::new(root.path(), "config.toml").unwrap();
    site.load().unwrap();
    let error = site.build().unwrap_err();
    assert!(format!("{error:#}").contains(expected));
}
