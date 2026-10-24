use tree_sitter::{Node, Parser};

#[derive(Debug, PartialEq, Eq)]
pub enum KotlinValidationError {
    ParseError(String),
    SyntaxError { line: usize, col: usize, msg: String },
    PackageMismatch { found: String, expected: String },
    DisallowedApi { label: String, snippet: String },
}

impl std::fmt::Display for KotlinValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KotlinValidationError::ParseError(msg) => write!(f, "{}", msg),
            KotlinValidationError::SyntaxError { line, col, msg } => {
                write!(f, "Kotlin syntax error at {}:{}: {}", line, col, msg)
            }
            KotlinValidationError::PackageMismatch { found: _, expected } => {
                write!(f, "must declare `package {}`", expected)
            }
            KotlinValidationError::DisallowedApi { label, snippet } => {
                write!(f, "uses a disallowed API ({}): {:?}", label, snippet)
            }
        }
    }
}

impl std::error::Error for KotlinValidationError {}

const APPROVED_IMPORT_PREFIXES: &[&str] = &[
    "androidx.",
    "android.",
    "kotlin.",
    "kotlinx.coroutines.",
    "java.util.",
    "java.time.",
    "java.text.",
    "java.io.Serializable",
    "org.json.",
    "com.genesis.app",
];

const FORBIDDEN_RULES: &[(&str, &[&str])] = &[
    (
        "process execution",
        &["Runtime.getRuntime", "ProcessBuilder"],
    ),
    (
        "dynamic code loading",
        &[
            "DexClassLoader",
            "PathClassLoader",
            "InMemoryDexClassLoader",
            "URLClassLoader",
            "System.loadLibrary",
            "System.load",
            "Class.forName",
        ],
    ),
    (
        "networking",
        &[
            "java.net",
            "HttpURLConnection",
            "okhttp3",
            "okhttp",
            "retrofit2",
            "retrofit",
            "Socket",
            "android.net.http",
        ],
    ),
    (
        "reflection",
        &[
            "kotlin.reflect.full",
            "java.lang.reflect",
            "getDeclaredMethod",
            "getDeclaredField",
        ],
    ),
    (
        "native code",
        &["external fun", "@JvmStatic external"],
    ),
    (
        "webview",
        &["WebView"],
    ),
];

pub struct KotlinValidator {
    parser: Parser,
}

impl KotlinValidator {
    pub fn new() -> Self {
        let mut parser = Parser::new();
        let lang = tree_sitter_kotlin::language();
        parser.set_language(&lang).expect("failed to load tree-sitter-kotlin grammar");
        Self { parser }
    }

    pub fn validate_file(
        &mut self,
        content: &str,
        expected_package: &str,
    ) -> Result<(), KotlinValidationError> {
        let tree = self
            .parser
            .parse(content, None)
            .ok_or_else(|| KotlinValidationError::ParseError("Failed to parse Kotlin code".into()))?;

        let root = tree.root_node();
        let bytes = content.as_bytes();

        // 1. Check syntax errors
        if let Some(err_node) = find_error_node(root) {
            let start = err_node.start_position();
            return Err(KotlinValidationError::SyntaxError {
                line: start.row + 1,
                col: start.column + 1,
                msg: format!(
                    "unexpected syntax near {:?}",
                    &content[err_node.start_byte()..err_node.end_byte().min(err_node.start_byte() + 30)]
                ),
            });
        }

        // 2. Check package declaration
        let found_package = extract_package_name(root, bytes);
        match found_package {
            Some(ref pkg) if pkg == expected_package => {}
            Some(pkg) => {
                return Err(KotlinValidationError::PackageMismatch {
                    found: pkg,
                    expected: expected_package.to_string(),
                });
            }
            None => {
                return Err(KotlinValidationError::PackageMismatch {
                    found: String::new(),
                    expected: expected_package.to_string(),
                });
            }
        }

        // 3. Walk AST and check imports and forbidden patterns
        check_nodes(root, bytes)?;

        Ok(())
    }

    pub fn has_genesis_app_entry(&mut self, content: &str) -> bool {
        // Fast AST check for @Composable fun GenesisApp()
        let tree = match self.parser.parse(content, None) {
            Some(t) => t,
            None => return false,
        };
        find_genesis_app(tree.root_node(), content.as_bytes())
    }

    pub fn has_design_kit_import(&self, content: &str) -> bool {
        content.contains("com.genesis.app.ui.kit")
    }
}

fn find_error_node(node: Node) -> Option<Node> {
    if node.is_error() || node.is_missing() {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(err) = find_error_node(child) {
            return Some(err);
        }
    }
    None
}

fn extract_package_name(root: Node, bytes: &[u8]) -> Option<String> {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "package_header" {
            // Find the identifier child
            let mut sub_cursor = child.walk();
            for sub in child.children(&mut sub_cursor) {
                if sub.kind() == "identifier" {
                    return sub.utf8_text(bytes).ok().map(|s| s.to_string());
                }
            }
        }
    }
    None
}

