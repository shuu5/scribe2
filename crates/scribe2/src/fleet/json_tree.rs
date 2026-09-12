//! 入れ子の JSON を std だけで読む面（設計 docs/design/fleet-usage.md §3 / §8(a)）。
//!
//! 口座残量の応答は入れ子の object・配列・小数を持つので、1 段の flat object だけを受ける
//! [`super::json_lite`] では読めない。そこで**別の型** [`Tree`] を足す——flat 行の reader は
//! 触らない（受理を広げると event log の行の形が黙って緩み、綴り違いの key を拒む性質が消える）。
//!
//! 極性は fail-closed（[`POLARITY`]）: 読めない字面は**部分 parse を返さず** [`TreeError`] にする。
//! 行為（編集・起動・merge・書込）を止めうる判定ではないので、極性一覧（[`crate::polarity`]）の
//! guard には載せない（設計 §6）。依存は 0 本（NFR3）。

use crate::polarity::{OnFailure, Polarity, Timing};

/// 入れ子の深さの上限。超える入力は [`TreeError::TooDeep`]（再帰はここで止まる）。
pub const MAX_DEPTH: usize = 64;

/// `u64` が持てる 10 進の桁数（`u64::MAX` は 20 桁）。[`Tree::as_pct`] はこれを超える桁数の
/// 値を**桁を組む前に**捨てる（入力の指数に比例する仕事をしない）。
const MAX_PCT_DIGITS: i128 = 20;

/// この境界の極性（[`TreeError`]）: 読む時点で形を確かめ、読めない字面は Err にする。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 入れ子の JSON の値（RFC 8259 の 6 形）。
///
/// 数は**10 進の字面のまま**持つ（f64 に落とすと 30 桁の入力が黙って丸まり、`utilization` の
/// 切り捨てが入力と食い違う）。字面は [`parse`] が JSON の number として確かめた形である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tree {
    /// key と値の並び（重複 key は [`parse`] が拒むので、ここに同じ key は 2 度現れない）。
    Object(Vec<(String, Tree)>),
    /// 値の並び。
    Array(Vec<Tree>),
    /// 文字列（escape は解いた後）。
    Str(String),
    /// 数の 10 進の字面。
    Num(String),
    /// 真偽。
    Bool(bool),
    /// 値なし。
    Null,
}

impl Tree {
    /// object の key を引く。object でなければ `None`。
    pub fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(pairs) => pairs
                .iter()
                .find(|(found, _)| found == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// 文字列なら中身を借りる。
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(text) => Some(text),
            _ => None,
        }
    }

    /// 配列なら要素を借りる。
    pub fn as_array(&self) -> Option<&[Self]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    /// 真偽なら値を返す。
    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            Self::Bool(found) => Some(found),
            _ => None,
        }
    }

    /// 数を**整数 %（切り捨て・100 で cap しない）**へ落とす。負数と数でない値は `None`。
    ///
    /// ×100 した値が `u64` に収まらない入力も `None` である。判定は**桁数**（小数点の位置）で
    /// 行い、[`MAX_PCT_DIGITS`] を超える形は桁埋め・文字列の確保・`format!` の**前に** `None` を
    /// 返す（`1e9999999999` のような入力で指数由来の幅を確保すると abort する・`s2-07l.136` run 1）。
    pub fn as_pct(&self) -> Option<u64> {
        match self {
            Self::Num(text) => Decimal::split(text)?.hundred_floor(),
            _ => None,
        }
    }
}

/// 読めない字面の理由（閉じた enum・位置は byte offset）。極性は fail-closed（[`POLARITY`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeError {
    /// 値の形でない文字が在る（`found` が `None` なら入力が尽きた）。
    Unexpected {
        /// 何 byte 目か。
        at: usize,
        /// 見つけた文字。
        found: Option<char>,
    },
    /// 文字列が閉じないまま入力が尽きた。
    Unterminated {
        /// 何 byte 目か。
        at: usize,
    },
    /// `\` の後ろが escape の形でない（`\uXXXX` の桁・壊れた surrogate 対を含む）。
    BadEscape {
        /// 何 byte 目か。
        at: usize,
    },
    /// 数の形でない（指数が `i64` に収まらない形を含む）。
    BadNumber {
        /// 何 byte 目か。
        at: usize,
    },
    /// object の key が重複する（先勝ちで後の値を黙って捨てない）。
    DuplicateKey {
        /// 何 byte 目か。
        at: usize,
        /// 重複した key。
        key: String,
    },
    /// 入れ子が [`MAX_DEPTH`] を超えた。
    TooDeep {
        /// 何 byte 目か。
        at: usize,
    },
    /// 値の後ろに余分な文字が在る。
    Trailing {
        /// 何 byte 目か。
        at: usize,
        /// 見つけた文字。
        found: char,
    },
}

