//! memo の引き金の行の読み手（設計 docs/design/ledger-form.md §15・契約表の行 k・FR81 (c)・FR87・ADR-0085）。
//!
//! 純関数 1 本（[`read`]）だけを置く: 入力は description の字・notes の字・台帳の接頭辞（解けない周は `None`）で、I/O も
//! 時計も持たない。description の `### 昇格条件` の節の中の `引き金:` の行を、行ごとに読める引き金（[`Trigger`]・形と値）か
//! 読めない引き金（[`Line::Unreadable`]・行の字と理由）に読み、notes の `[再発]` の行の本数と `[keep]` の記帳の行を返す。
//! 満ちる条件の判定は持たない（dispatch の周の後の行が持つ）。起票の門・dispatch の周・`pipe dispatch ls`・局面の関数が
//! 同じ読み手を引き、自前の読みを持たない（C2）。期日は [`epoch_of`] に秒 `:00` を足した字で UNIX 秒へ、着地は
//! [`parse_pointer`] で読み、依存の形は [`is_bead_id`] 1 本が判じる。

use crate::fleet::epoch_of;
use crate::ledger::form::{is_bead_id, MEMO_SECTIONS};
use crate::pipe::table::{parse_pointer, Pointer};

/// 引き金の行の頭（ASCII の colon・全角の `引き金：` は引き金の行でない）。
pub const HEAD: &str = "引き金:";

/// 引き金の行の頭の前に置いてよい箇条の印（`* ` は引き金の行でない）。
const BULLET: &str = "- ";

/// notes の再発の行の頭。
pub const RECURRENCE_MARK: &str = "[再発]";

/// notes の keep の記帳の頭（ADR-0085）。
pub const KEEP_MARK: &str = "[keep]";

/// 同梱の値の頭に置けない字（write-set の印・絶対 path・home）。
const BUNDLE_BARRED_HEADS: [char; 5] = ['/', '+', '-', '=', '~'];

/// 引き金の 5 形（宣言順）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// notes の再発の行の本数が値以上。
    Recurrence,
    /// 開いた契約の行の write-set の項目が値の path に当たる。
    Bundle,
    /// 値の bead が closed。
    Dependency,
    /// 周の時刻が値の時刻以後。
    Deadline,
    /// acceptance の設計 pointer が値の bead が 1 本以上 closed。
    Landing,
}

impl Kind {
    /// 5 形（宣言順）。
    pub const ALL: [Self; 5] = [Self::Recurrence, Self::Bundle, Self::Dependency, Self::Deadline, Self::Landing];

    /// 行の 1 語目の字面。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recurrence => "再発",
            Self::Bundle => "同梱",
            Self::Dependency => "依存",
            Self::Deadline => "期日",
            Self::Landing => "着地",
        }
    }

    /// 値の形の置き字（断り文の 5 形の字面）。
    fn placeholder(self) -> &'static str {
        match self {
            Self::Recurrence => "<n>",
            Self::Bundle => "<path>",
            Self::Dependency => "<id>",
            Self::Deadline => "YYYY-MM-DDTHH:MMZ",
            Self::Landing => "<pointer>",
        }
    }
}

/// 読める引き金 1 つ（形と読んだ値）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trigger {
    /// 再発の行の本数の閾値（1 以上）。
    Recurrence(u64),
    /// repo 相対の path（末尾 `/` は dir）。
    Bundle(String),
    /// 同じ台帳の bead id。
    Dependency(String),
    /// UTC の時刻（UNIX 秒）。
    Deadline(u64),
    /// 設計 pointer。
    Landing(Pointer),
}

impl Trigger {
    /// 形。
    pub fn kind(&self) -> Kind {
        match self {
            Self::Recurrence(_) => Kind::Recurrence,
            Self::Bundle(_) => Kind::Bundle,
            Self::Dependency(_) => Kind::Dependency,
            Self::Deadline(_) => Kind::Deadline,
            Self::Landing(_) => Kind::Landing,
        }
    }
}

/// 引き金の行を読めない理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// 空白で割った語が 2 つ無い。
    Words,
    /// 1 語目が 5 形の外。
    Kind,
    /// 2 語目が形の値の形に合わない。
    Value(Kind),
    /// 依存の値を判じる台帳の接頭辞が解けない。
    NoPrefix,
}

