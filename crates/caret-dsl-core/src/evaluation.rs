//! 变量文档的有界类型检查、依赖排序与确定性求值。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::diagnostic::push_diagnostic_span;
use crate::{Diagnostic, Diagnostics, Document, Kind, ValueType, parse};

const MAX_EXPRESSION_DEPTH: usize = 128;
const MAX_EXPRESSION_OPERATORS: usize = 128;
const MAX_DEPENDENCY_DEPTH: usize = 128;
const MAX_EVALUATION_OPERATIONS: usize = 100_000;

/// 已求值的变量值；resource 保留逻辑引用，由应用层按工程路径策略解析。
#[derive(Debug, Clone, PartialEq)]
pub enum EvaluatedValue {
    Bool(bool),
    Int(i64),
    Real(f64),
    String(String),
    Resource(String),
}

/// 求值完成的不可变有序变量表。
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluatedVariables {
    values: BTreeMap<String, EvaluatedValue>,
    source_ranges: BTreeMap<String, std::ops::Range<usize>>,
}

impl EvaluatedVariables {
    /// 按名称读取已求值的变量。
    pub fn get(&self, name: &str) -> Option<&EvaluatedValue> {
        self.values.get(name)
    }

    /// 按字典序遍历变量。
    pub fn iter(&self) -> impl Iterator<Item = (&str, &EvaluatedValue)> {
        self.values
            .iter()
            .map(|(name, value)| (name.as_str(), value))
    }

    /// 变量数量。
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// 返回声明表达式在原始 UTF-8 文本中的字节范围。
    pub fn source_range(&self, name: &str) -> Option<std::ops::Range<usize>> {
        self.source_ranges.get(name).cloned()
    }
}

/// 解析并求值一个 `kind: variables` 文档。
pub fn evaluate_variables(source: &str) -> Result<EvaluatedVariables, Diagnostics> {
    let document = parse(source)?;
    evaluate_for_kind(&document, source, Kind::Variables)
}

/// 解析并求值一个 `kind: theme` 文档。
pub fn evaluate_theme(source: &str) -> Result<EvaluatedVariables, Diagnostics> {
    let document = parse(source)?;
    evaluate_for_kind(&document, source, Kind::Theme)
}