impl std::fmt::Display for TreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unexpected { at, found: Some(found) } => {
                write!(f, "json: 値の形でない（位置 {at}・{found:?}）")
            }
            Self::Unexpected { at, found: None } => write!(f, "json: 入力が尽きた（位置 {at}）"),
            Self::Unterminated { at } => write!(f, "json: 文字列が閉じていない（位置 {at}）"),
            Self::BadEscape { at } => write!(f, "json: escape の形でない（位置 {at}）"),
            Self::BadNumber { at } => write!(f, "json: 数の形でない（位置 {at}）"),
            Self::DuplicateKey { at, key } => {
                write!(f, "json: key {key} が重複する（位置 {at}）")
            }
            Self::TooDeep { at } => {
                write!(f, "json: 入れ子が上限 {MAX_DEPTH} を超える（位置 {at}）")
            }
            Self::Trailing { at, found } => {
                write!(f, "json: 値の後ろに余分がある（位置 {at}・{found:?}）")
            }
        }
    }
}

/// 1 つの値を読む。末尾に余分が在れば [`TreeError::Trailing`]。
pub fn parse(text: &str) -> Result<Tree, TreeError> {
    let mut parser = Parser {
        text,
        at: 0,
        depth: 0,
    };
    let tree = parser.value()?;
    parser.skip_ws();
    match parser.rest().chars().next() {
        None => Ok(tree),
        Some(found) => Err(TreeError::Trailing {
            at: parser.at,
            found,
        }),
    }
}

/// 読み進める位置と深さ。
struct Parser<'a> {
    /// 入力の全体。
    text: &'a str,
    /// いまの byte 位置。
    at: usize,
    /// いまの入れ子の深さ。
    depth: usize,
}