impl Reason {
    /// 断り文と一覧に出す説明（改行を持たない）。
    pub fn describe(self) -> String {
        match self {
            Self::Words => "形と値の 2 語が無い".to_owned(),
            Self::Kind => format!("形が {} の外", Kind::ALL.map(Kind::as_str).join(" / ")),
            Self::Value(kind) => format!("{} の値が {} の形でない", kind.as_str(), kind.placeholder()),
            Self::NoPrefix => "台帳の接頭辞が解けず依存の値を読めない".to_owned(),
        }
    }
}

/// 昇格条件の節の `引き金:` の行 1 本の読み。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// 読める引き金。
    Readable(Trigger),
    /// 読めない引き金（行の字は行頭の空白を除いた字）。
    Unreadable {
        /// 行の字。
        text: String,
        /// 理由。
        reason: Reason,
    },
}

/// memo 1 つの引き金の読み。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reading {
    /// description に `### 昇格条件` の節が在るか。
    pub section: bool,
    /// 節の中の `引き金:` の行（出てきた順）。
    pub lines: Vec<Line>,
    /// notes の `[再発]` の行の本数。
    pub recurrences: usize,
    /// notes の `[keep]` の記帳の行（行頭の空白を除いた字・出てきた順）。
    pub keeps: Vec<String>,
}

impl Reading {
    /// 読める引き金（出てきた順）。
    pub fn readable(&self) -> impl Iterator<Item = &Trigger> {
        self.lines.iter().filter_map(|line| match line {
            Line::Readable(trigger) => Some(trigger),
            Line::Unreadable { .. } => None,
        })
    }

    /// 最初の読めない引き金の行の字と理由。
    pub fn first_unreadable(&self) -> Option<(&str, Reason)> {
        self.lines.iter().find_map(|line| match line {
            Line::Unreadable { text, reason } => Some((text.as_str(), *reason)),
            Line::Readable(_) => None,
        })
    }
}

/// 5 形の字面（`引き金: 再発 <n> / 同梱 <path> / …`・断り文が名指す）。
pub fn shapes() -> String {
    let all = Kind::ALL.map(|kind| format!("{} {}", kind.as_str(), kind.placeholder()));
    format!("{HEAD} {}", all.join(" / "))
}

/// memo の引き金を読む（**純関数**・§15 の文法）。`prefix` は台帳の接頭辞（解けない周は `None`＝依存の値が読めない）。
pub fn read(description: &str, notes: &str, prefix: Option<&str>) -> Reading {
    let [.., promotion] = MEMO_SECTIONS;
    let mut reading = Reading::default();
    let mut inside = false;
    for line in description.lines() {
        if line.trim() == promotion {
            (inside, reading.section) = (true, true);
        } else if is_heading(line) {
            inside = false;
        } else if let Some(rest) = trigger_rest(line).filter(|_| inside) {
            reading.lines.push(line_of(line.trim_start(), rest, prefix));
        }
    }
    let notes: Vec<&str> = notes.lines().map(str::trim_start).collect();
    reading.recurrences = notes.iter().filter(|line| line.starts_with(RECURRENCE_MARK)).count();
    reading.keeps = notes.iter().filter(|line| line.starts_with(KEEP_MARK)).map(|line| (*line).to_owned()).collect();
    reading
}

/// 引き金が満ちたかを判じる世界（入力は全部書き手が集めて渡す・I/O も時計も持たない）。
#[derive(Debug, Clone, Copy)]
pub struct World<'a> {
    /// notes の `[再発]` の行の本数。
    pub recurrences: usize,
    /// 開いた契約の write-set の項目（`+` `-` `=` の印つきの字のまま）。
    pub write_set: &'a [String],
    /// 閉じた bead id の集合。
    pub closed: &'a [String],
    /// 閉じた bead の設計 pointer の集合。
    pub closed_pointers: &'a [Pointer],
    /// 周の時刻（UNIX 秒）。
    pub now: u64,
}

/// write-set の項目の印（`+` `-` `=`）。
const WRITE_SET_MARKS: [char; 3] = ['+', '-', '='];

