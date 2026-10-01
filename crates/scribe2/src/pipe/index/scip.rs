//! SCIP の wire（protobuf）を std だけで読む面（設計 docs/design/reverse-index.md §4 形 3）。
//!
//! 器は外の道具の library を持たない（NFR3）ので、必要な欄（document の相対 path と position_encoding・
//! occurrence の範囲と symbol と定義の印と囲む範囲・symbol の情報の名と種類）だけを読み、知らない欄は wire の型で
//! 飛ばす。外の crate の symbol（repo のどの document にも定義が無い）と関数の中の local は返さない。壊れた wire
//! （途中で切れた varint・長さが本体を越える欄）は [`ScipError`] にする（部分の結果を返さない）。

use std::collections::BTreeSet;

/// 関数の中の local の symbol の接頭辞（SCIP の仕様）。
const LOCAL_PREFIX: &str = "local ";
/// `symbol_roles` の定義の bit。
const DEFINITION_ROLE: u64 = 1;
/// varint の最大の長さ（64 bit を 7 bit ずつ）。
const MAX_VARINT: usize = 10;

/// 読めない wire の理由（閉じた enum・位置は入力の先頭からの byte）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScipError {
    /// varint か固定幅の欄が途中で切れた。
    Truncated {
        /// 何 byte 目か。
        at: usize,
    },
    /// varint が 10 byte を越えて続く。
    Overlong {
        /// 何 byte 目か。
        at: usize,
    },
    /// 長さを持つ欄の長さが本体の残りを越える。
    Overrun {
        /// 何 byte 目か。
        at: usize,
        /// 欄が名乗る長さ。
        len: u64,
    },
    /// 読み飛ばせない wire の型（group）。
    WireType {
        /// 何 byte 目か。
        at: usize,
        /// 見つけた型の番号。
        wire: u8,
    },
    /// 文字列の欄が UTF-8 でない。
    Text {
        /// 何 byte 目か。
        at: usize,
    },
    /// 範囲が 3 つか 4 つの非負整数でない。
    Range {
        /// 何 byte 目か。
        at: usize,
    },
}

impl std::fmt::Display for ScipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Truncated { at } => write!(f, "scip: 欄が途中で切れた（位置 {at}）"),
            Self::Overlong { at } => write!(f, "scip: varint が長すぎる（位置 {at}）"),
            Self::Overrun { at, len } => write!(f, "scip: 欄の長さ {len} が本体を越える（位置 {at}）"),
            Self::WireType { at, wire } => write!(f, "scip: 読み飛ばせない wire の型 {wire}（位置 {at}）"),
            Self::Text { at } => write!(f, "scip: 文字列が UTF-8 でない（位置 {at}）"),
            Self::Range { at } => write!(f, "scip: 範囲の形でない（位置 {at}）"),
        }
    }
}

/// 範囲（行・列とも 0 始まり・列は document の position_encoding の単位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// 始まりの行。
    pub start_line: u32,
    /// 始まりの列。
    pub start_col: u32,
    /// 終わりの行。
    pub end_line: u32,
    /// 終わりの列（含まない）。
    pub end_col: u32,
}

impl Span {
    /// 3 つ（同じ行）か 4 つの整数から組む。
    fn from_ints(ints: &[u32]) -> Option<Self> {
        match *ints {
            [line, start_col, end_col] => Some(Self { start_line: line, start_col, end_line: line, end_col }),
            [start_line, start_col, end_line, end_col] => Some(Self { start_line, start_col, end_line, end_col }),
            _ => None,
        }
    }
}

/// occurrence 1 つ（symbol と範囲と定義の印と囲む範囲）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    /// 範囲。
    pub span: Span,
    /// symbol の字。
    pub symbol: String,
    /// 定義の occurrence か。
    pub definition: bool,
    /// 定義が囲む範囲（無ければ `None`）。
    pub enclosing: Option<Span>,
}

/// symbol の情報（名と種類）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolInfo {
    /// symbol の字。
    pub symbol: String,
    /// 表示の名。
    pub name: String,
    /// 種類の番号（SCIP の `SymbolInformation.Kind`）。
    pub kind: u32,
}

/// document 1 つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// repo の root からの相対 path。
    pub path: String,
    /// 列の単位（SCIP の `PositionEncoding`・0 は未指定で UTF-8 と読む）。
    pub encoding: u32,
    /// occurrence（外の crate と local を落とした後）。
    pub occurrences: Vec<Occurrence>,
    /// symbol の情報。
    pub infos: Vec<SymbolInfo>,
}

/// 欄 1 つの値。
enum Value<'a> {
    /// varint。
    Varint(u64),
    /// 長さを持つ欄の本体。
    Bytes(&'a [u8]),
    /// 固定幅の欄（読み飛ばした）。
    Fixed,
}

