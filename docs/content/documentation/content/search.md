+++
title = "Search"
weight = 100
+++

Zola can build a search index from the sections and pages content to
be used by a JavaScript library such as [elasticlunr](http://elasticlunr.com/) or [fuse](https://www.fusejs.io).

To enable it, you only need to set `build_search_index = true` in your `zola.toml` and Zola will
generate an index for the `default_language` set for all pages not excluded from the search index.

It is very important to set the `default_language` in your `zola.toml` if you are writing a site not in
English; the index building pipelines are very different depending on the language.

As each site will be different, Zola makes no assumptions about your search function and doesn't provide
the JavaScript/CSS code to do an actual search and display results. You can look at how this site
implements it (using elasticlunr) to get an idea: [search.js](https://github.com/getzola/zola/tree/master/docs/static/search.js).


## Configuring the search index
In some cases, the default indexing strategy is not suitable. You can customize which fields to include and whether
to truncate the content in the [search configuration](@/documentation/getting-started/configuration.md).

## Searchable-content export

Server-side search systems can consume a provider-neutral, build-only export instead of parsing the
generated site. Configure a directory relative to the site root:

```toml
[search]
content_export = "build/search"
```

The directory must be outside the public output directory. `zola build` writes a versioned manifest
and a hash-named JSONL file containing published, searchable Markdown pages and explicit authored
sections. Each record includes its source path, canonical URL, title, taxonomies, and structured
rendered text under its headings. Implicit sections, generated taxonomies, redirects, drafts,
`render = false` content, and entries excluded from search are omitted.

The manifest is promoted after the complete JSONL file and covers its exact bytes with SHA-256.
Consumers should verify the schema version, record count, filename, and hash before using an export.
Zola does not define provider-specific chunks, record IDs, search weights, or synchronization.

## Index Formats

### Elasticlunr

Compatible with [elasticlunr](http://elasticlunr.com/). Also produces `elasticlunr.min.js`.

```toml
# zola.toml
[search]
index_format = "elasticlunr_javascript" # or "elasticlunr_json"
```

If you are using a language other than English, you will also need to include the corresponding JavaScript stemmer file.
See <https://github.com/weixsong/lunr-languages#in-a-web-browser> for details.

### Fuse

Compatible with [fuse.js](https://www.fusejs.io/) and [tinysearch](https://github.com/tinysearch/tinysearch).

```toml
# zola.toml
[search]
index_format = "fuse_javascript" # or "fuse_json"
```
