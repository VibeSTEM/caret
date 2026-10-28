//! 验证主题加载、校验和运行时切换。

use super::*;

const GUI: &str = include_str!("../../../../../qml/Themes/gui-default.pa");
const LIGHT: &str = include_str!("../../../../../qml/Themes/light.pa");
const DARK: &str = include_str!("../../../../../qml/Themes/dark.pa");

fn service() -> Result<ThemeService, ThemeError> {
    ThemeService::new(
        GUI,
        vec![
            ("light".to_owned(), LIGHT.to_owned()),
            ("dark".to_owned(), DARK.to_owned()),
        ],
        "light",
    )
}

#[test]
fn built_in_themes_merge_full_typed_snapshots() -> Result<(), ThemeError> {
    let mut service = service()?;
    assert_eq!(service.current().id, "light");
    assert_eq!(service.theme_ids().collect::<Vec<_>>(), ["dark", "light"]);
    let dark = service.select("dark")?;
    assert_eq!(dark.tokens.len(), THEME_TOKEN_SPECS.len());
    assert_eq!(
        dark.tokens
            .iter()
            .find(|token| token.qml_name == "colorPanel")
            .map(|token| &token.value),
        Some(&ThemeTokenValue::Color("#292a2d".to_owned()))
    );
    assert_eq!(
        dark.tokens
            .iter()
            .find(|token| token.qml_name == "controlHeight")
            .map(|token| &token.value),
        Some(&ThemeTokenValue::Integer(28))
    );
    let light = service.select("light")?;
    assert_eq!(
        light
            .tokens
            .iter()
            .find(|token| token.qml_name == "colorPanel")
            .map(|token| &token.value),
        Some(&ThemeTokenValue::Color("#ffffff".to_owned()))
    );
    Ok(())
}

#[test]
fn missing_default_theme_is_rejected() {
    assert_eq!(
        ThemeService::new(
            GUI,
            vec![
                ("light".to_owned(), LIGHT.to_owned()),
                ("dark".to_owned(), DARK.to_owned()),
            ],
            "missing",
        )
        .err()
        .map(|error| error.code),
        Some("theme.unknown_id")
    );
}

#[test]
fn themes_can_override_typed_layout_tokens() -> Result<(), ThemeError> {
    let layout_overrides = LIGHT.replace(
            "kind: theme\n",
            "kind: theme\n\nvalues:\n  control-height: int = 48\n  font-body: int = 15\n  caret-width-centipixels: int = 125\n",
        );
    let mut service = ThemeService::new(
        GUI,
        vec![
            ("light".to_owned(), layout_overrides),
            ("dark".to_owned(), DARK.to_owned()),
        ],
        "light",
    )?;
    let light = service.select("light")?;
    assert_eq!(
        light
            .tokens
            .iter()
            .find(|token| token.qml_name == "controlHeight")
            .map(|token| &token.value),
        Some(&ThemeTokenValue::Integer(48))
    );
    assert_eq!(
        light
            .tokens
            .iter()
            .find(|token| token.qml_name == "fontBody")
            .map(|token| &token.value),
        Some(&ThemeTokenValue::Integer(15))
    );
    assert_eq!(
        light
            .tokens
            .iter()
            .find(|token| token.qml_name == "caretWidthCentipixels")
            .map(|token| &token.value),
        Some(&ThemeTokenValue::Integer(125))
    );
    Ok(())
}

#[test]
fn optional_visual_tokens_allow_zero_but_required_dimensions_remain_positive()
-> Result<(), ThemeError> {
    let zero_visual_values = LIGHT.replace(
            "kind: theme\n",
            "kind: theme\n\nvalues:\n  spacing-small: int = 0\n  radius-small: int = 0\n  border-width: int = 0\n  tool-tip-delay: int = 0\n",
        );
    let mut service = ThemeService::new(
        GUI,
        vec![
            ("light".to_owned(), zero_visual_values),
            ("dark".to_owned(), DARK.to_owned()),
        ],
        "light",
    )?;
    let light = service.select("light")?;
    for token_name in ["spacingSmall", "radiusSmall", "borderWidth", "toolTipDelay"] {
        assert_eq!(
            light
                .tokens
                .iter()
                .find(|token| token.qml_name == token_name)
                .map(|token| &token.value),
            Some(&ThemeTokenValue::Integer(0)),
            "{token_name} should allow zero"
        );
    }

    let zero_control_height = LIGHT.replace(
        "kind: theme\n",
        "kind: theme\n\nvalues:\n  control-height: int = 0\n",
    );
    assert_eq!(
        ThemeService::new(
            GUI,
            vec![
                ("light".to_owned(), zero_control_height),
                ("dark".to_owned(), DARK.to_owned()),
            ],
            "light",
        )
        .err()
        .map(|error| error.code),
        Some("theme.token_out_of_range")
    );
    Ok(())
}

