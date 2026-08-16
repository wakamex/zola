use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FrontMatterMode {
    #[default]
    Required,
    Optional,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Content {
    pub front_matter: FrontMatterMode,
    pub taxonomy_shorthand: bool,
    pub root_index: bool,
    pub implicit_sections: bool,
}
