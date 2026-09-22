//! 純移動の機械証明（設計 docs/design/pipeline.md §5.3・`s2-07l.266`・FR9 / NFR1 / FR5）。
//!
//! 便の diff が**純移動**——(名, 本文の hash) の多重集合が base と HEAD で一致し、移動した item が
//! 1 つ以上在り、残差分が宣言と札とコメントと空行だけ——なら、lens へ diff の代わりに**要約**
//! （[`MoveSummary`]）を渡す。分割便の diff は本文を 2 度（`-` と `+`）運ぶので予算の cap に当たるが、
//! 移動の事実は要約の方が小さく正確に運べる（cap を一時的に上げた `s2-07l.265` の恒久解）。
//!
//! **判定は純関数**である。file の読み（git）は gate 側が閉じた口 [`Side`] の closure で担い、
//! ここは I/O を持たない（`lens-input.txt` の書き出しの helper [`keep`] だけが fs に触る）。
//!
//! **下界**（C13・構文木を持たない・字面走査）: 列 0 から始まる宣言単位だけを item と数え、macro が
//! 生む item・1 行に複数の item・入れ子の item（`impl {}` の中の fn・inline `mod tests {}` の歯）は外側の
//! item 1 本に畳む。読めない形は**純移動でない側**へ倒す——誤判定は lens から diff を奪う側
//! （FailOpen・PostHoc・[`POLARITY`]）なので、迷った周は従来どおり diff を渡す。

use crate::polarity::{OnFailure, Polarity, Timing};
use std::collections::BTreeMap;
use std::path::Path;

/// diff を読んで item の列にする前段の群（設計 §44・`s2-07l.499` の純移動）。
mod read;

use read::{items_of, parse_diff, strip_visibility, FileDiff, Located};
// 歯（親の `mod tests`）が `super::` で引く 3 名。親が同じ名を持たないと**移した歯の本文の path を
// 書き換える**ことになり、純移動の機械証明（設計 §5.3）の (名, 本文の hash) が動く。実装側は呼ばないので
// `cfg(test)` で借りる（`pub use` は要らない＝前段の名を `move_proof::` で引く呼び手は 0 件）。
#[cfg(test)]
use read::{declaration_of, split_git_paths, Item};

/// 純移動の証明の極性（[`LensInput`]）: 実装の後に測り、誤判定は lens から diff を奪う側（通す側）へ倒れる。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::PostHoc,
    on_failure: OnFailure::FailOpen,
};

/// run dir に残す lens の入力の写し（純移動の周だけ・事後に読める・NFR4）。
pub const LENS_INPUT_FILE: &str = "lens-input.txt";

/// run dir に残す裁定の写し（便の質問と planner の回答の対・発生順・質問の在った周だけ・`s2-07l.309`）。
///
/// gate が event log から写し、lens は契約の写しの**隣**（同じ dir）から同じ名で読む。無いことが
/// 「裁定なし」である（C10・空の file を書かない）。
pub const RULINGS_FILE: &str = "rulings.txt";

/// 要約の先頭行（雛形 `lens.txt` の `{diff}` の穴に入る本文が diff でないことを名乗る）。
const HEADLINE: &str = "これは diff ではなく純移動の要約である";

/// 純移動の札（残差分に許す `// flip-check:` の形は **moved** だけ・lens v2 medium）。
const MOVED_MARK: &str = "// flip-check: moved ";

/// `// flip-check:` で始まる札の共通の頭。
const FLIP_MARK: &str = "// flip-check:";

/// lens への入力（閉じた型・C3.3「判定入力を自由文にしない」）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LensInput {
    /// 従来の diff（純移動でない周・理由を伴う）。
    Diff(NotPure),
    /// 純移動の要約（本文は雛形の `{diff}` の穴へそのまま入る）。
    Summary(MoveSummary),
}

impl LensInput {
    /// 判定行の `lens-input=` の語。
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Diff(_) => "diff",
            Self::Summary(_) => "summary",
        }
    }

    /// lens の stdin へ渡す本文（予算の照合もこの byte で行う・FR9）。
    pub fn body<'a>(&'a self, diff: &'a [u8]) -> &'a [u8] {
        match self {
            Self::Diff(_) => diff,
            Self::Summary(summary) => summary.text.as_bytes(),
        }
    }

    /// 純移動でない周に gate の stderr へ出す 1 行（typed・自由文にしない）。
    pub fn notice(&self) -> Option<String> {
        match self {
            Self::Diff(why) => Some(format!("pipe: lens-input=diff reason={}", why.as_str())),
            Self::Summary(_) => None,
        }
    }
}

/// 純移動でない理由（閉じた enum・(1e)）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotPure {
    /// diff の形か、diff に現れる file の本文を読めない。
    Unreadable,
    /// (名, 本文の hash) の多重集合が base と HEAD で一致しない（追加・削除・本文差）。
    ItemsDiffer,
    /// 移動した item が 0（宣言だけの便）。
    NothingMoved,
    /// 残差分に宣言と札とコメント以外の行が在る。
    ResidualLine,
    /// `// flip-check:` の札のうち `moved` 以外（`retroactive` 等）が残差分に在る。
    ForeignMarker,
}

/// [`NotPure`] の全 variant。
pub const NOT_PURE: &[NotPure] = &[
    NotPure::Unreadable,
    NotPure::ItemsDiffer,
    NotPure::NothingMoved,
    NotPure::ResidualLine,
    NotPure::ForeignMarker,
];

impl NotPure {
    /// stderr の判定行に載せる名（kebab）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unreadable => "unreadable",
            Self::ItemsDiffer => "items-differ",
            Self::NothingMoved => "nothing-moved",
            Self::ResidualLine => "residual-line",
            Self::ForeignMarker => "foreign-marker",
        }
    }
}

/// gate が読む側（closure の第 1 引数・rev の解決は gate が持つ）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Side {
    /// 便の base。
    Base,
    /// 便の HEAD。
    Head,
}

/// 純移動の要約（型は閉じ、本文 `text` は雛形の穴へそのまま入る）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveSummary {
    /// 描いた本文（先頭行は [`HEADLINE`]）。
    text: String,
    /// file を跨いで動いた item の HEAD 側の区間（(file, (始, 終))・1 始まり・両端含む・設計 gate-cost.md §14 約束 1）。
    ///
    /// **本文 `text` には載らない**（要約の字面は不変）。同じ file に留まった item（可視性・字下げ・コメントだけの差）は
    /// 持たない＝検出線の母集団に残る（約束 5）。
    moved: Vec<(String, (usize, usize))>,
}

impl MoveSummary {
    /// 本文。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 動いた item の HEAD 側の区間。
    pub fn moved(&self) -> &[(String, (usize, usize))] {
        &self.moved
    }
}

/// 検出線の母集団の diff（純移動の周だけ・設計 gate-cost.md §14 約束 2 / 3）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Population {
    /// 動いた item の区間の `+` 行を落とした diff（残る `+` 行を連なりごとの挿入の hunk に組む・0 本なら空）。
    pub text: String,
    /// 落とした `+` 行の本数（record の `pure-move=`）。
    pub dropped: usize,
}

/// 便の diff から、要約が名指す動いた item の区間に入る `+` 行を落とした母集団の diff を組む（**純関数**）。
///
/// 残る `+` 行は hunk の中で連なる本数ごとに `@@ -<o>,0 +<n>,<k> @@` の挿入の hunk へ切り直す——HEAD 側の行番号は
/// 元の diff のまま（道具は行番号で変異を母集団へ当てる）。`-` 行と context は運ばない（母集団は追加行だけ）。
/// 残る `+` 行の無い file は見出しごと落とす。
pub fn population(diff: &str, summary: &MoveSummary) -> Population {
    let mut out = Population { text: String::new(), dropped: 0 };
    let mut file = Cut::default();
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            file.flush(&mut out.text);
            file = Cut { header: vec![line], ..Cut::default() };
            continue;
        }
        if let Some(rest) = line.strip_prefix("@@ ") {
            file.flush(&mut out.text);
            file.cursor = hunk_cursor(rest);
            continue;
        }
        let Some(at) = file.cursor else {
            if let Some(path) = line.strip_prefix("+++ ") {
                file.head = (path != "/dev/null").then(|| path.strip_prefix("b/").unwrap_or(path).to_owned());
            }
            file.header.push(line);
            continue;
        };
        if line.starts_with('+') {
            let inside = file.head.as_ref().is_some_and(|head| {
                summary.moved.iter().any(|(path, span)| path == head && covers(*span, at.1))
            });
            if inside {
                out.dropped = out.dropped.saturating_add(1);
                file.flush(&mut out.text);
            } else {
                file.run.get_or_insert((at, Vec::new())).1.push(line);
            }
            file.cursor = Some((at.0, at.1.saturating_add(1)));
        } else if line.starts_with('\\') {
            if let Some((_, run)) = file.run.as_mut() {
                run.push(line);
            }
        } else {
            file.flush(&mut out.text);
            let old = at.0.saturating_add(1);
            let new = if line.starts_with('-') { at.1 } else { at.1.saturating_add(1) };
            file.cursor = Some((old, new));
        }
    }
    file.flush(&mut out.text);
    out
}