/// 入力を前から読む位置。
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
    /// 入力全体の中での `bytes` の先頭の位置（誤りの位置を絶対にする）。
    base: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8], base: usize) -> Self {
        Self { bytes, at: 0, base }
    }

    fn here(&self) -> usize {
        self.base.saturating_add(self.at)
    }

    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let found = self.bytes.get(self.at..self.at.checked_add(len)?)?;
        self.at = self.at.saturating_add(len);
        Some(found)
    }

    fn varint(&mut self) -> Result<u64, ScipError> {
        let mut value: u64 = 0;
        for shift in 0..MAX_VARINT {
            let byte = *self.bytes.get(self.at).ok_or(ScipError::Truncated { at: self.here() })?;
            self.at = self.at.saturating_add(1);
            value |= u64::from(byte & 0x7f) << (shift * 7);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(ScipError::Overlong { at: self.here() })
    }

    fn fixed(&mut self, len: usize) -> Result<Value<'a>, ScipError> {
        let at = self.here();
        self.take(len).map(|_| Value::Fixed).ok_or(ScipError::Truncated { at })
    }

    /// 次の欄（番号と値）。入力が尽きたら `None`。
    fn field(&mut self) -> Result<Option<(u64, Value<'a>)>, ScipError> {
        if self.at >= self.bytes.len() {
            return Ok(None);
        }
        let tag = self.varint()?;
        let value = match tag & 7 {
            0 => Value::Varint(self.varint()?),
            1 => self.fixed(8)?,
            5 => self.fixed(4)?,
            2 => {
                let len = self.varint()?;
                let at = self.here();
                let body = usize::try_from(len).ok().and_then(|size| self.take(size));
                Value::Bytes(body.ok_or(ScipError::Overrun { at, len })?)
            }
            wire => return Err(ScipError::WireType { at: self.here(), wire: u8::try_from(wire).unwrap_or(0) }),
        };
        Ok(Some((tag >> 3, value)))
    }
}

/// 文字列の欄。
fn text_of(bytes: &[u8], at: usize) -> Result<String, ScipError> {
    std::str::from_utf8(bytes).map(str::to_owned).map_err(|_| ScipError::Text { at })
}

/// 範囲の欄（packed の本体か 1 要素ずつの varint）を `ints` へ足す。
fn push_ints(ints: &mut Vec<u32>, value: &Value<'_>, at: usize) -> Result<(), ScipError> {
    match value {
        Value::Varint(one) => ints.push(u32::try_from(*one).map_err(|_| ScipError::Range { at })?),
        Value::Bytes(body) => {
            let mut cursor = Cursor::new(body, at);
            while cursor.at < body.len() {
                ints.push(u32::try_from(cursor.varint()?).map_err(|_| ScipError::Range { at })?);
            }
        }
        Value::Fixed => return Err(ScipError::Range { at }),
    }
    Ok(())
}

/// occurrence 1 つ。
fn read_occurrence(body: &[u8], base: usize) -> Result<Occurrence, ScipError> {
    let (mut range, mut enclosing) = (Vec::new(), Vec::new());
    let (mut symbol, mut roles) = (String::new(), 0);
    let mut cursor = Cursor::new(body, base);
    while let Some((number, value)) = cursor.field()? {
        let at = cursor.here();
        match (number, &value) {
            (1, _) => push_ints(&mut range, &value, at)?,
            (7, _) => push_ints(&mut enclosing, &value, at)?,
            (2, Value::Bytes(text)) => symbol = text_of(text, at)?,
            (3, Value::Varint(bits)) => roles = *bits,
            _ => {}
        }
    }
    let span = Span::from_ints(&range).ok_or(ScipError::Range { at: base })?;
    Ok(Occurrence { span, symbol, definition: roles & DEFINITION_ROLE != 0, enclosing: Span::from_ints(&enclosing) })
}

/// symbol の情報 1 つ。
fn read_info(body: &[u8], base: usize) -> Result<SymbolInfo, ScipError> {
    let mut info = SymbolInfo { symbol: String::new(), name: String::new(), kind: 0 };
    let mut cursor = Cursor::new(body, base);
    while let Some((number, value)) = cursor.field()? {
        let at = cursor.here();
        match (number, value) {
            (1, Value::Bytes(text)) => info.symbol = text_of(text, at)?,
            (6, Value::Bytes(text)) => info.name = text_of(text, at)?,
            (5, Value::Varint(kind)) => info.kind = u32::try_from(kind).unwrap_or(0),
            _ => {}
        }
    }
    Ok(info)
}

