//! 局面の出力の 2 本の線の読みと記帳（設計 case-lifecycle.md §11・ADR-0100・FR90）。
//!
//! 切り替えの線は detail を持たない最初の [`EventKind::LifecycleCutover`]、close-check の線は detail が
//! [`CLOSE_CHECK`] の最初の行である。close-check の線は新しい kind でも key でもなく、旧い版の器が log の全部を読めなくなる
//! 形を避けて既存の `detail` に載せる（読み手の表は変えない）。

use super::store::{append_if, Condition, LockPolicy, Refusal, StoreError};
use super::{cli, Case, Event, EventKind, ACTOR_MACHINE, SCHEMA};

/// close-check の線の行が `detail` に持つ語。
pub const CLOSE_CHECK: &str = "close-check";

/// 線の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// 切り替えの線（この版の器が局面の出力を始めた時）。
    Cutover,
    /// close-check の線（閉じを数え始める時）。
    CloseCheck,
}

/// 線の 1 本（行の version・main・ts）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// 線を引いた器の版。
    pub version: String,
    /// 線を引いた時の main の sha。
    pub main: String,
    /// 行の ts。
    pub ts: String,
}

/// event の列から読んだ 2 本の線（無ければ `None`）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Lines {
    /// 切り替えの線。
    pub cutover: Option<Line>,
    /// close-check の線。
    pub close_check: Option<Line>,
}

/// event の列から 2 本の線を読む（最初の行が勝つ・detail がほかの値の行はどちらの線にも数えない）。
pub fn read_lines(events: &[Event]) -> Lines {
    let mut lines = Lines::default();
    for event in events.iter().filter(|event| event.kind == EventKind::LifecycleCutover) {
        let Some(Case::Cutover { version, main }) = &event.case else { continue };
        let slot = match event.detail.as_deref() {
            None => &mut lines.cutover,
            Some(CLOSE_CHECK) => &mut lines.close_check,
            Some(_) => continue,
        };
        slot.get_or_insert_with(|| Line { version: version.clone(), main: main.clone(), ts: event.ts.clone() });
    }
    lines
}

/// 線を 1 度だけ記帳する。足したら `true`・既に在る線と、切り替えの線の無い log への close-check の線は何も足さず `false`。
///
/// 版は `CARGO_PKG_VERSION`・`main` は呼び手が読んだ main の先端の sha（小文字の 16 進・読み手が拒む字は書かずに `Err`）。
/// 述語は event の lock の内側で評価する（[`Condition::LineAbsent`]）。
pub fn book(dir: &std::path::Path, kind: Kind, main: &str, policy: LockPolicy) -> Result<bool, StoreError> {
    if main.is_empty() || !main.chars().all(|ch| matches!(ch, '0'..='9' | 'a'..='f')) {
        return Err(StoreError::Io(format!("main の sha {main:?} は小文字の 16 進でない")));
    }
    let event = Event {
        schema: SCHEMA,
        ts: cli::now_utc(),
        kind: EventKind::LifecycleCutover,
        run: String::new(),
        bead: String::new(),
        host: cli::host(),
        actor: ACTOR_MACHINE.to_owned(),
        stage: None,
        seat: None,
        pid: None,
        detail: (kind == Kind::CloseCheck).then(|| CLOSE_CHECK.to_owned()),
        allowance: None,
        registration: None,
        mark: None,
        account: None,
        cost: None,
        rule: None,
        case: Some(Case::Cutover { version: env!("CARGO_PKG_VERSION").to_owned(), main: main.to_owned() }),
    };
    match append_if(dir, &event, policy, Condition::LineAbsent { close_check: kind == Kind::CloseCheck }) {
        Ok(_) => Ok(true),
        Err(StoreError::Refused(Refusal::Present | Refusal::NoCutover)) => Ok(false),
        Err(other) => Err(other),
    }
}