/// [`population`] の file 1 本の途中の状態（見出し・HEAD の path・hunk の中の位置・残す `+` 行の連なり）。
#[derive(Default)]
struct Cut<'a> {
    /// file の見出しの行（`diff --git` から最初の `@@` の前まで）。
    header: Vec<&'a str>,
    /// HEAD 側の path（`+++` の行・削除 file は `None`）。
    head: Option<String>,
    /// hunk の中の次の (base, HEAD) の行番号（hunk の外は `None`）。
    cursor: Option<(usize, usize)>,
    /// 残す `+` 行の連なり（始まりの位置と行）。
    run: Option<((usize, usize), Vec<&'a str>)>,
    /// 見出しを書いたか。
    written: bool,
}

impl Cut<'_> {
    /// 連なりを挿入の hunk 1 本として書き出す（初めての周は見出しを先に書く）。
    fn flush(&mut self, text: &mut String) {
        let Some(((old, new), lines)) = self.run.take() else { return };
        if !self.written {
            self.written = true;
            for line in &self.header {
                text.push_str(line);
                text.push('\n');
            }
        }
        let added = lines.iter().filter(|line| line.starts_with('+')).count();
        text.push_str(&format!("@@ -{},0 +{new},{added} @@\n", old.saturating_sub(1)));
        for line in lines {
            text.push_str(line);
            text.push('\n');
        }
    }
}

/// hunk 見出し `-l[,n] +l[,n] @@ …` の 2 つの開始行番号（読めない見出しは `None`＝以後の行を hunk の外と読む）。
fn hunk_cursor(rest: &str) -> Option<(usize, usize)> {
    let (range, _) = rest.split_once(" @@")?;
    let (old, new) = range.split_once(' ')?;
    let start = |token: &str, sign: char| -> Option<usize> { token.strip_prefix(sign)?.split(',').next()?.parse().ok() };
    Some((start(old, '-')?, start(new, '+')?))
}

/// 要約の本文を run dir の [`LENS_INPUT_FILE`] へ残す。
pub fn keep(run_dir: &Path, summary: &MoveSummary) -> Result<(), String> {
    let path = run_dir.join(LENS_INPUT_FILE);
    std::fs::write(&path, summary.text()).map_err(|err| format!("{} を書けない: {err}", path.display()))
}

/// diff を判定して lens への入力を決める（**純関数**・file の本文は `read` が返す）。
///
/// `read` は (側, repo 相対 path) → 本文（読めない周は `None`）。diff が新規 file と言う path の base 側・
/// 削除 file の HEAD 側は読まない（空として扱う）。
pub fn judge(diff: &str, read: &dyn Fn(Side, &str) -> Option<String>) -> LensInput {
    match prove(diff, read) {
        Ok(summary) => LensInput::Summary(summary),
        Err(why) => LensInput::Diff(why),
    }
}

/// 判定の本体（4 条件を順に見る・(1d)）。
fn prove(diff: &str, read: &dyn Fn(Side, &str) -> Option<String>) -> Result<MoveSummary, NotPure> {
    let files = parse_diff(diff)?;
    let mut base: Vec<Located> = Vec::new();
    let mut head: Vec<Located> = Vec::new();
    let mut spans: BTreeMap<(Side, String), Vec<(usize, usize)>> = BTreeMap::new();
    for file in &files {
        for (side, path) in [(Side::Base, &file.base), (Side::Head, &file.head)] {
            let Some(path) = path else { continue };
            let text = read(side, path).ok_or(NotPure::Unreadable)?;
            let items = if path.ends_with(".rs") { items_of(&text) } else { Vec::new() };
            let located = items.into_iter().map(|item| Located { file: path.clone(), item });
            match side {
                Side::Base => base.extend(located),
                Side::Head => head.extend(located),
            }
            spans.insert((side, path.clone()), use_spans(&text));
        }
    }
    let matched = pair_items(&base, &head)?;
    if matched.moved == 0 {
        return Err(NotPure::NothingMoved);
    }
    let (residual, carried) = residual_lines(&files, &base, &head, &spans)?;
    Ok(render(&matched, &residual, base.len(), carried))
}

// ───────── 残差と差の材料 ─────────

/// 除いたコメント行の差の逐語（位置ごとに違う行 + 片側にしか無い行・区間の順・両側とも空なら差なし）。
///
/// 返す組は (base 側の行, head 側の行)。件数（位置ごとに違う行数 + 行数の差）は両側の多い方に等しい
/// （要約の件数の行と逐語の本数が対になる・C10）。
fn comment_diff(base: &[String], head: &[String]) -> (Vec<String>, Vec<String>) {
    let mut old_lines = Vec::new();
    let mut new_lines = Vec::new();
    for (old, new) in base.iter().zip(head) {
        if old != new {
            old_lines.push(old.clone());
            new_lines.push(new.clone());
        }
    }
    let common = base.len().min(head.len());
    old_lines.extend(base.iter().skip(common).cloned());
    new_lines.extend(head.iter().skip(common).cloned());
    (old_lines, new_lines)
}

/// `use` の宣言のうち複数行に渡る区間（列 0 の `use …{` から `;` で終わる行まで・1 始まり・両端含む）。
fn use_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut open: Option<usize> = None;
    for (index, line) in text.lines().enumerate() {
        let number = index.saturating_add(1);
        match open {
            None if is_use_head(line) && !line.trim_end().ends_with(';') => open = Some(number),
            Some(start) if line.trim_end().ends_with(';') => {
                spans.push((start, number));
                open = None;
            }
            _ => {}
        }
    }
    spans
}

/// `use` の宣言の頭か（`pub` 系の可視性を許す）。
fn is_use_head(line: &str) -> bool {
    strip_visibility(line).1.starts_with("use ")
}

/// `mod x;` の宣言か（inline の `mod x {` は item の側）。
fn is_mod_declaration(line: &str) -> bool {
    let body = strip_visibility(line).1;
    body.starts_with("mod ") && body.trim_end().ends_with(';')
}

// ───────── 判定 ─────────

/// file の間を動いた item の束（元 → 先ごと）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Move {
    /// 移動元。
    from: String,
    /// 移動先。
    to: String,
    /// 動いた item の名。
    names: Vec<String>,
    /// 動いた行数（HEAD 側の区間の合計）。
    lines: usize,
}

/// 可視性が変わった item。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Visibility {
    /// HEAD 側の file。
    file: String,
    /// 名。
    name: String,
    /// 前（base）。
    before: String,
    /// 後（HEAD）。
    after: String,
}

/// コメント行の差を持つ item（hash から除いた行の差・0 の item は持たない・設計 §25）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct CommentDiff {
    /// HEAD 側の file。
    file: String,
    /// 名。
    name: String,
    /// 違う行数（`base` / `head` の多い方＝逐語の本数と対）。
    lines: usize,
    /// base 側の違う行の逐語（indent を落とした字面・区間の順・要約に `-` で載る）。
    base: Vec<String>,
    /// head 側の違う行の逐語（indent を落とした字面・区間の順・要約に `+` で載る）。
    head: Vec<String>,
}

