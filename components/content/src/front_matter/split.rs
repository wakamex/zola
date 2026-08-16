use std::path::Path;

use errors::{Context, Result, bail};
use regex::Regex;
use std::sync::LazyLock;

use crate::front_matter::page::PageFrontMatter;
use crate::front_matter::section::SectionFrontMatter;
use config::{Config, FrontMatterMode};

static TOML_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^[[:space:]]*\+\+\+[[:space:]]*(\r?\n(?s).*?(?-s))\+\+\+[[:space:]]*(?:$|(?:\r?\n((?s).*(?-s))$))",
    )
    .unwrap()
});

static YAML_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[[:space:]]*---[[:space:]]*(\r?\n(?s).*?(?-s))---[[:space:]]*(?:$|(?:\r?\n((?s).*(?-s))$))")
        .unwrap()
});

pub enum RawFrontMatter<'a> {
    Toml(&'a str),
    Yaml(&'a str),
}

impl RawFrontMatter<'_> {
    pub(crate) fn deserialize<T>(&self) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let f: T = match self {
            RawFrontMatter::Toml(s) => toml::from_str(s)?,
            RawFrontMatter::Yaml(s) => match serde_yaml::from_str(s) {
                Ok(d) => d,
                Err(e) => bail!("YAML deserialize error: {:?}", e),
            },
        };
        Ok(f)
    }

    pub(crate) fn deserialize_page(&self, taxonomies: &[String]) -> Result<PageFrontMatter> {
        match self {
            RawFrontMatter::Toml(source) => {
                let mut value: toml::Value = toml::from_str(source)?;
                let table = value
                    .as_table_mut()
                    .ok_or_else(|| errors::anyhow!("Page front matter must be a table"))?;
                for taxonomy in taxonomies {
                    let Some(shorthand) = table.remove(taxonomy) else { continue };
                    let canonical = table
                        .entry("taxonomies")
                        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
                        .as_table_mut()
                        .ok_or_else(|| errors::anyhow!("`taxonomies` must be a table"))?;
                    if canonical.contains_key(taxonomy) {
                        bail!(
                            "Taxonomy `{taxonomy}` is defined using both shorthand and `taxonomies.{taxonomy}`"
                        );
                    }
                    canonical.insert(taxonomy.clone(), shorthand);
                }
                Ok(value.try_into()?)
            }
            RawFrontMatter::Yaml(source) => {
                let mut value: serde_yaml::Value = serde_yaml::from_str(source)
                    .map_err(|e| errors::anyhow!("YAML deserialize error: {e:?}"))?;
                let mapping = value
                    .as_mapping_mut()
                    .ok_or_else(|| errors::anyhow!("Page front matter must be a mapping"))?;
                for taxonomy in taxonomies {
                    let key = serde_yaml::Value::String(taxonomy.clone());
                    let Some(shorthand) = mapping.remove(&key) else { continue };
                    let taxonomies_key = serde_yaml::Value::String("taxonomies".to_string());
                    if !mapping.contains_key(&taxonomies_key) {
                        mapping.insert(
                            taxonomies_key.clone(),
                            serde_yaml::Value::Mapping(serde_yaml::Mapping::new()),
                        );
                    }
                    let canonical = mapping
                        .get_mut(&taxonomies_key)
                        .and_then(serde_yaml::Value::as_mapping_mut)
                        .ok_or_else(|| errors::anyhow!("`taxonomies` must be a mapping"))?;
                    if canonical.contains_key(&key) {
                        bail!(
                            "Taxonomy `{taxonomy}` is defined using both shorthand and `taxonomies.{taxonomy}`"
                        );
                    }
                    canonical.insert(key, shorthand);
                }
                serde_yaml::from_value(value)
                    .map_err(|e| errors::anyhow!("YAML deserialize error: {e:?}"))
            }
        }
    }
}

fn has_opening_delimiter(content: &str) -> bool {
    matches!(content.trim_start().lines().next().map(str::trim_end), Some("+++") | Some("---"))
}

/// Split a file between the front matter and its content
/// Will return an error if the front matter wasn't found
fn split_content<'c>(file_path: &Path, content: &'c str) -> Result<(RawFrontMatter<'c>, &'c str)> {
    let (caps, is_toml) = if let Some(caps) = TOML_RE.captures(content) {
        (caps, true)
    } else if let Some(caps) = YAML_RE.captures(content) {
        (caps, false)
    } else {
        bail!(
            "Couldn't find front matter in `{}`. Did you forget to add `+++` or `---`?",
            file_path.to_string_lossy()
        );
    };

    // 2. extract the front matter and the content
    // caps[0] is the full match
    // caps[1] => front matter
    // caps[2] => content
    let front_matter = caps.get(1).unwrap().as_str();
    let content = caps.get(2).map_or("", |m| m.as_str());

    if is_toml {
        Ok((RawFrontMatter::Toml(front_matter), content))
    } else {
        Ok((RawFrontMatter::Yaml(front_matter), content))
    }
}

