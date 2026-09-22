//! gate の lens の呼び出しと parse（lens に渡す本文の型の判定 [`lens_input`]・diff の畳み
//! [`fold_renamed_paths`]・`--lens` の cmd の穴埋め・起動・stdout の JSON 1 行の読み・`verdict.json` の書き・
//! [`super`] から純移動・`s2-07l.286`）。判定の順と終端は親（[`super::gate`]）が持つ。

use super::findings::{Tally, Unread};
use super::{Verdict, JSON_HEAD};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::Usage;
use crate::pipe::confine::{self, Confinement, Reason, Released};
use crate::pipe::git_bytes;
use crate::pipe::move_proof::{self, LensInput, Side};
use std::io::Write;
use std::path::Path;
use std::process::Stdio;

/// 純移動の機械証明（設計 §5.3・`s2-07l.266`）: 判定は純関数で、**file の読みだけ**をここが担う
/// （base 側は `<base>:<path>`・HEAD 側は `HEAD:<path>` を git から読む・読めない周は純移動でない側）。
pub(super) fn lens_input(worktree: &Path, base: &str, diff: &[u8]) -> LensInput {
    let read = |side: Side, path: &str| {
        let rev = match side {
            Side::Base => base,
            Side::Head => "HEAD",
        };
        git_bytes(worktree, &["show", &format!("{rev}:{path}")])
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    };
    move_proof::judge(&String::from_utf8_lossy(diff), &read)
}

/// diff の file の見出し（畳みの走査が hunk の終わりを知る）。
const FILE_HEAD: &[u8] = b"diff --git ";

/// hunk の見出し（本文の行は `-` / `+` / 空白 / `\` で始まり、この字面では始まらない）。
const HUNK_HEAD: &[u8] = b"@@";

/// HEAD 側の path の見出し（削除の file は `/dev/null` を指す＝畳みの面の外）。
const NEW_SIDE_HEAD: &[u8] = b"+++ ";

/// rename の対の旧 path の見出し。
const RENAME_FROM: &[u8] = b"rename from ";

/// rename の対の新 path の見出し。
const RENAME_TO: &[u8] = b"rename to ";

/// 畳みの面（`+++` の側・設計 gate-cost.md §41 形 4）。code file の同じ置換は lens が読む対象なので畳まない。
const FOLD_DIR: &[u8] = b"b/docs/design/";

/// 畳みの面の拡張子。
const FOLD_EXT: &[u8] = b".md";

/// 畳んだ hunk の本文に置く 1 行の印（`-N/+N` は省いた `-` / `+` の行の本数）。
fn elided_mark(minus: u64, plus: u64) -> String {
    format!("~ rename の置換だけの hunk（-{minus}/+{plus} 行）を省いた\n")
}

/// lens に渡す diff から、rename の対の path 置換だけの `docs/design/` の `.md` の hunk を 1 行の印に畳む
/// （設計 gate-cost.md §41 形 1・**pure**＝diff の字面だけを読み git を呼ばない）。
///
/// 戻りは（畳んだ後の本文, 畳んだ hunk 数, 畳んだ行数）で、行数は畳んだ hunk の `-` と `+` の行の和。畳むのは
/// 条件を全部満たす hunk だけ——判定の順は path の絞り（[`folds_here`]）→ 置換後の一致 → 各行の効き → 1 塊
/// （[`replaced_only`]）で、1 つでも欠ければ逐語のまま。header（`diff --git` / `---` / `+++` / `@@`）は残す。
/// rename の対が 0 の diff は置換が 1 行も効かないので本文そのまま（0/0）。
pub(super) fn fold_renamed_paths(diff: &[u8]) -> (Vec<u8>, u64, u64) {
    let mut fold = Fold { pairs: rename_pairs(diff), out: Vec::with_capacity(diff.len()), hunks: 0, lines: 0 };
    let mut docs = false;
    let mut hunk: Option<Vec<&[u8]>> = None;
    for line in diff.split_inclusive(|byte| *byte == b'\n') {
        if line.starts_with(FILE_HEAD) || line.starts_with(HUNK_HEAD) {
            fold.flush(hunk.take(), docs);
            fold.out.extend_from_slice(line);
            hunk = line.starts_with(HUNK_HEAD).then(Vec::new);
            continue;
        }
        match hunk.as_mut() {
            Some(body) => body.push(line),
            None => {
                if let Some(side) = line.strip_prefix(NEW_SIDE_HEAD) {
                    docs = folds_here(side);
                }
                fold.out.extend_from_slice(line);
            }
        }
    }
    fold.flush(hunk, docs);
    (fold.out, fold.hunks, fold.lines)
}

