//! write-set の項目の読みと上限の余地（設計 docs/design/contract-source.md §3「項目の実在と展開」「上限の余地」・
//! §14・SRS FR48・pure）。
//!
//! 契約の write-set の各項目を base（tracked file の一覧）に対して [`read_write_set`] で読み（実在する file / 末尾 `/`
//! の dir / `+` の新規 file / `-` の縮む file の 4 形）、`.rs` の項目ごとに上限（R-C4-2 / R-C4-1）の余地を
//! [`headroom_shortfalls`] で測る。宣言（`.vessel.toml`）の読みと上限の突き合わせは親 module `declaration.rs` に
//! 置いたまま。呼び手（`pipe::table` / `pipe::cli::intake`）の `use` は親の再 export を通る。

/// write-set の 1 項目を base（tracked file の一覧）に対して読んだもの。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteSetItem {
    /// base に実在する file（repo 相対 path）。
    File(String),
    /// 末尾 `/` の dir。base の配下の file を**展開した**列で持つ（辞書順）。
    Dir(Vec<String>),
    /// `+` 接頭辞で宣言した新規 file（base に無い・接頭辞を剥がした path）。
    New(String),
    /// `-` 接頭辞で宣言した**縮む面**（base に実在する file・接頭辞を剥がした path・設計 contract-source.md §3）。
    /// 増分は負なので上限の余地を求めず、core の見積の本数にも数えない。閉包・交差・guard は [`Self::File`] と同じ
    /// 素の path として読む。
    Shrink(String),
}

/// `+` の項目（新規 file の**宣言**）を base（tracked の**実測**）に対して読む場面（設計 contract-source.md §3・C10）。
/// 場面の違いはこの閉じた型の値 1 つで渡し、読む関数は [`read_write_set`] の 1 本（C2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewFilePolicy {
    /// 受付（intake）: base に在れば解けない＝planner の `+` の誤りは入口で止まる（FR39）。
    MustBeAbsent,
    /// 契約表の検査（`contracts check`・land 後の main）: tracked に在れば land 済みの実在 file（[`WriteSetItem::File`]）
    /// と読む（契約表の行は履歴を持つ＝land のたびに `+` が解けなくなる罠を塞ぐ・`s2-07l.346`）。
    MayBeLanded,
}

/// write-set の各項目を base に対して読む。**解けない項目は全件**（1 件目で止めない）。
///
/// 解ける形は 4 つだけ: base に実在する file / 末尾 `/` で base に配下の file を持つ dir / `+` 接頭辞で base に**無い**
/// 新規 file / `-` 接頭辞で base に**在る**縮む file。それ以外（無い file・空の dir・base に在る file への `+`・base に
/// 無い file への `-`）は `Err` に項目の字面で積む。base に在る file への `+` だけは `policy` で読みが変わる
/// （[`NewFilePolicy::MayBeLanded`] は [`WriteSetItem::File`] に解く）。
pub fn read_write_set(
    write_set: &[String],
    tracked: &[String],
    policy: NewFilePolicy,
) -> Result<Vec<WriteSetItem>, Vec<String>> {
    let (mut items, mut unresolved) = (Vec::new(), Vec::new());
    for item in write_set {
        match read_item(item, tracked, policy) {
            Some(found) => items.push(found),
            None => unresolved.push(item.clone()),
        }
    }
    if unresolved.is_empty() {
        Ok(items)
    } else {
        Err(unresolved)
    }
}

/// 1 項目を読む（解けなければ `None`）。
fn read_item(item: &str, tracked: &[String], policy: NewFilePolicy) -> Option<WriteSetItem> {
    if let Some(dir) = item.strip_suffix('/') {
        let under: Vec<String> = tracked.iter().filter(|path| is_under(path, dir)).cloned().collect();
        return (!under.is_empty()).then_some(WriteSetItem::Dir(under));
    }
    if let Some(new) = item.strip_prefix(crate::pipe::refuse::NEW_FILE) {
        if new.is_empty() {
            return None;
        }
        return match (tracked.iter().any(|path| path == new), policy) {
            (false, _) => Some(WriteSetItem::New(new.to_owned())),
            (true, NewFilePolicy::MayBeLanded) => Some(WriteSetItem::File(new.to_owned())),
            (true, NewFilePolicy::MustBeAbsent) => None,
        };
    }
    if let Some(old) = item.strip_prefix(crate::pipe::refuse::SHRINK_FILE) {
        let present = !old.is_empty() && tracked.iter().any(|path| path == old);
        return present.then(|| WriteSetItem::Shrink(old.to_owned()));
    }
    tracked.iter().any(|path| path == item).then(|| WriteSetItem::File(item.to_owned()))
}

