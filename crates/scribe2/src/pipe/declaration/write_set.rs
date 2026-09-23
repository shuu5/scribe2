//! write-set の項目の読みと上限の余地（設計 docs/design/contract-source.md §3「項目の実在と展開」「上限の余地」・
//! §14・SRS FR48・pure）。
//!
//! 契約の write-set の各項目を base（tracked file の一覧）に対して [`read_write_set`] で読み（実在する file / 末尾 `/`
//! の dir / `+` の新規 file / `-` の縮む file / `~` の消える file / `=` の置き場だけの file の 6 形）、`.rs` の項目ごとに上限（R-C4-2 / R-C4-1）の余地を
//! [`headroom_shortfalls`] で測る。宣言（`.vessel.toml`）の読みと上限の突き合わせは親 module `declaration.rs` に
//! 置いたまま。呼び手（`pipe::table` / `pipe::cli::intake`）の `use` は親の再 export を通る。行の任意の欄 growth
//! （file ごとの見込み行数・§46）の読み手 [`WriteSetItem::read_growth`] と file の見込み [`Caps::estimate`] も同じ群に
//! 置く（親の再 export の型の関連 fn＝呼び手は型の path で引く）。

use crate::pipe::refuse::{DELETE_FILE, NEW_FILE, PLACE_ONLY_FILE, SHRINK_FILE};

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
    /// `~` 接頭辞で宣言した**着地で消える file**（接頭辞を剥がした path・設計 contract-source.md §24）。受付
    /// （[`NewFilePolicy::MustBeAbsent`]）は base に実在する file を要し（消す予定の file が無い＝宣言の誤り）、
    /// 契約表の検査（[`NewFilePolicy::MayBeLanded`]）は tracked に無ければ**着地で消えた**と読んで解く（着地済みの
    /// 行が永久に解けなくなる罠を塞ぐ）。増分は負なので [`Self::Shrink`] と同じく上限の余地を求めず、core の見積の
    /// 本数にも数えない。閉包・交差・guard・gate の照合は [`Self::File`] と同じ素の path として読む。
    Delete(String),
    /// `=` 接頭辞で宣言した**置き場だけの file**（base に実在する file・接頭辞を剥がした path・設計 contract-source.md
    /// §43 (1)・行 ar）。中身を変えず verify の置き場として載せただけなので、[`Self::Shrink`] と同じく上限の余地を
    /// 求めず core の見積の本数にも数えない。閉包・交差・guard・gate の照合は [`Self::File`] と同じ素の path として読む。
    PlaceOnly(String),
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
/// 解ける形は 6 つだけ: base に実在する file / 末尾 `/` で base に配下の file を持つ dir / `+` 接頭辞で base に**無い**
/// 新規 file / `-` 接頭辞で base に**在る**縮む file / `~` 接頭辞で base に**在る**消える file / `=` 接頭辞で base に
/// **在る**置き場だけの file。それ以外（無い file・空の dir・base に在る file への `+`・base に無い file への `-` /
/// `~` / `=`）は `Err` に項目の字面で積む。接頭辞付きの
/// 2 形だけは `policy` で読みが変わる（[`NewFilePolicy::MayBeLanded`] は base に在る `+` を [`WriteSetItem::File`] に、
/// base に無い `~` を [`WriteSetItem::Delete`]〔着地で消えた〕に解く）。
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
    if let Some(place) = item.strip_prefix(crate::pipe::refuse::PLACE_ONLY_FILE) {
        // `-` と同じ弁別（base に在る file だけ・policy を見ない＝受付と契約表の検査で読みを変えない・§43 (1)）。
        let present = !place.is_empty() && tracked.iter().any(|path| path == place);
        return present.then(|| WriteSetItem::PlaceOnly(place.to_owned()));
    }
    if let Some(gone) = item.strip_prefix(crate::pipe::refuse::DELETE_FILE) {
        // 契約表の検査は tracked に無くても解く（着地で消えた＝行の履歴・§24）。受付は在ることを要する。
        let landed = matches!(policy, NewFilePolicy::MayBeLanded);
        let resolvable = !gone.is_empty() && (landed || tracked.iter().any(|path| path == gone));
        return resolvable.then(|| WriteSetItem::Delete(gone.to_owned()));
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
    /// その file の見込み（growth に在ればその値・無ければ `size` の値・core は core に属する file の見込みの和）。
    pub estimate: u64,
}

/// growth の読めた項目（path と見込み行数・書かれた順）。
type Estimates = Vec<(String, u64)>;

/// growth の崩れた項目（項目の字面と理由の 1 行・書かれた順）。
type Unfit = Vec<(String, String)>;