/// [`fold_renamed_paths`] の走査の途中の状態（出力と件数）。
struct Fold {
    /// rename の対（旧 path, 新 path・長い旧 path から順）。
    pairs: Vec<(Vec<u8>, Vec<u8>)>,
    /// 畳んだ後の本文。
    out: Vec<u8>,
    /// 畳んだ hunk 数。
    hunks: u64,
    /// 畳んだ行数。
    lines: u64,
}

impl Fold {
    /// 終わった hunk の本文を、畳めれば印 1 行・畳めなければ逐語で出す。
    fn flush(&mut self, hunk: Option<Vec<&[u8]>>, docs: bool) {
        let Some(body) = hunk else { return };
        let folded = if docs { replaced_only(&body, &self.pairs) } else { None };
        match folded {
            Some((minus, plus)) => {
                self.out.extend_from_slice(elided_mark(minus, plus).as_bytes());
                self.hunks = self.hunks.saturating_add(1);
                self.lines = self.lines.saturating_add(minus).saturating_add(plus);
            }
            None => body.iter().for_each(|line| self.out.extend_from_slice(line)),
        }
    }
}

/// `+++ ` の後の字面が畳みの面（`docs/design/` 配下の `.md`）を指すか。
fn folds_here(side: &[u8]) -> bool {
    let side = side.strip_suffix(b"\n").unwrap_or(side);
    side.strip_prefix(FOLD_DIR).is_some_and(|rest| rest.ends_with(FOLD_EXT))
}

/// diff の header から rename の対（旧 path, 新 path）を集める（長い旧 path から順）。
///
/// 長い順に並べるのは、短い旧 path が長い旧 path の頭に当たって先に置き換わるのを塞ぐため。空の旧 path は
/// 持たない（置換の走査が進まなくなる）。
fn rename_pairs(diff: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut pairs = Vec::new();
    let mut from: Option<&[u8]> = None;
    for line in diff.split(|byte| *byte == b'\n') {
        if let Some(old) = line.strip_prefix(RENAME_FROM) {
            from = Some(old);
        } else if let Some(new) = line.strip_prefix(RENAME_TO) {
            if let Some(old) = from.take().filter(|old| !old.is_empty()) {
                pairs.push((old.to_vec(), new.to_vec()));
            }
        }
    }
    pairs.sort_by_key(|(old, _)| std::cmp::Reverse(old.len()));
    pairs
}

/// hunk の本文が rename の置換だけで説明が付くなら（`-` の行数, `+` の行数）。付かなければ `None`。
///
/// 3 条件を順に見る: `-` の列に置換を当てた結果が `+` の列と順序も本数も同じ → `-` の各行が置換で 1 字以上
/// 変わる（行の並べ替えや同文の消して足すを隠さない）→ 本文が context・`-` の連続・`+` の連続・context の 1 塊
/// （`-` と `+` の間に context が在る＝置換を伴う行の移動を隠さない）。`\ No newline` の注記は行に数えない。
fn replaced_only(body: &[&[u8]], pairs: &[(Vec<u8>, Vec<u8>)]) -> Option<(u64, u64)> {
    let lines: Vec<&[u8]> = body
        .iter()
        .map(|line| line.strip_suffix(b"\n").unwrap_or(line))
        .filter(|line| !line.starts_with(b"\\"))
        .collect();
    let minus: Vec<&[u8]> = lines.iter().filter_map(|line| line.strip_prefix(b"-")).collect();
    let plus: Vec<&[u8]> = lines.iter().filter_map(|line| line.strip_prefix(b"+")).collect();
    let replaced: Vec<Vec<u8>> = minus.iter().map(|line| replace_paths(line, pairs)).collect();
    if replaced != plus {
        return None;
    }
    if replaced.iter().zip(&minus).any(|(new, old)| new == old) {
        return None;
    }
    if !one_block(&lines) {
        return None;
    }
    Some((line_count(minus.len()), line_count(plus.len())))
}