/// 引き金が満ちたか（**純関数**・FR87 と部分の書き直しの期日の移りが同じ 1 本を引く）。
///
/// 再発は本数が値以上・同梱は印を外した項目が値の path と等しいか、値が `/` で終わって項目がそれで始まる・依存は値の bead が
/// 閉じた・期日は周の時刻が値以後・着地は値の pointer を持つ bead が 1 本以上閉じた。
pub fn met(trigger: &Trigger, world: &World<'_>) -> bool {
    match trigger {
        Trigger::Recurrence(count) => u64::try_from(world.recurrences).is_ok_and(|found| found >= *count),
        Trigger::Bundle(path) => world.write_set.iter().any(|item| {
            let bare = item.strip_prefix(WRITE_SET_MARKS).unwrap_or(item);
            bare == path || (path.ends_with('/') && bare.starts_with(path.as_str()))
        }),
        Trigger::Dependency(id) => world.closed.contains(id),
        Trigger::Deadline(at) => world.now >= *at,
        Trigger::Landing(pointer) => world.closed_pointers.contains(pointer),
    }
}

/// 見出しの行か（行頭の空白を除いて `#` が 1〜3 個と空白で始まる・`####` は節の中）。
fn is_heading(line: &str) -> bool {
    let line = line.trim_start();
    let rest = line.trim_start_matches('#');
    (1..=3).contains(&line.len().saturating_sub(rest.len())) && rest.starts_with(char::is_whitespace)
}

/// 引き金の行なら頭の後ろの字（行頭の空白と任意の `- ` を除いて `引き金:` で始まる行）。
fn trigger_rest(line: &str) -> Option<&str> {
    let line = line.trim_start();
    line.strip_prefix(BULLET).unwrap_or(line).strip_prefix(HEAD)
}

/// 引き金の行 1 本を読む（1 語目が形・2 語目が値・3 語目より後は読まない）。
fn line_of(text: &str, rest: &str, prefix: Option<&str>) -> Line {
    let mut words = rest.split_whitespace();
    let found = match (words.next(), words.next()) {
        (Some(kind), Some(value)) => match Kind::ALL.into_iter().find(|found| found.as_str() == kind) {
            Some(kind) => value_of(kind, value, prefix),
            None => Err(Reason::Kind),
        },
        _ => Err(Reason::Words),
    };
    found.map_or_else(|reason| Line::Unreadable { text: text.to_owned(), reason }, Line::Readable)
}

/// 形ごとの値の形で値を読む。
fn value_of(kind: Kind, value: &str, prefix: Option<&str>) -> Result<Trigger, Reason> {
    let bad = Reason::Value(kind);
    match kind {
        Kind::Recurrence => value
            .bytes()
            .all(|found| found.is_ascii_digit())
            .then(|| value.parse::<u64>().ok())
            .flatten()
            .filter(|count| *count >= 1)
            .map(Trigger::Recurrence)
            .ok_or(bad),
        Kind::Bundle => is_repo_path(value).then(|| Trigger::Bundle(value.to_owned())).ok_or(bad),
        Kind::Dependency => {
            let prefix = prefix.ok_or(Reason::NoPrefix)?;
            is_bead_id(value, prefix).then(|| Trigger::Dependency(value.to_owned())).ok_or(bad)
        }
        Kind::Deadline => value
            .strip_suffix('Z')
            .and_then(|minute| epoch_of(&format!("{minute}:00Z")))
            .map(Trigger::Deadline)
            .ok_or(bad),
        Kind::Landing => parse_pointer(value).map(Trigger::Landing).map_err(|_| bad),
    }
}

/// 同梱の値の形か（頭が [`BUNDLE_BARRED_HEADS`] でない・`#` を持たない・`..` の段を持たない）。
fn is_repo_path(value: &str) -> bool {
    !value.starts_with(BUNDLE_BARRED_HEADS) && !value.contains('#') && !value.split('/').any(|step| step == "..")
}

#[cfg(test)]
mod tests {
    use super::{met, read, shapes, Kind, Line, Reason, Trigger, World};
    use crate::pipe::table::Pointer;

    /// 引き金の行 1 本を節の中に置いて読む。
    fn one(line: &str, prefix: Option<&str>) -> Vec<Line> {
        read(&format!("### 出所\n### 昇格条件\n{line}\n"), "", prefix).lines
    }

