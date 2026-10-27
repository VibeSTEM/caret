//! Theme DSL 的应用级校验与不可变快照；不依赖 Qt 或桌面持久化。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};
use std::sync::Arc;

use caret_dsl_core::{EvaluatedValue, EvaluatedVariables, evaluate_theme, evaluate_variables};

mod schema;

use schema::{THEME_TOKEN_SPECS, ThemeTokenSpec};

const MINIMUM_THEME_COUNT: usize = 2;
pub const DEFAULT_THEME_ID: &str = "light";

/// QML token 在桥接快照中的值类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeTokenKind {
    Color,
    Integer,
    Real,
    Boolean,
}

/// 已校验视觉 token；QML 属性名由单一主题 schema 提供。
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeToken {
    pub qml_name: String,
    pub kind: ThemeTokenKind,
    pub value: ThemeTokenValue,
}

/// 不可变主题值，保持数值类型直到 UI 适配边界。
#[derive(Debug, Clone, PartialEq)]
pub enum ThemeTokenValue {
    Color(String),
    Integer(i64),
    Real(f64),
    Boolean(bool),
}

/// 已完整合并并校验的主题快照。
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeSnapshot {
    pub id: String,
    pub tokens: Vec<ThemeToken>,
}

/// 主题 schema 或文档校验失败；错误码保持稳定供适配层展示和测试。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeError {
    pub code: &'static str,
    pub detail: String,
}

impl Display for ThemeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for ThemeError {}

/// 主题服务持有基线与已验证主题，不读写文件，也不拥有用户偏好。
#[derive(Debug, Clone)]
pub struct ThemeService {
    themes: Arc<BTreeMap<String, ThemeSnapshot>>,
    active_theme_id: String,
}

impl ThemeService {
    /// 解析 GUI 基线和内置主题，并显式选择默认主题；任一输入非法时不返回部分服务。
    pub fn new(
        gui_source: &str,
        theme_sources: impl IntoIterator<Item = (String, String)>,
        default_theme_id: &str,
    ) -> Result<Self, ThemeError> {
        let baseline = evaluate_variables(gui_source).map_err(|diagnostics| ThemeError {
            code: "theme.invalid_gui_baseline",
            detail: diagnostics.to_string(),
        })?;
        let baseline = validate_values(&baseline, true)?;
        let mut themes = BTreeMap::new();

        for (id, source) in theme_sources {
            validate_theme_id(&id)?;
            if themes.contains_key(&id) {
                return Err(ThemeError {
                    code: "theme.duplicate_id",
                    detail: format!("theme id '{id}' is repeated"),
                });
            }
            let values = evaluate_theme(&source).map_err(|diagnostics| ThemeError {
                code: "theme.invalid_definition",
                detail: diagnostics.to_string(),
            })?;
            let overrides = validate_values(&values, false)?;
            let mut merged = baseline.clone();
            merged.extend(overrides);
            themes.insert(
                id.clone(),
                ThemeSnapshot {
                    id,
                    tokens: merged.into_values().collect(),
                },
            );
        }

        if themes.len() < MINIMUM_THEME_COUNT {
            return Err(ThemeError {
                code: "theme.insufficient_definitions",
                detail: format!("at least {MINIMUM_THEME_COUNT} themes are required"),
            });
        }
        if !themes.contains_key(default_theme_id) {
            return Err(ThemeError {
                code: "theme.unknown_id",
                detail: format!("default theme id '{default_theme_id}' is not defined"),
            });
        }
        Ok(Self {
            themes: Arc::new(themes),
            active_theme_id: default_theme_id.to_owned(),
        })
    }

    /// 当前完整快照。
    pub fn current(&self) -> &ThemeSnapshot {
        // `active_theme_id` 只在构造/选择成功时写入，且主题集合不可变。
        &self.themes[&self.active_theme_id]
    }

    /// 所有可选主题 ID，按字典序稳定返回。
    pub fn theme_ids(&self) -> impl Iterator<Item = &str> {
        self.themes.keys().map(String::as_str)
    }

    /// 只读检查主题目录，不克隆或切换主题快照。
    pub fn contains(&self, id: &str) -> bool {
        self.themes.contains_key(id)
    }

    /// 选择已预校验的主题；未知 ID 不改变活动快照。
    pub fn select(&mut self, id: &str) -> Result<&ThemeSnapshot, ThemeError> {
        if !self.themes.contains_key(id) {
            return Err(ThemeError {
                code: "theme.unknown_id",
                detail: format!("unknown theme id '{id}'"),
            });
        }
        self.active_theme_id = id.to_owned();
        Ok(self.current())
    }