/// `path` が dir `dir`（末尾 `/` 無しの字面）の配下か。
pub(crate) fn is_under(path: &str, dir: &str) -> bool {
    path.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/'))
}

/// 上限の余地の入力（rules 行の値・設計 contract-source.md §3「上限の余地」）。数は manifest から読む（C1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    /// 1 file の行数の上限（R-C4-2）。
    pub file_lines: u64,
    /// core の総行数の上限（R-C4-1）。
    pub core_lines: u64,
    /// `size` 1 段の 1 file あたりの見積（行・`pipe.size_<s|m|l>_lines` のうち契約の size の行）。
    pub size_lines: u64,
}

/// 余地の足りない 1 件（file は repo 相対・core の合計は [`CORE`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Headroom {
    /// 余地の足りない file。
    pub file: String,
    /// 残っている行数。
    pub headroom: u64,
}

/// core の合計を名指す `file` の字面。
pub const CORE: &str = "core";

/// 行数（xtask check の file-lines / core-lines と同じ式 = 幅 `width` で正規化した行数・[`crate::pipe::closure::weighted_lines`]）。
pub fn line_count(text: &str, width: u64) -> u64 {
    let width = usize::try_from(width).unwrap_or(usize::MAX);
    u64::try_from(crate::pipe::closure::weighted_lines(text, width)).unwrap_or(u64::MAX)
}

/// 上限の余地を測る（**受付だけが撃つ**・pure・I/O は呼び手）。
///
/// `lines` は base の tracked `.rs` の (path, 行数)。write-set の `.rs`（dir は展開した配下・新規 file は 0 行）の
/// うち R-C4-2 の測定範囲（`crates/<c>/src/` 配下＝[`core_of`] が `Some`）のそれぞれについて `file_lines − 行数` を
/// 余地とし、`size_lines` が余地を超える file を名指す（範囲外の `tests/` 等は門の対象外で測らない）。core（write-set
/// の `.rs` が在る `crates/<c>/src/` の総行数）は `size_lines × その core に属する write-set の .rs 本数` を見積として
/// 同じ式で 1 回（母集団は file の余地と同じ [`core_of`] が `Some` の集合＝`tests/` の歯は本数に入れない・C10）。
/// **縮む面（`-`）は増分が負**なので、file の余地も求めず core の本数にも数えない（満杯の file を割る便を受付が
/// 断って満杯が固定される型を塞ぐ・§3「上限の余地」）。
pub fn headroom_shortfalls(items: &[WriteSetItem], lines: &[(String, u64)], caps: Caps) -> Vec<Headroom> {
    let files: Vec<&str> = items
        .iter()
        .flat_map(|item| match *item {
            WriteSetItem::File(ref path) | WriteSetItem::New(ref path) => vec![path.as_str()],
            WriteSetItem::Dir(ref under) => under.iter().map(String::as_str).collect(),
            WriteSetItem::Shrink(_) => Vec::new(),
        })
        .filter(|path| path.ends_with(".rs"))
        .collect();
    let lines_of = |path: &str| lines.iter().find(|(found, _)| found == path).map_or(0, |(_, count)| *count);
    let mut found: Vec<Headroom> = files
        .iter()
        .filter(|path| core_of(path).is_some())
        .filter_map(|path| {
            let headroom = caps.file_lines.saturating_sub(lines_of(path));
            (caps.size_lines > headroom).then(|| Headroom { file: (*path).to_owned(), headroom })
        })
        .collect();
    let mut cores: Vec<&str> = files.iter().filter_map(|path| core_of(path)).collect();
    cores.sort_unstable();
    cores.dedup();
    for core in cores {
        let members = files.iter().filter(|path| core_of(path) == Some(core)).count();
        let estimate = caps.size_lines.saturating_mul(u64::try_from(members).unwrap_or(u64::MAX));
        let total: u64 = lines.iter().filter(|(path, _)| core_of(path) == Some(core)).map(|(_, count)| *count).sum();
        let headroom = caps.core_lines.saturating_sub(total);
        if estimate > headroom {
            found.push(Headroom { file: CORE.to_owned(), headroom });
        }
    }
    found
}