    /// 読めない 1 本（行の字と理由）。
    fn unreadable(text: &str, reason: Reason) -> Vec<Line> {
        vec![Line::Unreadable { text: text.to_owned(), reason }]
    }

    /// 5 形 × 読める値: 再発の 1 以上の整数・同梱の repo 相対の path と dir・依存の同じ台帳の id・期日の分までの UTC・着地の
    /// 設計 pointer。
    #[test]
    fn ledger_trigger_reads_each_kind_with_a_readable_value() {
        let pointer = Pointer { path: "docs/design/ledger-form.md".to_owned(), id: "k".to_owned() };
        for (line, want) in [
            ("引き金: 再発 1", Trigger::Recurrence(1)),
            ("引き金: 再発 12", Trigger::Recurrence(12)),
            ("引き金: 同梱 crates/scribe2/src/ledger/trigger.rs", Trigger::Bundle("crates/scribe2/src/ledger/trigger.rs".to_owned())),
            ("引き金: 同梱 crates/scribe2/src/", Trigger::Bundle("crates/scribe2/src/".to_owned())),
            ("引き金: 依存 s2-07l.738.12", Trigger::Dependency("s2-07l.738.12".to_owned())),
            ("引き金: 期日 2026-09-30T12:00Z", Trigger::Deadline(1_790_769_600)),
            ("引き金: 着地 docs/design/ledger-form.md#k", Trigger::Landing(pointer.clone())),
        ] {
            let found = one(line, Some("s2"));
            assert_eq!(found, vec![Line::Readable(want.clone())], "{line}");
            assert_eq!(Some(want.kind().as_str()), line.split_whitespace().nth(1), "{line}: 形の語");
        }
    }

    /// 5 形 × 読めない値（値に括弧が続く・0・全角の数字・印の頭・`#`・`..`・別の台帳・秒つき・日付だけ・時差つき・`#` の無い
    /// pointer）と、語が 2 つ無い・形が 5 つの外。
    #[test]
    fn ledger_trigger_refuses_each_kind_with_an_unreadable_value() {
        for (line, kind) in [
            ("引き金: 再発 3（同じ落ち方）", Kind::Recurrence),
            ("引き金: 再発 0", Kind::Recurrence),
            ("引き金: 再発 ３", Kind::Recurrence),
            ("引き金: 同梱 +crates/x.rs", Kind::Bundle),
            ("引き金: 同梱 /etc/x", Kind::Bundle),
            ("引き金: 同梱 ~x", Kind::Bundle),
            ("引き金: 同梱 docs/design/x.md#k", Kind::Bundle),
            ("引き金: 同梱 crates/../x.rs", Kind::Bundle),
            ("引き金: 依存 tz-1", Kind::Dependency),
            ("引き金: 依存 s2-", Kind::Dependency),
            ("引き金: 依存 s2-a..b", Kind::Dependency),
            ("引き金: 期日 2026-09-30T12:00:00Z", Kind::Deadline),
            ("引き金: 期日 2026-09-30", Kind::Deadline),
            ("引き金: 期日 2026-09-30T12:00+09:00", Kind::Deadline),
            ("引き金: 着地 docs/design/ledger-form.md", Kind::Landing),
            ("引き金: 着地 docs/x.txt#k", Kind::Landing),
        ] {
            assert_eq!(one(line, Some("s2")), unreadable(line, Reason::Value(kind)), "{line}");
        }
        assert_eq!(one("引き金: 再発", Some("s2")), unreadable("引き金: 再発", Reason::Words));
        assert_eq!(one("引き金:", Some("s2")), unreadable("引き金:", Reason::Words));
        assert_eq!(one("引き金: 失敗 3", Some("s2")), unreadable("引き金: 失敗 3", Reason::Kind));
        assert!(Reason::Value(Kind::Deadline).describe().contains("YYYY-MM-DDTHH:MMZ"));
    }