fn evaluate_for_kind(
    document: &Document,
    source: &str,
    expected_kind: Kind,
) -> Result<EvaluatedVariables, Diagnostics> {
    if document.kind != expected_kind {
        let (code, description) = match expected_kind {
            Kind::Variables => (
                "variables.wrong_document_kind",
                "expected a variables document",
            ),
            Kind::Theme => ("theme.wrong_document_kind", "expected a theme document"),
            Kind::Language => ("values.wrong_document_kind", "expected a values document"),
        };
        return Err(Diagnostics::one(code, description, 0, source));
    }

    let mut diagnostics = Vec::new();
    let names: HashMap<&str, usize> = document
        .values
        .iter()
        .enumerate()
        .map(|(index, variable)| (variable.name.as_str(), index))
        .collect();
    let mut expressions = Vec::with_capacity(document.values.len());
    let mut dependencies = Vec::with_capacity(document.values.len());

    for variable in &document.values {
        let expression = match variable.value_type {
            ValueType::Bool => parse_bool_literal(variable, source, &mut diagnostics),
            ValueType::String => parse_string_literal(variable, source, &mut diagnostics),
            ValueType::Resource => parse_resource_literal(variable, source, &mut diagnostics),
            ValueType::Int | ValueType::Real => {
                let mut parser = ExpressionParser::new(&variable.expression);
                match parser.parse() {
                    Ok(expression) => Some(expression),
                    Err(error) => {
                        push_variable_error(
                            &mut diagnostics,
                            "variables.expression_syntax",
                            error.message,
                            variable,
                            error.start,
                            error.end,
                            source,
                        );
                        None
                    }
                }
            }
        };
        let refs = expression.as_ref().map_or_else(Vec::new, Expr::references);
        for (name, start, end) in &refs {
            if !names.contains_key(name.as_str()) {
                push_variable_error(
                    &mut diagnostics,
                    "variables.unknown_reference",
                    format!("unknown variable '{name}'"),
                    variable,
                    *start,
                    *end,
                    source,
                );
            }
        }
        expressions.push(expression);
        dependencies.push(
            refs.into_iter()
                .map(|(name, _, _)| name)
                .collect::<HashSet<_>>(),
        );
    }

    if !diagnostics.is_empty() {
        return Err(Diagnostics { diagnostics });
    }

    let mut dependents = vec![Vec::new(); document.values.len()];
    let mut indegree = vec![0usize; document.values.len()];
    for (index, refs) in dependencies.iter().enumerate() {
        let mut unique = BTreeSet::new();
        for name in refs {
            let dependency = names[name.as_str()];
            if unique.insert(dependency) {
                indegree[index] += 1;
                dependents[dependency].push(index);
            }
        }
    }

    let mut ready = BTreeSet::new();
    for (index, count) in indegree.iter().enumerate() {
        if *count == 0 {
            ready.insert(index);
        }
    }
    let mut ordered = Vec::with_capacity(document.values.len());
    let mut dependency_depths = vec![0usize; document.values.len()];
    while let Some(index) = ready.pop_first() {
        ordered.push(index);
        for dependent in &dependents[index] {
            dependency_depths[*dependent] =
                dependency_depths[*dependent].max(dependency_depths[index].saturating_add(1));
            if dependency_depths[*dependent] > MAX_DEPENDENCY_DEPTH {
                let variable = &document.values[*dependent];
                push_variable_error(
                    &mut diagnostics,
                    "variables.dependency_depth",
                    format!("dependency depth exceeds {MAX_DEPENDENCY_DEPTH}"),
                    variable,
                    0,
                    variable.expression.len(),
                    source,
                );
            }
            indegree[*dependent] -= 1;
            if indegree[*dependent] == 0 {
                ready.insert(*dependent);
            }
        }
    }

    if ordered.len() != document.values.len() {
        for (index, count) in indegree.iter().enumerate() {
            if *count != 0 {
                let variable = &document.values[index];
                push_variable_error(
                    &mut diagnostics,
                    "variables.dependency_cycle",
                    format!(
                        "variable '{}' is part of or depends on a cycle",
                        variable.name
                    ),
                    variable,
                    0,
                    variable.expression.len(),
                    source,
                );
            }
        }
        return Err(Diagnostics { diagnostics });
    }

    let mut values = BTreeMap::new();
    let mut operations = 0usize;
    for index in ordered {
        let variable = &document.values[index];
        let Some(expression) = expressions[index].as_ref() else {
            continue;
        };
        let mut evaluator = Evaluator {
            variables: &values,
            operations: &mut operations,
            diagnostics: &mut diagnostics,
            variable,
            source,
        };
        if let Some(value) = evaluator.evaluate(expression, 0) {
            if let Some(value) = coerce_to_declared_type(value, &variable.value_type) {
                values.insert(variable.name.clone(), value);
            } else {
                push_variable_error(
                    evaluator.diagnostics,
                    "variables.type_mismatch",
                    format!(
                        "expression does not produce the declared {:?} type",
                        variable.value_type
                    ),
                    variable,
                    0,
                    variable.expression.len(),
                    source,
                );
            }
        }
        if diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "variables.evaluation_limit")
        {
            break;
        }
    }

    if diagnostics.is_empty() && values.len() == document.values.len() {
        let source_ranges = document
            .values
            .iter()
            .map(|variable| {
                (
                    variable.name.clone(),
                    variable.expression_start..variable.expression_end,
                )
            })
            .collect();
        Ok(EvaluatedVariables {
            values,
            source_ranges,
        })
    } else {
        if diagnostics.is_empty() {
            return Err(Diagnostics::one(
                "variables.dependency_failed",
                "one or more variables could not be evaluated",
                0,
                source,
            ));
        }
        Err(Diagnostics { diagnostics })
    }
}

