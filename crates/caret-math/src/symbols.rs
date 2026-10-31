//! LaTeX 数学命令到 Unicode 字符与 TeX 原子类别的映射。
//!
//! 表格按 TeX/LaTeX 的命令语义和 Unicode 数学字符整理，供解析器判定命令合法性、
//! 序列化器反查命令名以及布局阶段取字形与间距类别。

/// TeX 原子类别，决定相邻原子之间的数学间距。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AtomClass {
    Ord,
    Op,
    Bin,
    Rel,
    Open,
    Close,
    Punct,
    Inner,
}

impl AtomClass {
    /// 文档属性 `atomClass` 使用的名称。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ord => "ord",
            Self::Op => "op",
            Self::Bin => "bin",
            Self::Rel => "rel",
            Self::Open => "open",
            Self::Close => "close",
            Self::Punct => "punct",
            Self::Inner => "inner",
        }
    }

    /// 解析文档属性 `atomClass`。
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "ord" => Self::Ord,
            "op" => Self::Op,
            "bin" => Self::Bin,
            "rel" => Self::Rel,
            "open" => Self::Open,
            "close" => Self::Close,
            "punct" => Self::Punct,
            "inner" => Self::Inner,
            _ => return None,
        })
    }
}

/// 一个具名数学符号。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamedSymbol {
    pub name: &'static str,
    pub character: char,
    pub class: AtomClass,
}

use AtomClass::{Bin, Close, Inner, Op, Open, Ord, Punct, Rel};

macro_rules! symbols {
    ($($name:literal => $ch:literal, $class:ident;)*) => {
        &[$(NamedSymbol { name: $name, character: $ch, class: $class }),*]
    };
}

