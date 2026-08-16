use std::io::Write;
use std::path::Path;

use errors::Result;
use site::Site;

pub fn inspect(
    root_dir: &Path,
    config_file: &Path,
    output: Option<&Path>,
    include_drafts: bool,
) -> Result<()> {
    let mut site = Site::new(root_dir, config_file)?;
    if include_drafts {
        site.include_drafts();
    }
    site.load()?;
    let mut json = serde_json::to_string_pretty(&site.publication_manifest())?;
    json.push('\n');
    if let Some(output) = output {
        std::fs::write(output, json)?;
    } else {
        std::io::stdout().write_all(json.as_bytes())?;
    }
    Ok(())
}