fn parse_bool_literal(
    variable: &crate::Variable,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Expr> {
    let literal = match variable.expression.trim() {
        "true" => EvaluatedValue::Bool(true),
        "false" => EvaluatedValue::Bool(false),
        _ => {
            push_variable_error(
                diagnostics,
                "variables.literal_syntax",
                "bool value must be 'true' or 'false'",
                variable,
                0,
                variable.expression.len(),
                source,
            );
            return None;
        }
    };
    Some(Expr::Literal(literal, 0, variable.expression.len()))
}

fn parse_string_literal(
    variable: &crate::Variable,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Expr> {
    decode_literal(&variable.expression, variable, source, diagnostics)
        .map(|value| Expr::Literal(EvaluatedValue::String(value), 0, variable.expression.len()))
}

fn parse_resource_literal(
    variable: &crate::Variable,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Expr> {
    let value = decode_literal(&variable.expression, variable, source, diagnostics)?;
    if value.trim().is_empty() {
        push_variable_error(
            diagnostics,
            "variables.invalid_resource",
            "resource reference cannot be empty",
            variable,
            0,
            variable.expression.len(),
            source,
        );
        return None;
    }
    Some(Expr::Literal(
        EvaluatedValue::Resource(value),
        0,
        variable.expression.len(),
    ))
}

fn decode_literal(
    expression: &str,
    variable: &crate::Variable,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    let leading_whitespace = expression.len() - expression.trim_start().len();
    let trimmed = &expression[leading_whitespace..];
    let (raw, base_offset) = if trimmed.starts_with('"') {
        if !trimmed.ends_with('"') || trimmed.len() < 2 {
            push_variable_error(
                diagnostics,
                "variables.literal_syntax",
                "unterminated quoted literal",
                variable,
                0,
                expression.len(),
                source,
            );
            return None;
        }
        (&trimmed[1..trimmed.len() - 1], leading_whitespace + 1)
    } else {
        (trimmed, leading_whitespace)
    };
    let mut result = String::with_capacity(raw.len());
    let mut chars = raw.char_indices();
    while let Some((offset, character)) = chars.next() {
        if character != '\\' {
            if character == '"' {
                push_variable_error(
                    diagnostics,
                    "variables.literal_syntax",
                    "embedded quote must be escaped",
                    variable,
                    base_offset + offset,
                    base_offset + offset + character.len_utf8(),
                    source,
                );
                return None;
            }
            result.push(character);
            continue;
        }
        let Some((escaped_offset, escaped)) = chars.next() else {
            push_variable_error(
                diagnostics,
                "variables.invalid_escape",
                "trailing escape in literal",
                variable,
                base_offset + offset,
                base_offset + offset + 1,
                source,
            );
            return None;
        };
        match escaped {
            'n' => result.push('\n'),
            'r' => result.push('\r'),
            't' => result.push('\t'),
            ':' | '=' | ' ' | '\\' | '"' => result.push(escaped),
            _ => {
                push_variable_error(
                    diagnostics,
                    "variables.invalid_escape",
                    format!("unsupported escape \\{escaped}"),
                    variable,
                    base_offset + offset,
                    base_offset + escaped_offset + escaped.len_utf8(),
                    source,
                );
                return None;
            }
        }
    }
    Some(result)
}

fn coerce_to_declared_type(value: EvaluatedValue, expected: &ValueType) -> Option<EvaluatedValue> {
    match (expected, value) {
        (ValueType::Bool, value @ EvaluatedValue::Bool(_))
        | (ValueType::Int, value @ EvaluatedValue::Int(_))
        | (ValueType::Real, value @ EvaluatedValue::Real(_))
        | (ValueType::String, value @ EvaluatedValue::String(_))
        | (ValueType::Resource, value @ EvaluatedValue::Resource(_)) => Some(value),
        (ValueType::Real, EvaluatedValue::Int(value)) => Some(EvaluatedValue::Real(value as f64)),
        _ => None,
    }
}

fn push_variable_error(
    diagnostics: &mut Vec<Diagnostic>,
    code: &str,
    message: impl Into<String>,
    variable: &crate::Variable,
    relative_start: usize,
    relative_end: usize,
    source: &str,
) {
    let start = variable
        .expression_start
        .saturating_add(relative_start)
        .min(variable.expression_end);
    let end = variable
        .expression_start
        .saturating_add(relative_end)
        .clamp(start, variable.expression_end);
    push_diagnostic_span(diagnostics, code, message, start, end, source);
}

#[derive(Debug, Clone)]
enum Expr {
    Literal(EvaluatedValue, usize, usize),
    Reference(String, usize, usize),
    UnaryMinus(Box<Self>, usize, usize),
    Add(Box<Self>, Box<Self>, usize, usize),
    Subtract(Box<Self>, Box<Self>, usize, usize),
    Multiply(Box<Self>, Box<Self>, usize, usize),
    Divide(Box<Self>, Box<Self>, usize, usize),
}

impl Expr {
    fn references(&self) -> Vec<(String, usize, usize)> {
        let mut result = Vec::new();
        self.collect_references(&mut result);
        result
    }

    fn collect_references(&self, output: &mut Vec<(String, usize, usize)>) {
        match self {
            Self::Literal(..) => {}
            Self::Reference(name, start, end) => output.push((name.clone(), *start, *end)),
            Self::UnaryMinus(value, ..) => value.collect_references(output),
            Self::Add(left, right, ..)
            | Self::Subtract(left, right, ..)
            | Self::Multiply(left, right, ..)
            | Self::Divide(left, right, ..) => {
                left.collect_references(output);
                right.collect_references(output);
            }
        }
    }
}

struct Evaluator<'a> {
    variables: &'a BTreeMap<String, EvaluatedValue>,
    operations: &'a mut usize,
    diagnostics: &'a mut Vec<Diagnostic>,
    variable: &'a crate::Variable,
    source: &'a str,
}

impl Evaluator<'_> {
    fn evaluate(&mut self, expression: &Expr, depth: usize) -> Option<EvaluatedValue> {
        if depth > MAX_EXPRESSION_DEPTH {
            push_variable_error(
                self.diagnostics,
                "variables.expression_depth",
                format!("expression depth exceeds {MAX_EXPRESSION_DEPTH}"),
                self.variable,
                0,
                self.variable.expression.len(),
                self.source,
            );
            return None;
        }
        if *self.operations >= MAX_EVALUATION_OPERATIONS {
            return self.error(
                "variables.evaluation_limit",
                "evaluation operation limit exceeded",
                expr_start(expression),
                expr_end(expression),
            );
        }
        *self.operations = (*self.operations).saturating_add(1);
        match expression {
            Expr::Literal(value, ..) => Some(value.clone()),
            Expr::Reference(name, start, end) => self.variables.get(name).cloned().or_else(|| {
                push_variable_error(
                    self.diagnostics,
                    "variables.dependency_failed",
                    format!("variable '{name}' could not be evaluated"),
                    self.variable,
                    *start,
                    *end,
                    self.source,
                );
                None
            }),
            Expr::UnaryMinus(value, start, end) => match self.evaluate(value, depth + 1)? {
                EvaluatedValue::Int(number) => {
                    number.checked_neg().map(EvaluatedValue::Int).or_else(|| {
                        self.error(
                            "variables.integer_overflow",
                            "integer negation overflow",
                            *start,
                            *end,
                        )
                    })
                }
                EvaluatedValue::Real(number) => self.finite(-number, *start, *end),
                _ => self.error(
                    "variables.type_mismatch",
                    "unary '-' requires a number",
                    *start,
                    *end,
                ),
            },
            Expr::Add(left, right, start, end) => {
                self.numeric_binary(left, right, depth, *start, *end, NumericOp::Add)
            }
            Expr::Subtract(left, right, start, end) => {
                self.numeric_binary(left, right, depth, *start, *end, NumericOp::Subtract)
            }
            Expr::Multiply(left, right, start, end) => {
                self.numeric_binary(left, right, depth, *start, *end, NumericOp::Multiply)
            }
            Expr::Divide(left, right, start, end) => {
                self.numeric_binary(left, right, depth, *start, *end, NumericOp::Divide)
            }
        }
    }

    fn numeric_binary(
        &mut self,
        left: &Expr,
        right: &Expr,
        depth: usize,
        start: usize,
        end: usize,
        operation: NumericOp,
    ) -> Option<EvaluatedValue> {
        let left = self.evaluate(left, depth + 1)?;
        let right = self.evaluate(right, depth + 1)?;
        match (left, right) {
            (EvaluatedValue::Int(left), EvaluatedValue::Int(right)) => match operation {
                NumericOp::Add => left
                    .checked_add(right)
                    .map(EvaluatedValue::Int)
                    .or_else(|| {
                        self.error(
                            "variables.integer_overflow",
                            "integer addition overflow",
                            start,
                            end,
                        )
                    }),
                NumericOp::Subtract => {
                    left.checked_sub(right)
                        .map(EvaluatedValue::Int)
                        .or_else(|| {
                            self.error(
                                "variables.integer_overflow",
                                "integer subtraction overflow",
                                start,
                                end,
                            )
                        })
                }
                NumericOp::Multiply => {
                    left.checked_mul(right)
                        .map(EvaluatedValue::Int)
                        .or_else(|| {
                            self.error(
                                "variables.integer_overflow",
                                "integer multiplication overflow",
                                start,
                                end,
                            )
                        })
                }
                NumericOp::Divide => self.real_division(left as f64, right as f64, start, end),
            },
            (left, right) => {
                let (Some(left), Some(right)) = (to_real(left), to_real(right)) else {
                    return self.error(
                        "variables.type_mismatch",
                        "arithmetic requires numeric operands",
                        start,
                        end,
                    );
                };
                match operation {
                    NumericOp::Add => self.finite(left + right, start, end),
                    NumericOp::Subtract => self.finite(left - right, start, end),
                    NumericOp::Multiply => self.finite(left * right, start, end),
                    NumericOp::Divide => self.real_division(left, right, start, end),
                }
            }
        }
    }

    fn real_division(
        &mut self,
        numerator: f64,
        denominator: f64,
        start: usize,
        end: usize,
    ) -> Option<EvaluatedValue> {
        if denominator == 0.0 {
            return self.error("variables.division_by_zero", "division by zero", start, end);
        }
        self.finite(numerator / denominator, start, end)
    }

    fn finite(&mut self, value: f64, start: usize, end: usize) -> Option<EvaluatedValue> {
        if value.is_finite() {
            Some(EvaluatedValue::Real(value))
        } else {
            self.error(
                "variables.non_finite",
                "expression produced a non-finite number",
                start,
                end,
            )
        }
    }

    fn error<T>(&mut self, code: &str, message: &str, start: usize, end: usize) -> Option<T> {
        push_variable_error(
            self.diagnostics,
            code,
            message,
            self.variable,
            start,
            end,
            self.source,
        );
        None
    }
}

fn to_real(value: EvaluatedValue) -> Option<f64> {
    match value {
        EvaluatedValue::Int(value) => Some(value as f64),
        EvaluatedValue::Real(value) => Some(value),
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum NumericOp {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TokenKind {
    Number,
    Name,
    Plus,
    Minus,
    Star,
    Slash,
    LeftParen,
    RightParen,
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    text: String,
    start: usize,
    end: usize,
}

struct ParseError {
    message: String,
    start: usize,
    end: usize,
}

struct ExpressionParser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    cursor: usize,
    recursion: usize,
    operators: usize,
}

impl<'a> ExpressionParser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            tokens: Vec::new(),
            cursor: 0,
            recursion: 0,
            operators: 0,
        }
    }

    fn parse(&mut self) -> Result<Expr, ParseError> {
        self.tokens = tokenize(self.source)?;
        if self.tokens.is_empty() {
            return Err(ParseError {
                message: "numeric expression cannot be empty".to_owned(),
                start: 0,
                end: self.source.len(),
            });
        }
        let expression = self.parse_precedence(0)?;
        if let Some(token) = self.tokens.get(self.cursor) {
            return Err(ParseError {
                message: format!("unexpected token '{}'", token.text),
                start: token.start,
                end: token.end,
            });
        }
        Ok(expression)
    }

    fn parse_precedence(&mut self, minimum: u8) -> Result<Expr, ParseError> {
        self.recursion += 1;
        if self.recursion > MAX_EXPRESSION_DEPTH {
            self.recursion -= 1;
            return Err(ParseError {
                message: format!("expression nesting exceeds {MAX_EXPRESSION_DEPTH}"),
                start: 0,
                end: self.source.len(),
            });
        }
        let mut left = self.parse_prefix()?;
        while let Some(operator) = self.tokens.get(self.cursor).cloned() {
            let (precedence, operation) = match operator.kind {
                TokenKind::Plus => (1, NumericOp::Add),
                TokenKind::Minus => (1, NumericOp::Subtract),
                TokenKind::Star => (2, NumericOp::Multiply),
                TokenKind::Slash => (2, NumericOp::Divide),
                _ => break,
            };
            if precedence < minimum {
                break;
            }
            if matches!(operation, NumericOp::Add | NumericOp::Subtract)
                && !has_whitespace_around(self.source, operator.start, operator.end)
            {
                return Err(ParseError {
                    message: "binary '+' and '-' require surrounding whitespace".to_owned(),
                    start: operator.start,
                    end: operator.end,
                });
            }
            self.cursor += 1;
            self.count_operator(&operator)?;
            let right = self.parse_precedence(precedence + 1)?;
            let start = expr_start(&left);
            let end = expr_end(&right);
            left = match operation {
                NumericOp::Add => Expr::Add(Box::new(left), Box::new(right), start, end),
                NumericOp::Subtract => Expr::Subtract(Box::new(left), Box::new(right), start, end),
                NumericOp::Multiply => Expr::Multiply(Box::new(left), Box::new(right), start, end),
                NumericOp::Divide => Expr::Divide(Box::new(left), Box::new(right), start, end),
            };
        }
        self.recursion -= 1;
        Ok(left)
    }

    fn parse_prefix(&mut self) -> Result<Expr, ParseError> {
        let token = self
            .tokens
            .get(self.cursor)
            .cloned()
            .ok_or_else(|| ParseError {
                message: "expected a value".to_owned(),
                start: self.source.len(),
                end: self.source.len(),
            })?;
        self.cursor += 1;
        match token.kind {
            TokenKind::Number => {
                if token.text.contains(['.', 'e', 'E']) {
                    let value = token
                        .text
                        .parse::<f64>()
                        .ok()
                        .filter(|number| number.is_finite());
                    value
                        .map(|number| {
                            Expr::Literal(EvaluatedValue::Real(number), token.start, token.end)
                        })
                        .ok_or_else(|| ParseError {
                            message: "invalid or non-finite real literal".to_owned(),
                            start: token.start,
                            end: token.end,
                        })
                } else {
                    token
                        .text
                        .parse::<i64>()
                        .map(|number| {
                            Expr::Literal(EvaluatedValue::Int(number), token.start, token.end)
                        })
                        .map_err(|_| ParseError {
                            message: "integer literal is outside the supported range".to_owned(),
                            start: token.start,
                            end: token.end,
                        })
                }
            }
            TokenKind::Name => Ok(Expr::Reference(token.text, token.start, token.end)),
            TokenKind::Minus => {
                self.count_operator(&token)?;
                if let Some(number) = self.tokens.get(self.cursor)
                    && number.kind == TokenKind::Number
                    && number.text == "9223372036854775808"
                {
                    self.cursor += 1;
                    return Ok(Expr::Literal(
                        EvaluatedValue::Int(i64::MIN),
                        token.start,
                        number.end,
                    ));
                }
                let value = self.parse_precedence(3)?;
                let end = expr_end(&value);
                Ok(Expr::UnaryMinus(Box::new(value), token.start, end))
            }
            TokenKind::Plus => {
                self.count_operator(&token)?;
                self.parse_precedence(3)
            }
            TokenKind::LeftParen => {
                let value = self.parse_precedence(0)?;
                let Some(close) = self.tokens.get(self.cursor) else {
                    return Err(ParseError {
                        message: "missing closing parenthesis".to_owned(),
                        start: token.start,
                        end: self.source.len(),
                    });
                };
                if close.kind != TokenKind::RightParen {
                    return Err(ParseError {
                        message: "expected closing parenthesis".to_owned(),
                        start: close.start,
                        end: close.end,
                    });
                }
                self.cursor += 1;
                Ok(value)
            }
            _ => Err(ParseError {
                message: format!("unexpected token '{}'", token.text),
                start: token.start,
                end: token.end,
            }),
        }
    }

    fn count_operator(&mut self, token: &Token) -> Result<(), ParseError> {
        self.operators += 1;
        if self.operators > MAX_EXPRESSION_OPERATORS {
            return Err(ParseError {
                message: format!(
                    "expression contains more than {MAX_EXPRESSION_OPERATORS} operators"
                ),
                start: token.start,
                end: token.end,
            });
        }
        Ok(())
    }
}

