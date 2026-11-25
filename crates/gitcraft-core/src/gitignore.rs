//! Starting points for a new repository's `.gitignore`: what a language's tools leave behind,
//! and what editors and operating systems scatter around.

/// The languages offered, by key and name.
pub const LANGUAGES: [(&str, &str); 7] = [
    ("rust", "Rust"),
    ("node", "Node.js"),
    ("python", "Python"),
    ("java", "Java / Kotlin"),
    ("dotnet", ".NET"),
    ("go", "Go"),
    ("cpp", "C / C++"),
];

/// The lines for a language key; empty for one not listed.
pub fn language(key: &str) -> &'static str {
    match key {
        "rust" => "# Rust\n/target/\n**/*.rs.bk\n",
        "node" => "# Node.js\nnode_modules/\ndist/\n.npm/\n*.log\n.env\n",
        "python" => {
            "# Python\n__pycache__/\n*.py[cod]\n.venv/\nvenv/\nbuild/\ndist/\n*.egg-info/\n.pytest_cache/\n"
        }
        "java" => "# Java / Kotlin\n*.class\n*.jar\ntarget/\nbuild/\n.gradle/\nout/\n",
        "dotnet" => "# .NET\nbin/\nobj/\n*.user\n*.suo\n.vs/\n",
        "go" => "# Go\n*.exe\n*.test\n*.out\nvendor/\n",
        "cpp" => "# C / C++\n*.o\n*.obj\n*.a\n*.lib\n*.so\n*.dll\n*.exe\nbuild/\n",
        _ => "",
    }
}

/// Editor settings and operating-system clutter.
pub const EDITORS_AND_SYSTEMS: &str = "# Editors and operating systems\n.idea/\n.vscode/\n*.swp\n*~\n.DS_Store\nThumbs.db\ndesktop.ini\n";

/// The whole file for a language (or none) and, if wanted, the editor and system block.
pub fn file(language_key: Option<&str>, editors: bool) -> String {
    let mut parts = Vec::new();
    if let Some(lines) = language_key.map(language).filter(|lines| !lines.is_empty()) {
        parts.push(lines);
    }
    if editors {
        parts.push(EDITORS_AND_SYSTEMS);
    }
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_the_chosen_parts() {
        assert_eq!(file(None, false), "");
        assert!(file(Some("rust"), false).contains("/target/"));
        let both = file(Some("node"), true);
        assert!(both.contains("node_modules/") && both.contains(".DS_Store"));
        assert_eq!(file(Some("nonsense"), false), "");
        for (key, _) in LANGUAGES {
            assert!(!language(key).is_empty(), "{key}");
        }
    }
}
