pub(super) fn is_extension(extension: &str, extensions: &[&str]) -> bool {
    extensions.contains(&extension)
}

pub(super) fn is_text_filename(filename: &str) -> bool {
    matches!(
        filename,
        "dockerfile"
            | "makefile"
            | "rakefile"
            | "gemfile"
            | "procfile"
            | "license"
            | "readme"
            | ".editorconfig"
            | ".gitattributes"
            | ".gitignore"
            | ".npmrc"
            | ".prettierrc"
            | ".tool-versions"
    )
}

pub(super) const WEBPAGE_EXTENSIONS: &[&str] = &["htm", "html", "xhtml"];
pub(super) const IMAGE_EXTENSIONS: &[&str] = &[
    "apng", "avif", "bmp", "gif", "ico", "jfif", "jpeg", "jpg", "png", "svg", "tif", "tiff", "webp",
];
pub(super) const MEDIA_EXTENSIONS: &[&str] = &[
    "3gp", "aac", "aiff", "alac", "avi", "flac", "m4a", "m4v", "mkv", "mov", "mp3", "mp4", "mpeg",
    "mpg", "oga", "ogg", "ogv", "opus", "wav", "webm", "wma", "wmv",
];
pub(super) const OFFICE_EXTENSIONS: &[&str] = &[
    "csv", "doc", "docm", "docx", "dot", "dotm", "dotx", "odp", "ods", "odt", "pot", "potm",
    "potx", "pps", "ppsm", "ppsx", "ppt", "pptm", "pptx", "rtf", "tsv", "xls", "xlsb", "xlsm",
    "xlsx", "xlt", "xltm", "xltx",
];
pub(super) const TEXTUAL_OFFICE_EXTENSIONS: &[&str] = &["csv", "rtf", "tsv"];
pub(super) const TEXT_EXTENSIONS: &[&str] = &[
    "bash",
    "c",
    "cc",
    "cfg",
    "clj",
    "cljs",
    "cmake",
    "conf",
    "cpp",
    "cs",
    "css",
    "dart",
    "diff",
    "env",
    "fish",
    "go",
    "h",
    "hpp",
    "ini",
    "java",
    "js",
    "json",
    "json5",
    "jsx",
    "kt",
    "kts",
    "less",
    "lua",
    "md",
    "mdx",
    "mjs",
    "php",
    "pl",
    "properties",
    "proto",
    "ps1",
    "py",
    "rb",
    "rs",
    "rst",
    "sass",
    "scala",
    "scss",
    "sh",
    "sql",
    "svelte",
    "swift",
    "tex",
    "text",
    "toml",
    "ts",
    "tsx",
    "txt",
    "vue",
    "xml",
    "yaml",
    "yml",
    "zsh",
];
