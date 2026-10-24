use sqlparser::ast::{Expr, LimitClause, Query, Statement, Value};
use sqlparser::dialect::SQLiteDialect;
use sqlparser::parser::{Parser, ParserError};

#[derive(Debug, PartialEq, Eq)]
pub enum SqlValidationError {
    EmptyQuery,
    ParseError(String),
    MultipleStatements,
    ForbiddenOperation(String),
}

impl std::fmt::Display for SqlValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SqlValidationError::EmptyQuery => write!(f, "Empty SQL query"),
            SqlValidationError::ParseError(msg) => write!(f, "SQL parse error: {}", msg),
            SqlValidationError::MultipleStatements => {
                write!(f, "Multiple SQL statements are not permitted")
            }
            SqlValidationError::ForbiddenOperation(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for SqlValidationError {}

/// Strip outer markdown code blocks (e.g. ```sql ... ```) before parsing.
pub fn strip_markdown_fences(raw_sql: &str) -> &str {
    let mut s = raw_sql.trim();
    if s.starts_with("```") {
        if let Some(first_nl) = s.find('\n') {
            s = &s[first_nl + 1..];
        } else {
            return "";
        }
        if let Some(last_fence) = s.rfind("```") {
            s = &s[..last_fence];
        }
    }
    s.trim()
}

/// Validate and normalize an SQL query ensuring read-only SELECT execution
/// and safe LIMIT enforcement.
const FORBIDDEN_WORDS: &[&str] = &[
    "PRAGMA", "ATTACH", "DETACH", "VACUUM", "REINDEX", "TRANSACTION", "COMMIT", "ROLLBACK", "LOAD_EXTENSION"
];

pub fn validate_and_normalize_sql(
    raw_sql: &str,
    max_limit: u64,
    default_limit: u64,
) -> Result<String, SqlValidationError> {
    let clean = strip_markdown_fences(raw_sql);
    if clean.is_empty() {
        return Err(SqlValidationError::EmptyQuery);
    }

    let upper = clean.to_uppercase();
    for kw in FORBIDDEN_WORDS {
        if upper.split_whitespace().any(|w| w.trim_matches(|c: char| !c.is_alphabetic()) == *kw) {
            return Err(SqlValidationError::ForbiddenOperation(format!(
                "Forbidden SQL keyword detected: {}", kw
            )));
        }
    }

    let dialect = SQLiteDialect {};
    let mut statements = Parser::parse_sql(&dialect, clean).map_err(|e: ParserError| {
        SqlValidationError::ParseError(e.to_string())
    })?;

    if statements.is_empty() {
        return Err(SqlValidationError::EmptyQuery);
    }
    if statements.len() > 1 {
        return Err(SqlValidationError::MultipleStatements);
    }

    let stmt = statements.remove(0);
    match stmt {
        Statement::Query(mut query_box) => {
            clamp_or_apply_limit(&mut query_box, max_limit, default_limit);
            Ok(query_box.to_string())
        }
        _other => {
            Err(SqlValidationError::ForbiddenOperation(
                "Only SELECT or WITH ... SELECT queries are permitted".to_string()
            ))
        }
    }
}

fn clamp_or_apply_limit(query: &mut Query, max_limit: u64, default_limit: u64) {
    match &mut query.limit_clause {
        Some(LimitClause::LimitOffset { limit, .. }) => {
            if let Some(limit_expr) = limit {
                if let Expr::Value(val_with_span) = limit_expr {
                    if let Value::Number(ref num_str, _) = val_with_span.value {
                        if let Ok(limit_val) = num_str.parse::<u64>() {
                            if limit_val > max_limit {
                                *limit_expr = Expr::Value(Value::Number(max_limit.to_string(), false).into());
                            }
                        }
                    }
                }
            } else {
                *limit = Some(Expr::Value(Value::Number(default_limit.to_string(), false).into()));
            }
        }
        None => {
            query.limit_clause = Some(LimitClause::LimitOffset {
                limit: Some(Expr::Value(Value::Number(default_limit.to_string(), false).into())),
                offset: None,
                limit_by: Vec::new(),
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_select() {
        let sql = "SELECT id, name FROM customers WHERE city = 'Delhi'";
        let res = validate_and_normalize_sql(sql, 100, 50).unwrap();
        assert!(res.contains("LIMIT 50"));
    }

    #[test]
    fn test_semicolon_in_string_allowed() {
        let sql = "SELECT * FROM orders WHERE notes = 'Urgent; please call customer'";
        let res = validate_and_normalize_sql(sql, 100, 50).unwrap();
        assert!(res.contains("Urgent; please call customer"));
    }

    #[test]
    fn test_column_named_update_allowed() {
        let sql = "SELECT id, update_time FROM products";
        let res = validate_and_normalize_sql(sql, 100, 50).unwrap();
        assert!(res.contains("update_time"));
    }

    #[test]
    fn test_mutations_blocked() {
        assert!(validate_and_normalize_sql("DELETE FROM customers", 100, 50).is_err());
        assert!(validate_and_normalize_sql("UPDATE customers SET name = 'x'", 100, 50).is_err());
        assert!(validate_and_normalize_sql("DROP TABLE customers", 100, 50).is_err());
        assert!(validate_and_normalize_sql("INSERT INTO customers VALUES (1)", 100, 50).is_err());
    }

    #[test]
    fn test_multiple_statements_blocked() {
        let sql = "SELECT 1; SELECT 2";
        assert_eq!(
            validate_and_normalize_sql(sql, 100, 50),
            Err(SqlValidationError::MultipleStatements)
        );
    }

    #[test]
    fn test_clamping_limit() {
        let sql = "SELECT * FROM items LIMIT 500";
        let res = validate_and_normalize_sql(sql, 100, 50).unwrap();
        assert!(res.contains("LIMIT 100"));
    }
}