    /// 依存は接頭辞の無い周に読めない（形が合っていても）。ほかの 4 形は接頭辞に依らない。
    #[test]
    fn ledger_trigger_dependency_needs_the_ledger_prefix() {
        assert_eq!(one("引き金: 依存 s2-1", None), unreadable("引き金: 依存 s2-1", Reason::NoPrefix));
        assert_eq!(one("引き金: 再発 2", None), vec![Line::Readable(Trigger::Recurrence(2))]);
        assert_eq!(one("引き金: 依存 toy-a1.2", Some("toy")), vec![Line::Readable(Trigger::Dependency("toy-a1.2".to_owned()))]);
    }

    /// 節の境: `####` は節の中・`##` と `###` で終わる・節の外と notes の引き金の行は読まない・節が複数在れば全部読む。
    #[test]
    fn ledger_trigger_reads_only_inside_the_promotion_sections() {
        let description = "引き金: 再発 9\n### 昇格条件\n#### 補足\n引き金: 再発 1\n## 次\n引き金: 再発 2\n### 昇格条件\n  引き金: 再発 3\n### 候補\n引き金: 再発 4\n";
        let found = read(description, "引き金: 再発 5\n", None);
        assert!(found.section);
        assert_eq!(found.lines, vec![Line::Readable(Trigger::Recurrence(1)), Line::Readable(Trigger::Recurrence(3))]);
        let bare = read("### 出所\n引き金: 再発 1\n", "", None);
        assert!(!bare.section && bare.lines.is_empty(), "節の無い本文");
        assert!(read("### 昇格条件 の注\n引き金: 再発 1\n", "", None).lines.is_empty(), "trim が完全一致の見出しだけ");
    }

    /// 行頭: 空白と `- ` を許し、全角の colon・`* `・行の途中の `引き金:` は引き金の行でない。3 語目より後は読まない。
    #[test]
    fn ledger_trigger_line_head_and_ignored_tail() {
        assert_eq!(one("  - 引き金: 再発 2 （同じ落ち方が 2 回）", None), vec![Line::Readable(Trigger::Recurrence(2))]);
        assert_eq!(one("- 引き金:再発\u{3000}4", None), vec![Line::Readable(Trigger::Recurrence(4))], "Unicode の空白で割る");
        for line in ["引き金： 再発 1", "* 引き金: 再発 1", "- 散文の 引き金: 再発 1", "-- 引き金: 再発 1"] {
            assert_eq!(one(line, None), Vec::new(), "{line}");
        }
        let found = one("- 引き金: 再発 3（x）", None);
        assert_eq!(found, unreadable("- 引き金: 再発 3（x）", Reason::Value(Kind::Recurrence)), "行の字は行頭の空白だけを除く");
    }

    /// notes: 行頭の空白を除いて `[再発]` で始まる行の本数と `[keep]` の行を数え、description の印は数えない。
    #[test]
    fn ledger_trigger_counts_recurrence_and_keep_marks_in_notes() {
        let notes = "[再発] 2026-09-28 同じ\n  [再発] 2026-09-29\nx [再発]\n[keep] 2026-09-29 残す\n[再発]\n";
        let found = read("[再発] x\n[keep] y\n", notes, None);
        assert_eq!(found.recurrences, 3);
        assert_eq!(found.keeps, ["[keep] 2026-09-29 残す"]);
        assert!(!found.section);
        let text = shapes();
        for kind in Kind::ALL {
            assert!(text.contains(&format!("{} {}", kind.as_str(), kind.placeholder())), "{text}");
        }
        assert!(text.starts_with("引き金: 再発 <n> / "), "{text}");
    }