/// 1 周の記帳: 切り替えの線を先に、`close_check` が真なら続けて close-check の線を足す。足した線を足した順に返す。
pub fn book_lines(dir: &std::path::Path, main: &str, close_check: bool, policy: LockPolicy) -> Result<Vec<Kind>, StoreError> {
    let wanted = [Some(Kind::Cutover), close_check.then_some(Kind::CloseCheck)];
    let mut added = Vec::new();
    for kind in wanted.into_iter().flatten() {
        if book(dir, kind, main, policy)? {
            added.push(kind);
        }
    }
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::{book, book_lines, read_lines, Kind, Line, CLOSE_CHECK};
    use crate::fleet::store::{events_path, read_all, LockPolicy};
    use crate::fleet::{Case, Event, EventKind};
    use crate::pipe::fixture::event;
    use std::path::PathBuf;

    const POLICY: LockPolicy = LockPolicy { retry_ms: 5_000, stale_ms: 600_000 };
    const MAIN: &str = "0123456789abcdef0123456789abcdef01234567";

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lifecycle-line-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    fn cutover(ts: &str, main: &str, detail: Option<&str>) -> Event {
        Event {
            ts: ts.to_owned(),
            run: String::new(),
            bead: String::new(),
            case: Some(Case::Cutover { version: "0.1.0".to_owned(), main: main.to_owned() }),
            ..event("", EventKind::LifecycleCutover, None, None, detail)
        }
    }

    fn line(ts: &str, main: &str) -> Line {
        Line { version: "0.1.0".to_owned(), main: main.to_owned(), ts: ts.to_owned() }
    }

    /// (1) 2 つの線の読み: 最初の行が勝ち、detail がほかの値の行はどちらの線にも数えない。線の無い列は両方 `None`。
    #[test]
    fn cutover_line_read_takes_first_of_each_and_skips_other_details() {
        let events = [
            cutover("2026-09-30T00:00:01Z", "aa", Some("other")),
            event("r", EventKind::RunStage, None, None, None),
            cutover("2026-09-30T00:00:02Z", "bb", Some(CLOSE_CHECK)),
            cutover("2026-09-30T00:00:03Z", "cc", None),
            cutover("2026-09-30T00:00:04Z", "dd", None),
            cutover("2026-09-30T00:00:05Z", "ee", Some(CLOSE_CHECK)),
        ];
        let lines = read_lines(&events);
        assert_eq!(lines.cutover, Some(line("2026-09-30T00:00:03Z", "cc")), "detail の無い最初の行");
        assert_eq!(lines.close_check, Some(line("2026-09-30T00:00:02Z", "bb")), "close-check の最初の行");
        let only_other = read_lines(&events[..2]);
        assert_eq!((only_other.cutover, only_other.close_check), (None, None), "ほかの detail は数えない");
        assert_eq!(read_lines(&[]), Default::default());
    }

    /// (3) 記帳: 切り替えの線の無い log への close-check の記帳は何も足さず、切り替えの線の後は足す（対）。1 度だけ・
    /// 同じ周は切り替えの線が先・記帳した行の version は器の版で main は渡した sha・close-check の線は detail を持つ。
    #[test]
    fn cutover_line_book_adds_once_with_cutover_first() {
        let dir = scratch("book");
        assert_eq!(book(&dir, Kind::CloseCheck, MAIN, POLICY), Ok(false), "切り替えの線の無い log");
        assert!(!events_path(&dir).exists() || read_all(&dir).expect("読める").is_empty(), "何も足さない");
        assert_eq!(book_lines(&dir, MAIN, true, POLICY), Ok(vec![Kind::Cutover, Kind::CloseCheck]), "同じ周は切り替えの線が先");
        let events = read_all(&dir).expect("読める");
        assert_eq!(events.len(), 2);
        assert_eq!((events[0].detail.as_deref(), events[1].detail.as_deref()), (None, Some(CLOSE_CHECK)));
        let lines = read_lines(&events);
        for found in [lines.cutover, lines.close_check] {
            let found = found.expect("線が在る");
            assert_eq!((found.version.as_str(), found.main.as_str()), (env!("CARGO_PKG_VERSION"), MAIN));
        }
        assert_eq!(book_lines(&dir, "abc123", true, POLICY), Ok(Vec::new()), "在る線は動かさない");
        assert_eq!(book(&dir, Kind::Cutover, "abc123", POLICY), Ok(false));
        assert_eq!(read_all(&dir).expect("読める"), events, "log は変わらない");
        let solo = scratch("book-solo");
        assert_eq!(book_lines(&solo, MAIN, false, POLICY), Ok(vec![Kind::Cutover]), "close-check が偽の周は切り替えの線だけ");
        assert_eq!(book_lines(&solo, MAIN, true, POLICY), Ok(vec![Kind::CloseCheck]), "後の周は在る線を足さない");
        assert!(book(&solo, Kind::Cutover, "XYZ", POLICY).is_err(), "読めない sha は書かない");
        assert_eq!(read_all(&solo).expect("読めるまま").len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&solo);
    }

    /// (4) close-check の線の行は今の読み手で読め、既知の表の外の key を 1 つ足した字は読めない（読み手の表を変えない）。
    #[test]
    fn cutover_line_close_check_row_reads_and_unknown_key_does_not() {
        let row = cutover("2026-09-30T00:00:00Z", MAIN, Some(CLOSE_CHECK));
        let text = row.to_line();
        assert!(text.contains(r#""detail":"close-check""#), "{text}");
        assert_eq!(Event::from_line(&text), Ok(row), "読めて元の行に戻る");
        let extra = format!("{},\"bogus\":\"x\"}}", text.trim_end().trim_end_matches('}'));
        assert!(Event::from_line(&extra).is_err(), "表の外の key は読めない: {extra}");
    }
}