/// 具名符号表。同一字符可有多个命令名，反查时取第一个。
pub const NAMED_SYMBOLS: &[NamedSymbol] = symbols! {
    // 小写希腊字母
    "alpha" => 'α', Ord; "beta" => 'β', Ord; "gamma" => 'γ', Ord; "delta" => 'δ', Ord;
    "epsilon" => 'ϵ', Ord; "varepsilon" => 'ε', Ord; "zeta" => 'ζ', Ord; "eta" => 'η', Ord;
    "theta" => 'θ', Ord; "vartheta" => 'ϑ', Ord; "iota" => 'ι', Ord; "kappa" => 'κ', Ord;
    "lambda" => 'λ', Ord; "mu" => 'μ', Ord; "nu" => 'ν', Ord; "xi" => 'ξ', Ord;
    "pi" => 'π', Ord; "varpi" => 'ϖ', Ord; "rho" => 'ρ', Ord; "varrho" => 'ϱ', Ord;
    "sigma" => 'σ', Ord; "varsigma" => 'ς', Ord; "tau" => 'τ', Ord; "upsilon" => 'υ', Ord;
    "phi" => 'ϕ', Ord; "varphi" => 'φ', Ord; "chi" => 'χ', Ord; "psi" => 'ψ', Ord;
    "omega" => 'ω', Ord;
    // 大写希腊字母
    "Gamma" => 'Γ', Ord; "Delta" => 'Δ', Ord; "Theta" => 'Θ', Ord; "Lambda" => 'Λ', Ord;
    "Xi" => 'Ξ', Ord; "Pi" => 'Π', Ord; "Sigma" => 'Σ', Ord; "Upsilon" => 'Υ', Ord;
    "Phi" => 'Φ', Ord; "Psi" => 'Ψ', Ord; "Omega" => 'Ω', Ord;
    // 普通符号
    "infty" => '∞', Ord; "partial" => '∂', Ord; "nabla" => '∇', Ord; "hbar" => 'ℏ', Ord;
    "ell" => 'ℓ', Ord; "wp" => '℘', Ord; "Re" => 'ℜ', Ord; "Im" => 'ℑ', Ord;
    "aleph" => 'ℵ', Ord; "emptyset" => '∅', Ord; "varnothing" => '∅', Ord;
    "forall" => '∀', Ord; "exists" => '∃', Ord; "nexists" => '∄', Ord; "neg" => '¬', Ord;
    "lnot" => '¬', Ord; "top" => '⊤', Ord; "bot" => '⊥', Ord; "angle" => '∠', Ord;
    "triangle" => '△', Ord; "prime" => '′', Ord; "backslash" => '\\', Ord;
    "surd" => '√', Ord; "flat" => '♭', Ord; "sharp" => '♯', Ord; "natural" => '♮', Ord;
    "clubsuit" => '♣', Ord; "diamondsuit" => '♢', Ord; "heartsuit" => '♡', Ord;
    "spadesuit" => '♠', Ord; "imath" => 'ı', Ord; "jmath" => 'ȷ', Ord; "mho" => '℧', Ord;
    "checkmark" => '✓', Ord; "Box" => '□', Ord; "vert" => '|', Ord; "Vert" => '‖', Ord;
    "ldots" => '…', Inner; "dots" => '…', Inner; "cdots" => '⋯', Inner;
    "vdots" => '⋮', Ord; "ddots" => '⋱', Inner;
    // 定界符形态的符号
    "lbrace" => '{', Open; "rbrace" => '}', Close; "langle" => '⟨', Open;
    "rangle" => '⟩', Close; "lfloor" => '⌊', Open; "rfloor" => '⌋', Close;
    "lceil" => '⌈', Open; "rceil" => '⌉', Close; "lvert" => '|', Open; "rvert" => '|', Close;
    "lVert" => '‖', Open; "rVert" => '‖', Close; "lgroup" => '⟮', Open; "rgroup" => '⟯', Close;
    "lmoustache" => '⎰', Open; "rmoustache" => '⎱', Close;
    // 二元运算符
    "pm" => '±', Bin; "mp" => '∓', Bin; "times" => '×', Bin; "div" => '÷', Bin;
    "cdot" => '⋅', Bin; "ast" => '∗', Bin; "star" => '⋆', Bin; "circ" => '∘', Bin;
    "bullet" => '∙', Bin; "oplus" => '⊕', Bin; "ominus" => '⊖', Bin; "otimes" => '⊗', Bin;
    "oslash" => '⊘', Bin; "odot" => '⊙', Bin; "cup" => '∪', Bin; "cap" => '∩', Bin;
    "sqcup" => '⊔', Bin; "sqcap" => '⊓', Bin; "vee" => '∨', Bin; "lor" => '∨', Bin;
    "wedge" => '∧', Bin; "land" => '∧', Bin; "setminus" => '∖', Bin; "wr" => '≀', Bin;
    "diamond" => '⋄', Bin; "bigtriangleup" => '△', Bin; "bigtriangledown" => '▽', Bin;
    "triangleleft" => '◁', Bin; "triangleright" => '▷', Bin; "uplus" => '⊎', Bin;
    "amalg" => '⨿', Bin; "dagger" => '†', Bin; "ddagger" => '‡', Bin;
    "ltimes" => '⋉', Bin; "rtimes" => '⋊', Bin;
    // 关系符
    "leq" => '≤', Rel; "le" => '≤', Rel; "geq" => '≥', Rel; "ge" => '≥', Rel;
    "neq" => '≠', Rel; "ne" => '≠', Rel; "equiv" => '≡', Rel; "approx" => '≈', Rel;
    "sim" => '∼', Rel; "simeq" => '≃', Rel; "cong" => '≅', Rel; "propto" => '∝', Rel;
    "ll" => '≪', Rel; "gg" => '≫', Rel; "subset" => '⊂', Rel; "supset" => '⊃', Rel;
    "subseteq" => '⊆', Rel; "supseteq" => '⊇', Rel; "in" => '∈', Rel; "ni" => '∋', Rel;
    "notin" => '∉', Rel; "perp" => '⊥', Rel; "parallel" => '∥', Rel; "mid" => '∣', Rel;
    "models" => '⊨', Rel; "vdash" => '⊢', Rel; "dashv" => '⊣', Rel; "prec" => '≺', Rel;
    "succ" => '≻', Rel; "preceq" => '⪯', Rel; "succeq" => '⪰', Rel; "asymp" => '≍', Rel;
    "doteq" => '≐', Rel; "to" => '→', Rel; "rightarrow" => '→', Rel; "leftarrow" => '←', Rel;
    "gets" => '←', Rel; "leftrightarrow" => '↔', Rel; "Rightarrow" => '⇒', Rel;
    "Leftarrow" => '⇐', Rel; "Leftrightarrow" => '⇔', Rel; "implies" => '⟹', Rel;
    "impliedby" => '⟸', Rel; "iff" => '⟺', Rel; "mapsto" => '↦', Rel;
    "longrightarrow" => '⟶', Rel; "longleftarrow" => '⟵', Rel;
    "longleftrightarrow" => '⟷', Rel; "Longrightarrow" => '⟹', Rel;
    "Longleftarrow" => '⟸', Rel; "uparrow" => '↑', Rel; "downarrow" => '↓', Rel;
    "updownarrow" => '↕', Rel; "Uparrow" => '⇑', Rel; "Downarrow" => '⇓', Rel;
    "Updownarrow" => '⇕', Rel; "nearrow" => '↗', Rel; "searrow" => '↘', Rel;
    "swarrow" => '↙', Rel; "nwarrow" => '↖', Rel; "hookrightarrow" => '↪', Rel;
    "hookleftarrow" => '↩', Rel; "leadsto" => '⇝', Rel; "rightleftharpoons" => '⇌', Rel;
    "sqsubseteq" => '⊑', Rel; "sqsupseteq" => '⊒', Rel; "smile" => '⌣', Rel;
    "frown" => '⌢', Rel; "bowtie" => '⋈', Rel; "leqslant" => '⩽', Rel;
    "geqslant" => '⩾', Rel; "nleq" => '≰', Rel; "ngeq" => '≱', Rel;
    "coloneqq" => '≔', Rel; "triangleq" => '≜', Rel;
    // 大型运算符（求和、乘积和积分另有专门节点）
    "coprod" => '∐', Op; "bigcup" => '⋃', Op; "bigcap" => '⋂', Op; "bigoplus" => '⨁', Op;
    "bigotimes" => '⨂', Op; "bigodot" => '⨀', Op; "bigvee" => '⋁', Op;
    "bigwedge" => '⋀', Op; "biguplus" => '⨄', Op; "bigsqcup" => '⨆', Op;
    // 标点
    "colon" => ':', Punct; "ldotp" => '.', Punct; "cdotp" => '·', Punct;
};