/// 多重集合の照合の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Matched {
    /// 移動（元 → 先の順）。
    moves: Vec<Move>,
    /// 可視性の変化（HEAD の file・行の順）。
    visibility: Vec<Visibility>,
    /// コメント行の差（HEAD の file・行の順）。
    comments: Vec<CommentDiff>,
    /// 動いた item の本数。
    moved: usize,
    /// 動いた item の HEAD 側の区間（[`MoveSummary::moved`] へ運ぶ）。
    spans: Vec<(String, (usize, usize))>,
}

/// (名, hash) の多重集合を突き合わせ、同じ file の対を先に取り、残りを移動と数える。
fn pair_items(base: &[Located], head: &[Located]) -> Result<Matched, NotPure> {
    let mut keyed: BTreeMap<(String, u64), (Vec<usize>, Vec<usize>)> = BTreeMap::new();
    for (index, found) in base.iter().enumerate() {
        keyed.entry((found.item.name.clone(), found.item.hash)).or_default().0.push(index);
    }
    for (index, found) in head.iter().enumerate() {
        keyed.entry((found.item.name.clone(), found.item.hash)).or_default().1.push(index);
    }
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (_, (mut olds, mut news)) in keyed {
        if olds.len() != news.len() {
            return Err(NotPure::ItemsDiffer);
        }
        // 同じ file の対を先に取る（動いていない item を移動と数えない）。
        let mut index = 0;
        while let Some(old) = olds.get(index).copied() {
            let same = news.iter().position(|new| base.get(old).map(|found| &found.file) == head.get(*new).map(|found| &found.file));
            match same {
                Some(at) => {
                    pairs.push((old, news.remove(at)));
                    olds.remove(index);
                }
                None => index = index.saturating_add(1),
            }
        }
        pairs.extend(olds.into_iter().zip(news));
    }
    pairs.sort_by_key(|(_, new)| *new);
    Ok(matched_of(&pairs, base, head))
}

/// 対の列から移動と可視性の変化とコメント行の差を集める。
fn matched_of(pairs: &[(usize, usize)], base: &[Located], head: &[Located]) -> Matched {
    let mut moves: Vec<Move> = Vec::new();
    let mut visibility = Vec::new();
    let mut comments = Vec::new();
    let mut moved: usize = 0;
    let mut spans = Vec::new();
    for (old, new) in pairs {
        let (Some(from), Some(to)) = (base.get(*old), head.get(*new)) else { continue };
        if from.item.visibility != to.item.visibility {
            visibility.push(Visibility {
                file: to.file.clone(),
                name: to.item.name.clone(),
                before: from.item.visibility.clone(),
                after: to.item.visibility.clone(),
            });
        }
        let (old_lines, new_lines) = comment_diff(&from.item.comments, &to.item.comments);
        let differ = old_lines.len().max(new_lines.len());
        if differ > 0 {
            comments.push(CommentDiff {
                file: to.file.clone(),
                name: to.item.name.clone(),
                lines: differ,
                base: old_lines,
                head: new_lines,
            });
        }
        if from.file == to.file {
            continue;
        }
        moved = moved.saturating_add(1);
        spans.push((to.file.clone(), to.item.lines));
        let lines =to.item.lines.1.saturating_sub(to.item.lines.0).saturating_add(1);
        match moves.iter_mut().find(|found| found.from == from.file && found.to == to.file) {
            Some(found) => {
                found.names.push(to.item.name.clone());
                found.lines = found.lines.saturating_add(lines);
            }
            None => moves.push(Move { from: from.file.clone(), to: to.file.clone(), names: vec![to.item.name.clone()], lines }),
        }
    }
    Matched { moves, visibility, comments, moved, spans }
}

/// 残差分（file ごとの逐語の行・`-` / `+` 付き）。
type Residual = Vec<(String, Vec<String>)>;

/// 残差分（どの item の区間にも入らない diff 行）を file ごとに集め、許されない行が在れば理由を返す。
///
/// 札（`// flip-check:`）は item の中の行も残差の行も同じ規則で見る（コメント行の除外が `retroactive` を item の
/// 中に隠さない・indent の後の字面で見る）: `moved <id>` はその場で許し、他は両側の**多重集合の対**で見る
/// （持ち越し・`s2-07l.362`）＝base 側と head 側で同じ字面（id まで）の札は対にして外し、対の無い札
/// （head だけの新規・id 違い・base だけの消えた札）は `ForeignMarker`。返す組は (残差, 持ち越した札の本数)。
fn residual_lines(
    files: &[FileDiff],
    base: &[Located],
    head: &[Located],
    spans: &BTreeMap<(Side, String), Vec<(usize, usize)>>,
) -> Result<(Residual, usize), NotPure> {
    let mut residual = Residual::new();
    let mut markers: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for file in files {
        let mut lines = Vec::new();
        let sides = [(Side::Base, &file.base, &file.removed, '-', base), (Side::Head, &file.head, &file.added, '+', head)];
        for (side, path, changed, sign, items) in sides {
            let Some(path) = path else { continue };
            let in_use = spans.get(&(side, path.clone())).map(Vec::as_slice).unwrap_or_default();
            for (number, text) in changed {
                // item の中の行は hash が照合済み（札だけ見る）。残差の札は列 0 の形だけ札と読む（従来どおり）。
                let in_item = items.iter().any(|found| found.file == *path && covers(found.item.lines, *number));
                let shown = if in_item { text.trim_start() } else { text.as_str() };
                if shown.starts_with(FLIP_MARK) {
                    tally_marker(shown, side, &mut markers)?;
                } else if !in_item {
                    residual_allowed(text, in_use.iter().any(|span| covers(*span, *number)))?;
                }
                if !in_item {
                    lines.push(format!("{sign}{text}"));
                }
            }
        }
        if !lines.is_empty() {
            let shown = file.head.clone().or_else(|| file.base.clone()).unwrap_or_default();
            residual.push((shown, lines));
        }
    }
    let carried = markers.values().try_fold(0_usize, |sum, (old, new)| (old == new).then(|| sum.saturating_add(*old)).ok_or(NotPure::ForeignMarker))?;
    Ok((residual, carried))
}

/// 札の 1 行を数える: `moved <id>` はその場で判定し（id 無しは `ForeignMarker`）、他は字面ごとに側の本数へ足す。
fn tally_marker(shown: &str, side: Side, markers: &mut BTreeMap<String, (usize, usize)>) -> Result<(), NotPure> {
    if shown.starts_with(MOVED_MARK.trim_end()) {
        return marker_allowed(shown);
    }
    let counts = markers.entry(shown.to_owned()).or_default();
    match side {
        Side::Base => counts.0 = counts.0.saturating_add(1),
        Side::Head => counts.1 = counts.1.saturating_add(1),
    }
    Ok(())
}

/// 区間（両端含む）が行番号を含むか。
fn covers(span: (usize, usize), number: usize) -> bool {
    span.0 <= number && number <= span.1
}

/// 残差分の 1 行が許される形か（宣言 / 札 / item に付かない裸のコメント / 空行）。
fn residual_allowed(text: &str, in_use_span: bool) -> Result<(), NotPure> {
    if text.trim().is_empty() || in_use_span {
        return Ok(());
    }
    if text.starts_with(FLIP_MARK) {
        return marker_allowed(text);
    }
    let declaration = is_use_head(text) || is_mod_declaration(text) || text.starts_with("#[path") || text == "#[cfg(test)]";
    (text.starts_with("//") || declaration).then_some(()).ok_or(NotPure::ResidualLine)
}

/// 札の 1 行（`// flip-check:` で始まる）が許される形か: `moved <id>` だけ・他は `ForeignMarker`。
fn marker_allowed(text: &str) -> Result<(), NotPure> {
    text.strip_prefix(MOVED_MARK)
        .is_some_and(|id| !id.trim().is_empty())
        .then_some(())
        .ok_or(NotPure::ForeignMarker)
}

// ───────── 要約 ─────────

/// 可視性の字面（無しは `private`）。
fn shown_visibility(visibility: &str) -> &str {
    if visibility.is_empty() {
        "private"
    } else {
        visibility
    }
}