    /// 给桌面入口的快照。键是 QML 属性名，值带 kind，便于原样放进 `QVariant`。
    pub(crate) fn bridge_json(&self) -> Result<String, ThemeError> {
        let mut themes = serde_json::Map::new();
        for (id, snapshot) in self.themes.iter() {
            let mut tokens = serde_json::Map::new();
            for token in &snapshot.tokens {
                let kind = match token.kind {
                    ThemeTokenKind::Color => "color",
                    ThemeTokenKind::Integer => "int",
                    ThemeTokenKind::Real => "real",
                    ThemeTokenKind::Boolean => "bool",
                };
                let value = match &token.value {
                    ThemeTokenValue::Color(value) => serde_json::Value::String(value.clone()),
                    ThemeTokenValue::Integer(value) => serde_json::Value::from(*value),
                    ThemeTokenValue::Real(value) => {
                        let Some(number) = serde_json::Number::from_f64(*value) else {
                            return Err(ThemeError {
                                code: "theme.invalid_token_type",
                                detail: format!("token '{}' is not a finite real", token.qml_name),
                            });
                        };
                        serde_json::Value::Number(number)
                    }
                    ThemeTokenValue::Boolean(value) => serde_json::Value::from(*value),
                };
                tokens.insert(
                    token.qml_name.clone(),
                    serde_json::json!({ "kind": kind, "value": value }),
                );
            }
            themes.insert(id.clone(), serde_json::Value::Object(tokens));
        }
        serde_json::to_string(&serde_json::json!({
            "active": self.active_theme_id,
            "themes": themes,
        }))
        .map_err(|error| ThemeError {
            code: "theme.invalid_definition",
            detail: error.to_string(),
        })
    }
}

fn validate_theme_id(id: &str) -> Result<(), ThemeError> {
    if id.is_empty()
        || id.len() > 32
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(ThemeError {
            code: "theme.invalid_id",
            detail: format!(
                "theme id '{id}' must use 1–32 lowercase ASCII letters, digits, or '-'"
            ),
        });
    }
    Ok(())
}

fn validate_values(
    evaluated: &EvaluatedVariables,
    require_complete: bool,
) -> Result<BTreeMap<String, ThemeToken>, ThemeError> {
    let specs = THEME_TOKEN_SPECS
        .iter()
        .map(|spec| (spec.dsl_key, spec))
        .collect::<BTreeMap<_, _>>();
    let mut values = BTreeMap::new();

    for (key, value) in evaluated.iter() {
        let Some(spec) = specs.get(key) else {
            return Err(ThemeError {
                code: "theme.unknown_or_disallowed_token",
                detail: format!("token '{key}' is not allowed in this document"),
            });
        };
        let token_value = validate_value(key, value, spec)?;
        values.insert(
            spec.qml_name.to_owned(),
            ThemeToken {
                qml_name: spec.qml_name.to_owned(),
                kind: spec.kind,
                value: token_value,
            },
        );
    }

    let present = evaluated
        .iter()
        .map(|(key, _)| key)
        .collect::<BTreeSet<_>>();
    let missing = if require_complete {
        specs
            .keys()
            .filter(|key| !present.contains(*key))
            .copied()
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if !missing.is_empty() {
        return Err(ThemeError {
            code: "theme.missing_tokens",
            detail: format!("missing required tokens: {}", missing.join(", ")),
        });
    }
    Ok(values)
}

fn validate_value(
    key: &str,
    value: &EvaluatedValue,
    spec: &ThemeTokenSpec,
) -> Result<ThemeTokenValue, ThemeError> {
    let invalid_type = || ThemeError {
        code: "theme.invalid_token_type",
        detail: format!(
            "token '{key}' has a value incompatible with {:?}",
            spec.kind
        ),
    };
    let invalid_range = |value: f64| ThemeError {
        code: "theme.token_out_of_range",
        detail: format!(
            "token '{key}' value {value} is outside {}..={}",
            spec.minimum, spec.maximum
        ),
    };
    let token = match (spec.kind, value) {
        (ThemeTokenKind::Color, EvaluatedValue::String(color)) if valid_color(color) => {
            ThemeTokenValue::Color(color.clone())
        }
        (ThemeTokenKind::Color, EvaluatedValue::String(_)) => {
            return Err(ThemeError {
                code: "theme.invalid_color",
                detail: format!("token '{key}' is not a supported color"),
            });
        }
        (ThemeTokenKind::Integer, EvaluatedValue::Int(number)) => {
            let number_as_real = *number as f64;
            if !(spec.minimum..=spec.maximum).contains(&number_as_real) {
                return Err(invalid_range(number_as_real));
            }
            ThemeTokenValue::Integer(*number)
        }
        (ThemeTokenKind::Real, EvaluatedValue::Real(number)) => {
            if !number.is_finite() || !(spec.minimum..=spec.maximum).contains(number) {
                return Err(invalid_range(*number));
            }
            ThemeTokenValue::Real(*number)
        }
        (ThemeTokenKind::Boolean, EvaluatedValue::Bool(value)) => ThemeTokenValue::Boolean(*value),
        _ => return Err(invalid_type()),
    };
    Ok(token)
}

/// 用仓库里的基线和 light/dark 主题构造服务。源文件就是 QML 资源，不另写默认值。
pub fn builtin_theme_service() -> Result<ThemeService, ThemeError> {
    ThemeService::new(
        include_str!("../../../../qml/Themes/gui-default.pa"),
        [
            (
                "light".to_owned(),
                include_str!("../../../../qml/Themes/light.pa").to_owned(),
            ),
            (
                "dark".to_owned(),
                include_str!("../../../../qml/Themes/dark.pa").to_owned(),
            ),
        ],
        DEFAULT_THEME_ID,
    )
}

/// 返回供桌面入口注入 QML 的完整主题目录快照。
pub fn builtin_theme_catalog() -> Result<String, ThemeError> {
    let service = builtin_theme_service()?;
    service.bridge_json()
}

fn valid_color(value: &str) -> bool {
    if value == "transparent" {
        return true;
    }
    let Some(hex) = value.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 6 | 8) && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests;