impl<'a> Parser<'a> {
    /// まだ読んでいない部分。
    fn rest(&self) -> &'a str {
        self.text.get(self.at..).unwrap_or_default()
    }

    /// いまの位置の byte。
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }

    /// `len` byte 進む。
    fn bump(&mut self, len: usize) {
        self.at = self.at.saturating_add(len);
    }

    /// 1 文字読んで進む。
    fn next_char(&mut self) -> Option<char> {
        let found = self.rest().chars().next()?;
        self.bump(found.len_utf8());
        Some(found)
    }

    /// 空白（JSON の 4 種）を読み飛ばす。
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.bump(1);
        }
    }

    /// いまの位置を「値の形でない」として断る。
    fn unexpected(&self) -> TreeError {
        TreeError::Unexpected {
            at: self.at,
            found: self.rest().chars().next(),
        }
    }

    /// 1 byte を確かめて進む。
    fn take(&mut self, want: u8) -> Result<(), TreeError> {
        if self.peek() == Some(want) {
            self.bump(1);
            Ok(())
        } else {
            Err(self.unexpected())
        }
    }

    /// 入れ子へ 1 段降りる（上限を超えたら断る）。
    fn enter(&mut self) -> Result<(), TreeError> {
        self.depth = self.depth.saturating_add(1);
        if self.depth > MAX_DEPTH {
            return Err(TreeError::TooDeep { at: self.at });
        }
        Ok(())
    }

    /// 入れ子から 1 段上がる。
    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// 値 1 つを読む。
    fn value(&mut self) -> Result<Tree, TreeError> {
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Tree::Str),
            Some(b't') => self.keyword("true", Tree::Bool(true)),
            Some(b'f') => self.keyword("false", Tree::Bool(false)),
            Some(b'n') => self.keyword("null", Tree::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.unexpected()),
        }
    }

    /// `true` / `false` / `null` を読む。
    fn keyword(&mut self, word: &str, tree: Tree) -> Result<Tree, TreeError> {
        if self.rest().starts_with(word) {
            self.bump(word.len());
            Ok(tree)
        } else {
            Err(self.unexpected())
        }
    }

    /// object を読む。
    fn object(&mut self) -> Result<Tree, TreeError> {
        self.enter()?;
        self.bump(1);
        let mut pairs: Vec<(String, Tree)> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.bump(1);
            self.leave();
            return Ok(Tree::Object(pairs));
        }
        loop {
            self.member(&mut pairs)?;
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.bump(1),
                Some(b'}') => {
                    self.bump(1);
                    break;
                }
                _ => return Err(self.unexpected()),
            }
        }
        self.leave();
        Ok(Tree::Object(pairs))
    }

    /// object の `"key": 値` を 1 組読んで足す。重複 key は断る。
    fn member(&mut self, pairs: &mut Vec<(String, Tree)>) -> Result<(), TreeError> {
        self.skip_ws();
        let at = self.at;
        let key = self.string()?;
        if pairs.iter().any(|(found, _)| *found == key) {
            return Err(TreeError::DuplicateKey { at, key });
        }
        self.skip_ws();
        self.take(b':')?;
        let value = self.value()?;
        pairs.push((key, value));
        Ok(())
    }

    /// 配列を読む。
    fn array(&mut self) -> Result<Tree, TreeError> {
        self.enter()?;
        self.bump(1);
        let mut items: Vec<Tree> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.bump(1);
            self.leave();
            return Ok(Tree::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.bump(1),
                Some(b']') => {
                    self.bump(1);
                    break;
                }
                _ => return Err(self.unexpected()),
            }
        }
        self.leave();
        Ok(Tree::Array(items))
    }

    /// 文字列を読む（escape は解く）。
    fn string(&mut self) -> Result<String, TreeError> {
        self.take(b'"')?;
        let mut out = String::new();
        loop {
            let at = self.at;
            let Some(found) = self.next_char() else {
                return Err(TreeError::Unterminated { at });
            };
            match found {
                '"' => return Ok(out),
                '\\' => out.push(self.escape()?),
                other if (other as u32) < 0x20 => {
                    return Err(TreeError::Unexpected {
                        at,
                        found: Some(other),
                    })
                }
                other => out.push(other),
            }
        }
    }

    /// `\` の次を解く。
    fn escape(&mut self) -> Result<char, TreeError> {
        let at = self.at;
        let Some(found) = self.next_char() else {
            return Err(TreeError::Unterminated { at });
        };
        match found {
            '"' => Ok('"'),
            '\\' => Ok('\\'),
            '/' => Ok('/'),
            'b' => Ok('\u{0008}'),
            'f' => Ok('\u{000c}'),
            'n' => Ok('\n'),
            'r' => Ok('\r'),
            't' => Ok('\t'),
            'u' => self.unicode(at),
            _ => Err(TreeError::BadEscape { at }),
        }
    }

    /// `\uXXXX`（上位 surrogate なら下位 surrogate の対まで）を解く。
    fn unicode(&mut self, at: usize) -> Result<char, TreeError> {
        let first = self.hex4(at)?;
        match first {
            0xd800..=0xdbff => {
                if !self.rest().starts_with("\\u") {
                    return Err(TreeError::BadEscape { at });
                }
                self.bump(2);
                let low = self.hex4(at)?;
                if !(0xdc00..=0xdfff).contains(&low) {
                    return Err(TreeError::BadEscape { at });
                }
                let code = 0x10000_u32
                    .saturating_add(first.saturating_sub(0xd800) << 10)
                    .saturating_add(low.saturating_sub(0xdc00));
                char::from_u32(code).ok_or(TreeError::BadEscape { at })
            }
            // 対にならない下位 surrogate は文字にならない。
            0xdc00..=0xdfff => Err(TreeError::BadEscape { at }),
            _ => char::from_u32(first).ok_or(TreeError::BadEscape { at }),
        }
    }

    /// 16 進 4 桁を読む。
    ///
    /// `from_str_radix` は先頭の `+` を受理するので、桁を字で確かめてから渡す
    /// （[`super::json_lite`] の `\u` と同じ形）。
    fn hex4(&mut self, at: usize) -> Result<u32, TreeError> {
        let digits = self.rest().get(..4).ok_or(TreeError::BadEscape { at })?;
        if !digits.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Err(TreeError::BadEscape { at });
        }
        self.bump(4);
        u32::from_str_radix(digits, 16).map_err(|_| TreeError::BadEscape { at })
    }

    /// 数を読む。字面は verbatim のまま持ち、形は [`Decimal::split`] が確かめる。
    fn number(&mut self) -> Result<Tree, TreeError> {
        let at = self.at;
        let rest = self.rest();
        let end = rest
            .find(|ch: char| !matches!(ch, '0'..='9' | '-' | '+' | '.' | 'e' | 'E'))
            .unwrap_or(rest.len());
        let literal = rest.get(..end).ok_or(TreeError::BadNumber { at })?;
        Decimal::split(literal).ok_or(TreeError::BadNumber { at })?;
        self.bump(end);
        Ok(Tree::Num(literal.to_owned()))
    }
}

/// 10 進の字面の内訳（符号・整数部・小数部・指数）。
///
/// 値は `(-1)^negative × 0.<int><frac> × 10^(int.len() + exp)` である。指数は `i64` に収める
/// （収まらない字面は数の形でない＝[`TreeError::BadNumber`]）。
struct Decimal<'a> {
    /// 先頭の `-` が在るか。
    negative: bool,
    /// 整数部の桁（`0` か `[1-9][0-9]*`）。
    int: &'a str,
    /// 小数部の桁（`.` が無ければ空）。
    frac: &'a str,
    /// 指数。
    exp: i64,
}

