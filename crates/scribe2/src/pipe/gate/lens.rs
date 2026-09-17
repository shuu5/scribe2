//! gate の lens の呼び出しと parse（lens に渡す本文の型の判定 [`lens_input`]・`--lens` の cmd の
//! 穴埋め・起動・stdout の JSON 1 行の読み・`verdict.json` の書き・[`super`] から純移動・
//! `s2-07l.286`）。判定の順と終端は親（[`super::gate`]）が持つ。

use super::findings::Tally;
use super::{Verdict, JSON_HEAD};
use crate::fleet::json_lite::{self, Value};
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
}

/// 判定に届かなかった周の戻り（集計は無い）。
pub(super) fn unjudged(evidence: String) -> Judged {
    Judged { verdict: Verdict::Inconclusive, evidence, tally: None }
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

/// lens の stdout から最後の JSON 行を読む。読めない周は INCONCLUSIVE。
///
/// **`findings` と `population` は必須 key である**（`s2-07l.188`・設計 §6 / §17）: どちらかが
/// 欠けた周・表に無い category・母集団 0 の周は、3 値が何であれ INCONCLUSIVE へ倒す——件数の
/// 無い verdict は「見て 0 件だった」と「見ていない」を弁別できず、後段がそれを裏書きする
/// （C10・fail-closed C11.2・既存の INCONCLUSIVE 経路なので便は測り直せる）。
fn parse_lens(text: &str) -> Judged {
    let pairs = match last_json_object(text) {
        Ok(parsed) => parsed,
        Err(reason) => return unjudged(format!("lens の{reason}")),
    };
    let get = |key: &str| {
        pairs
            .iter()
            .find(|(found_key, _)| found_key == key)
            .and_then(|(_, value)| value.as_str())
    };
    let evidence = get("evidence").unwrap_or_default().to_owned();
    let Some(verdict) = get("verdict").and_then(Verdict::parse) else {
        return unjudged("lens の verdict が 3 値でない".to_owned());
    };
    // **欠けた key を名指す**（どちらが無いのかで直す先が違う）。lens 自身の evidence（cap 超過
    // 等）も併せて残す——2 key を持たない出力の理由はここでしか残らない。
    let missing = |key: &str| format!("lens の verdict に {key} が無い（evidence: {evidence}）");
    let read = match (get("findings"), get("population")) {
        (None, _) => Err(missing("findings")),
        (_, None) => Err(missing("population")),
        (Some(counted), Some(population)) => {
            Tally::parse(counted, population).map_err(|reason| format!("lens の{reason}"))
        }
    };
    match read {
        Err(reason) => unjudged(reason),
        Ok(tally) => Judged { verdict, evidence, tally: Some(tally) },
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