/// Split a file between the front matter and its content.
/// Returns a parsed `SectionFrontMatter` and the rest of the content
pub fn split_section_content<'c>(
    file_path: &Path,
    content: &'c str,
) -> Result<(SectionFrontMatter, &'c str)> {
    let (front_matter, content) = split_content(file_path, content)?;
    let meta = SectionFrontMatter::parse(&front_matter).with_context(|| {
        format!("Error when parsing front matter of section `{}`", file_path.to_string_lossy())
    })?;

    Ok((meta, content))
}

pub fn split_section_content_with_config<'c>(
    file_path: &Path,
    content: &'c str,
    config: &Config,
) -> Result<(SectionFrontMatter, &'c str)> {
    if config.content.front_matter == FrontMatterMode::Optional && !has_opening_delimiter(content) {
        return Ok((SectionFrontMatter::default(), content));
    }
    split_section_content(file_path, content)
}

/// Split a file between the front matter and its content
/// Returns a parsed `PageFrontMatter` and the rest of the content
fn split_page_content<'c>(
    file_path: &Path,
    content: &'c str,
) -> Result<(PageFrontMatter, &'c str)> {
    let (front_matter, content) = split_content(file_path, content)?;
    let meta = PageFrontMatter::parse(&front_matter).with_context(|| {
        format!("Error when parsing front matter of page `{}`", file_path.to_string_lossy())
    })?;
    Ok((meta, content))
}

pub fn split_page_content_with_config<'c>(
    file_path: &Path,
    content: &'c str,
    config: &Config,
) -> Result<(PageFrontMatter, &'c str)> {
    if config.content.front_matter == FrontMatterMode::Optional && !has_opening_delimiter(content) {
        return Ok((PageFrontMatter::default(), content));
    }

    if !config.content.taxonomy_shorthand {
        return split_page_content(file_path, content);
    }

    let (front_matter, content) = split_content(file_path, content)?;
    let taxonomies =
        config.taxonomies.iter().map(|taxonomy| taxonomy.name.clone()).collect::<Vec<_>>();
    let meta = PageFrontMatter::parse_with_taxonomy_shorthand(&front_matter, &taxonomies)
        .with_context(|| {
            format!("Error when parsing front matter of page `{}`", file_path.to_string_lossy())
        })?;
    Ok((meta, content))
}

#[cfg(test)]
mod tests {
    use crate::PageFrontMatter;
    use config::{Config, FrontMatterMode, TaxonomyConfig};
    use std::path::Path;
    use test_case::test_case;

    use super::{split_page_content, split_page_content_with_config, split_section_content};

    fn knowledge_config() -> Config {
        let mut config = Config::default_for_test();
        config.content.front_matter = FrontMatterMode::Optional;
        config.content.taxonomy_shorthand = true;
        config.taxonomies =
            vec![TaxonomyConfig { name: "tags".to_string(), ..TaxonomyConfig::default() }];
        config
    }

    #[test]
    fn optional_front_matter_preserves_the_complete_body() {
        let content = "# No metadata\n\nThe body starts at byte zero.\n";
        let (meta, body) =
            split_page_content_with_config(Path::new("plain.md"), content, &knowledge_config())
                .unwrap();
        assert_eq!(meta, PageFrontMatter::default());
        assert_eq!(body, content);
    }

    #[test]
    fn optional_front_matter_still_rejects_an_unclosed_delimiter() {
        let content = "---\ntitle: Broken\n";
        assert!(
            split_page_content_with_config(Path::new("broken.md"), content, &knowledge_config())
                .is_err()
        );
    }

    #[test_case("---\ntitle: Tagged\ntags: [evidence, person]\n---\nBody\n"; "yaml")]
    #[test_case("+++\ntitle = \"Tagged\"\ntags = [\"evidence\", \"person\"]\n+++\nBody\n"; "toml")]
    fn normalizes_configured_taxonomy_shorthand(content: &str) {
        let (meta, _) =
            split_page_content_with_config(Path::new("tagged.md"), content, &knowledge_config())
                .unwrap();
        assert_eq!(meta.taxonomies["tags"], ["evidence", "person"]);
    }