impl<'a> Decimal<'a> {
    /// JSON の number の文法ちょうどに割る。形が違えば `None`。
    fn split(text: &'a str) -> Option<Self> {
        let (negative, rest) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (int, rest) = take_int(rest)?;
        let (frac, rest) = take_frac(rest)?;
        let exp = take_exp(rest)?;
        Some(Self {
            negative,
            int,
            frac,
            exp,
        })
    }

    /// ×100 して切り捨てた値。負数と `u64` に収まらない値は `None`。
    ///
    /// 桁を組むのは**上限の内側だと分かった後だけ**である（指数に比例する仕事をしない）。
    fn hundred_floor(&self) -> Option<u64> {
        let digits = self.int.len().saturating_add(self.frac.len());
        let lead = self.leading_zeros();
        if lead == digits {
            return Some(0); // 全桁が 0 ＝符号に依らず値は 0。
        }
        if self.negative {
            return None;
        }
        let int_len = i128::try_from(self.int.len()).ok()?;
        let skipped = i128::try_from(lead).ok()?;
        // ×100 の後の小数点の位置を、有効桁の先頭から数えた整数部の桁数として求める。
        let point = i128::from(self.exp)
            .saturating_add(int_len)
            .saturating_add(2)
            .saturating_sub(skipped);
        if point <= 0 {
            return Some(0); // 1 未満＝切り捨てて 0。
        }
        if point > MAX_PCT_DIGITS {
            return None;
        }
        self.compose(point, lead)
    }

    /// 有効桁の先頭から `width` 桁を組む（足りない桁は 0 埋め）。`width` は 20 以下である。
    fn compose(&self, width: i128, lead: usize) -> Option<u64> {
        let width = usize::try_from(width).ok()?;
        let mut value: u128 = 0;
        for offset in 0..width {
            let index = lead.checked_add(offset)?;
            value = value
                .checked_mul(10)?
                .checked_add(u128::from(self.digit_at(index)))?;
        }
        u64::try_from(value).ok()
    }

    /// 整数部と小数部を繋いだ並びの `index` 桁目（末尾を超えたら 0 ＝桁埋め）。
    fn digit_at(&self, index: usize) -> u8 {
        let int = self.int.as_bytes();
        match int.get(index) {
            Some(byte) => byte.saturating_sub(b'0'),
            None => self
                .frac
                .as_bytes()
                .get(index.saturating_sub(int.len()))
                .map_or(0, |byte| byte.saturating_sub(b'0')),
        }
    }

    /// 整数部 + 小数部の先頭に並ぶ `0` の数。
    fn leading_zeros(&self) -> usize {
        let int_zeros = self
            .int
            .len()
            .saturating_sub(self.int.trim_start_matches('0').len());
        if int_zeros < self.int.len() {
            return int_zeros;
        }
        let frac_zeros = self
            .frac
            .len()
            .saturating_sub(self.frac.trim_start_matches('0').len());
        self.int.len().saturating_add(frac_zeros)
    }
}

/// 整数部（`0` か `[1-9][0-9]*`）と残り。
fn take_int(text: &str) -> Option<(&str, &str)> {
    let end = text.find(|ch: char| !ch.is_ascii_digit()).unwrap_or(text.len());
    let int = text.get(..end)?;
    if int.is_empty() || (int.len() > 1 && int.starts_with('0')) {
        return None;
    }
    Some((int, text.get(end..)?))
}

/// 小数部（`.` の後は 1 桁以上）と残り。`.` が無ければ空。
fn take_frac(text: &str) -> Option<(&str, &str)> {
    let Some(after) = text.strip_prefix('.') else {
        return Some(("", text));
    };
    let end = after
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(after.len());
    let frac = after.get(..end)?;
    if frac.is_empty() {
        return None;
    }
    Some((frac, after.get(end..)?))
}

/// 指数（`e` / `E` の後は符号任意 + 1 桁以上）。無ければ 0。`i64` に収まらなければ `None`。
fn take_exp(text: &str) -> Option<i64> {
    if text.is_empty() {
        return Some(0);
    }
    let after = text
        .strip_prefix('e')
        .or_else(|| text.strip_prefix('E'))?;
    let (negative, digits) = match after.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, after.strip_prefix('+').unwrap_or(after)),
    };
    if digits.is_empty() || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let value = digits.parse::<i128>().ok()?;
    i64::try_from(if negative { -value } else { value }).ok()
}