impl WriteSetItem {
    /// 行の任意の欄 growth（file ごとの見込み行数・設計 contract-source.md §46）を write-set に対して読む（受付と
    /// 契約表の検査が撃つ 1 本・C2・呼び手は親の再 export の型から引く）。
    ///
    /// 項目は path と行数を `:` で結んだ 1 語（行数は 0 以上の整数）。path は write-set の file の項目から接頭辞を
    /// 剥がした素の path で、`+` の新規 file か素の file に限る（dir 項目は展開しない・`-` / `~` / `=` は余地を求めない
    /// ので見込みを持てない）。**崩れた項目は全件**（項目の字面と理由）を返す: 形でない・write-set に無い・`.rs` で
    /// ない・`-` / `~` / `=` の項目・同じ path が 2 回。
    pub fn read_growth(growth: &[String], write_set: &[String]) -> Result<Estimates, Unfit> {
        let (mut found, mut unfit): (Estimates, Unfit) = (Vec::new(), Vec::new());
        for item in growth {
            match growth_item(item, write_set, &found) {
                Ok(pair) => found.push(pair),
                Err(reason) => unfit.push((item.clone(), reason)),
            }
        }
        if unfit.is_empty() {
            Ok(found)
        } else {
            Err(unfit)
        }
    }
}

/// growth の 1 項目を読む（`seen` は先に読めた項目・崩れは理由の 1 行）。
fn growth_item(item: &str, write_set: &[String], seen: &[(String, u64)]) -> Result<(String, u64), String> {
    let Some((path, count)) = item.rsplit_once(':') else {
        return Err("path と行数を : で結んだ形でない（: が無い）".to_owned());
    };
    // 符号（`+5`）は整数の parse が通すので、数字だけの字面に限ってから読む。
    let lines = count.chars().all(|found| found.is_ascii_digit()).then(|| count.parse::<u64>().ok()).flatten();
    let Some(lines) = lines else {
        return Err(format!("行数 {count:?} が 0 以上の整数でない"));
    };
    // 余地を求めない 3 つの接頭辞（縮む面・消える file・置き場だけ）。
    let exempt = [SHRINK_FILE, DELETE_FILE, PLACE_ONLY_FILE];
    let prefixes = [NEW_FILE, SHRINK_FILE, DELETE_FILE, PLACE_ONLY_FILE];
    let declared: Vec<&String> =
        write_set.iter().filter(|entry| entry.strip_prefix(prefixes).unwrap_or(entry) == path).collect();
    if declared.is_empty() {
        return Err("path が write-set の file の項目（接頭辞を剥がした素の path）に無い".to_owned());
    }
    if !path.ends_with(".rs") {
        return Err("path が .rs でない".to_owned());
    }
    if declared.iter().any(|entry| entry.starts_with(exempt)) {
        return Err("path が - / ~ / = の項目（余地を求めない面は見込みを持てない）".to_owned());
    }
    if seen.iter().any(|(earlier, _)| earlier == path) {
        return Err("同じ path が 2 回在る".to_owned());
    }
    Ok((path.to_owned(), lines))
}

impl Caps {
    /// file 1 本の見込み（`growth` に在ればその値・無ければ `size_lines`・設計 contract-source.md §46）。受付の判定と
    /// preflight の `headroom=` の見積が同じこの 1 本を読む。
    pub fn estimate(&self, path: &str, growth: &[(String, u64)]) -> u64 {
        growth.iter().find(|(named, _)| named == path).map_or(self.size_lines, |(_, lines)| *lines)
    }
}

/// core の合計を名指す `file` の字面。
pub const CORE: &str = "core";

/// 行数（xtask check の file-lines / core-lines と同じ式 = 幅 `width` で正規化した行数・[`crate::pipe::closure::weighted_lines`]）。
pub fn line_count(text: &str, width: u64) -> u64 {
    let width = usize::try_from(width).unwrap_or(usize::MAX);
    u64::try_from(crate::pipe::closure::weighted_lines(text, width)).unwrap_or(u64::MAX)
}

/// base の tracked `.rs` 1 本の行数の 2 面（幅で正規化・[`line_count`]）。file の余地（R-C4-2）は全体で、core の
/// 合計（R-C4-1）は本体だけで数える（xtask check の file-lines / core-lines と同じ切り方・設計 core-boundary.md §2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileLines {
    /// repo 相対 path。
    pub path: String,
    /// file 全体の行数（xtask の file-lines と同じ式・R-C4-2 の余地の分母）。
    pub total: u64,
    /// 本体の行数（最初の行頭 `#[cfg(test)]` より前・[`crate::pipe::closure::src_region`]・xtask の core-lines と
    /// 同じ式＝in-file の歯は R-C4-3 が数える側で core の合計に入れない）。
    pub src: u64,
}

impl FileLines {
    /// 本文から 2 面を数える（式はここ 1 か所・xtask と crate は互いに依存しないので同じ fixture の歯が一致を守る）。
    pub fn of(path: &str, text: &str, width: u64) -> Self {
        Self {
            path: path.to_owned(),
            total: line_count(text, width),
            src: line_count(crate::pipe::closure::src_region(text), width),
        }
    }
}