    #[test_case("---\ntags: [short]\ntaxonomies:\n  tags: [canonical]\n---\n"; "yaml")]
    #[test_case("+++\ntags = [\"short\"]\n[taxonomies]\ntags = [\"canonical\"]\n+++\n"; "toml")]
    fn rejects_duplicate_taxonomy_forms(content: &str) {
        let error =
            split_page_content_with_config(Path::new("duplicate.md"), content, &knowledge_config())
                .unwrap_err();
        assert!(format!("{error:#}").contains("both shorthand"));
    }

    #[test_case(r#"
+++
title = "Title"
description = "hey there"
date = 2002-10-12
+++
Hello
"#; "toml")]
    #[test_case(r#"
---
title: Title
description: hey there
date: 2002-10-12
---
Hello
"#; "yaml")]
    #[test_case(r#"
+++  
title = "Title"
description = "hey there"
date = 2002-10-12
+++
Hello
"#; "toml with trailing whitespace")]
    #[test_case(r#"
---  
title: Title
description: hey there
date: 2002-10-12
---
Hello
"#; "yaml with trailing whitespace")]
    fn can_split_page_content_valid(content: &str) {
        let (front_matter, content) = split_page_content(Path::new(""), content).unwrap();
        assert_eq!(content, "Hello\n");
        assert_eq!(front_matter.title.unwrap(), "Title");
    }

    #[test_case(r#"
+++
paginate_by = 10
+++
Hello
"#; "toml")]
    #[test_case(r#"
---
paginate_by: 10
---
Hello
"#; "yaml")]
    fn can_split_section_content_valid(content: &str) {
        let (front_matter, content) = split_section_content(Path::new(""), content).unwrap();
        assert_eq!(content, "Hello\n");
        assert!(front_matter.is_paginated());
    }

    #[test_case(r#"
+++
title = "Title"
description = "hey there"
date = 2002-10-12
+++
"#; "toml")]
    #[test_case(r#"
---
title: Title
description: hey there
date: 2002-10-12
---
"#; "yaml")]
    #[test_case(r#"
+++
title = "Title"
description = "hey there"
date = 2002-10-12
+++"#; "toml no newline")]
    #[test_case(r#"
---
title: Title
description: hey there
date: 2002-10-12
---"#; "yaml no newline")]
    fn can_split_content_with_only_frontmatter_valid(content: &str) {
        let (front_matter, content) = split_page_content(Path::new(""), content).unwrap();
        assert_eq!(content, "");
        assert_eq!(front_matter.title.unwrap(), "Title");
    }

    #[test_case(r#"
+++
title = "Title"
description = "hey there"
date = 2002-10-02T15:00:00Z
+++
+++"#, "+++"; "toml with pluses in content")]
    #[test_case(r#"
+++
title = "Title"
description = "hey there"
date = 2002-10-02T15:00:00Z
+++
---"#, "---"; "toml with minuses in content")]
    #[test_case(r#"
---
title: Title
description: hey there
date: 2002-10-02T15:00:00Z
---
+++"#, "+++"; "yaml with pluses in content")]
    #[test_case(r#"
---
title: Title
description: hey there
date: 2002-10-02T15:00:00Z
---
---"#, "---"; "yaml with minuses in content")]
    fn can_split_content_lazily(content: &str, expected: &str) {
        let (front_matter, content) = split_page_content(Path::new(""), content).unwrap();
        assert_eq!(content, expected);
        assert_eq!(front_matter.title.unwrap(), "Title");
    }

    #[test_case(r#"
+++
title = "Title"
description = "hey there"
date = 2002-10-12"#; "toml")]
    #[test_case(r#"
+++
title = "Title"
description = "hey there"
date = 2002-10-12
---"#; "toml unmatched")]
    #[test_case(r#"
+++
title = "Title"
description = "hey there"
date = 2002-10-12
++++"#; "toml too many pluses")]
    #[test_case(r#"
---
title: Title
description: hey there
date: 2002-10-12"#; "yaml")]
    #[test_case(r#"
---
title: Title
description: hey there
date: 2002-10-12
+++"#; "yaml unmatched")]
    #[test_case(r#"
---
title: Title
description: hey there
date: 2002-10-12
----"#; "yaml too many dashes")]
    fn errors_if_cannot_locate_frontmatter(content: &str) {
        let res = split_page_content(Path::new(""), content);
        assert!(res.is_err());
    }
}
