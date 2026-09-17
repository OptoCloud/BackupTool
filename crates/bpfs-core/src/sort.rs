//! Heuristics for ordering blobs before they're written into a generation's
//! data sections, so that similar/related file types end up adjacent — this
//! measurably improves the ratio of whichever compressor runs over the
//! concatenated section.

/// Extension-based category. Lower sorts first. Plaintext-ish formats that
/// compress well together are grouped early; already-compressed/binary blobs
/// (which mostly end up in the `Stored` section anyway) sort last.
pub fn extension_category(ext: &str) -> u8 {
    let ext = ext.to_ascii_lowercase();
    match ext.as_str() {
        "txt" | "md" | "csv" | "log" | "ini" | "cfg" | "conf" | "toml" | "yaml" | "yml" => 0,
        "json" | "xml" | "html" | "htm" | "css" | "svg" => 1,
        "c" | "h" | "cpp" | "hpp" | "cc" | "rs" | "py" | "js" | "ts" | "java" | "cs" | "go"
        | "rb" | "sh" | "ps1" | "lua" => 2,
        "" => 9,
        _ => 5,
    }
}

/// Ordering key for a single blob: `(category, extension, size)`. Grouping by
/// category first, then extension, then smallest-first keeps small text-like
/// files (which dominate typical repos) contiguous.
pub fn sort_key(ext: &str, size: u64) -> (u8, String, u64) {
    (extension_category(ext), ext.to_ascii_lowercase(), size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plaintext_sorts_before_binary() {
        assert!(extension_category("txt") < extension_category("bin"));
    }

    #[test]
    fn code_sorts_before_unknown_binary() {
        assert!(extension_category("rs") < extension_category("dat"));
    }

    #[test]
    fn no_extension_sorts_last() {
        assert!(extension_category("") > extension_category("dat"));
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(extension_category("RS"), extension_category("rs"));
    }

    #[test]
    fn sort_key_orders_by_category_then_ext_then_size() {
        let mut keys = vec![
            sort_key("bin", 100),
            sort_key("txt", 50),
            sort_key("txt", 10),
            sort_key("rs", 5),
        ];
        keys.sort();
        assert_eq!(
            keys,
            vec![
                sort_key("txt", 10),
                sort_key("txt", 50),
                sort_key("rs", 5),
                sort_key("bin", 100),
            ]
        );
    }
}