/// `.rs` の path が属する core（`crates/<c>/src/…` の `crates/<c>/src`）。その形でなければ `None`。
fn core_of(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("crates/")?;
    let (crate_name, tail) = rest.split_once("/src/")?;
    let head = path.len().checked_sub(tail.len().saturating_add(1))?;
    (!crate_name.is_empty() && !crate_name.contains('/')).then(|| path.get(..head)).flatten()
}

#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.373

    use super::super::tests::strings;
    use super::{headroom_shortfalls, line_count, read_write_set, Caps, Headroom, NewFilePolicy, WriteSetItem, CORE};

    /// base の tracked file（write-set の項目の fixture）。
    fn base() -> Vec<String> {
        strings(&["crates/toy/src/a.rs", "crates/toy/src/b.rs", "snap/x.snap", "docs/d.md", "crates/toy/tests/t.rs"])
    }

    /// write-set の項目は 4 形（実在する file / 末尾 `/` で配下を持つ dir〔展開される〕/ `+` の新規 file〔base に無い〕/
    /// `-` の縮む file〔base に在る〕）だけが解け、それ以外は**全件**項目の字面で返る（設計 contract-source.md §3
    /// 「項目の実在と展開」）。
    #[test]
    fn declaration_write_set_items_resolve_only_the_three_forms_and_name_every_unresolved_item() {
        let read = read_write_set(
            &strings(&["crates/toy/src/a.rs", "snap/", "+crates/toy/src/new.rs", "-crates/toy/src/b.rs"]),
            &base(),
            NewFilePolicy::MustBeAbsent,
        );
        assert_eq!(
            read,
            Ok(vec![
                WriteSetItem::File("crates/toy/src/a.rs".to_owned()),
                WriteSetItem::Dir(vec!["snap/x.snap".to_owned()]),
                WriteSetItem::New("crates/toy/src/new.rs".to_owned()),
                WriteSetItem::Shrink("crates/toy/src/b.rs".to_owned()),
            ]),
            "4 形が解け dir は配下に展開され、- は接頭辞を剥がした path で持つ"
        );
        let unresolved = read_write_set(
            &strings(&[
                "crates/toy/src/none.rs",
                "empty/",
                "+crates/toy/src/a.rs",
                "+",
                "snap",
                "-crates/toy/src/none.rs",
                "-",
                "docs/d.md",
            ]),
            &base(),
            NewFilePolicy::MustBeAbsent,
        );
        assert_eq!(
            unresolved,
            Err(strings(&["crates/toy/src/none.rs", "empty/", "+crates/toy/src/a.rs", "+", "snap", "-crates/toy/src/none.rs", "-"])),
            "無い file・空の dir・base に在る file への +・空の +・base に無い file への -・空の - は解けない（末尾 / 無しの dir も file としては無い）"
        );
    }

    /// `+` の 2 場面（`s2-07l.346`・設計 contract-source.md §3）の表: tracked / untracked × [`NewFilePolicy`] の 4 組。
    /// 違うのは「tracked な `+`」の 1 組だけ（`MustBeAbsent` は解けない・`MayBeLanded` は実在 file に解く）。untracked な
    /// `+` はどちらも `New`・空の `+` はどちらも解けない・`+` の無い項目は policy を見ない。
    #[test]
    fn declaration_write_set_landed_plus_resolves_as_file_only_when_the_policy_allows_it() {
        let (landed, fresh) = ("+crates/toy/src/a.rs", "+crates/toy/src/new.rs");
        let table = [
            (NewFilePolicy::MustBeAbsent, landed, Err(strings(&[landed]))),
            (NewFilePolicy::MayBeLanded, landed, Ok(vec![WriteSetItem::File("crates/toy/src/a.rs".to_owned())])),
            (NewFilePolicy::MustBeAbsent, fresh, Ok(vec![WriteSetItem::New("crates/toy/src/new.rs".to_owned())])),
            (NewFilePolicy::MayBeLanded, fresh, Ok(vec![WriteSetItem::New("crates/toy/src/new.rs".to_owned())])),
        ];
        for (policy, item, want) in table {
            assert_eq!(read_write_set(&strings(&[item]), &base(), policy), want, "{policy:?} × {item}");
        }
        for policy in [NewFilePolicy::MustBeAbsent, NewFilePolicy::MayBeLanded] {
            assert_eq!(read_write_set(&strings(&["+"]), &base(), policy), Err(strings(&["+"])), "空の + は {policy:?} でも解けない");
            let plain = read_write_set(&strings(&["crates/toy/src/none.rs", "-crates/toy/src/a.rs"]), &base(), policy);
            assert_eq!(plain, Err(strings(&["crates/toy/src/none.rs"])), "+ の無い項目は {policy:?} を見ない");
        }
    }

    /// 上限の余地: write-set の `.rs` ごとに `file_lines − 行数` を余地とし、size の見積が超える file を名指す。core
    /// （`crates/<c>/src/` の合計）は `見積 × .rs 本数` で 1 回。`.rs` でない項目と別 crate の行は数えない。
    #[test]
    fn declaration_headroom_names_the_file_and_the_core_whose_room_is_short() {
        let lines = vec![
            ("crates/toy/src/a.rs".to_owned(), 1_400),
            ("crates/toy/src/b.rs".to_owned(), 100),
            ("crates/other/src/z.rs".to_owned(), 5_000),
            ("crates/toy/tests/t.rs".to_owned(), 900),
        ];
        let policy = NewFilePolicy::MustBeAbsent;
        let items = read_write_set(&strings(&["crates/toy/src/a.rs", "snap/", "+crates/toy/src/new.rs"]), &base(), policy)
            .unwrap_or_default();
        let caps = |size_lines: u64, core_lines: u64| Caps { file_lines: 1_500, core_lines, size_lines };
        assert_eq!(
            headroom_shortfalls(&items, &lines, caps(300, 40_000)),
            vec![Headroom { file: "crates/toy/src/a.rs".to_owned(), headroom: 100 }],
            "M（300）は余地 100 の a.rs に入らない・新規 file は余地いっぱい・core は余裕"
        );
        assert!(headroom_shortfalls(&items, &lines, caps(100, 40_000)).is_empty(), "S（100）は余地 100 に入る");
        // core: 合計 1500（tests/ と別 crate は数えない）・見積 = 100 × 2 本 = 200 > 余地 100。
        assert_eq!(
            headroom_shortfalls(&items, &lines, caps(100, 1_600)),
            vec![Headroom { file: CORE.to_owned(), headroom: 100 }],
            "core の余地は crates/<c>/src/ の合計で 1 回"
        );
        let only_b = read_write_set(&strings(&["crates/toy/src/b.rs", "docs/d.md"]), &base(), policy).unwrap_or_default();
        assert!(headroom_shortfalls(&only_b, &lines, caps(300, 40_000)).is_empty(), "余地の無い file を持たない行は通る");
        // 縮む面（`-`）: 満杯の a.rs を減らす便は file の余地を求めず、core の見積の本数にも数えない（新規 1 本だけ）。
        let shrink =
            read_write_set(&strings(&["-crates/toy/src/a.rs", "+crates/toy/src/new.rs"]), &base(), policy).unwrap_or_default();
        assert!(headroom_shortfalls(&shrink, &lines, caps(300, 40_000)).is_empty(), "- の a.rs は余地 100 でも M を通す");
        assert!(headroom_shortfalls(&shrink, &lines, caps(100, 1_600)).is_empty(), "core の見積は 100 × 1 本 = 100 ≤ 余地 100");
        assert_eq!(
            headroom_shortfalls(&shrink, &lines, caps(101, 1_600)),
            vec![Headroom { file: CORE.to_owned(), headroom: 100 }],
            "新規 1 本の見積 101 は core の余地 100 を超える（- を数えないだけで core は測る）"
        );
        assert_eq!(line_count("a\nb\n", 120), 2, "幅に収まる行は改行で区切った行の数");
        assert_eq!(line_count("a\nb", 120), 2, "末尾改行の有無で差を出さない");
        assert_eq!(line_count(&format!("{}\nb\n", "a".repeat(250)), 120), 4, "幅を超える行は ceil(250 ÷ 120) = 3 行");
    }

    /// 門の範囲の外（R-C4-2 は `crates/<c>/src/` 配下だけ）の fixture: 余地 50 の src・余地 0 の tests・`.rs` でない doc。
    fn outside_the_gate_range() -> (Vec<WriteSetItem>, Vec<(String, u64)>) {
        let items = vec![
            WriteSetItem::File("crates/toy/src/a.rs".to_owned()),
            WriteSetItem::File("crates/toy/tests/e2e/t.rs".to_owned()),
            WriteSetItem::File("docs/d.md".to_owned()),
        ];
        let lines = vec![("crates/toy/src/a.rs".to_owned(), 1_450), ("crates/toy/tests/e2e/t.rs".to_owned(), 2_000)];
        (items, lines)
    }

    /// 受付の余地は門（R-C4-2）と同じ範囲だけを測る: `tests/` の歯は行数が上限を超えていても名指さない。
    #[test]
    fn declaration_headroom_ignores_files_outside_the_gate_range() {
        let (items, lines) = outside_the_gate_range();
        let caps = Caps { file_lines: 1_500, core_lines: 40_000, size_lines: 100 };
        assert_eq!(
            headroom_shortfalls(&items, &lines, caps),
            vec![Headroom { file: "crates/toy/src/a.rs".to_owned(), headroom: 50 }],
            "名指すのは src の a.rs だけ（tests/e2e/t.rs は門の対象外）"
        );
    }

    /// core の見積の本数も門の範囲だけ: src 1 本 + tests 1 本 + doc の write-set は `size_lines × 1`（tests/ の歯を
    /// 本数に入れると 2 本で余地を超え、src だけなら通る便を受付が断る＝.303 run 3 の型）。
    #[test]
    fn declaration_headroom_core_estimate_counts_only_files_in_the_gate_range() {
        let (items, _) = outside_the_gate_range();
        let lines = vec![("crates/toy/src/a.rs".to_owned(), 100), ("crates/toy/tests/e2e/t.rs".to_owned(), 2_000)];
        let caps = |core_lines: u64| Caps { file_lines: 1_500, core_lines, size_lines: 100 };
        assert!(
            headroom_shortfalls(&items, &lines, caps(250)).is_empty(),
            "core の見積は src 1 本 × 100 = 100 ≤ 余地 150（tests/e2e/t.rs は本数に入れない）"
        );
        assert_eq!(
            headroom_shortfalls(&items, &lines, caps(150)),
            vec![Headroom { file: CORE.to_owned(), headroom: 50 }],
            "余地 50 では src 1 本の見積 100 が超える（見積が 0 に潰れていない）"
        );
    }

    /// 余地を測る file の集合 = `core_of` が `Some` の file の集合（xtask の file-lines と同じ範囲）。
    #[test]
    fn declaration_headroom_range_matches_the_gate_predicate() {
        let (items, lines) = outside_the_gate_range();
        // 余地を必ず超える見積で、測られた file が全部名指される形にする（core は余裕）。
        let caps = Caps { file_lines: 1_500, core_lines: u64::MAX, size_lines: 1_501 };
        let measured: Vec<String> =
            headroom_shortfalls(&items, &lines, caps).into_iter().map(|found| found.file).collect();
        let in_range: Vec<String> = items
            .iter()
            .filter_map(|item| match *item {
                WriteSetItem::File(ref path) | WriteSetItem::New(ref path) => {
                    super::core_of(path).map(|_| path.clone())
                }
                WriteSetItem::Dir(_) | WriteSetItem::Shrink(_) => None,
            })
            .collect();
        assert_eq!(measured, in_range, "余地を測る範囲は core_of の述語と同じ");
        assert_eq!(measured, strings(&["crates/toy/src/a.rs"]), "範囲は空でない（空虚な一致を断つ）");
    }
}