fn check_nodes(node: Node, bytes: &[u8]) -> Result<(), KotlinValidationError> {
    let kind = node.kind();

    // 1. Skip comments completely
    if kind == "line_comment" || kind == "multiline_comment" {
        return Ok(());
    }

    // 2. Import header
    if kind == "import_header" {
        let text = node.utf8_text(bytes).unwrap_or("");
        let path = text
            .trim_start_matches("import")
            .trim()
            .trim_end_matches(';')
            .trim();
        let allowed = APPROVED_IMPORT_PREFIXES
            .iter()
            .any(|prefix| path.starts_with(prefix));
        if !allowed {
            return Err(KotlinValidationError::DisallowedApi {
                label: "unapproved libraries".to_string(),
                snippet: text.trim().to_string(),
            });
        }
        return Ok(());
    }

    // 3. String literals: check for shell/su, but inspect interpolations
    if kind == "string_literal" || kind == "character_literal" {
        let text = node.utf8_text(bytes).unwrap_or("");
        if text.contains("/system/bin") || text == "\"su\"" || text == "'su'" {
            return Err(KotlinValidationError::DisallowedApi {
                label: "shell/su".to_string(),
                snippet: text.trim().to_string(),
            });
        }
        // If string has template expressions (e.g. "${Runtime.getRuntime()}"), recurse into them!
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "interpolated_expression" || child.kind() == "interpolated_identifier" {
                check_nodes(child, bytes)?;
            }
        }
        return Ok(());
    }

    // 4. Specific expression/identifier/call nodes
    if is_leaf_or_call_node(kind) {
        let node_text = node.utf8_text(bytes).unwrap_or("");
        for (label, patterns) in FORBIDDEN_RULES {
            for pattern in *patterns {
                if node_text == *pattern || node_text.contains(pattern) {
                    return Err(KotlinValidationError::DisallowedApi {
                        label: label.to_string(),
                        snippet: node_text.trim().to_string(),
                    });
                }
            }
        }
    }

    // Recurse into children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        check_nodes(child, bytes)?;
    }

    Ok(())
}

fn is_leaf_or_call_node(kind: &str) -> bool {
    matches!(
        kind,
        "call_expression"
            | "navigation_expression"
            | "simple_identifier"
            | "identifier"
            | "user_type"
            | "constructor_invocation"
            | "function_declaration"
    )
}

fn find_genesis_app(node: Node, bytes: &[u8]) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "function_declaration" {
            let text = child.utf8_text(bytes).unwrap_or("");
            if text.contains("@Composable") && text.contains("fun GenesisApp()") {
                return true;
            }
        }
        if find_genesis_app(child, bytes) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_file() {
        let mut v = KotlinValidator::new();
        let code = r#"package com.genesis.app

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import com.genesis.app.ui.kit.Spacing

@Composable
fun GenesisApp() {
    Text("hi")
}
"#;
        assert!(v.validate_file(code, "com.genesis.app").is_ok());
        assert!(v.has_genesis_app_entry(code));
        assert!(v.has_design_kit_import(code));
    }

    #[test]
    fn test_package_mismatch() {
        let mut v = KotlinValidator::new();
        let code = "package com.genesis.app\nclass Store\n";
        assert!(matches!(
            v.validate_file(code, "com.genesis.app.data"),
            Err(KotlinValidationError::PackageMismatch { .. })
        ));
    }

    #[test]
    fn test_forbidden_api_blocked() {
        let mut v = KotlinValidator::new();
        let code = "package com.genesis.app\nval p = Runtime.getRuntime().exec(\"ls\")\n";
        assert!(matches!(
            v.validate_file(code, "com.genesis.app"),
            Err(KotlinValidationError::DisallowedApi { .. })
        ));
    }

    #[test]
    fn test_forbidden_api_in_comment_is_allowed() {
        let mut v = KotlinValidator::new();
        let code = "package com.genesis.app\n// uses no ProcessBuilder\nval s = \"safe\"\n";
        assert!(v.validate_file(code, "com.genesis.app").is_ok());
    }

    #[test]
    fn test_forbidden_api_in_plain_string_is_allowed() {
        let mut v = KotlinValidator::new();
        let code = "package com.genesis.app\nval s = \"Runtime.getRuntime() is not used\"\n";
        assert!(v.validate_file(code, "com.genesis.app").is_ok());
    }

    #[test]
    fn test_unapproved_library_import_blocked() {
        let mut v = KotlinValidator::new();
        let code = "package com.genesis.app\nimport com.squareup.picasso.Picasso\n";
        assert!(matches!(
            v.validate_file(code, "com.genesis.app"),
            Err(KotlinValidationError::DisallowedApi { .. })
        ));
    }
}