    /// 何も満たさない世界（周の時刻 0）。
    fn empty() -> World<'static> {
        World { recurrences: 0, write_set: &[], closed: &[], closed_pointers: &[], now: 0 }
    }

    /// 同梱の世界（write-set の項目だけ持つ）。
    fn with_items(items: &[String]) -> World<'_> {
        World { write_set: items, ..empty() }
    }

    /// 再発: 本数＝値で満ち、値−1 で満ちない。
    #[test]
    fn trigger_met_recurrence_at_the_threshold() {
        assert!(met(&Trigger::Recurrence(3), &World { recurrences: 3, ..empty() }));
        assert!(met(&Trigger::Recurrence(3), &World { recurrences: 4, ..empty() }));
        assert!(!met(&Trigger::Recurrence(3), &World { recurrences: 2, ..empty() }));
    }

    /// 期日: 周の時刻＝値で満ち、1 秒前で満ちない。
    #[test]
    fn trigger_met_deadline_at_the_second() {
        assert!(met(&Trigger::Deadline(1_790_769_600), &World { now: 1_790_769_600, ..empty() }));
        assert!(!met(&Trigger::Deadline(1_790_769_600), &World { now: 1_790_769_599, ..empty() }));
    }

    /// 依存: 値の bead が閉じた集合に在れば満ち、無ければ満ちない。
    #[test]
    fn trigger_met_dependency_closed_or_not() {
        let closed = ["s2-a".to_owned(), "s2-b".to_owned()];
        assert!(met(&Trigger::Dependency("s2-b".to_owned()), &World { closed: &closed, ..empty() }));
        assert!(!met(&Trigger::Dependency("s2-c".to_owned()), &World { closed: &closed, ..empty() }));
        assert!(!met(&Trigger::Dependency("s2-b".to_owned()), &empty()));
    }

    /// 着地: 値の pointer を持つ bead が 1 本以上閉じれば満ち、別の pointer だけなら満ちない。
    #[test]
    fn trigger_met_landing_pointer_closed_or_not() {
        let pointer = |id: &str| Pointer { path: "docs/design/ledger-form.md".to_owned(), id: id.to_owned() };
        let closed = [pointer("a"), pointer("k")];
        assert!(met(&Trigger::Landing(pointer("k")), &World { closed_pointers: &closed, ..empty() }));
        assert!(!met(&Trigger::Landing(pointer("z")), &World { closed_pointers: &closed, ..empty() }));
        assert!(!met(&Trigger::Landing(pointer("k")), &empty()));
    }

    /// 同梱: 印（`+` `-` `=`）を外した項目が値の path と等しければ満ち、違う path は満ちない。
    #[test]
    fn trigger_met_bundle_equal_after_stripping_the_mark() {
        let want = Trigger::Bundle("crates/x/a.rs".to_owned());
        for item in ["crates/x/a.rs", "+crates/x/a.rs", "-crates/x/a.rs", "=crates/x/a.rs"] {
            assert!(met(&want, &with_items(&[item.to_owned()])), "{item}");
        }
        assert!(!met(&want, &with_items(&["+crates/x/b.rs".to_owned()])));
        assert!(!met(&want, &empty()));
    }

    /// 同梱: 値が `/` で終わる dir は項目がそれで始まれば満ち（印を外して）、別の dir は満ちない。
    #[test]
    fn trigger_met_bundle_dir_prefix() {
        let want = Trigger::Bundle("crates/x/".to_owned());
        assert!(met(&want, &with_items(&["+crates/x/a.rs".to_owned()])));
        assert!(met(&want, &with_items(&["crates/x/deep/b.rs".to_owned()])));
        assert!(!met(&want, &with_items(&["crates/xy/a.rs".to_owned()])));
        assert!(!met(&want, &with_items(&["+crates/y/x/a.rs".to_owned()])));
    }

    /// 同梱: 値が `/` で終わらなければ前方一致で満ちない（値の字が項目の頭と一致するだけで等しくない組）。
    #[test]
    fn trigger_met_bundle_without_trailing_slash_is_not_a_prefix() {
        let want = Trigger::Bundle("crates/x/a".to_owned());
        assert!(!met(&want, &with_items(&["+crates/x/a.rs".to_owned()])));
        assert!(!met(&want, &with_items(&["crates/x/a/b.rs".to_owned()])));
        assert!(met(&want, &with_items(&["crates/x/a".to_owned()])), "等しい組は満ちる");
    }

    /// 5 形とも、何も無い世界では満ちない（全部を真に倒す実装を落とす）。
    #[test]
    fn trigger_met_nothing_in_an_empty_world() {
        let pointer = Pointer { path: "docs/design/ledger-form.md".to_owned(), id: "k".to_owned() };
        for trigger in [
            Trigger::Recurrence(1),
            Trigger::Bundle("crates/x/".to_owned()),
            Trigger::Dependency("s2-a".to_owned()),
            Trigger::Deadline(1),
            Trigger::Landing(pointer),
        ] {
            assert!(!met(&trigger, &empty()), "{trigger:?}");
        }
    }
}