/// 要約の本文を描く（外形は snapshot・C12.5）。持ち越した札の本数 `carried` は 1 以上の周だけ 1 行で名乗る。
fn render(matched: &Matched, residual: &Residual, total: usize, carried: usize) -> MoveSummary {
    let mut lines = vec![HEADLINE.to_owned(), "## 移動（元 -> 先: 本数 / 行数）".to_owned()];
    for found in &matched.moves {
        lines.push(format!("{} -> {}: items={} lines={}", found.from, found.to, found.names.len(), found.lines));
        lines.extend(found.names.iter().map(|name| format!("  {name}")));
    }
    lines.push("## 可視性（名: 前 -> 後）".to_owned());
    for found in &matched.visibility {
        lines.push(format!(
            "{} {}: {} -> {}",
            found.file,
            found.name,
            shown_visibility(&found.before),
            shown_visibility(&found.after)
        ));
    }
    lines.push("## コメント行の差（名: 行数）".to_owned());
    for found in &matched.comments {
        lines.push(format!("{} {}: {}", found.file, found.name, found.lines));
        lines.extend(found.base.iter().map(|line| format!("-{line}")));
        lines.extend(found.head.iter().map(|line| format!("+{line}")));
    }
    lines.push("## 残差分（逐語）".to_owned());
    for (file, changed) in residual {
        lines.push(file.clone());
        lines.extend(changed.iter().cloned());
    }
    if carried > 0 {
        lines.push(format!("carried markers: {carried}"));
    }
    lines.push(format!(
        "判定: 名 + 本文の多重集合が一致 items={total} moved={} visibility={}",
        matched.moved,
        matched.visibility.len()
    ));
    let mut text = lines.join("\n");
    text.push('\n');
    MoveSummary { text, moved: matched.spans.clone() }
}