fn tokenize(source: &str) -> Result<Vec<Token>, ParseError> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if byte.is_ascii_whitespace() {
            cursor += 1;
            continue;
        }
        let start = cursor;
        let kind = match byte {
            b'+' => TokenKind::Plus,
            b'-' => TokenKind::Minus,
            b'*' => TokenKind::Star,
            b'/' => TokenKind::Slash,
            b'(' => TokenKind::LeftParen,
            b')' => TokenKind::RightParen,
            b'0'..=b'9' | b'.'
                if byte.is_ascii_digit()
                    || bytes.get(cursor + 1).is_some_and(u8::is_ascii_digit) =>
            {
                cursor = scan_number(bytes, cursor)?;
                tokens.push(Token {
                    kind: TokenKind::Number,
                    text: source[start..cursor].to_owned(),
                    start,
                    end: cursor,
                });
                continue;
            }
            b'A'..=b'Z' | b'a'..=b'z' => {
                cursor += 1;
                while bytes
                    .get(cursor)
                    .is_some_and(|next| next.is_ascii_alphanumeric() || *next == b'-')
                {
                    cursor += 1;
                }
                tokens.push(Token {
                    kind: TokenKind::Name,
                    text: source[start..cursor].to_owned(),
                    start,
                    end: cursor,
                });
                continue;
            }
            _ => {
                let character = source[cursor..].chars().next().unwrap_or('\0');
                let end = cursor + character.len_utf8();
                return Err(ParseError {
                    message: format!("unexpected character '{character}'"),
                    start: cursor,
                    end,
                });
            }
        };
        cursor += 1;
        tokens.push(Token {
            kind,
            text: source[start..cursor].to_owned(),
            start,
            end: cursor,
        });
    }
    Ok(tokens)
}