/// 本文の行の頭の列から前後の context を除いた残りに context が無い（`-` と `+` が 1 塊）か。
fn one_block(lines: &[&[u8]]) -> bool {
    let kinds: Vec<u8> = lines.iter().map(|line| line.first().copied().unwrap_or(b' ')).collect();
    !kinds.trim_ascii().contains(&b' ')
}

/// 1 行に全ての対の置換を 1 走査で当てる（同じ位置では長い旧 path が先・置き換えた字面は再び見ない）。
fn replace_paths(line: &[u8], pairs: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(line.len());
    let mut rest = line;
    while let Some((&byte, tail)) = rest.split_first() {
        let hit = pairs
            .iter()
            .find_map(|(old, new)| rest.strip_prefix(old.as_slice()).map(|after| (new, after)));
        match hit {
            Some((new, after)) => {
                out.extend_from_slice(new);
                rest = after;
            }
            None => {
                out.push(byte);
                rest = tail;
            }
        }
    }
    out
}

/// 行の本数を件数の型へ（溢れは上限へ丸める）。
fn line_count(count: usize) -> u64 {
    u64::try_from(count).unwrap_or(u64::MAX)
}

/// lens の scope の unit 名に載せる段の名。
pub(super) const LENS_STAGE: &str = "lens";

/// lens 1 本から得たもの（3 値・理由・findings の集計）。
///
/// 集計は**読めた周だけ** `Some` である（`s2-07l.188`）——2 key を持たない出力は判定に届いて
/// いないので [`Verdict::Inconclusive`] へ倒れ、`verdict.json` にも field が生えない（C10:
/// 「0 件だった」と「見ていない」を型で分ける）。
pub(super) struct Judged {
    /// 3 値。
    pub(super) verdict: Verdict,
    /// 理由（lens の evidence か、判定に届かなかった理由）。
    pub(super) evidence: String,
    /// findings の集計（読めた周だけ）。
    pub(super) tally: Option<Tally>,
    /// lens の**出力は在るが形が読めなかった**か（設計 gate-cost.md §29・`s2-07l.495`）。
    ///
    /// 立てるのは [`parse_lens`] の `Err` の分岐だけ——JSON でない・`verdict` が 3 値でない・key が
    /// 無い・集計の [`Unread::Malformed`]。「読めたが規則で断った」（母集団 0）と、箱の中の死・rc 非 0・
    /// 起動の失敗は伏せたまま（撃ち直しで向きが変わらない）。親（`decide`）はこの印の周だけ 1 回撃ち直す。
    pub(super) reread: bool,
    /// lens の claude の消費の 6 値（判定 object の `usage` / `turns` / `wall_ms`・設計 gate-cost.md §26 形 (2)）。
    ///
    /// **無くても判定を変えない**（古い lens・偽 lens の周は field を欠くだけ＝`None`）。読めた周だけ消費の event を書く。
    pub(super) usage: Option<Usage>,
}

/// 判定に届かなかった周の戻り（集計は無い・撃ち直しの印は伏せた側）。
pub(super) fn unjudged(evidence: String) -> Judged {
    Judged { verdict: Verdict::Inconclusive, evidence, tally: None, reread: false, usage: None }
}

/// 出力の形が読めなかった周の戻り（[`unjudged`] に撃ち直しの印を立てた形・[`parse_lens`] 専用）。
fn unreadable(evidence: String) -> Judged {
    Judged { reread: true, ..unjudged(evidence) }
}