#[test]
fn invalid_theme_selection_preserves_current_snapshot() -> Result<(), ThemeError> {
    let mut service = service()?;
    service.select("light")?;
    let before = service.current().clone();
    assert_eq!(
        service.select("missing").err().map(|error| error.code),
        Some("theme.unknown_id")
    );
    assert_eq!(*service.current(), before);
    Ok(())
}

#[test]
fn schema_rejects_unknown_and_invalid_color_tokens() -> Result<(), ThemeError> {
    let unknown = DARK.replace("color-panel", "color-unknown");
    assert_eq!(
        ThemeService::new(
            GUI,
            vec![
                ("light".to_owned(), unknown),
                ("dark".to_owned(), DARK.to_owned())
            ],
            "light",
        )
        .err()
        .map(|error| error.code),
        Some("theme.unknown_or_disallowed_token")
    );

    let mut incomplete = ThemeService::new(
        GUI,
        vec![
            ("light".to_owned(), LIGHT.to_owned()),
            (
                "dark".to_owned(),
                DARK.replace("  color-panel: string = \"#292a2d\"\n", ""),
            ),
        ],
        "light",
    )?;
    assert_eq!(
        incomplete
            .select("dark")?
            .tokens
            .iter()
            .find(|token| token.qml_name == "colorPanel")
            .map(|token| &token.value),
        Some(&ThemeTokenValue::Color("#ffffff".to_owned()))
    );

    let invalid = DARK.replace("#292a2d", "not-a-color");
    assert_eq!(
        ThemeService::new(
            GUI,
            vec![
                ("light".to_owned(), invalid),
                ("dark".to_owned(), DARK.to_owned())
            ],
            "light",
        )
        .err()
        .map(|error| error.code),
        Some("theme.invalid_color")
    );

    let wrong_color_type =
        DARK.replace("color-panel: string = \"#292a2d\"", "color-panel: int = 12");
    assert_eq!(
        ThemeService::new(
            GUI,
            vec![
                ("light".to_owned(), wrong_color_type),
                ("dark".to_owned(), DARK.to_owned())
            ],
            "light",
        )
        .err()
        .map(|error| error.code),
        Some("theme.invalid_token_type")
    );

    let wrong_layout_type = LIGHT.replace(
        "kind: theme\n",
        "kind: theme\n\nvalues:\n  control-height: string = \"large\"\n",
    );
    assert_eq!(
        ThemeService::new(
            GUI,
            vec![
                ("light".to_owned(), wrong_layout_type),
                ("dark".to_owned(), DARK.to_owned())
            ],
            "light",
        )
        .err()
        .map(|error| error.code),
        Some("theme.invalid_token_type")
    );

    let out_of_range_override = LIGHT.replace(
        "kind: theme\n",
        "kind: theme\n\nvalues:\n  control-height: int = 5000\n",
    );
    assert_eq!(
        ThemeService::new(
            GUI,
            vec![
                ("light".to_owned(), out_of_range_override),
                ("dark".to_owned(), DARK.to_owned())
            ],
            "light",
        )
        .err()
        .map(|error| error.code),
        Some("theme.token_out_of_range")
    );
    Ok(())
}

#[test]
fn baseline_requires_all_tokens_and_rejects_out_of_range_values() {
    let missing = GUI.replace("  control-height: int = 24\n", "");
    assert_eq!(
        ThemeService::new(
            &missing,
            vec![
                ("light".to_owned(), LIGHT.to_owned()),
                ("dark".to_owned(), DARK.to_owned())
            ],
            "light",
        )
        .err()
        .map(|error| error.code),
        Some("theme.missing_tokens")
    );

    let oversized = GUI.replace("control-height: int = 24", "control-height: int = 5000");
    assert_eq!(
        ThemeService::new(
            &oversized,
            vec![
                ("light".to_owned(), LIGHT.to_owned()),
                ("dark".to_owned(), DARK.to_owned())
            ],
            "light",
        )
        .err()
        .map(|error| error.code),
        Some("theme.token_out_of_range")
    );
}

#[test]
fn builtin_catalog_json_carries_the_light_panel_color() {
    let json = builtin_theme_catalog().unwrap_or_else(|error| panic!("{error}"));
    let value: serde_json::Value =
        serde_json::from_str(&json).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(value["active"], "light");
    assert_eq!(value["themes"]["light"]["colorPanel"]["kind"], "color");
    assert_eq!(value["themes"]["light"]["colorPanel"]["value"], "#ffffff");
    assert_eq!(value["themes"]["dark"]["fontBody"]["kind"], "int");
}