/// 按命令名查找具名符号。
pub fn named_symbol(name: &str) -> Option<&'static NamedSymbol> {
    NAMED_SYMBOLS.iter().find(|symbol| symbol.name == name)
}

/// 单个字面字符在数学模式里的 TeX 原子类别。
pub fn literal_class(character: char) -> AtomClass {
    match character {
        '+' | '-' | '*' | '−' => Bin,
        '=' | '<' | '>' | ':' => Rel,
        '(' | '[' => Open,
        ')' | ']' | '!' | '?' => Close,
        ',' | ';' => Punct,
        _ => NAMED_SYMBOLS
            .iter()
            .find(|symbol| symbol.character == character && symbol.class != Ord)
            .map_or(Ord, |symbol| symbol.class),
    }
}

/// 字面字符在数学字体中实际显示的字符；连字符显示为减号。
pub fn literal_display_char(character: char) -> char {
    match character {
        '-' => '−',
        '*' => '∗',
        '\'' => '′',
        other => other,
    }
}

/// 带上下限的文字运算符，`true` 表示显示模式下极限放在正上下方。
pub const TEXT_OPERATORS: &[(&str, bool)] = &[
    ("lim", true),
    ("liminf", true),
    ("limsup", true),
    ("max", true),
    ("min", true),
    ("sup", true),
    ("inf", true),
    ("det", true),
    ("gcd", true),
    ("Pr", true),
    ("sin", false),
    ("cos", false),
    ("tan", false),
    ("cot", false),
    ("sec", false),
    ("csc", false),
    ("arcsin", false),
    ("arccos", false),
    ("arctan", false),
    ("sinh", false),
    ("cosh", false),
    ("tanh", false),
    ("coth", false),
    ("log", false),
    ("ln", false),
    ("lg", false),
    ("exp", false),
    ("ker", false),
    ("dim", false),
    ("arg", false),
    ("deg", false),
    ("hom", false),
];

/// 查询文字运算符的上下限位置。
pub fn text_operator_limits(name: &str) -> Option<bool> {
    TEXT_OPERATORS
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, limits)| *limits)
}

/// 重音和装饰命令名，与文档属性 `accent` 的取值一一对应。
pub const ACCENTS: &[&str] = &[
    "hat",
    "check",
    "breve",
    "acute",
    "grave",
    "tilde",
    "bar",
    "vec",
    "dot",
    "ddot",
    "dddot",
    "ddddot",
    "widehat",
    "widetilde",
    "overline",
    "underline",
    "overbrace",
    "underbrace",
    "overleftarrow",
    "overrightarrow",
    "overleftrightarrow",
    "underleftarrow",
    "underrightarrow",
    "underleftrightarrow",
    "cancel",
    "bcancel",
    "xcancel",
    "boxed",
    "not",
];

/// 重音命令名到文档属性值；`\mathring` 存为 `ring`。
pub fn accent_attribute(command: &str) -> Option<&'static str> {
    if command == "mathring" {
        return Some("ring");
    }
    ACCENTS.iter().copied().find(|accent| *accent == command)
}