/// `--lens` の cmd の `{contract}` / `{worktree}` を run の path へ置く。
///
/// **置く穴は 2 つである**（`{contract}` / `{worktree}`・出所 s2-07l.60）。1 つだった頃の
/// 理由（読み手を増やさない・planner 裁定 2026-09-10 Q1）は生きているが、lens に憲法を
/// 載せる経路が起動 cwd しか無く、tracked file に絶対 path は書けない（PUBLIC repo）ので
/// worktree は gate が埋めるほかない（planner 裁定 2026-09-10・admin Q2）。`--runner` 側
/// （[`crate::pipe::spawn`]）と共有するのは placeholder の**語彙**であって関数ではない。
///
/// **1 走査で埋める**。重ねて replace すると、先に埋めた path の中の `{worktree}` まで
/// 展開されうる（runner / lens の prompt と同じ理由）。
///
/// **渡すのは path であって本文ではない**。cmd は `sh -c` へ渡る 1 行なので、本文を
/// 埋めると契約の中の引用符 1 つで cmd の構造が変わる。
pub(super) fn substitute(cmd: &str, contract: &Path, worktree: &Path) -> String {
    crate::headless::fill(
        cmd,
        &[
            ("{contract}", &contract.display().to_string()),
            ("{worktree}", &worktree.display().to_string()),
        ],
    )
}

/// lens へ本文（diff か純移動の要約・[`LensInput::body`]）を stdin で渡し、stdout の JSON 1 行を読む。
///
/// 契約は cmd の `{contract}`（[`substitute`] が埋めた path）で渡る＝**stdin は本文専用**（FR5・
/// 要約も同じ 1 つの口で渡る）。
///
/// lens も scope で包む（設計 gate-cost.md §4.1 の 3 つ目）。**箱の中で殺された周は
/// INCONCLUSIVE** ——FR9 の既存極性そのままで、stdout を parse できない周と同じ経路である
/// （判定順は動かさない・便の成果は残っているので終端しない・設計 §4.2）。
pub(super) fn ask_lens(
    cmd: &str,
    worktree: &Path,
    body: &[u8],
    wrap: &confine::Wrap<'_>,
) -> (Judged, Option<Released>) {
    let (mut command, confinement) = confine::wrap_line(cmd, wrap);
    let spawned = command
        .current_dir(worktree)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(found) => found,
        Err(err) => return (unjudged(format!("lens を起動できない: {err}")), None),
    };
    if let Some(mut stdin) = child.stdin.take() {
        // 読まずに終える lens への write は EPIPE になる。**判定は出力で決める**ので
        // ここの失敗は理由にしない（take で drop され、lens は EOF を見る）。
        let _ = stdin.write_all(body);
    }
    let waited = child.wait_with_output();
    // **終端で scope を片付ける**（verify 行と同じ・設計 §4.4 errata）。判定は変えない。
    let scope = confine::release_scope(&confinement);
    (lens_outcome(waited, &confinement), scope)
}

/// 終わった lens の出力から判定を読む。
fn lens_outcome(waited: std::io::Result<std::process::Output>, confinement: &Confinement) -> Judged {
    let out = match waited {
        Ok(found) => found,
        Err(err) => return unjudged(format!("lens の出力を読めない: {err}")),
    };
    let text = String::from_utf8_lossy(&out.stdout);
    // **箱の中で死んだ周は rc より先に見る**（設計 §4.2）。溢れた箱で死んだ lens の rc を
    // 「lens が rc N で終わった」と記すと、外からの kill と弁別できない（lens-132d L1）。
    if confinement.confined() {
        let usage = confine::read_usage(&text);
        let killed = usage.oom_kill.is_some_and(|count| count >= 1).then_some(Reason::OomKill);
        let killed = killed.or_else(|| (out.status.code().is_none()).then_some(Reason::Signal));
        if let Some(reason) = killed {
            return unjudged(format!("lens が scope の中で死んだ（reason={}）", reason.as_str()));
        }
    }
    if !out.status.success() {
        let rc = out.status.code().unwrap_or(-1);
        return unjudged(format!("lens が rc {rc} で終わった"));
    }
    parse_lens(&text)
}

