use std::path::Path;

use globset::GlobSet;
use serde::{Deserialize, Serialize};
use utils::globs::build_ignore_glob_set;

use errors::Result;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FrontMatterMode {
    #[default]
    Required,
    Optional,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Content {
    pub front_matter: FrontMatterMode,
    pub taxonomy_shorthand: bool,
    pub root_index: bool,
    pub implicit_sections: bool,
    /// Glob patterns for content assets. `None` retains upstream publish-all behavior.
    pub asset_include: Option<Vec<String>>,
    #[serde(skip)]
    pub asset_include_globset: Option<GlobSet>,
    /// Maximum permitted size for any generated output file, in bytes.
    pub max_file_size: Option<u64>,
}

impl Content {
    pub fn resolve_globset(&mut self) -> Result<()> {
        self.asset_include_globset = match &self.asset_include {
            Some(patterns) => Some(build_ignore_glob_set(patterns, "content.asset_include")?),
            None => None,
        };
        Ok(())
    }

    pub fn is_asset_allowed(&self, relative_path: &Path) -> bool {
        self.asset_include_globset.as_ref().is_none_or(|globset| globset.is_match(relative_path))
    }
}

impl Default for Content {
    fn default() -> Self {
        Self {
            front_matter: FrontMatterMode::Required,
            taxonomy_shorthand: false,
            root_index: false,
            implicit_sections: false,
            asset_include: None,
            asset_include_globset: None,
            max_file_size: None,
        }
    }
}