fn scan_number(bytes: &[u8], start: usize) -> Result<usize, ParseError> {
    let mut cursor = start;
    let mut integral_digits = 0usize;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        integral_digits += 1;
        cursor += 1;
    }
    let mut fractional_digits = 0usize;
    if bytes.get(cursor) == Some(&b'.') {
        cursor += 1;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            fractional_digits += 1;
            cursor += 1;
        }
    }
    if integral_digits == 0 && fractional_digits == 0 {
        return Err(ParseError {
            message: "invalid numeric literal".to_owned(),
            start,
            end: cursor,
        });
    }
    if bytes
        .get(cursor)
        .is_some_and(|byte| matches!(byte, b'e' | b'E'))
    {
        cursor += 1;
        if bytes
            .get(cursor)
            .is_some_and(|byte| matches!(byte, b'+' | b'-'))
        {
            cursor += 1;
        }
        let exponent_start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        if exponent_start == cursor {
            return Err(ParseError {
                message: "real exponent requires digits".to_owned(),
                start,
                end: cursor,
            });
        }
    }
    Ok(cursor)
}

fn has_whitespace_around(source: &str, start: usize, end: usize) -> bool {
    source[..start]
        .chars()
        .next_back()
        .is_some_and(char::is_whitespace)
        && source[end..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
}

fn expr_start(expression: &Expr) -> usize {
    match expression {
        Expr::Literal(_, start, _)
        | Expr::Reference(_, start, _)
        | Expr::UnaryMinus(_, start, _)
        | Expr::Add(_, _, start, _)
        | Expr::Subtract(_, _, start, _)
        | Expr::Multiply(_, _, start, _)
        | Expr::Divide(_, _, start, _) => *start,
    }
}

fn expr_end(expression: &Expr) -> usize {
    match expression {
        Expr::Literal(_, _, end)
        | Expr::Reference(_, _, end)
        | Expr::UnaryMinus(_, _, end)
        | Expr::Add(_, _, _, end)
        | Expr::Subtract(_, _, _, end)
        | Expr::Multiply(_, _, _, end)
        | Expr::Divide(_, _, _, end) => *end,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{EvaluatedValue, MAX_DEPENDENCY_DEPTH, evaluate_variables};

    fn evaluate(values: &str) -> Result<super::EvaluatedVariables, crate::Diagnostics> {
        evaluate_variables(&format!("version: 1\nkind: variables\n\nvalues:\n{values}"))
    }

    #[test]
    fn evaluates_forward_references_and_arithmetic() {
        let result = evaluate(
            "  total: int = width * 2\n  width: int = 12\n  half: real = total / 4\n  scaled: real = half + 0.5\n",
        )
        .expect("valid expressions");
        assert_eq!(result.get("total"), Some(&EvaluatedValue::Int(24)));
        assert_eq!(result.get("half"), Some(&EvaluatedValue::Real(6.0)));
        assert_eq!(result.get("scaled"), Some(&EvaluatedValue::Real(6.5)));
    }

    #[test]
    fn accepts_typed_literals_and_escapes() {
        let result = evaluate(
            "  enabled: bool = true\n  label: string = \"feed\\nport\"\n  mesh: resource = project:/assets/mesh.vtu\n  promoted: real = 2\n",
        )
        .expect("valid literals");
        assert_eq!(result.get("enabled"), Some(&EvaluatedValue::Bool(true)));
        assert_eq!(
            result.get("label"),
            Some(&EvaluatedValue::String("feed\nport".to_owned()))
        );
        assert_eq!(
            result.get("mesh"),
            Some(&EvaluatedValue::Resource(
                "project:/assets/mesh.vtu".to_owned()
            ))
        );
        assert_eq!(result.get("promoted"), Some(&EvaluatedValue::Real(2.0)));
    }

    #[test]
    fn rejects_cycles_unknown_references_type_mismatches_and_division_by_zero() {
        for (source, expected) in [
            (
                "  first: int = second\n  second: int = first\n",
                "variables.dependency_cycle",
            ),
            ("  first: int = missing\n", "variables.unknown_reference"),
            ("  first: int = 1 / 2\n", "variables.type_mismatch"),
            ("  first: real = 1 / 0\n", "variables.division_by_zero"),
            ("  first: bool = yes\n", "variables.literal_syntax"),
        ] {
            let error = evaluate(source).expect_err("invalid variables");
            assert!(
                error
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == expected),
                "expected {expected}: {error}"
            );
        }
    }

    #[test]
    fn reports_source_spans_for_expression_errors() {
        let source = "version: 1\nkind: variables\n\nvalues:\n  good: int = 1\n  bad: int = good + unknown\n";
        let error = evaluate_variables(source).expect_err("unknown variable");
        let diagnostic = error
            .diagnostics
            .iter()
            .find(|item| item.code == "variables.unknown_reference")
            .expect("reference diagnostic");
        assert_eq!(
            &source[diagnostic.start_offset..diagnostic.end_offset],
            "unknown"
        );
        assert_eq!(diagnostic.line, 6);
    }

    #[test]
    fn supports_signed_minimum_integer_and_canonical_string_quoting() {
        let source = "version: 1\nkind: variables\n\nvalues:\n  minimum: int = -9223372036854775808\n  label: string = \"feed port\"\n";
        let result = evaluate_variables(source).expect("valid boundaries");
        assert_eq!(result.get("minimum"), Some(&EvaluatedValue::Int(i64::MIN)));
        let formatted = crate::format_source(source).expect("format source");
        assert_eq!(formatted, source);
        assert_eq!(evaluate_variables(&formatted).expect("re-evaluate"), result);
    }

    #[test]
    fn preserves_escaped_trailing_space_and_valid_unicode_diagnostic_spans() {
        let result = evaluate("  label: string = end\\ \n").expect("escaped trailing space");
        assert_eq!(
            result.get("label"),
            Some(&EvaluatedValue::String("end ".to_owned()))
        );

        let source = "version: 1\nkind: variables\n\nvalues:\n  label: string = \\\u{4e2d}\n";
        let error = evaluate_variables(source).expect_err("unsupported Unicode escape");
        let diagnostic = error
            .diagnostics
            .iter()
            .find(|item| item.code == "variables.invalid_escape")
            .expect("escape diagnostic");
        assert_eq!(
            &source[diagnostic.start_offset..diagnostic.end_offset],
            "\\中"
        );
    }

    #[test]
    fn variable_names_true_and_false_are_references_in_numeric_expressions() {
        let result = evaluate("  result: int = true\n  true: int = 5\n")
            .expect("boolean-looking variable name is a reference");
        assert_eq!(result.get("result"), Some(&EvaluatedValue::Int(5)));
    }

    #[test]
    fn rejects_excessive_operators_before_building_a_deep_tree() {
        let expression = std::iter::repeat_n("1", 2_000)
            .collect::<Vec<_>>()
            .join(" * ");
        let error = evaluate(&format!("  result: int = {expression}\n"))
            .expect_err("flat expression exceeds the operator limit");
        assert!(
            error
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "variables.expression_syntax")
        );

        let terms = std::iter::repeat_n("+1", 64)
            .collect::<Vec<_>>()
            .join(" + ");
        let result = evaluate(&format!("  result: int = + {terms}\n"))
            .expect("the expression operator limit is inclusive");
        assert_eq!(result.get("result"), Some(&EvaluatedValue::Int(64)));

        let terms = std::iter::repeat_n("+1", 65)
            .collect::<Vec<_>>()
            .join(" + ");
        let error = evaluate(&format!("  result: int = {terms}\n"))
            .expect_err("unary operators count toward the expression limit");
        assert!(
            error
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "variables.expression_syntax")
        );
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "依赖深度与求值预算压力样本的解释执行成本过高；普通测试仍验证预算边界"
    )]
    fn rejects_dependency_depth_and_operation_budget_overruns() {
        let mut dependencies = String::new();
        for index in 0..=MAX_DEPENDENCY_DEPTH {
            dependencies.push_str(&format!("  v-{index}: int = v-{}\n", index + 1));
        }
        dependencies.push_str(&format!("  v-{}: int = 1\n", MAX_DEPENDENCY_DEPTH + 1));
        let error = evaluate(&dependencies).expect_err("dependency depth is bounded");
        assert!(
            error
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "variables.dependency_depth")
        );

        let expression = std::iter::repeat_n("1", 50).collect::<Vec<_>>().join(" + ");
        let mut operations = String::new();
        for index in 0..1_100 {
            operations.push_str(&format!("  value-{index}: int = {expression}\n"));
        }
        let error = evaluate(&operations).expect_err("evaluation work is bounded");
        assert!(
            error
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "variables.evaluation_limit")
        );
    }
}