/// stdout の**最後の JSON 行**を 1 つの flat object に読む（lens の verdict と runner の
/// 質問 record が**共有する 1 本**・設計 pipeline-question.md §3）。読む条件は呼び手が持つ
/// （gate は lens の rc 0 の周・spawn は包みの rc [`crate::pipe::RC_QUESTION`] の周）。
pub(crate) fn last_json_object(text: &str) -> Result<Vec<(String, Value)>, String> {
    let found = text
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with(JSON_HEAD));
    let Some(line) = found else {
        return Err("出力に JSON 行が無い".to_owned());
    };
    json_lite::parse_object(line.trim()).map_err(|reason| format!("出力を読めない: {reason}"))
}

/// lens の stdout の最後の JSON 行から消費の 6 値を読む（gate の [`parse_lens`] と審査の口が**同じ 1 本**の形で読む・
/// 読めない・揃わない周は `None`＝判定は動かさない）。
pub(crate) fn lens_usage(text: &str) -> Option<Usage> {
    last_json_object(text).ok().and_then(|pairs| Usage::from_pairs(&pairs))
}

/// lens の stdout から最後の JSON 行を読む。読めない周は INCONCLUSIVE。
///
/// **`findings` と `population` は必須 key である**（`s2-07l.188`・設計 §6 / §17）: どちらかが
/// 欠けた周・表に無い category・母集団 0 の周は、3 値が何であれ INCONCLUSIVE へ倒す——件数の
/// 無い verdict は「見て 0 件だった」と「見ていない」を弁別できず、後段がそれを裏書きする
/// （C10・fail-closed C11.2・既存の INCONCLUSIVE 経路なので便は測り直せる）。
///
/// 読めなさは **2 値**に割る（設計 gate-cost.md §29）: 形が読めない周は [`unreadable`]（撃ち直しの印）、
/// 読めたが規則で断った周（母集団 0・[`Unread::Refused`]）は [`unjudged`]（印なし）。理由の字面は同じ。
fn parse_lens(text: &str) -> Judged {
    let pairs = match last_json_object(text) {
        Ok(parsed) => parsed,
        Err(reason) => return unreadable(format!("lens の{reason}")),
    };
    // 消費の 6 値は `findings` / `population` と同じ flat な object から読む（揃わない周は `None`・判定は動かさない）。
    Judged { usage: Usage::from_pairs(&pairs), ..judge_pairs(&pairs) }
}

/// 読めた object から 3 値と集計を読む（[`parse_lens`] の本体・消費の 6 値は呼び手が足す）。
fn judge_pairs(pairs: &[(String, Value)]) -> Judged {
    let get = |key: &str| {
        pairs
            .iter()
            .find(|(found_key, _)| found_key == key)
            .and_then(|(_, value)| value.as_str())
    };
    let evidence = get("evidence").unwrap_or_default().to_owned();
    let Some(verdict) = get("verdict").and_then(Verdict::parse) else {
        return unreadable("lens の verdict が 3 値でない".to_owned());
    };
    // **欠けた key を名指す**（どちらが無いのかで直す先が違う）。lens 自身の evidence（cap 超過
    // 等）も併せて残す——2 key を持たない出力の理由はここでしか残らない。
    let missing = |key: &str| format!("lens の verdict に {key} が無い（evidence: {evidence}）");
    let read = match (get("findings"), get("population")) {
        (None, _) => return unreadable(missing("findings")),
        (_, None) => return unreadable(missing("population")),
        (Some(counted), Some(population)) => Tally::parse(counted, population),
    };
    match read {
        Ok(tally) => Judged { verdict, evidence, tally: Some(tally), reread: false, usage: None },
        Err(unread) => {
            let evidence = format!("lens の{}", unread.reason());
            match unread {
                Unread::Malformed(_) => unreadable(evidence),
                Unread::Refused(_) => unjudged(evidence),
            }
        }
    }
}

/// 書きかけの `verdict.json` の拡張子（同じ dir に置いて rename する）。
const VERDICT_PARTIAL_EXT: &str = "json.partial";

