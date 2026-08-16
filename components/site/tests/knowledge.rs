use std::fs;

use site::Site;
use tempfile::TempDir;

fn knowledge_site(extra_config: &str) -> TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("content/guides")).unwrap();
    fs::create_dir_all(root.path().join("content/bundles/example")).unwrap();
    fs::create_dir_all(root.path().join("templates")).unwrap();
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

[content]
front_matter = "optional"
root_index = true
implicit_sections = true
{extra_config}
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