/// 上限の余地を測る（**受付だけが撃つ**・pure・I/O は呼び手）。
///
/// `lines` は base の tracked `.rs` の行数（[`FileLines`]・全体と本体の 2 面）。write-set の `.rs`（dir は展開した配下・
/// 新規 file は 0 行）のうち R-C4-2 の測定範囲（`crates/<c>/src/` 配下＝[`core_of`] が `Some`）のそれぞれについて
/// `file_lines − 全体の行数` を余地とし、file の見込み（[`Caps::estimate`]: `growth` に在ればその値・無ければ
/// `size_lines`・§46）が余地を超える file を名指す（範囲外の `tests/` 等は門の対象外で測らない）。core（write-set の
/// `.rs` が在る `crates/<c>/src/` の**本体**の総行数＝in-file の歯を除く・xtask の core-lines と同じ母集団）は
/// その core に属する write-set の `.rs` ごとの見込みの**和**を見積として同じ式で 1 回（母集団は file の余地と同じ
/// [`core_of`] が `Some` の集合＝`tests/` の歯は本数に入れない・C10）。
/// **縮む面（`-`）と消える file（`~`）は増分が負**なので、file の余地も求めず core の本数にも数えない（満杯の
/// file を割る便を受付が断って満杯が固定される型を塞ぐ・§3「上限の余地」・§24）。**置き場だけの file（`=`）** は
/// 増分 0 なので同じ腕（§43 (1)）。
pub fn headroom_shortfalls(items: &[WriteSetItem], lines: &[FileLines], growth: &[(String, u64)], caps: Caps) -> Vec<Headroom> {
    let files: Vec<&str> = items
        .iter()
        .flat_map(|item| match *item {
            WriteSetItem::File(ref path) | WriteSetItem::New(ref path) => vec![path.as_str()],
            WriteSetItem::Dir(ref under) => under.iter().map(String::as_str).collect(),
            WriteSetItem::Shrink(_) | WriteSetItem::Delete(_) | WriteSetItem::PlaceOnly(_) => Vec::new(),
        })
        .filter(|path| path.ends_with(".rs"))
        .collect();
    let lines_of = |path: &str| lines.iter().find(|found| found.path == path).map_or(0, |found| found.total);
    let estimate = |path: &str| caps.estimate(path, growth);
    let mut found: Vec<Headroom> = files
        .iter()
        .filter(|path| core_of(path).is_some())
        .filter_map(|path| {
            let headroom = caps.file_lines.saturating_sub(lines_of(path));
            let estimate = estimate(path);
            (estimate > headroom).then(|| Headroom { file: (*path).to_owned(), headroom, estimate })
        })
        .collect();
    let mut cores: Vec<&str> = files.iter().filter_map(|path| core_of(path)).collect();
    cores.sort_unstable();
    cores.dedup();
    for core in cores {
        let members = files.iter().filter(|path| core_of(path) == Some(core));
        let estimate = members.fold(0_u64, |sum, path| sum.saturating_add(estimate(path)));
        // core の合計は本体だけ（in-file の歯を除く＝xtask の core-lines と同じ母集団）。
        let total: u64 = lines.iter().filter(|found| core_of(&found.path) == Some(core)).map(|found| found.src).sum();
        let headroom = caps.core_lines.saturating_sub(total);
        if estimate > headroom {
            found.push(Headroom { file: CORE.to_owned(), headroom, estimate });
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
    use super::{headroom_shortfalls, line_count, read_write_set, Caps, FileLines, Headroom, NewFilePolicy, WriteSetItem, CORE};

    /// base の tracked file（write-set の項目の fixture）。
    fn base() -> Vec<String> {
        strings(&["crates/toy/src/a.rs", "crates/toy/src/b.rs", "snap/x.snap", "docs/d.md", "crates/toy/tests/t.rs"])
    }

    /// 歯を持たない file の行数（全体 = 本体）の列。
    fn whole(rows: &[(&str, u64)]) -> Vec<FileLines> {
        rows.iter().map(|(path, count)| FileLines { path: (*path).to_owned(), total: *count, src: *count }).collect()
    }

    /// 歯と本体を持つ file の fixture。**xtask の `check_sizes::tests` / `workspace::tests` の歯と同じ字面・同じ値**
    /// （幅 10 で test 6 / src 4・全体 10）＝2 crate の切り方の一致を守る。
    const SPLIT_FIXTURE: &str = "fn a() {}\nabcdefghijklmnopqrstuvwxy\n#[cfg(test)]\nmod t {}\nabcdefghijklmnopqrstuvwxy\n";

    /// 歯を持たない file の fixture（幅 10 で 3 行・全部本体）。
    const BARE_FIXTURE: &str = "abcdefghijklmnopqrstuvwxy\n";

    /// 行数の 2 面（[`FileLines::of`]）は xtask の core-lines / file-lines と同じ切り方: 同じ fixture が幅 10 で
    /// 全体 10 / 本体 4（xtask の split の歯は (test, src) = (6, 4)）、幅 120 で全体 5 / 本体 2、歯の無い file は
    /// 全体 = 本体 = 3。行頭でない `#[cfg(test)]`（字下げ）は印でない・file 先頭の印は本体 0。
    #[test]
    fn pipe_intake_core_headroom_src_side_matches_the_xtask_split_fixture() {
        let split = FileLines::of("crates/toy/src/x.rs", SPLIT_FIXTURE, 10);
        assert_eq!(split, FileLines { path: "crates/toy/src/x.rs".to_owned(), total: 10, src: 4 }, "(全体, 本体) = (6 + 4, 1 + 3)");
        let wide = FileLines::of("crates/toy/src/x.rs", SPLIT_FIXTURE, 120);
        assert_eq!((wide.total, wide.src), (5, 2), "幅が広ければ改行の数");
        let bare = FileLines::of("crates/toy/src/y.rs", BARE_FIXTURE, 10);
        assert_eq!((bare.total, bare.src), (3, 3), "歯の無い file は全部本体");
        let indented = FileLines::of("crates/toy/src/z.rs", "fn a() {}\n    #[cfg(test)]\nfn b() {}\n", 120);
        assert_eq!((indented.total, indented.src), (3, 3), "字下げの印は本体を切らない");
        let leading = FileLines::of("crates/toy/src/w.rs", "#[cfg(test)]\nmod t {}\n", 120);
        assert_eq!((leading.total, leading.src), (2, 0), "先頭の印は本体 0");
    }

    /// core の合計は**本体**だけで、file の余地は**全体**で測る（同じ file の 2 面が別々に効く）: 本体 1000 / 全体 1399 の
    /// file を持つ base で、上限 1450 の core の余地は 450（全体で数えれば 51）＝S の新規 1 本（100）は入り、同じ file
    /// への M（300）は file の余地 101 で断られる（file の余地まで本体で数える実装は 500 で通してしまう）。
    #[test]
    fn pipe_intake_core_headroom_counts_the_core_total_by_src_side_and_the_file_by_whole() {
        let lines = vec![FileLines { path: "crates/toy/src/a.rs".to_owned(), total: 1_399, src: 1_000 }];
        let policy = NewFilePolicy::MustBeAbsent;
        let fresh = read_write_set(&strings(&["+crates/toy/src/new.rs"]), &base(), policy).unwrap_or_default();
        let caps = |size_lines: u64, core_lines: u64| Caps { file_lines: 1_500, core_lines, size_lines };
        assert!(headroom_shortfalls(&fresh, &lines, &[], caps(100, 1_450)).is_empty(), "本体 1000 → 余地 450 に S の 1 本は入る");
        assert_eq!(
            headroom_shortfalls(&fresh, &lines, &[], caps(100, 1_099)),
            vec![Headroom { file: CORE.to_owned(), headroom: 99, estimate: 100 }],
            "余地は本体から数える（全体なら 0 でなく 99）"
        );
        let same = read_write_set(&strings(&["crates/toy/src/a.rs"]), &base(), policy).unwrap_or_default();
        assert_eq!(
            headroom_shortfalls(&same, &lines, &[], caps(300, 40_000)),
            vec![Headroom { file: "crates/toy/src/a.rs".to_owned(), headroom: 101, estimate: 300 }],
            "file の余地は全体 1399 から（本体なら 500 で M が通る）"
        );
    }

    /// write-set の項目は 5 形（実在する file / 末尾 `/` で配下を持つ dir〔展開される〕/ `+` の新規 file〔base に無い〕/
    /// `-` の縮む file〔base に在る〕/ `~` の消える file〔受付の場面は base に在る・§24〕）だけが解け、それ以外は
    /// **全件**項目の字面で返る（設計 contract-source.md §3「項目の実在と展開」）。
    #[test]
    fn declaration_write_set_items_resolve_only_the_three_forms_and_name_every_unresolved_item() {
        let read = read_write_set(
            &strings(&["crates/toy/src/a.rs", "snap/", "+crates/toy/src/new.rs", "-crates/toy/src/b.rs", "~docs/d.md"]),
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
                WriteSetItem::Delete("docs/d.md".to_owned()),
            ]),
            "5 形が解け dir は配下に展開され、- と ~ は接頭辞を剥がした path で持つ"
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
                "~crates/toy/src/none.rs",
                "~",
                "docs/d.md",
            ]),
            &base(),
            NewFilePolicy::MustBeAbsent,
        );
        assert_eq!(
            unresolved,
            Err(strings(&[
                "crates/toy/src/none.rs",
                "empty/",
                "+crates/toy/src/a.rs",
                "+",
                "snap",
                "-crates/toy/src/none.rs",
                "-",
                "~crates/toy/src/none.rs",
                "~",
            ])),
            "無い file・空の dir・base に在る file への +・空の +・base に無い file への - / ~・空の - / ~ は解けない（末尾 / 無しの dir も file としては無い）"
        );
    }

    /// `~` の 2 場面（§24）の表: tracked / untracked × [`NewFilePolicy`] の 4 組。違うのは「untracked な `~`」の
    /// 1 組だけ（受付は解けない＝消す予定の file が無い・契約表の検査は着地で消えたと読んで [`WriteSetItem::Delete`]
    /// に解く）。tracked な `~` はどちらの場面でも `Delete`・空の `~` はどちらでも解けない。
    #[test]
    fn declaration_write_set_delete_resolves_without_the_file_only_for_the_landed_policy() {
        let (present, landed) = ("~crates/toy/src/a.rs", "~crates/toy/src/gone.rs");
        let delete = |path: &str| Ok(vec![WriteSetItem::Delete(path.to_owned())]);
        let table = [
            (NewFilePolicy::MustBeAbsent, present, delete("crates/toy/src/a.rs")),
            (NewFilePolicy::MayBeLanded, present, delete("crates/toy/src/a.rs")),
            (NewFilePolicy::MustBeAbsent, landed, Err(strings(&[landed]))),
            (NewFilePolicy::MayBeLanded, landed, delete("crates/toy/src/gone.rs")),
        ];
        for (policy, item, want) in table {
            assert_eq!(read_write_set(&strings(&[item]), &base(), policy), want, "{policy:?} × {item}");
        }
        for policy in [NewFilePolicy::MustBeAbsent, NewFilePolicy::MayBeLanded] {
            assert_eq!(read_write_set(&strings(&["~"]), &base(), policy), Err(strings(&["~"])), "空の ~ は {policy:?} でも解けない");
        }
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

    /// 置き場だけの `=`（§43 (1)・行 ar）: base に在る file にだけ解け（2 場面とも同じ読み）、無い file・空の `=` は従来の
    /// 未解決として字面で返る。解けた `=` は余地の母集団に入らず core の見積の本数にも数えない——同じ fixture の素の
    /// path の項目は従来どおり入る（母集団 = 項目の本数と対で数える）。
    #[test]
    fn contract_place_only_item_resolves_only_in_base_and_asks_no_headroom() {
        for policy in [NewFilePolicy::MustBeAbsent, NewFilePolicy::MayBeLanded] {
            let read = read_write_set(&strings(&["=crates/toy/src/a.rs"]), &base(), policy);
            assert_eq!(read, Ok(vec![WriteSetItem::PlaceOnly("crates/toy/src/a.rs".to_owned())]), "{policy:?} で base の = は解ける");
            let missing = read_write_set(&strings(&["=crates/toy/src/none.rs", "="]), &base(), policy);
            assert_eq!(missing, Err(strings(&["=crates/toy/src/none.rs", "="])), "{policy:?} で無い file・空の = は解けない");
        }
        let lines = whole(&[("crates/toy/src/a.rs", 1_400), ("crates/toy/src/b.rs", 1_400)]);
        let caps = |size_lines: u64, core_lines: u64| Caps { file_lines: 1_500, core_lines, size_lines };
        let policy = NewFilePolicy::MustBeAbsent;
        let mixed = read_write_set(&strings(&["=crates/toy/src/a.rs", "crates/toy/src/b.rs"]), &base(), policy).unwrap_or_default();
        assert_eq!(mixed.len(), 2, "母集団は 2 項目");
        assert_eq!(
            headroom_shortfalls(&mixed, &lines, &[], caps(300, 40_000)),
            vec![Headroom { file: "crates/toy/src/b.rs".to_owned(), headroom: 100, estimate: 300 }],
            "余地 100 の 2 file のうち名指すのは素の b.rs だけ（= の a.rs は余地を求めない）"
        );
        // core: 合計 2800・上限 2900（余地 100）・S の見積は = を数えない 1 本 × 100 で入り、2 本なら 200 で超える。
        assert!(headroom_shortfalls(&mixed, &lines, &[], caps(100, 2_900)).is_empty(), "core の見積は素の 1 本だけ");
        let plain = read_write_set(&strings(&["crates/toy/src/a.rs", "crates/toy/src/b.rs"]), &base(), policy).unwrap_or_default();
        assert_eq!(
            headroom_shortfalls(&plain, &lines, &[], caps(100, 2_900)),
            vec![Headroom { file: CORE.to_owned(), headroom: 100, estimate: 200 }],
            "印を外すと 2 本 × 100 = 200 が core の余地 100 を超える"
        );
    }

    /// 上限の余地: write-set の `.rs` ごとに `file_lines − 行数` を余地とし、size の見積が超える file を名指す。core
    /// （`crates/<c>/src/` の合計）は `見積 × .rs 本数` で 1 回。`.rs` でない項目と別 crate の行は数えない。
    #[test]
    fn declaration_headroom_names_the_file_and_the_core_whose_room_is_short() {
        let lines = whole(&[
            ("crates/toy/src/a.rs", 1_400),
            ("crates/toy/src/b.rs", 100),
            ("crates/other/src/z.rs", 5_000),
            ("crates/toy/tests/t.rs", 900),
        ]);
        let policy = NewFilePolicy::MustBeAbsent;
        let items = read_write_set(&strings(&["crates/toy/src/a.rs", "snap/", "+crates/toy/src/new.rs"]), &base(), policy)
            .unwrap_or_default();
        let caps = |size_lines: u64, core_lines: u64| Caps { file_lines: 1_500, core_lines, size_lines };
        assert_eq!(
            headroom_shortfalls(&items, &lines, &[], caps(300, 40_000)),
            vec![Headroom { file: "crates/toy/src/a.rs".to_owned(), headroom: 100, estimate: 300 }],
            "M（300）は余地 100 の a.rs に入らない・新規 file は余地いっぱい・core は余裕"
        );
        assert!(headroom_shortfalls(&items, &lines, &[], caps(100, 40_000)).is_empty(), "S（100）は余地 100 に入る");
        // core: 合計 1500（tests/ と別 crate は数えない）・見積 = 100 × 2 本 = 200 > 余地 100。
        assert_eq!(
            headroom_shortfalls(&items, &lines, &[], caps(100, 1_600)),
            vec![Headroom { file: CORE.to_owned(), headroom: 100, estimate: 200 }],
            "core の余地は crates/<c>/src/ の合計で 1 回"
        );
        let only_b = read_write_set(&strings(&["crates/toy/src/b.rs", "docs/d.md"]), &base(), policy).unwrap_or_default();
        assert!(headroom_shortfalls(&only_b, &lines, &[], caps(300, 40_000)).is_empty(), "余地の無い file を持たない行は通る");
        // 縮む面（`-`）: 満杯の a.rs を減らす便は file の余地を求めず、core の見積の本数にも数えない（新規 1 本だけ）。
        let shrink =
            read_write_set(&strings(&["-crates/toy/src/a.rs", "+crates/toy/src/new.rs"]), &base(), policy).unwrap_or_default();
        assert!(headroom_shortfalls(&shrink, &lines, &[], caps(300, 40_000)).is_empty(), "- の a.rs は余地 100 でも M を通す");
        assert!(headroom_shortfalls(&shrink, &lines, &[], caps(100, 1_600)).is_empty(), "core の見積は 100 × 1 本 = 100 ≤ 余地 100");
        assert_eq!(
            headroom_shortfalls(&shrink, &lines, &[], caps(101, 1_600)),
            vec![Headroom { file: CORE.to_owned(), headroom: 100, estimate: 101 }],
            "新規 1 本の見積 101 は core の余地 100 を超える（- を数えないだけで core は測る）"
        );
        // 消える file（`~`・§24）も増分は負＝`-` と同じ扱い（余地も本数も数えない）。
        let doomed =
            read_write_set(&strings(&["~crates/toy/src/a.rs", "+crates/toy/src/new.rs"]), &base(), policy).unwrap_or_default();
        assert!(headroom_shortfalls(&doomed, &lines, &[], caps(300, 40_000)).is_empty(), "~ の a.rs は余地 100 でも M を通す");
        assert!(headroom_shortfalls(&doomed, &lines, &[], caps(100, 1_600)).is_empty(), "core の見積は 100 × 1 本 = 100 ≤ 余地 100");
        assert_eq!(line_count("a\nb\n", 120), 2, "幅に収まる行は改行で区切った行の数");
        assert_eq!(line_count("a\nb", 120), 2, "末尾改行の有無で差を出さない");
        assert_eq!(line_count(&format!("{}\nb\n", "a".repeat(250)), 120), 4, "幅を超える行は ceil(250 ÷ 120) = 3 行");
    }

    /// 門の範囲の外（R-C4-2 は `crates/<c>/src/` 配下だけ）の fixture: 余地 50 の src・余地 0 の tests・`.rs` でない doc。
    fn outside_the_gate_range() -> (Vec<WriteSetItem>, Vec<FileLines>) {
        let items = vec![
            WriteSetItem::File("crates/toy/src/a.rs".to_owned()),
            WriteSetItem::File("crates/toy/tests/e2e/t.rs".to_owned()),
            WriteSetItem::File("docs/d.md".to_owned()),
        ];
        let lines = whole(&[("crates/toy/src/a.rs", 1_450), ("crates/toy/tests/e2e/t.rs", 2_000)]);
        (items, lines)
    }

    /// 受付の余地は門（R-C4-2）と同じ範囲だけを測る: `tests/` の歯は行数が上限を超えていても名指さない。
    #[test]
    fn declaration_headroom_ignores_files_outside_the_gate_range() {
        let (items, lines) = outside_the_gate_range();
        let caps = Caps { file_lines: 1_500, core_lines: 40_000, size_lines: 100 };
        assert_eq!(
            headroom_shortfalls(&items, &lines, &[], caps),
            vec![Headroom { file: "crates/toy/src/a.rs".to_owned(), headroom: 50, estimate: 100 }],
            "名指すのは src の a.rs だけ（tests/e2e/t.rs は門の対象外）"
        );
    }

    /// core の見積の本数も門の範囲だけ: src 1 本 + tests 1 本 + doc の write-set は `size_lines × 1`（tests/ の歯を
    /// 本数に入れると 2 本で余地を超え、src だけなら通る便を受付が断る＝.303 run 3 の型）。
    #[test]
    fn declaration_headroom_core_estimate_counts_only_files_in_the_gate_range() {
        let (items, _) = outside_the_gate_range();
        let lines = whole(&[("crates/toy/src/a.rs", 100), ("crates/toy/tests/e2e/t.rs", 2_000)]);
        let caps = |core_lines: u64| Caps { file_lines: 1_500, core_lines, size_lines: 100 };
        assert!(
            headroom_shortfalls(&items, &lines, &[], caps(250)).is_empty(),
            "core の見積は src 1 本 × 100 = 100 ≤ 余地 150（tests/e2e/t.rs は本数に入れない）"
        );
        assert_eq!(
            headroom_shortfalls(&items, &lines, &[], caps(150)),
            vec![Headroom { file: CORE.to_owned(), headroom: 50, estimate: 100 }],
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
            headroom_shortfalls(&items, &lines, &[], caps).into_iter().map(|found| found.file).collect();
        let in_range: Vec<String> = items
            .iter()
            .filter_map(|item| match *item {
                WriteSetItem::File(ref path) | WriteSetItem::New(ref path) => {
                    super::core_of(path).map(|_| path.clone())
                }
                WriteSetItem::Dir(_) | WriteSetItem::Shrink(_) | WriteSetItem::Delete(_) | WriteSetItem::PlaceOnly(_) => None,
            })
            .collect();
        assert_eq!(measured, in_range, "余地を測る範囲は core_of の述語と同じ");
        assert_eq!(measured, strings(&["crates/toy/src/a.rs"]), "範囲は空でない（空虚な一致を断つ）");
    }

    /// (c) 余地の境界（§31）: 見積が余地を**超える**ときだけ断る。file の余地 = 見積（ちょうど）は通り、余地 = 見積 −1
    /// は名指される（`>` を `>=` にするとちょうどの便が断られる）。core の余地も同じ両側で測る。
    #[test]
    fn contract_closure_ext_survivor_c_headroom_refuses_only_above_the_room() {
        // flip-check: retroactive s2-07l.277
        let items = vec![WriteSetItem::File("crates/toy/src/a.rs".to_owned())];
        let lines = whole(&[("crates/toy/src/a.rs", 1_400)]);
        let caps = |size_lines: u64, core_lines: u64| Caps { file_lines: 1_500, core_lines, size_lines };
        let exact = headroom_shortfalls(&items, &lines, &[], caps(100, 40_000));
        assert!(exact.is_empty(), "file の余地 100 = 見積 100 は通る（断り {} 件 / 母集団 1 file）", exact.len());
        let over = headroom_shortfalls(&items, &lines, &[], caps(101, 40_000));
        assert_eq!(
            over,
            vec![Headroom { file: "crates/toy/src/a.rs".to_owned(), headroom: 100, estimate: 101 }],
            "file の余地 100 = 見積 101 −1 は断る（断り {} 件 / 母集団 1 file）",
            over.len()
        );
        let core_exact = headroom_shortfalls(&items, &lines, &[], caps(100, 1_500));
        assert!(core_exact.is_empty(), "core の余地 100 = 見積 100 は通る（断り {} 件 / 母集団 1 core）", core_exact.len());
        let core_over = headroom_shortfalls(&items, &lines, &[], caps(100, 1_499));
        assert_eq!(
            core_over,
            vec![Headroom { file: CORE.to_owned(), headroom: 99, estimate: 100 }],
            "core の余地 99 = 見積 100 −1 は断る（断り {} 件 / 母集団 1 core）",
            core_over.len()
        );
    }

    /// (d) core の名の切り出し（§31）: crate 名が空でなく、かつ `/` を含まないときだけ `crates/<c>/src` と読む
    /// （`&&` を `||` にすると `crates//src/` と `crates/a/b/src/` が core として通る）。
    #[test]
    fn contract_closure_ext_survivor_d_core_of_needs_a_single_nonempty_crate_segment() {
        // flip-check: retroactive s2-07l.277
        let table = [
            ("crates/toy/src/a.rs", Some("crates/toy/src")),
            ("crates//src/a.rs", None),
            ("crates/a/b/src/a.rs", None),
        ];
        let got: Vec<(&str, Option<&str>)> = table.iter().map(|(path, _)| (*path, super::core_of(path))).collect();
        let hits = got.iter().zip(&table).filter(|(found, want)| found == want).count();
        assert_eq!(got, table, "core と読む形は 1 つだけ（一致 {hits} 件 / 母集団 {} 形）", table.len());
    }

    /// growth の見込みの組（path と行数）。
    fn grown(pairs: &[(&str, u64)]) -> Vec<(String, u64)> {
        pairs.iter().map(|(path, lines)| ((*path).to_owned(), *lines)).collect()
    }

    /// §46 (1): growth に在る file はその値で余地と比べ、無い file は `size` で比べる。余地 100 の 2 file で、`size` 300 が
    /// 余地を超えても growth 50 の a.rs は通り（名指すのは growth の無い b.rs だけ・見込み 300）、`size` 100 が入っても
    /// growth 150 の a.rs は断る（見込み 150・b.rs は `size` で入る）。growth の無い周は 2 file とも `size` で断る。
    #[test]
    fn declaration_growth_file_estimate_overrides_size_only_for_the_named_file() {
        let lines = whole(&[("crates/toy/src/a.rs", 1_400), ("crates/toy/src/b.rs", 1_400)]);
        let items = read_write_set(&strings(&["crates/toy/src/a.rs", "crates/toy/src/b.rs"]), &base(), NewFilePolicy::MustBeAbsent)
            .unwrap_or_default();
        let caps = |size_lines: u64| Caps { file_lines: 1_500, core_lines: 40_000, size_lines };
        let short = |file: &str, estimate: u64| Headroom { file: file.to_owned(), headroom: 100, estimate };
        let (a, b) = ("crates/toy/src/a.rs", "crates/toy/src/b.rs");
        assert_eq!(
            headroom_shortfalls(&items, &lines, &grown(&[(a, 50)]), caps(300)),
            vec![short(b, 300)],
            "size が余地を超えても growth が入る a.rs は通る"
        );
        assert_eq!(
            headroom_shortfalls(&items, &lines, &grown(&[(a, 150)]), caps(100)),
            vec![short(a, 150)],
            "size が入っても growth が余地を超える a.rs は断る"
        );
        assert_eq!(headroom_shortfalls(&items, &lines, &[], caps(300)), vec![short(a, 300), short(b, 300)], "growth が無ければ size");
        assert_eq!(caps(300).estimate(a, &grown(&[(a, 0)])), 0, "growth 0 は 0 の見込み（size に戻らない）");
        assert_eq!(caps(300).estimate(b, &grown(&[(a, 0)])), 300, "名指されない file は size");
    }

    /// §46 (1): core の見積は core に属する file の見込みの**和**（`size` × 本数と値が割れる fixture で A/B）。本体 200 の
    /// core に 3 本（a.rs 10・b.rs 20・新規 file は `size` 300）の和 330 は余地 330 に入り、`size` × 3 = 900 なら断る。
    /// 余地 329 では和 330 が超えて core を名指す（見込みは 330）。
    #[test]
    fn declaration_growth_core_estimate_is_the_sum_of_file_estimates() {
        let lines = whole(&[("crates/toy/src/a.rs", 100), ("crates/toy/src/b.rs", 100)]);
        let write_set = strings(&["crates/toy/src/a.rs", "crates/toy/src/b.rs", "+crates/toy/src/new.rs"]);
        let items = read_write_set(&write_set, &base(), NewFilePolicy::MustBeAbsent).unwrap_or_default();
        let growth = grown(&[("crates/toy/src/a.rs", 10), ("crates/toy/src/b.rs", 20)]);
        let caps = |core_lines: u64| Caps { file_lines: 1_500, core_lines, size_lines: 300 };
        assert!(headroom_shortfalls(&items, &lines, &growth, caps(530)).is_empty(), "和 10 + 20 + 300 = 330 は余地 330 に入る");
        assert_eq!(
            headroom_shortfalls(&items, &lines, &[], caps(530)),
            vec![Headroom { file: CORE.to_owned(), headroom: 330, estimate: 900 }],
            "growth の無い周は size × 3 本 = 900"
        );
        assert_eq!(
            headroom_shortfalls(&items, &lines, &growth, caps(529)),
            vec![Headroom { file: CORE.to_owned(), headroom: 329, estimate: 330 }],
            "和 330 は余地 329 を超える"
        );
    }

    /// §46 (4): growth の読み手は正しい項目を (path, 行数) に分け、崩れた項目を**全件**（字面と理由）返す: (a) `:` が無い・
    /// 行数が整数でない（符号付きも）(b) write-set の file の項目に無い（dir 項目は展開しない）(c) `.rs` でない (d) `-` /
    /// `~` / `=` の項目 (e) 同じ path が 2 回。
    #[test]
    fn declaration_growth_reader_names_every_unfit_item() {
        let write_set = strings(&[
            "crates/toy/src/a.rs",
            "+crates/toy/src/new.rs",
            "-crates/toy/src/b.rs",
            "~crates/toy/src/c.rs",
            "=crates/toy/src/d.rs",
            "docs/d.md",
            "crates/toy/src/",
        ]);
        let good = strings(&["crates/toy/src/a.rs:10", "crates/toy/src/new.rs:0"]);
        assert_eq!(
            WriteSetItem::read_growth(&good, &write_set),
            Ok(grown(&[("crates/toy/src/a.rs", 10), ("crates/toy/src/new.rs", 0)])),
            "素の file と + の新規 file は path と行数に分かれる"
        );
        let cases = [
            ("crates/toy/src/a.rs", ": が無い"),
            ("crates/toy/src/a.rs:x", "0 以上の整数でない"),
            ("crates/toy/src/a.rs:+5", "0 以上の整数でない"),
            ("crates/toy/src/none.rs:5", "write-set の file の項目"),
            ("crates/toy/src/x.rs:5", "write-set の file の項目"),
            ("docs/d.md:5", ".rs でない"),
            ("crates/toy/src/b.rs:5", "- / ~ / ="),
            ("crates/toy/src/c.rs:5", "- / ~ / ="),
            ("crates/toy/src/d.rs:5", "- / ~ / ="),
            ("crates/toy/src/a.rs:20", "2 回"),
        ];
        let mut items = good.clone();
        items.extend(cases.iter().map(|(item, _)| (*item).to_owned()));
        let unfit = WriteSetItem::read_growth(&items, &write_set).expect_err("崩れた項目が在る");
        let named: Vec<&str> = unfit.iter().map(|(item, _)| item.as_str()).collect();
        let want: Vec<&str> = cases.iter().map(|(item, _)| *item).collect();
        assert_eq!(named, want, "崩れた項目を全件・書かれた順に（正しい 2 項目は名指さない）");
        for ((item, reason), (_, needle)) in unfit.iter().zip(cases) {
            assert!(reason.contains(needle), "{item} は {needle} を名乗る: {reason}");
        }
    }
}