/// `verdict.json` を **atomic に**書く（`s2-07l.147`・設計 gate-cost.md §6・**書きはこの 1 本**）。
///
/// 同じ dir の書きかけへ書いて rename する＝読み手（land の着地待ちの列）は途中の file を見ない。
/// 撃ち直しで判定を書き直す瞬間を「読めない」と測らせない（`Unmeasurable` の瞬間を出す側で塞ぐ）。
/// 書けなかった周は書きかけを残さず、本 file も生まれない（前の判定のまま）。
pub(super) fn write_verdict(path: &Path, text: &str) -> Result<(), String> {
    let partial = path.with_extension(VERDICT_PARTIAL_EXT);
    std::fs::write(&partial, text)
        .and_then(|()| std::fs::rename(&partial, path))
        .map_err(|err| {
            let _ = std::fs::remove_file(&partial);
            format!("{} を書けない: {err}", path.display())
        })
}

#[cfg(test)]
mod tests {
    // flip-check: moved s2-07l.286
    use super::super::verify::tests::{names, scratch};
    use super::{substitute, write_verdict};
    use std::path::Path;

    /// 判定の書きは完了後に書きかけを残さず、本 file の中身は完全（前の判定を丸ごと置き換える）。
    #[test]
    fn pipe_order_verdict_write_is_whole_and_leaves_no_partial() {
        let dir = scratch("whole");
        let path = dir.join("verdict.json");
        std::fs::write(&path, "{\"verdict\":\"PASS\",\"evidence\":\"a longer previous verdict\"}\n")
            .expect("前の判定を置ける");
        let body = "{\"schema\":1,\"verdict\":\"FAIL\"}\n";
        assert_eq!(write_verdict(&path, body), Ok(()));
        assert_eq!(std::fs::read_to_string(&path).ok().as_deref(), Some(body), "本 file の中身が完全");
        assert_eq!(names(&dir), ["verdict.json"], "書きかけを残さない");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 書けない周は本 file が生まれず（部分 file 0）、書きかけも残さない: 親 dir が無い / 行き先が dir で
    /// rename が断られる（書きかけは書けた後に落ちる形）。
    #[test]
    fn pipe_order_verdict_write_failure_leaves_no_partial_file() {
        let dir = scratch("unwritable");
        let absent = dir.join("absent").join("verdict.json");
        assert!(write_verdict(&absent, "{}\n").is_err(), "親 dir が無い");
        assert!(!absent.exists(), "本 file が生まれない");
        let blocked = dir.join("verdict.json");
        std::fs::create_dir_all(blocked.join("inside")).expect("行き先を dir で塞げる");
        assert!(write_verdict(&blocked, "{}\n").is_err(), "rename が断られる");
        assert!(blocked.is_dir(), "行き先は元のまま");
        assert_eq!(names(&dir), ["verdict.json"], "書きかけを残さない");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `--lens` の cmd の穴は **2 つ**（契約 / worktree）で、どちらも埋まる。
    ///
    /// worktree 側が埋まらないと lens は `--worktree` を受け取れず rc 1 で断り、gate は
    /// INCONCLUSIVE になる（＝憲法の載らない判定は出ないが、便も進まない）。
    #[test]
    fn gate_substitute_fills_contract_and_worktree() {
        let line = substitute(
            "lens --contract {contract} --worktree {worktree}",
            Path::new("/state/CONTRACT-MARKER.toml"),
            Path::new("/runs/WORKTREE-MARKER"),
        );
        assert_eq!(
            line,
            "lens --contract /state/CONTRACT-MARKER.toml --worktree /runs/WORKTREE-MARKER",
        );
    }

    /// **1 走査で埋める**。埋めた値の中の marker は展開しない。
    ///
    /// 重ねて replace すると、契約 path の中に `{worktree}` が在るだけで cmd の構造へ
    /// 触れられる（runner / lens の prompt と同じ経路）。
    #[test]
    fn gate_substitute_does_not_expand_filled_values() {
        let line = substitute(
            "lens --contract {contract}",
            Path::new("/state/{worktree}/CONTRACT-MARKER.toml"),
            Path::new("/runs/WORKTREE-MARKER"),
        );
        assert_eq!(line, "lens --contract /state/{worktree}/CONTRACT-MARKER.toml");
    }
}