/// document 1 つ。
fn read_document(body: &[u8], base: usize) -> Result<Document, ScipError> {
    let mut doc = Document { path: String::new(), encoding: 0, occurrences: Vec::new(), infos: Vec::new() };
    let mut cursor = Cursor::new(body, base);
    while let Some((number, value)) = cursor.field()? {
        let at = cursor.here();
        match (number, value) {
            (1, Value::Bytes(text)) => doc.path = text_of(text, at)?,
            (2, Value::Bytes(item)) => doc.occurrences.push(read_occurrence(item, at.saturating_sub(item.len()))?),
            (3, Value::Bytes(item)) => doc.infos.push(read_info(item, at.saturating_sub(item.len()))?),
            (6, Value::Varint(encoding)) => doc.encoding = u32::try_from(encoding).unwrap_or(0),
            _ => {}
        }
    }
    Ok(doc)
}

/// repo の外の symbol（どの document にも定義が無い）と local を落とす。
fn keep_in_repo(docs: &mut [Document]) {
    let mut defined: BTreeSet<String> = BTreeSet::new();
    for doc in docs.iter() {
        defined.extend(doc.infos.iter().map(|info| info.symbol.clone()));
        defined.extend(doc.occurrences.iter().filter(|occ| occ.definition).map(|occ| occ.symbol.clone()));
    }
    let inside = |symbol: &str| !symbol.starts_with(LOCAL_PREFIX) && defined.contains(symbol);
    for doc in docs.iter_mut() {
        doc.occurrences.retain(|occ| inside(&occ.symbol));
        doc.infos.retain(|info| inside(&info.symbol));
    }
}

/// SCIP の index の bytes を読む。返すのは document の列（外の crate の symbol と local を落とした後）。
pub fn read_scip(bytes: &[u8]) -> Result<Vec<Document>, ScipError> {
    let mut docs = Vec::new();
    let mut cursor = Cursor::new(bytes, 0);
    while let Some((number, value)) = cursor.field()? {
        if let (2, Value::Bytes(body)) = (number, value) {
            docs.push(read_document(body, cursor.here().saturating_sub(body.len()))?);
        }
    }
    keep_in_repo(&mut docs);
    Ok(docs)
}

#[cfg(test)]
mod tests {
    use super::{read_scip, ScipError};

    /// varint の書き。
    fn varint(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let low = u8::try_from(value & 0x7f).unwrap_or(0);
            value >>= 7;
            if value == 0 {
                out.push(low);
                return out;
            }
            out.push(low | 0x80);
        }
    }

    /// 長さを持つ欄の書き。
    fn bytes_field(number: u64, body: &[u8]) -> Vec<u8> {
        let mut out = varint((number << 3) | 2);
        out.extend(varint(body.len() as u64));
        out.extend_from_slice(body);
        out
    }

    /// 定義の occurrence を 1 つ持つ document の bytes。
    fn one_document(extra: &[u8]) -> Vec<u8> {
        let mut occurrence = bytes_field(1, &[0, 0, 3]);
        occurrence.extend(bytes_field(2, b"rust-analyzer cargo k 0.1.0 a/B#"));
        occurrence.extend([0x18, 1]);
        let mut doc = bytes_field(1, b"a.rs");
        doc.extend(bytes_field(2, &occurrence));
        doc.extend_from_slice(extra);
        bytes_field(2, &doc)
    }

    /// 型ごとの読み飛ばし: varint・fixed64・fixed32・長さつきの知らない欄を document の外と中に置いても、読める欄は
    /// 同じに読める。
    #[test]
    fn pipe_index_read_skips_unknown_fields_by_wire_type() {
        let mut unknown = vec![0x38, 0x96, 0x01];
        unknown.extend([0x41, 1, 2, 3, 4, 5, 6, 7, 8]);
        unknown.extend([0x4d, 1, 2, 3, 4]);
        unknown.extend(bytes_field(15, b"zz"));
        let plain = read_scip(&one_document(&[])).unwrap_or_default();
        let mut with = one_document(&unknown);
        with.extend(unknown.clone());
        assert_eq!(read_scip(&with).unwrap_or_default(), plain);
        assert_eq!(plain.first().map(|doc| doc.occurrences.len()), Some(1));
    }

    /// 壊れた wire は 3 形とも読めない理由になる（部分の結果を返さない）。
    #[test]
    fn pipe_index_read_names_each_broken_wire() {
        let cut = [0x12, 0x80];
        assert!(matches!(read_scip(&cut), Err(ScipError::Truncated { .. })));
        let overrun = [0x12, 0x09, 1, 2];
        assert!(matches!(read_scip(&overrun), Err(ScipError::Overrun { len: 9, .. })));
        let group = [0x0b];
        assert!(matches!(read_scip(&group), Err(ScipError::WireType { wire: 3, .. })));
        let long = [0x80; 11];
        assert!(matches!(read_scip(&long), Err(ScipError::Overlong { .. })));
    }
}