#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.499
    use super::{declaration_of, items_of, judge, residual_allowed, strip_visibility, use_spans, LensInput, NotPure, Side};
    use proptest::prelude::*;
    use proptest::test_runner::Config;

    /// 反例の永続化を切り、case 数を 256 に pin する（`pipe::closure` の歯と同じ形）。
    fn config() -> Config {
        Config {
            cases: 256,
            failure_persistence: None,
            ..Config::default()
        }
    }

    /// 可視性の剥がし: 3 形を剥がして記録し、`pub` で始まる識別子（`pubsub`）と `pub(` の閉じない形は剥がさない。
    #[test]
    fn move_proof_strips_the_three_visibility_prefixes_only() {
        assert_eq!(strip_visibility("pub fn a() {"), ("pub".to_owned(), "fn a() {"));
        assert_eq!(strip_visibility("pub(crate) fn a() {"), ("pub(crate)".to_owned(), "fn a() {"));
        assert_eq!(strip_visibility("pub(super) struct A;"), ("pub(super)".to_owned(), "struct A;"));
        assert_eq!(strip_visibility("fn a() {"), (String::new(), "fn a() {"));
        assert_eq!(strip_visibility("pubsub fn a() {"), (String::new(), "pubsub fn a() {"));
        assert_eq!(strip_visibility("pub(crate fn a() {"), (String::new(), "pub(crate fn a() {"));
    }

    /// 宣言の読み: 9 種の keyword・修飾（`unsafe` / `const fn`）・`mod x;` は宣言でない・字下げ行は読まない。
    #[test]
    fn move_proof_reads_column_zero_declarations_only() {
        let named: &[(&str, &str)] = &[
            ("fn one() -> u8 {", "fn one"),
            ("pub(crate) const fn two() {", "fn two"),
            ("pub unsafe fn three() {", "fn three"),
            ("struct Pair;", "struct Pair"),
            ("pub enum Hue {", "enum Hue"),
            ("impl<T> Foo<T> for Bar {", "impl <T> Foo<T> for Bar"),
            ("impl Foo {}", "impl Foo"),
            ("trait Tr {", "trait Tr"),
            ("const CAP: u8 = 1;", "const CAP"),
            ("static ONCE: u8 = 1;", "static ONCE"),
            ("type Alias = u8;", "type Alias"),
            ("mod tests {", "mod tests"),
        ];
        for (line, want) in named {
            assert_eq!(declaration_of(line).map(|decl| decl.name).as_deref(), Some(*want), "{line}");
        }
        // `mod x;` は宣言（item ではない）・字下げ行は読まない・keyword の後ろは空白。
        for line in ["mod tests;", "use std::fs;", "    fn nested() {", "// fn commented() {", "fnord() {"] {
            assert_eq!(declaration_of(line), None, "{line}");
        }
    }

    /// 入れ子の切り出し: `impl {}` の中の fn と inline `mod tests {}` の歯は外側の 1 本に畳む。属性と doc は
    /// item に付き、`;` で終わる const は 1 行、複数行の const は `];` まで、複数行の署名の fn は列 0 の `)` で
    /// 切れず `}` まで、末尾の空行は含めない。
    #[test]
    fn move_proof_folds_nested_items_into_the_outer_item() {
        let text = "//! doc\n\nuse std::fs;\n\n/// d\n#[derive(Debug)]\npub struct A {\n    b: u8,\n}\n\nimpl A {\n    fn m(&self) {}\n    pub fn n(&self) {}\n}\n\nconst ONE: u8 = 1;\n\nconst MANY: &[u8] = &[\n    1,\n];\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n\nfn wide(\n    a: u8,\n) -> u8 {\n    a\n}\n";
        let items = items_of(text);
        let shown: Vec<(&str, (usize, usize))> = items.iter().map(|item| (item.name.as_str(), item.lines)).collect();
        assert_eq!(
            shown,
            [
                ("struct A", (5, 9)),
                ("impl A", (11, 14)),
                ("const ONE", (16, 16)),
                ("const MANY", (18, 20)),
                ("mod tests", (22, 26)),
                ("fn wide", (28, 32)),
            ],
            "列 0 の宣言単位だけ（入れ子は外側に畳む）"
        );
        assert_eq!(items.first().map(|item| item.visibility.as_str()), Some("pub"));
    }

    /// indent の正規化と可視性: 同じ本文を字下げ・可視性だけ変えても hash は同じで、本文が 1 字違えば異なる。
    /// コメント行（doc・行内）の差は hash を動かさず [`super::Item::comments`] に写り、`//` で始まる文字列 literal
    /// の行（`"// …"`）は本文＝動かす。
    #[test]
    fn move_proof_hash_ignores_indent_and_visibility_but_not_the_body() {
        let plain = items_of("fn a() {\n    1\n}\n");
        let shifted = items_of("pub(super) fn a() {\n        1\n}\n");
        let changed = items_of("fn a() {\n    2\n}\n");
        let hash = |items: &[super::Item]| items.first().map(|item| item.hash);
        assert!(hash(&plain).is_some(), "母集団 1 本");
        assert_eq!(hash(&plain), hash(&shifted), "indent と可視性は hash に入らない");
        assert_ne!(hash(&plain), hash(&changed), "本文の差は hash に出る");
        assert_eq!(shifted.first().map(|item| item.visibility.as_str()), Some("pub(super)"));
        let commented = items_of("/// see [`super::a`]\nfn a() {\n    // note\n    1\n}\n");
        assert_eq!(hash(&plain), hash(&commented), "コメント行の差は hash を動かさない");
        assert_eq!(commented.first().map(|item| item.comments.clone()), Some(vec!["/// see [`super::a`]".to_owned(), "// note".to_owned()]));
        let quoted = items_of("fn a() {\n    \"// x\"\n}\n");
        assert_ne!(hash(&quoted), hash(&items_of("fn a() {\n    \"// y\"\n}\n")), "文字列 literal の `//` は本文");
        assert!(quoted.first().is_some_and(|item| item.comments.is_empty()), "literal はコメント行に数えない");
    }

    /// item の中のコメント行: doc の link path の書き換えだけの便は純移動で要約に「コメント行の差」（該当 item の
    /// 名と行数・直下に逐語）・item の中の札は残差と同じ規則（`moved` は許し `retroactive` は `ForeignMarker`）。
    #[test]
    fn move_proof_comment_diff_inside_items_is_counted_and_markers_inside_items_are_checked() {
        let base_lib = "//! lib\n\n/// see [`super::a`]\nfn b() {\n    2\n}\n\nfn c() {\n    3\n}\n";
        let head_lib = "//! lib\n\nmod m;\n\nfn c() {\n    3\n}\n";
        let head_m = "/// see [`crate::a`]\npub(super) fn b() {\n    2\n}\n";
        let read = |m: &str| -> (String, [(Side, &'static str, String); 3]) {
            let diff = format!("{}{}", hunk("src/lib.rs", Some(base_lib), head_lib), hunk("src/m.rs", None, m));
            (diff, [(Side::Base, "src/lib.rs", base_lib.to_owned()), (Side::Head, "src/lib.rs", head_lib.to_owned()), (Side::Head, "src/m.rs", m.to_owned())])
        };
        let (diff, files) = read(head_m);
        let shown: Vec<(Side, &str, &str)> = files.iter().map(|(side, path, text)| (*side, *path, text.as_str())).collect();
        let LensInput::Summary(summary) = judge(&diff, &table(&shown)) else {
            panic!("doc の link だけの差は純移動");
        };
        assert!(summary.text().contains("\n## コメント行の差（名: 行数）\nsrc/m.rs fn b: 1\n-/// see [`super::a`]\n+/// see [`crate::a`]\n## 残差分"), "{}", summary.text());
        for (marker, want) in [("moved s2-07l.294", Ok(1)), ("retroactive s2-07l.294", Err(NotPure::ForeignMarker))] {
            let (diff, files) = read(&head_m.replace("    2\n", &format!("    // flip-check: {marker}\n    2\n")));
            let shown: Vec<(Side, &str, &str)> = files.iter().map(|(side, path, text)| (*side, *path, text.as_str())).collect();
            assert_eq!(outcome(&judge(&diff, &table(&shown))), want, "{marker}");
        }
    }

    /// 上の歯と同じ fixture（`/// see [`super::a`]` → `/// see [`crate::a`]`）で `judge` した要約の本文
    /// （純移動でない周は理由の名＝歯の assert で割れる）。
    fn comment_only_summary() -> String {
        let base_lib = "//! lib\n\n/// see [`super::a`]\nfn b() {\n    2\n}\n\nfn c() {\n    3\n}\n";
        let head_lib = "//! lib\n\nmod m;\n\nfn c() {\n    3\n}\n";
        let head_m = "/// see [`crate::a`]\npub(super) fn b() {\n    2\n}\n";
        let diff = format!("{}{}", hunk("src/lib.rs", Some(base_lib), head_lib), hunk("src/m.rs", None, head_m));
        let files = [(Side::Base, "src/lib.rs", base_lib), (Side::Head, "src/lib.rs", head_lib), (Side::Head, "src/m.rs", head_m)];
        let input = judge(&diff, &table(&files));
        match input {
            LensInput::Summary(summary) => summary.text().to_owned(),
            LensInput::Diff(why) => why.as_str().to_owned(),
        }
    }

    /// 「## コメント行の差」の節の本文（見出しの次の行から「## 残差分」の直前まで）。
    fn comment_section(text: &str) -> Vec<&str> {
        text.lines()
            .skip_while(|line| *line != "## コメント行の差（名: 行数）")
            .skip(1)
            .take_while(|line| *line != "## 残差分（逐語）")
            .collect()
    }

    /// (a) 逐語は件数の行の直後: `src/m.rs fn b: 1` の次が `-/// see [`super::a`]`・その次が `+/// see [`crate::a`]`
    /// （設計 §25・base は件数だけ）。
    #[test]
    fn move_proof_comment_verbatim_lines_follow_the_count() {
        let text = comment_only_summary();
        assert_eq!(
            comment_section(&text),
            ["src/m.rs fn b: 1", "-/// see [`super::a`]", "+/// see [`crate::a`]"],
            "{text}"
        );
    }

    /// (b) 件数の行の n は逐語の本数（base 側 `-` と head 側 `+` の多い方）と一致（C10・母集団と対）: 位置ごとに
    /// 違う行 1 + head 側だけの行 1 = n 2 で `-` 1 本・`+` 2 本。
    #[test]
    fn move_proof_comment_verbatim_count_matches_the_lines() {
        let text = comment_only_summary();
        let section = comment_section(&text);
        let count = |sign: char| section.iter().filter(|line| line.starts_with(sign)).count();
        let n: usize = section.first().and_then(|line| line.rsplit(": ").next()).and_then(|n| n.parse().ok()).expect("件数の行");
        assert_eq!(n, count('-').max(count('+')), "{text}");
        assert_eq!(count('-'), 1, "{text}");
        assert_eq!(count('+'), 1, "{text}");
        let base = ["/// see [`super::a`]".to_owned(), "// same".to_owned()];
        let head = ["/// see [`crate::a`]".to_owned(), "// same".to_owned(), "// added".to_owned()];
        let (old_lines, new_lines) = super::comment_diff(&base, &head);
        assert_eq!(old_lines, ["/// see [`super::a`]"], "位置ごとに違う行だけ（同じ行は載らない）");
        assert_eq!(new_lines, ["/// see [`crate::a`]", "// added"], "違う行 + head 側だけの行");
        assert_eq!(old_lines.len().max(new_lines.len()), 2, "件数 = 違う行 1 + 行数の差 1");
        let (old_lines, new_lines) = super::comment_diff(&head, &base);
        assert_eq!((old_lines.len(), new_lines.len()), (2, 1), "base 側が長い周は `-` が多い");
    }

    /// (c) コメント差 0 の純移動（`move_proof_judge_pins_each_reason` と同じ fixture）では節が空＝`-` / `+` の行が出ない。
    #[test]
    fn move_proof_comment_verbatim_is_absent_when_comments_match() {
        let base_lib = "/// doc b\nfn a() {\n    1\n}\n\n/// doc b\nfn b() {\n    2\n}\n";
        let head_lib = "mod m;\n\n/// doc b\nfn a() {\n    1\n}\n";
        let head_m = "/// doc b\npub(super) fn b() {\n    2\n}\n";
        let diff = format!("{}{}", hunk("src/lib.rs", Some(base_lib), head_lib), hunk("src/m.rs", None, head_m));
        let files = [(Side::Base, "src/lib.rs", base_lib), (Side::Head, "src/lib.rs", head_lib), (Side::Head, "src/m.rs", head_m)];
        let LensInput::Summary(summary) = judge(&diff, &table(&files)) else {
            panic!("コメント差 0 の移動は純移動");
        };
        assert!(summary.text().contains("\n## コメント行の差（名: 行数）\n## 残差分（逐語）\n"), "{}", summary.text());
        assert_eq!(comment_section(summary.text()), Vec::<&str>::new(), "{}", summary.text());
        assert_eq!(super::comment_diff(&["// x".to_owned()], &["// x".to_owned()]), (Vec::new(), Vec::new()));
    }

    /// 残差分の弁別: 宣言と札とコメントと空行だけを許し、`moved` 以外の札は `ForeignMarker`、他は `ResidualLine`。
    #[test]
    fn move_proof_residual_lines_are_declarations_markers_comments_or_blank() {
        for line in ["", "   ", "mod alpha;", "pub mod alpha;", "pub(crate) mod b;", "use super::*;", "pub use x::Y;", "#[path = \"x.rs\"]", "#[cfg(test)]", "//! module doc", "/// stray doc", "// ── section ──", "// flip-check: moved s2-07l.261"] {
            assert_eq!(residual_allowed(line, false), Ok(()), "許す: {line:?}");
        }
        assert_eq!(residual_allowed("    Foo,", true), Ok(()), "複数行の use の中の行");
        assert_eq!(residual_allowed("// flip-check: retroactive s2-07l.1", false), Err(NotPure::ForeignMarker));
        assert_eq!(residual_allowed("// flip-check: moved ", false), Err(NotPure::ForeignMarker), "id の無い札");
        for line in ["x", "#![allow(dead_code)]", "    Foo,", "mod alpha {", "let a = 1;", "#[derive(Debug)]"] {
            assert_eq!(residual_allowed(line, false), Err(NotPure::ResidualLine), "許さない: {line:?}");
        }
        assert_eq!(use_spans("use a::{\n    b,\n};\nuse c;\n"), [(1, 3)], "複数行の use の区間だけ");
    }

    /// `judge` の 4 条件を 1 fixture ずつ: 純移動 → Summary・本文差 → ItemsDiffer・移動 0 → NothingMoved・
    /// 読めない file → Unreadable・binary の見出し → Unreadable。
    #[test]
    fn move_proof_judge_pins_each_reason() {
        let base_lib = "fn a() {\n    1\n}\n\nfn b() {\n    2\n}\n";
        let head_lib = "mod m;\n\nfn a() {\n    1\n}\n";
        let head_m = "pub(super) fn b() {\n    2\n}\n";
        let diff = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,7 +1,5 @@\n+mod m;\n+\n fn a() {\n     1\n }\n-\n-fn b() {\n-    2\n-}\ndiff --git a/src/m.rs b/src/m.rs\nnew file mode 100644\n--- /dev/null\n+++ b/src/m.rs\n@@ -0,0 +1,3 @@\n+pub(super) fn b() {\n+    2\n+}\n";
        let read = |side: Side, path: &str| -> Option<String> {
            match (side, path) {
                (Side::Base, "src/lib.rs") => Some(base_lib.to_owned()),
                (Side::Head, "src/lib.rs") => Some(head_lib.to_owned()),
                (Side::Head, "src/m.rs") => Some(head_m.to_owned()),
                _ => None,
            }
        };
        let LensInput::Summary(summary) = judge(diff, &read) else {
            panic!("純移動は Summary");
        };
        assert!(summary.text().starts_with("これは diff ではなく純移動の要約である\n"), "{}", summary.text());
        assert!(summary.text().contains("src/lib.rs -> src/m.rs: items=1 lines=3\n  fn b\n"), "{}", summary.text());
        assert!(summary.text().contains("src/m.rs fn b: private -> pub(super)\n"), "{}", summary.text());
        assert!(summary.text().contains("\n+mod m;\n"), "残差分は逐語: {}", summary.text());
        assert!(summary.text().ends_with("判定: 名 + 本文の多重集合が一致 items=2 moved=1 visibility=1\n"), "{}", summary.text());

        let changed = |side: Side, path: &str| read(side, path).map(|text| if side == Side::Head { text.replace("    2", "    3") } else { text });
        assert_eq!(judge(diff, &changed), LensInput::Diff(NotPure::ItemsDiffer), "本文が 1 行違う");
        let stayed = |side: Side, path: &str| match (side, path) {
            (Side::Head, "src/lib.rs") => Some(format!("mod m;\n\n{base_lib}")),
            (Side::Head, "src/m.rs") => Some("//! m\n".to_owned()),
            _ => read(side, path),
        };
        assert_eq!(judge(diff, &stayed), LensInput::Diff(NotPure::NothingMoved), "宣言だけ");
        let unreadable = |side: Side, path: &str| if path == "src/m.rs" { None } else { read(side, path) };
        assert_eq!(judge(diff, &unreadable), LensInput::Diff(NotPure::Unreadable), "file を読めない");
        let binary = "diff --git a/x.png b/x.png\nBinary files a/x.png and b/x.png differ\n";
        assert_eq!(judge(binary, &read), LensInput::Diff(NotPure::Unreadable), "binary の見出し");
        assert_eq!(judge("", &read), LensInput::Diff(NotPure::NothingMoved), "空の diff は移動 0");
    }

    /// 本文の断片（列 0 の宣言・入れ子・宣言でない行）。
    const FRAGMENTS: &[&str] = &[
        "fn a() {\n    1\n}\n",
        "pub(crate) fn a() {\n    1\n}\n",
        "/// d\n#[derive(Debug)]\npub struct S {\n    x: u8,\n}\n",
        "impl S {\n    fn m(&self) {}\n}\n",
        "const C: u8 = 1;\n",
        "const M: &[u8] = &[\n    1,\n];\n",
        "mod tests {\n    #[test]\n    fn t() {}\n}\n",
        "fn wide(\n    a: u8,\n) -> u8 {\n    a\n}\n",
        "use std::fs;\n",
        "mod x;\n",
        "// ── section ──\n",
        "\n",
        "    stray();\n",
    ];

    /// 断片の連結（0〜8 本）。
    fn bodies() -> impl Strategy<Value = String> {
        prop::collection::vec(prop::sample::select(FRAGMENTS), 0..9).prop_map(|parts| parts.concat())
    }

    proptest! {
        #![proptest_config(config())]

        /// item の区間は互いに重ならず file の中に収まり、hash は同じ本文で決定的・可視性を剥がしても不変。
        #[test]
        fn prop_move_proof_items_are_disjoint_and_hash_is_deterministic(text in bodies()) {
            let items = items_of(&text);
            let total = text.lines().count();
            let mut last_end = 0;
            for item in &items {
                prop_assert!(item.lines.0 > last_end);
                prop_assert!(item.lines.0 <= item.lines.1);
                prop_assert!(item.lines.1 <= total);
                last_end = item.lines.1;
            }
            let again = items_of(&text);
            prop_assert_eq!(&items, &again);
            let widened = text.replace("pub(crate) fn a()", "fn a()");
            let hashes: Vec<(String, u64)> = items.iter().map(|item| (item.name.clone(), item.hash)).collect();
            let widened_hashes: Vec<(String, u64)> = items_of(&widened).iter().map(|item| (item.name.clone(), item.hash)).collect();
            prop_assert_eq!(hashes, widened_hashes);
        }
    }

    // ───────── 純移動の生成器と生存変異の歯（s2-07l.290・.266 run 1 の生存 27 本） ─────────

    /// item の雛形（`@v` = 可視性・`@n` = 番号・`@i` = 本文の識別子・`@x` = 本文の値）。本文は複数行・入れ子 1 段。
    const TEMPLATES: &[&str] = &[
        "@vfn f@n() -> u32 {\n    let @i = @x;\n    if @i > 0 {\n        @i\n    } else {\n        0\n    }\n}\n",
        "@vstruct S@n {\n    @i: [u8; @x],\n}\n",
        "@venum E@n {\n    @i = @x,\n    B,\n}\n",
        "impl S@n {\n    fn @i(&self) -> u32 {\n        @x\n    }\n}\n",
        "@vconst C@n: &[u32] = &[\n    @i(@x),\n];\n",
        "@vconst K@n: u32 = @i(@x);\n",
        "@vtype T@n = @i<[u8; @x]>;\n",
        "@vmod m@n {\n    pub fn @i() -> u32 {\n        @x\n    }\n}\n",
        "@vfn w@n(\n    @i: u32,\n) -> u32 {\n    @i + @x\n}\n",
    ];

    /// 可視性（無し + 4 形）。
    const VISIBILITIES: &[&str] = &["", "pub ", "pub(crate) ", "pub(super) ", "pub(in crate::pipe) "];

    /// item の前に付く行（無し・doc・属性・doc + 複数行の属性）。
    const PREFIXES: &[&str] = &["", "/// doc\n", "#[inline]\n", "/// doc\n#[expect(\n    dead_code,\n    reason = \"x\"\n)]\n"];

    /// item の後ろの隙間（残差分に許される行だけ: 空行・区切り線・宣言・札）。
    const GAPS: &[&str] = &["\n", "\n// ── section ──\n\n", "\nuse std::fs;\n\n", "\nmod gen;\n\n", "\n// flip-check: moved s2-07l.290\n\n"];

    /// file の名（base は先頭の 1〜3 本・HEAD は 3 本のどれへでも置ける）。
    const FILES: &[&str] = &["src/a.rs", "src/b.rs", "src/c.rs"];

    /// (雛形, 番号, 値, 前置き)＝同じ key は同じ (名, 本文の hash) になる。
    type Key = (usize, usize, u32, usize);

    /// file ごとの本文（無い file は `None`）。
    type Files = Vec<Option<String>>;

    /// 生成した item 1 本: 本文を決める key と、base / HEAD の置き場。
    #[derive(Debug, Clone)]
    struct Placed {
        /// 本文を決める key。
        key: Key,
        /// base の (file, 可視性, 隙間)。
        base: (usize, usize, usize),
        /// HEAD の (file, 可視性, 隙間, 並びの key, 字下げを深くするか)。
        head: (usize, usize, usize, u8, bool),
    }

    /// item 2〜8 本（4 本に 1 本は直前の item と同じ key＝同じ名と本文の双子）。
    fn placements() -> impl Strategy<Value = Vec<Placed>> {
        let one = (
            (0..TEMPLATES.len(), 1..50_u32, 0..PREFIXES.len(), 0..4_usize),
            (0..FILES.len(), 0..VISIBILITIES.len(), 0..GAPS.len()),
            (0..FILES.len(), 0..VISIBILITIES.len(), 0..GAPS.len(), any::<u8>(), any::<bool>()),
        );
        prop::collection::vec(one, 2..9).prop_map(|drawn| {
            let mut items: Vec<Placed> = Vec::new();
            for (at, ((template, value, prefix, twin), base, head)) in drawn.into_iter().enumerate() {
                let key = match items.last() {
                    Some(last) if twin == 0 => last.key,
                    _ => (template, at, value, prefix),
                };
                items.push(Placed { key, base, head });
            }
            items
        })
    }

    /// 雛形から item を描く（`edit` = 本文を変える周の変え方 0〜3＝値 / 識別子 / 行の挿入 / 行の削除・`None` は変えない）。
    fn draw_item(key: Key, visibility: usize, indent: bool, edit: Option<usize>) -> String {
        let (template, number, value, prefix) = key;
        let value = if edit == Some(0) { value + 1000 } else { value };
        let ident = if edit == Some(1) { "edited" } else { "value" };
        let body = TEMPLATES[template]
            .replace("@v", VISIBILITIES[visibility])
            .replace("@n", &number.to_string())
            .replace("@i", ident)
            .replace("@x", &value.to_string());
        let mut lines: Vec<String> = body.lines().map(str::to_owned).collect();
        match edit {
            Some(3) if lines.len() > 1 => {
                lines.remove(1);
            }
            Some(2 | 3) => lines.insert(1, "    inserted();".to_owned()),
            _ => {}
        }
        let text = format!("{}{}", PREFIXES[prefix], lines.join("\n"));
        let shown: Vec<String> = text
            .lines()
            .map(|line| if indent && line.starts_with(' ') { format!("    {line}") } else { line.to_owned() })
            .collect();
        format!("{}\n", shown.join("\n"))
    }

    /// (base, HEAD) の file 群（`files` = base の file 数・`edit` = (本文を変える item, 変え方)）。
    fn sides(items: &[Placed], files: usize, edit: Option<(usize, usize)>) -> (Files, Files) {
        let base = items.iter().enumerate().map(|(at, item)| {
            let (file, visibility, gap) = item.base;
            (file % files, at, draw_item(item.key, visibility, false, None), GAPS[gap])
        });
        let head = items.iter().enumerate().map(|(at, item)| {
            let (file, visibility, gap, order, indent) = item.head;
            let how = edit.filter(|(target, _)| *target == at).map(|(_, how)| how);
            (file, usize::from(order), draw_item(item.key, visibility, indent, how), GAPS[gap])
        });
        (assemble(base.collect(), files), assemble(head.collect(), files))
    }

    /// file ごとに `//! 見出し` + (item + 隙間) を並びの順に連ねる（base に在る file は item 0 本でも見出しを持つ）。
    fn assemble(mut placed: Vec<(usize, usize, String, &'static str)>, files: usize) -> Files {
        placed.sort_by_key(|(file, order, _, _)| (*file, *order));
        let mut texts: Files = (0..FILES.len()).map(|file| (file < files).then(|| format!("//! {file}\n\n"))).collect();
        for (file, _, item, gap) in placed {
            let text = texts[file].get_or_insert_with(|| format!("//! {file}\n\n"));
            text.push_str(&item);
            text.push_str(gap);
        }
        texts
    }

    /// base と HEAD の file 群の unified diff（変わった file だけ・HEAD は base の file を全部持つ）。
    fn unified(base: &Files, head: &Files) -> String {
        let mut diff = String::new();
        for (at, path) in FILES.iter().enumerate() {
            let old = base.get(at).cloned().flatten();
            let Some(new) = head.get(at).cloned().flatten() else { continue };
            if old.as_deref() != Some(new.as_str()) {
                diff.push_str(&hunk(path, old.as_deref(), &new));
            }
        }
        diff
    }

    /// file 1 本の `git diff` の形（見出し + 共通の頭と尻を落とした 1 hunk・前後 3 行の context・`old` 無しは新規）。
    fn hunk(path: &str, old: Option<&str>, new: &str) -> String {
        let before: Vec<&str> = old.unwrap_or_default().lines().collect();
        let after: Vec<&str> = new.lines().collect();
        let prefix = before.iter().zip(&after).take_while(|(a, b)| a == b).count();
        let suffix = before[prefix..].iter().rev().zip(after[prefix..].iter().rev()).take_while(|(a, b)| a == b).count();
        let from = prefix - prefix.min(3);
        let (old_end, new_end) = (before.len() - suffix + suffix.min(3), after.len() - suffix + suffix.min(3));
        let mut out = format!("diff --git a/{path} b/{path}\n");
        match old {
            Some(_) => out.push_str(&format!("index 1111111..2222222 100644\n--- a/{path}\n")),
            None => out.push_str("new file mode 100644\nindex 0000000..2222222\n--- /dev/null\n"),
        }
        out.push_str(&format!("+++ b/{path}\n@@ -{},{} +{},{} @@\n", from + 1, old_end - from, from + 1, new_end - from));
        let lines = before[from..prefix]
            .iter()
            .map(|line| format!(" {line}"))
            .chain(before[prefix..before.len() - suffix].iter().map(|line| format!("-{line}")))
            .chain(after[prefix..after.len() - suffix].iter().map(|line| format!("+{line}")))
            .chain(before[before.len() - suffix..old_end].iter().map(|line| format!(" {line}")));
        for line in lines {
            out.push_str(&line);
            out.push('\n');
        }
        out
    }

    /// 生成した file 群の読み（`judge` の `read`）。
    fn read_from<'a>(base: &'a Files, head: &'a Files) -> impl Fn(Side, &str) -> Option<String> + 'a {
        move |side, path| {
            let at = FILES.iter().position(|file| *file == path)?;
            let files = if side == Side::Base { base } else { head };
            files.get(at).cloned().flatten()
        }
    }

    /// 固定 fixture の読み（(側, path, 本文) の表・表に無い読みは `None`）。
    fn table<'a>(files: &'a [(Side, &'a str, &'a str)]) -> impl Fn(Side, &str) -> Option<String> + 'a {
        move |side, path| files.iter().find(|(at, name, _)| *at == side && *name == path).map(|(_, _, text)| (*text).to_owned())
    }

    /// file を跨いだ item の数（key ごとに同じ file に残せる対を先に取った残り＝設計 §5.3 の「移動」）。
    fn crossed(items: &[Placed], files: usize) -> usize {
        let mut count: std::collections::BTreeMap<(Key, usize), (usize, usize)> = std::collections::BTreeMap::new();
        for item in items {
            count.entry((item.key, item.base.0 % files)).or_default().0 += 1;
            count.entry((item.key, item.head.0)).or_default().1 += 1;
        }
        items.len() - count.values().map(|(base, head)| (*base).min(*head)).sum::<usize>()
    }

    /// 判定を (要約の判定行の `moved=` | 理由) へ畳む。
    fn outcome(input: &LensInput) -> Result<usize, NotPure> {
        match input {
            LensInput::Diff(why) => Err(*why),
            LensInput::Summary(summary) => Ok(summary
                .text()
                .lines()
                .last()
                .and_then(|line| line.split_whitespace().find_map(|token| token.strip_prefix("moved=")))
                .and_then(|moved| moved.parse().ok())
                .unwrap_or(usize::MAX)),
        }
    }

    proptest! {
        #![proptest_config(config())]

        // flip-check: retroactive s2-07l.290
        /// (a) item の配置だけを変えた HEAD（file 間の移動・file 内の順序・可視性・字下げ・隙間の宣言 / 札 / コメント /
        /// 空行）は純移動: 要約の `moved=` は file を跨いだ item の数で、0 本なら `NothingMoved`。
        #[test]
        fn prop_move_proof_pure_moves_are_judged_pure(items in placements(), files in 1..=3_usize) {
            let (base, head) = sides(&items, files, None);
            let moved = crossed(&items, files);
            let want = if moved == 0 { Err(NotPure::NothingMoved) } else { Ok(moved) };
            prop_assert_eq!(outcome(&judge(&unified(&base, &head), &read_from(&base, &head))), want);
        }

        // flip-check: retroactive s2-07l.290
        /// (b) 同じ配置で 1 item の本文だけを 1 字〜1 行変えた HEAD（値・識別子・行の挿入 / 削除）は要約にならない。
        #[test]
        fn prop_move_proof_body_edits_are_not_pure(
            items in placements(),
            files in 1..=3_usize,
            target in any::<prop::sample::Index>(),
            how in 0..4_usize,
        ) {
            let (base, head) = sides(&items, files, Some((target.index(items.len()), how)));
            let input = judge(&unified(&base, &head), &read_from(&base, &head));
            prop_assert!(matches!(input, LensInput::Diff(_)));
        }
    }

    // flip-check: retroactive s2-07l.290
    /// `---` / `+++` の無い 100% の rename は `diff --git` の行だけが両側の path を運び、読んで 1 本の移動になる
    /// （`parse_diff` の base / head の field を消すと片側を読まず `ItemsDiffer`・`split_git_paths` を定数にすると
    /// 読めず `Unreadable`）。空の path は読まない（`&&` を `||` にすると片側だけ空の行を通す）。
    #[test]
    fn move_proof_mutant_parse_diff_pure_rename_reads_both_paths_from_the_git_line() {
        let (base_lib, head_lib, body) = ("//! lib\n\nmod old;\n", "//! lib\n\nmod new;\n", "fn b() {\n    2\n}\n");
        let rename = "diff --git a/src/old.rs b/src/new.rs\nsimilarity index 100%\nrename from src/old.rs\nrename to src/new.rs\n";
        let diff = format!("{}{rename}", hunk("src/lib.rs", Some(base_lib), head_lib));
        let files = [
            (Side::Base, "src/lib.rs", base_lib),
            (Side::Head, "src/lib.rs", head_lib),
            (Side::Base, "src/old.rs", body),
            (Side::Head, "src/new.rs", body),
        ];
        assert_eq!(outcome(&judge(&diff, &table(&files))), Ok(1), "rename は 1 本の移動: {diff}");
        assert_eq!(super::split_git_paths("a/src/old.rs b/src/new.rs"), Some(("src/old.rs".to_owned(), "src/new.rs".to_owned())));
        assert_eq!(super::split_git_paths("a/ b/src/new.rs"), None, "base が空");
        assert_eq!(super::split_git_paths("a/src/old.rs b/"), None, "HEAD が空");
    }

    // flip-check: retroactive s2-07l.290
    /// 行番号は hunk 見出しの開始行から数える: 10 行目からの hunk で書き換えた `macro_rules!` の本文（item でない）は
    /// `ResidualLine`（`hunk_start` を `Some((1, 1))` にすると 4 行目＝`fn a` の区間に数えられ、本文の書き換えが要約に化ける）。
    #[test]
    fn move_proof_mutant_hunk_start_numbers_lines_from_the_hunk_header() {
        let base_lib = "//! lib\n\nfn a() {\n    1\n    1\n    1\n    1\n    1\n    1\n}\n\nmacro_rules! m {\n    () => {};\n}\n";
        let head_lib = base_lib.replace("() => {};", "() => { evil() };");
        let (base_old, head_old, body) = ("//! old\n\nfn b() {\n    2\n}\n", "//! old\n", "fn b() {\n    2\n}\n");
        let moved = format!("{}{}", hunk("src/old.rs", Some(base_old), head_old), hunk("src/new.rs", None, body));
        let lib = hunk("src/lib.rs", Some(base_lib), &head_lib);
        assert!(lib.contains("@@ -10,5 +10,5 @@\n"), "前提: hunk は 10 行目から: {lib}");
        let files = [
            (Side::Base, "src/lib.rs", base_lib),
            (Side::Head, "src/lib.rs", head_lib.as_str()),
            (Side::Base, "src/old.rs", base_old),
            (Side::Head, "src/old.rs", head_old),
            (Side::Head, "src/new.rs", body),
        ];
        assert_eq!(outcome(&judge(&format!("{lib}{moved}"), &table(&files))), Err(NotPure::ResidualLine));
        assert_eq!(outcome(&judge(&moved, &table(&files))), Ok(1), "macro を書き換えない同じ移動は要約（別の理由で赤くならない）");
    }

    // flip-check: retroactive s2-07l.290
    /// 同じ (名, 本文) の item が 2 file に 1 本ずつ在り、どちらも動かない周は移動 0＝`NothingMoved`（`pair_items` の
    /// 同じ file の対を先に取る `==` を `!=` にすると 2 本を交差で対にし、動いていない双子を moved=2 と読んで要約に化ける）。
    #[test]
    fn move_proof_mutant_pair_items_twins_in_place_are_not_moves() {
        let twin = "fn d() {\n    1\n}\n";
        let (base_a, head_a) = (format!("//! a\n\n{twin}"), format!("//! a\n\n// note\n\n{twin}"));
        let (base_b, head_b) = (format!("//! b\n\n{twin}"), format!("//! b\n\n// note\n\n{twin}"));
        let diff = format!("{}{}", hunk("src/a.rs", Some(&base_a), &head_a), hunk("src/b.rs", Some(&base_b), &head_b));
        let files = [
            (Side::Base, "src/a.rs", base_a.as_str()),
            (Side::Head, "src/a.rs", head_a.as_str()),
            (Side::Base, "src/b.rs", base_b.as_str()),
            (Side::Head, "src/b.rs", head_b.as_str()),
        ];
        assert_eq!(outcome(&judge(&diff, &table(&files))), Err(NotPure::NothingMoved));
    }

    // flip-check: retroactive s2-07l.290
    /// `impl` で始まる列 0 の macro 呼び出し（`impl_x! { … }`）は item でない＝別 file へ移すと残差分の行で `ResidualLine`
    /// （`declaration_of` の `&&` を `||` にすると `impl` の後ろを見ずに item と読み、macro の移動を要約に化ける）。
    #[test]
    fn move_proof_mutant_declaration_of_impl_prefixed_macro_is_not_an_item() {
        assert_eq!(declaration_of("impl_x! {"), None);
        let base_lib = "//! lib\n\nfn a() {\n    1\n}\n\nimpl_x! {\n    A\n}\n\nfn b() {\n    2\n}\n";
        let (head_lib, head_m) = ("//! lib\n\nfn a() {\n    1\n}\n", "impl_x! {\n    A\n}\n\nfn b() {\n    2\n}\n");
        let diff = format!("{}{}", hunk("src/lib.rs", Some(base_lib), head_lib), hunk("src/m.rs", None, head_m));
        let files = [(Side::Base, "src/lib.rs", base_lib), (Side::Head, "src/lib.rs", head_lib), (Side::Head, "src/m.rs", head_m)];
        assert_eq!(outcome(&judge(&diff, &table(&files))), Err(NotPure::ResidualLine));
    }

    // flip-check: retroactive s2-07l.290
    /// `pub(in <path>)` は path が空でなく空白を含まない周だけ可視性（`is_scope` の `!` を消すと正しい形を剥がさずに
    /// item を読み落とし、`&&` を `||` にすると空の path と空白入りの path を剥がす）。
    #[test]
    fn move_proof_mutant_is_scope_reads_pub_in_with_a_plain_path_only() {
        assert_eq!(strip_visibility("pub(in crate::pipe) fn a() {"), ("pub(in crate::pipe)".to_owned(), "fn a() {"));
        for line in ["pub(in ) fn a() {", "pub(in a b) fn a() {"] {
            assert_eq!(strip_visibility(line), (String::new(), line), "{line}");
        }
    }

    // flip-check: retroactive s2-07l.290
    /// `where` 句で `{` が次の行へ落ちた struct（宣言行に `{` も `;` も無い）は次の item の直前で切れ、末尾の空行を含めない
    /// （`starts_item` を `false` にすると次の item を呑み、`item_end` の空行の刈り込みを `||` / `==` / `<` にすると
    /// 宣言行だけか空行込みになる）。
    #[test]
    fn move_proof_items_of_cuts_an_open_item_before_the_next_item() {
        let items = items_of("struct W<T>\nwhere\n    T: Copy,\n{\n    w: T,\n}\n\n\nfn b() {\n    x;\n}\n");
        let shown: Vec<(&str, (usize, usize))> = items.iter().map(|item| (item.name.as_str(), item.lines)).collect();
        assert_eq!(shown, [("struct W", (1, 6)), ("fn b", (9, 11))]);
    }
}