/// 重音属性值对应的组合字符或装饰字符。`None` 表示由规则线或特殊绘制实现。
pub fn accent_char(accent: &str) -> Option<char> {
    Some(match accent {
        "hat" | "widehat" => '\u{0302}',
        "check" => '\u{030C}',
        "breve" => '\u{0306}',
        "acute" => '\u{0301}',
        "grave" => '\u{0300}',
        "tilde" | "widetilde" => '\u{0303}',
        "bar" => '\u{0304}',
        "vec" => '\u{20D7}',
        "dot" => '\u{0307}',
        "ddot" => '\u{0308}',
        "dddot" => '\u{20DB}',
        "ddddot" => '\u{20DC}',
        "ring" => '\u{030A}',
        "overbrace" => '⏞',
        "underbrace" => '⏟',
        "overleftarrow" | "underleftarrow" => '←',
        "overrightarrow" | "underrightarrow" => '→',
        "overleftrightarrow" | "underleftrightarrow" => '↔',
        _ => return None,
    })
}

/// 文档定界符编码对应的显示字符；`empty` 返回 `None`。
pub fn delimiter_char(encoded: &str) -> Option<char> {
    if let Some(character) = encoded.strip_prefix("char:") {
        return character.chars().next();
    }
    let name = encoded.strip_prefix("named:")?;
    Some(match name {
        "{" => '{',
        "}" => '}',
        "|" => '‖',
        "backslash" => '\\',
        other => named_symbol(other)?.character,
    })
}

/// LaTeX 定界符记号转为文档编码。
pub fn encode_delimiter(token: &str) -> Option<String> {
    Some(match token {
        "." => "empty".to_owned(),
        "(" | ")" | "[" | "]" | "|" | "/" | "<" | ">" => format!("char:{token}"),
        "\\{" | "\\lbrace" => "named:{".to_owned(),
        "\\}" | "\\rbrace" => "named:}".to_owned(),
        "\\|" => "named:|".to_owned(),
        _ => {
            let name = token.strip_prefix('\\')?;
            match name {
                "langle" | "rangle" | "lfloor" | "rfloor" | "lceil" | "rceil" | "lvert"
                | "rvert" | "lVert" | "rVert" | "vert" | "Vert" | "uparrow" | "downarrow"
                | "Uparrow" | "Downarrow" | "updownarrow" | "Updownarrow" | "backslash"
                | "lgroup" | "rgroup" | "lmoustache" | "rmoustache" => format!("named:{name}"),
                _ => return None,
            }
        }
    })
}

/// 文档定界符编码转回 LaTeX 记号。
pub fn delimiter_source(encoded: &str) -> String {
    if encoded == "empty" {
        return ".".to_owned();
    }
    if let Some(character) = encoded.strip_prefix("char:") {
        return character.to_owned();
    }
    match encoded.strip_prefix("named:") {
        Some("{") => "\\{".to_owned(),
        Some("}") => "\\}".to_owned(),
        Some("|") => "\\|".to_owned(),
        Some(name) => format!("\\{name}"),
        None => ".".to_owned(),
    }
}

/// LaTeX 预定义颜色名到 CSS 十六进制颜色。
pub const NAMED_COLORS: &[(&str, &str)] = &[
    ("black", "#000000"),
    ("white", "#ffffff"),
    ("red", "#ff0000"),
    ("green", "#00ff00"),
    ("blue", "#0000ff"),
    ("cyan", "#00ffff"),
    ("magenta", "#ff00ff"),
    ("yellow", "#ffff00"),
    ("gray", "#808080"),
    ("darkgray", "#404040"),
    ("lightgray", "#bfbfbf"),
    ("orange", "#ff8000"),
    ("purple", "#bf0040"),
    ("brown", "#bf8040"),
    ("olive", "#808000"),
    ("teal", "#008080"),
    ("violet", "#800080"),
    ("pink", "#ffbfbf"),
    ("lime", "#bfff00"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_names_are_unique() {
        for (index, symbol) in NAMED_SYMBOLS.iter().enumerate() {
            assert!(
                NAMED_SYMBOLS[..index]
                    .iter()
                    .all(|other| other.name != symbol.name),
                "{} 重复",
                symbol.name
            );
        }
    }

    #[test]
    fn delimiter_encoding_round_trips() {
        for token in ["(", "]", ".", "\\{", "\\|", "\\langle", "\\rVert"] {
            let encoded =
                encode_delimiter(token).unwrap_or_else(|| panic!("{token} 应为合法定界符"));
            assert_eq!(delimiter_source(&encoded), token);
        }
    }
}
