//! gate の記録と診断（`verify.jsonl` の record [`step_record`]・赤い行の stderr の診断 file・
//! lens への入力の通知 [`record_notice`]・便の写しの読み・[`super`] から純移動・`s2-07l.286`）。
//! 判定の順と終端は親（[`super::gate`]）が持つ。

use super::verify::{is_unreadable, recorded_rc, run_checks_admitted, Admit, Check, Checks, Step};
use super::{Detection, DetectionSkip, Gate};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::{append_line, read_all, LockPolicy};
use crate::fleet::{Stage, SCHEMA};
use crate::pipe::admission;
use crate::pipe::confine::Reason;
use crate::pipe::declaration::Effective;
use crate::pipe::move_proof::{self, LensInput};
use crate::pipe::{contract_path, git_bytes, run_dir, verify_log_path, vessel_path};
use std::path::Path;

/// 赤い verify 行の stderr を残す診断 file の名（`verify.jsonl` と同じ dir）。
///
/// **機械はこの file を読まない**。`verify.jsonl` の record（`schema` / `n` / `rc` /
/// `cmd`）は跨版の契約なので形を変えず、「なぜ赤かったか」だけを別の面へ逃がす。
const STDERR_LOG_FILE: &str = "verify.stderr.log";

/// 診断 file に残す stderr の行数（末尾から数える）。
///
/// **判定に効く値ではない**（人が理由を読むための窓の大きさ）ので規則行にしない
/// ——rules manifest は判定を動かす閾値の置き場である（憲法 C1 / C5）。**末尾**を
/// 採るのは、落ちた command が理由を最後に出すためである。
pub(super) const STDERR_TAIL_LINES: usize = 20;

/// nextest の落ちた歯の進捗行の頭（`FAIL [ <秒>] (<i>/<n>) <binary> <歯の名>`・cargo-nextest 0.9.143 の実測）。
///
/// 行頭の空白を剥がして比べる（進捗行は桁揃えの空白を前に持つ）。`TRY n FAIL [` は頭が違うので当たらない。
const NEXTEST_FAIL: &str = "FAIL [";

/// 区間を閉じる nextest の行の頭（次の進捗行と Summary・[`NEXTEST_FAIL`] も閉じる）。
const NEXTEST_ENDS: [&str; 3] = [NEXTEST_FAIL, "PASS [", "Summary ["];

/// 落ちた歯ごとの stderr の小見出し（`FAIL [` の行の後に nextest が出す・行頭の空白は剥がして比べる）。
const NEXTEST_STDERR: &str = "stderr ───";

/// 写しの中で落ちた歯の区間を始める見出しの頭（`--- failed=<歯の名>`・段の見出し `## ` とは別の字面）。
const SECTION_HEAD: &str = "--- failed=";

/// nextest 形の stderr から読んだ、落ちた歯（record の `failed=` と `failed_stderr=`・設計 pipeline.md §35）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failed {
    /// 最初の `FAIL [` の行の歯の名（record の `failed=`）。
    pub name: String,
    /// 落ちた歯ごとの区間を順に継いだ字面（歯 1 本あたり [`STDERR_TAIL_LINES`] 行が上限・区間の無い周は空）。
    pub sections: String,
}

/// stderr の写し（診断 file へ書く字面と、落ちた歯）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Excerpt {
    /// 落ちた歯の区間（在れば）+ 末尾 [`STDERR_TAIL_LINES`] 行。
    pub(super) text: String,
    /// `FAIL [` の行が在った周だけ `Some`（無い周は field を書かない・C10）。
    pub(super) failed: Option<Failed>,
}

/// stderr の全文から写しを作る（**pure**・gate も主実測も [`super::verify::run_line_captured`] からここを通る・C2）。
///
/// 読むのは nextest の字面の閉じた 2 形だけである: `FAIL [` の進捗行（歯の名）と `stderr ───` の小見出し（区間の頭）。
/// 区間 = 小見出しの次の行から次の進捗行（[`NEXTEST_ENDS`]）の直前まで・末尾の空行を落として**区間の末尾**
/// [`STDERR_TAIL_LINES`] 行（panic は区間の最後に出る）。`FAIL [` の無い stderr（clippy 等の行）は従来どおり末尾だけ。
pub(super) fn excerpt_of(stderr: &str) -> Excerpt {
    let lines: Vec<&str> = stderr.lines().collect();
    let from = lines.len().saturating_sub(STDERR_TAIL_LINES);
    let tail = lines.get(from..).unwrap_or_default().join("\n");
    let mut first = None;
    let mut sections: Vec<String> = Vec::new();
    let mut owner: Option<&str> = None;
    let mut open: Option<(&str, Vec<&str>)> = None;
    for line in &lines {
        let head = line.trim_start();
        if NEXTEST_ENDS.iter().any(|end| head.starts_with(end)) {
            sections.extend(open.take().map(|(name, body)| section_text(name, &body)));
            owner = head.strip_prefix(NEXTEST_FAIL).and_then(tooth_name);
            first = first.or(owner);
        } else if let Some((_, body)) = open.as_mut() {
            body.push(line);
        } else if head == NEXTEST_STDERR {
            open = owner.map(|name| (name, Vec::new()));
        }
    }
    sections.extend(open.take().map(|(name, body)| section_text(name, &body)));
    let Some(name) = first else {
        return Excerpt { text: tail, failed: None };
    };
    let sections = sections.join("\n");
    let text = if sections.is_empty() { tail } else { format!("{sections}\n{tail}") };
    Excerpt { text, failed: Some(Failed { name: name.to_owned(), sections }) }
}

/// `FAIL [` の後ろの残りから歯の名（`]` の後の最後の語・進捗 `(i/n)` と binary id は飛ばす・無ければ `None`）。
fn tooth_name(rest: &str) -> Option<&str> {
    rest.split_once(']')?.1.split_whitespace().last()
}

/// 区間 1 つの字面（見出し 1 行 + 末尾の空行を落とした本文の末尾 [`STDERR_TAIL_LINES`] 行）。
fn section_text(name: &str, body: &[&str]) -> String {
    let kept = body.iter().rposition(|line| !line.trim().is_empty()).map_or(0, |last| last.saturating_add(1));
    let body = body.get(..kept).unwrap_or_default();
    let from = body.len().saturating_sub(STDERR_TAIL_LINES);
    let mut text = format!("{SECTION_HEAD}{name}");
    for line in body.get(from..).unwrap_or_default() {
        text.push('\n');
        text.push_str(line);
    }
    text
}

/// lens への入力の通知の頭（**段の見出し `## ` とは別の字面**・[`record_notice`]）。
///
/// 段の見出し（[`append_diagnosis`]）を数える読み手が通知を段と混同しないための 1 文字である
/// ——通知は段ではないので `n` も `rc` も持たない。
const NOTICE_HEAD: &str = "# ";

/// 通知の `reason=` が「理由を持たない」（＝要約が組めた周）ことを表す字面。
///
/// **0 とも空とも書かない**（C10: 「理由が無い」と「理由を測れなかった」を融合しない）。
const NO_REASON: &str = "-";

/// 包みが stdout の終端に出す行の見出し（[`crate::pipe::confine`] の `script` が printf する固定形）。
///
/// record の `line=` は**この行を剥がした残り**の末尾 1 行である（設計 gate-cost.md §5.1）——包みの
/// 測定行を「道具の判定行」として書くと、`peak_mb` と同じ数が別の名で 2 度残る。字面は
/// [`crate::pipe::confine::read_usage`] が読む見出しと同じで、in-file の歯が両者の一致を測る（ずれると
/// 剥がせない＝終端行が `line` に化ける）。
pub(super) const USAGE_HEAD: &str = "confine-usage";

/// verify 各行を撃ち、行ごとの rc を `verify.jsonl` へ逐条で残す。
///
/// **赤い行だけ** stderr の末尾を診断 file（[`STDERR_LOG_FILE`]）へも append する。
/// rc だけでは「何がどう赤いか」が便の外から読めず、gate が落ちるたびに人が同じ行を
/// 手で撃ち直して理由を取り直すことになる（実測 2026-09-10・`s2-07l.49`）。緑の行は
/// 残さない——読む理由が無い出力で診断 file を埋めると、赤い行の見出しが埋もれる。
///
/// 検出線を撃たない周（[`Detection::Skip`]・設計 pipeline.md §30）は写しの検出線の代わりに**空の列**を渡し
/// （行を撃つ実装は 1 本のまま）、その段の位置に skip record を 1 本置く（[`records_of`]・main 実測と同じ形）。
/// 撃っていないので `detection_unmeasured` は `None` のままである。前周の record を持ち越す周（[`Detection::Carry`]・
/// 設計 §40 形 (b)）も同じ位置に写しの record を 1 本置く（[`carried_record`]）。撃つ周の検出線の record は便の diff の
/// `patch_id` を持つ（[`detection_record`]）。
///
/// 撃った直後に検出線の判定行と出力を run dir の**周ごとの置き場**へ写す（[`keep_detection`]・設計
/// gate-cost.md §15 (1)）——便の worktree の出力は追随の撃ち直しが作り直すので、写さないと 1 周目の
/// 生存の一覧が消える（`.286` run 1 の実測）。
pub(super) fn record_verify(entry: &Gate<'_>, worktree: &Path, base: &str) -> Result<Counted, String> {
    let frozen = frozen_copy(entry)?;
    let admit = Admit {
        state_dir: entry.state_dir,
        run: entry.run,
        rules: admission::Rules {
            sizes: admission::Sizes {
                job_mb: entry.limits.job_memory_mb,
                reserve_mb: entry.limits.reserve_memory_mb,
            },
            cap: entry.limits.mutants_jobs,
            wait_s: entry.limits.slot_wait_s,
            policy: entry.policy,
        },
    };
    // 撃つ周だけ便の diff の patch-id を測り、検出線の record に `patch_id=` で残す（設計 §40 形 (b)・撃つ前の木）。
    // 純移動の周は母集団の diff を渡し、落とした `+` 行の本数を record の `pure-move=` で残す（設計 §14 約束 2 / 3）。
    let mut pure_move = None;
    let (detection, placed, patch): (Vec<String>, Option<Placed<'_>>, Option<String>) = match &entry.detection {
        Detection::Run => {
            let lines = aimed_lines(entry, frozen.detection_verify())?;
            let (lines, dropped) = population_lines(entry, worktree, base, lines)?;
            pure_move = dropped;
            let patch = if lines.is_empty() { None } else { super::patch_id(worktree, base, "HEAD") };
            (lines, None, patch)
        }
        Detection::Skip(reason) => (Vec::new(), Some(Placed::Skip(Skipped::detection(*reason, None))), None),
        Detection::Carry(carried) => (Vec::new(), Some(Placed::Carried(carried)), None),
    };
    let checks = Checks {
        worktree,
        base,
        contract: entry.contract,
        common: frozen.common_verify(),
        detection: &detection,
        host: entry.limits.breaker(),
    };
    let steps = run_checks_admitted(&checks, Some(&admit));
    keep_detection(entry, worktree, &steps)?;
    let path = verify_log_path(entry.state_dir, entry.run);
    let tail_path = path.with_file_name(STDERR_LOG_FILE);
    let mut red = 0;
    // 遮断器が閉じて撃たなかった最初の行の `n`（設計 gate-cost.md §32 約束 5・検出線の rc 2 と同じ形）。
    // skip record を挟む周も record の `n` と一致させるため、record 列の側で数える。
    let mut busy = None;
    // 段①が読めなかった周（rc -1）は**赤に数えない**——record は残す（現物を消さない）が、
    // 判定は「測れなかった」側へ倒す（`s2-07l.65`）。箱ごと OOM で殺された行も同じ極性で
    // ある（rc に依らず「測れなかった」・設計 gate-cost.md §4.2）。
    let unreadable = steps.iter().any(is_unreadable);
    let killed = steps.iter().find_map(box_kill);
    // 検出線が rc 2（測れなかった）で終えた周も同じ極性（`s2-07l.331`・設計 pipeline.md §5.3）。
    // 最初にそうなった行の `n` を持つ（理由に名指す・record は残す）。skip record は検出線の段の
    // 位置＝検出線の行より後ろにしか入らないので、この `n` は record の `n` と一致する。
    let unmeasured = steps
        .iter()
        .position(detection_unmeasured)
        .map(|index| index as u64 + 1);
    for record in records_placed(&steps, placed, Marks { patch: patch.as_deref(), pure_move }) {
        if let Some(step) = record.step {
            if step.is_closed() {
                // 撃っていない行は赤でも診断の対象でもない（record だけ残す）。
                busy = busy.or(Some(record.n));
            } else {
                if step.rc != 0 && !is_unreadable(step) && box_kill(step).is_none() && !detection_unmeasured(step) {
                    red += 1;
                }
            }
        }
        record.diagnose(&tail_path, entry.policy)?;
        append_line(&path, &record.body, entry.policy).map_err(|err| err.to_string())?;
    }
    Ok(Counted { red, unreadable, killed, detection_unmeasured: unmeasured, busy })
}

/// 検出線の的を渡す旗（`cargo xtask mutants-diff` の `--targets <file>`・設計 gate-cost.md §16 (2)）。
const TARGETS_FLAG: &str = "--targets";

/// 的の列を 1 行 1 本で置く file の名（run dir 直下・gate の周ごとに書き直す＝値は契約の写しの 1 つ）。
const TARGETS_FILE: &str = "targets";

/// 契約が的（[`crate::pipe::contract::TARGETS`]）を持つ便の検出線（設計 gate-cost.md §16 (3)）。
///
/// 的の在る便は、的の列を run dir の [`TARGETS_FILE`] へ書き、写しの検出線の各行の末尾に
/// `--targets <その path>` を足す（的を絞った口で撃つ）。**的の無い便は写しの行を 1 字も変えない**＝diff の
/// 追加行を母集団にする従来の経路のまま。行の穴（`{jobs}` 等）は残すので、受付と箱の選び方は変わらない。
/// 契約の写しを読めない周は理由を返す（的を空と読んで従来の経路へ黙って倒さない・C10）。
fn aimed_lines(entry: &Gate<'_>, lines: &[String]) -> Result<Vec<String>, String> {
    let targets = crate::pipe::contract::targets_of(&contract_path(entry.state_dir, entry.run))?;
    if targets.is_empty() {
        return Ok(lines.to_vec());
    }
    let path = run_dir(entry.state_dir, entry.run).join(TARGETS_FILE);
    write_copy(&path, &format!("{}\n", targets.join("\n")))?;
    let arg = shell_word(&path.display().to_string());
    Ok(lines.iter().map(|line| format!("{line} {TARGETS_FLAG} {arg}")).collect())
}

/// 検出線の母集団の diff を渡す旗（`cargo xtask mutants-diff` の `--diff <file>`・設計 gate-cost.md §14 約束 2）。
const DIFF_FLAG: &str = "--diff";

/// 母集団の diff を置く file の名（run dir 直下・純移動の周だけ・gate の周ごとに書き直す）。
const POPULATION_FILE: &str = "population.diff";

/// 純移動と証明された便の検出線（設計 gate-cost.md §14 約束 2 / 4）。
///
/// lens の入力が要約（[`LensInput::Summary`]）になる便は、動いた item の区間の `+` 行を落とした diff
/// （[`move_proof::population`]）を run dir の [`POPULATION_FILE`] へ書き、各行の末尾に `--diff <その path>` を足して
/// 落とした本数を返す。**要約にならない便（[`LensInput::Diff`]）と diff を読めない周は行を 1 字も変えない**＝
/// 道具が `git diff` の追加行を母集団にする従来の経路のまま（読めない周の判定は段①が持つ）。
fn population_lines(entry: &Gate<'_>, worktree: &Path, base: &str, lines: Vec<String>) -> Result<(Vec<String>, Option<usize>), String> {
    if lines.is_empty() {
        return Ok((lines, None));
    }
    let Some(diff) = git_bytes(worktree, &["diff", &format!("{base}..HEAD")]) else {
        return Ok((lines, None));
    };
    let LensInput::Summary(summary) = super::lens::lens_input(worktree, base, &diff) else {
        return Ok((lines, None));
    };
    let population = move_proof::population(&String::from_utf8_lossy(&diff), &summary);
    let path = run_dir(entry.state_dir, entry.run).join(POPULATION_FILE);
    write_copy(&path, &population.text)?;
    let arg = shell_word(&path.display().to_string());
    Ok((lines.iter().map(|line| format!("{line} {DIFF_FLAG} {arg}")).collect(), Some(population.dropped)))
}

/// shell の 1 語（安全な字だけの字面はそのまま・他は単引用符で包む）。
fn shell_word(text: &str) -> String {
    let plain = |found: char| found.is_ascii_alphanumeric() || "_./-+=:,@%".contains(found);
    if !text.is_empty() && text.chars().all(plain) {
        return text.to_owned();
    }
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// 周ごとの検出線の写しの置き場（run dir 直下・この下に**周の番号の dir** が並ぶ・設計 gate-cost.md §15 (1)）。
///
/// 1 つの置き場へ上書きしないのは、追随の撃ち直しが便の worktree の出力を作り直すためである
/// ——上書きすると 1 周目の生存の一覧が 2 周目で消える。
const COPY_DIR: &str = "detection";

/// 周の置き場の中の判定行の写し（1 行・record の `line=` と**同じ字面**）。
const COPY_LINE_FILE: &str = "line";

/// 周の置き場の中の段の秒（無い周は file を置かない＝読み手は `secs=` を出さない・C10）。
const COPY_SECS_FILE: &str = "secs";

/// 出力が 1 つも無かった周に置く marker（設計 gate-cost.md §15 (2)）。
///
/// **空の写しを「0 件だった」に倒さない**（C10 / NFR4）——出力の在る 0 件の周は `outcomes.json` が在り、
/// 出力を書かなかった周はこの marker が在る。
const COPY_ABSENT_OUTPUT: &str = "outputs-absent";

/// 判定行の無い周に残す 1 行（設計 gate-cost.md §15 (2)）。
///
/// **0 件の判定行と別の字面**である——道具の 1 行は `mutants-diff: total=0 …` の形で、こちらは
/// 判定行の形と衝突しない頭を持つ（空の file も 0 件の行も書かない・C10）。
const COPY_ABSENT_LINE: &str = "detection-line: absent";

/// 検出線の出力の置き場（便の worktree からの相対・cargo-mutants の出力 dir）。
const DETECTION_OUT: [&str; 4] = ["target", "mutants-diff", "out", "mutants.out"];

/// 写す出力の名（**在る物だけ**写す・数え直さないので中身は読まない）。
const DETECTION_OUTPUTS: [&str; 2] = ["outcomes.json", "missed.txt"];

/// 撃った検出線の判定行と出力を run dir の周ごとの置き場へ写す（設計 gate-cost.md §15 (1)(2)）。
///
/// 撃たなかった周（[`super::Detection::Skip`]）は段が列に無いので**何も写さない**（撃っていない周の
/// 置き場を作ると、撃った 0 件の周と読み分けられない）。出力が 1 つも無い周は [`COPY_ABSENT_OUTPUT`] の
/// marker を、判定行の無い周は [`COPY_ABSENT_LINE`] の 1 行を残す。
///
/// 数は**数え直さない**（設計 §15 (4)）——写すのは道具が出した 1 行と出力の byte だけである。
fn keep_detection(entry: &Gate<'_>, worktree: &Path, steps: &[Step]) -> Result<(), String> {
    let Some(step) = steps.iter().find(|step| step.stage == Check::Detection) else {
        return Ok(());
    };
    let copies = run_dir(entry.state_dir, entry.run).join(COPY_DIR);
    let dir = copies.join(round_of(entry, &copies).to_string());
    std::fs::create_dir_all(&dir).map_err(|err| format!("{} を作れない: {err}", dir.display()))?;
    let line = step.line.as_deref().unwrap_or(COPY_ABSENT_LINE);
    write_copy(&dir.join(COPY_LINE_FILE), &format!("{line}\n"))?;
    if let Some(secs) = step.secs {
        write_copy(&dir.join(COPY_SECS_FILE), &secs.to_string())?;
    }
    let out = DETECTION_OUT.iter().fold(worktree.to_path_buf(), |path, leaf| path.join(leaf));
    let mut copied = 0_usize;
    for name in DETECTION_OUTPUTS {
        copied = copied.saturating_add(copy_output(&out.join(name), &dir.join(name))?);
    }
    if copied == 0 {
        write_copy(&dir.join(COPY_ABSENT_OUTPUT), "")?;
    }
    Ok(())
}

/// この gate の周の番号（**[`Stage::Gated`] の件数の次**・設計 gate-cost.md §15 (1)）。
///
/// `Gated` は周の終端で 1 件追記されるので、写しを書く時点の件数は**済んだ周の数**である。既に在る
/// 写しの最大の番号も併せて見るのは、event log を読めない周に 1 周目の写しを潰さないためである
/// （上書きしないことが写しの目的そのもの）。
fn round_of(entry: &Gate<'_>, copies: &Path) -> u64 {
    let gated = read_all(entry.state_dir).map_or(0, |events| {
        let count = events
            .iter()
            .filter(|event| event.run == entry.run && event.stage == Some(Stage::Gated))
            .count();
        u64::try_from(count).unwrap_or(u64::MAX)
    });
    gated.max(kept_rounds(copies).into_iter().max().unwrap_or(0)).saturating_add(1)
}

/// 既に在る写しの周の番号（番号でない名の dir は母集団に入らない）。
fn kept_rounds(copies: &Path) -> Vec<u64> {
    std::fs::read_dir(copies)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|found| found.file_name().to_str().and_then(|name| name.parse().ok()))
        .collect()
}

/// 写しの file を 1 つ書く。
fn write_copy(path: &Path, body: &str) -> Result<(), String> {
    std::fs::write(path, body).map_err(|err| format!("{} を書けない: {err}", path.display()))
}

/// 出力 1 つを写す（写せた本数を返す）。
///
/// **無い周は 0**（写す物が無いだけで失敗ではない）で、在るのに読めない周は `Err`——「無い」と
/// 「読めない」を融合しない（C10）。
fn copy_output(from: &Path, to: &Path) -> Result<usize, String> {
    match std::fs::copy(from, to) {
        Ok(_) => Ok(1),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(err) => Err(format!("{} を写せない: {err}", from.display())),
    }
}

/// 周 1 つの写し（[`detection_copies`] の要素）。
pub struct DetectionCopy {
    /// 判定行の逐語（写しが無い・読めない・空の周は [`COPY_ABSENT_LINE`]）。
    pub line: String,
    /// 段の秒（写しの無い周は `None`＝読み手は `secs=` を出さない・C10）。
    pub secs: Option<u64>,
}

/// run dir の**周ごとの写し**を周の番号順に読む（`pipe show` の判定行の出所・設計 gate-cost.md §15 (3)）。
///
/// 母集団は写しの dir だけで **`verify.jsonl` は読まない**（判定行の出所を 2 つ持たない・C2）。写しの
/// 無い便（gate 前・検出線を撃たない便）は空の列である。
pub fn detection_copies(dir: &Path) -> Vec<DetectionCopy> {
    let copies = dir.join(COPY_DIR);
    let mut rounds: Vec<u64> = kept_rounds(&copies);
    rounds.sort_unstable();
    rounds.iter().map(|round| copy_of(&copies.join(round.to_string()))).collect()
}

/// 周 1 つの写しを読む（判定行が無い・読めない・空の周は [`COPY_ABSENT_LINE`]）。
fn copy_of(dir: &Path) -> DetectionCopy {
    let line = std::fs::read_to_string(dir.join(COPY_LINE_FILE))
        .ok()
        .and_then(|text| text.lines().next().map(str::to_owned))
        .filter(|found| !found.is_empty())
        .unwrap_or_else(|| COPY_ABSENT_LINE.to_owned());
    let secs = std::fs::read_to_string(dir.join(COPY_SECS_FILE))
        .ok()
        .and_then(|text| text.trim().parse().ok());
    DetectionCopy { line, secs }
}

/// lens へ何を渡したかの 1 行を、段の記録と**同じ log**（[`STDERR_LOG_FILE`]）へ残す（設計 §21 (3)）。
///
/// **rc に依らず・要約の周も diff の周も残す**。理由の 1 行は従来 [`LensInput::notice`] が
/// `Outcome.err` に載せるだけで、rc 0 で終わった gate の周は呼び手が捨てると便の外から二度と
/// 読めなかった（`.286` の実測: `run.stderr` 0 byte・rc 0 で「なぜ diff を渡したか」が引けない）。
///
/// 置き場が `verify.jsonl` ではなく診断 file なのは、record の**通し番号 `n`** を行数から導く読み手が
/// 在るためである（[`crate::pipe::land`] の追随が引き継ぎの skip record を書く周）——record でない行を
/// 混ぜると `n` が飛ぶ。診断 file の読み手は人だけで、通知は段の見出しを持たない 1 行に閉じる。
pub(super) fn record_notice(entry: &Gate<'_>, input: &LensInput, elided: (u64, u64)) -> Result<(), String> {
    let path = verify_log_path(entry.state_dir, entry.run).with_file_name(STDERR_LOG_FILE);
    append_line(&path, &notice_line(input, elided), entry.policy).map_err(|err| err.to_string())?;
    Ok(())
}

/// 通知の字面（`# lens-input=<kind> reason=<語>`・**pure**）。
///
/// `kind` は判定行が出すのと同じ語（[`LensInput::kind`]）、`語` は純移動でない理由（`NotPure`）で、
/// 要約が組めた周は [`NO_REASON`]。**[`crate::pipe::move_proof`] は触らない**——あちらの
/// [`LensInput::notice`] は呼び手の端末へ出す `pipe:` の 1 行で、ここは run dir に残る記録である。
///
/// lens 用の diff から hunk を 1 つ以上畳んだ周だけ末尾に ` elided=<hunk 数>/<行数>` を足す（設計 gate-cost.md
/// §41 形 3）。0 の周は従来の字面のまま。
fn notice_line(input: &LensInput, elided: (u64, u64)) -> String {
    let reason = match input {
        LensInput::Diff(why) => why.as_str(),
        LensInput::Summary(_) => NO_REASON,
    };
    let line = format!("{NOTICE_HEAD}lens-input={} reason={reason}", input.kind());
    match elided {
        (0, _) => line,
        (hunks, lines) => format!("{line} elided={hunks}/{lines}"),
    }
}

/// 赤い行と撃ち直した行の見出し + stderr の末尾を診断 file へ残す（緑で撃ち直しも無い行は残さない）。
///
/// 撃ち直した行（`retried_from` が `Some`・設計 gate-cost.md §21）は **1 回目の見出し
/// （`## n=<i> rc=<rc> retry=1 cmd=…`）+ 1 回目の末尾を先に**書き、その後に 2 回目を従来の見出しで書く
/// ——2 回目が緑でも書く（1 回目を捨てると「なぜ撃ち直したか」が便の外から読めない・C10）。
/// `verify.jsonl` の record は 2 回目の 1 本だけで、2 段になるのは診断 file だけである。
fn append_diagnosis(path: &Path, policy: LockPolicy, n: u64, step: &Step) -> Result<(), String> {
    if let Some((rc, tail)) = &step.retried_from {
        let head = format!("## n={n} rc={rc} retry=1 cmd={}", step.cmd);
        append_stderr(path, policy, &head, tail)?;
    }
    if step.rc != 0 || step.retried_from.is_some() {
        let head = format!("## n={n} rc={} cmd={}", step.rc, step.cmd);
        append_stderr(path, policy, &head, &step.stderr)?;
    }
    Ok(())
}

/// 撃たなかった周の材料（record の `skipped=` の段と `reason=`、主実測だけが持つ `tree=`）。
///
/// **構築は下の 3 つの口だけ**である（field は本 file に閉じる）——段と理由は別の軸で、
/// 呼び手が任意の組を書けると `kind=detection skipped=regate` のような無い形が生まれる。
#[derive(Debug, Clone, Copy)]
pub struct Skipped<'a> {
    /// 省いた段。
    stage: SkippedStage,
    /// 省いた理由。
    reason: DetectionSkip,
    /// land した木（主実測の record だけ・gate の再撃ちは木を持たない＝field を書かない）。
    tree: Option<&'a str>,
}

impl<'a> Skipped<'a> {
    /// 検出線の段だけを省いた周（`kind=detection skipped=detection`・主実測は木を持つ）。
    pub fn detection(reason: DetectionSkip, tree: Option<&'a str>) -> Self {
        Self { stage: SkippedStage::Detection, reason, tree }
    }

    /// 追随の再 gate を**丸ごと**省いて前周の判定を引き継いだ周（`kind=gate skipped=regate`・設計 §33）。
    ///
    /// 木は持たない——撃っていないので「どの木を測ったか」が無い（`tree` を書くと測った形に読める）。
    pub fn regate(reason: DetectionSkip) -> Self {
        Self { stage: SkippedStage::Regate, reason, tree: None }
    }

    /// land の主実測を**丸ごと**省いた周（`kind=main skipped=main reason=same-tree`・設計 gate-cost.md §27・
    /// ADR-0043 §2.2）: gate が判定した木と着地の木が同じ＝同じ木を同じ verify で撃ち直すだけなので撃たない。
    ///
    /// 理由は木の一致だけ（[`DetectionSkip::SameTree`]・面の外は主実測の省略の理由にならない）で、木は
    /// **必ず取る**（`&str`・`Option` にしない）——「どの木を gate が測ったか」が無い skip record は
    /// 撃っていない緑と読み分けられない。
    pub fn main(tree: &'a str) -> Self {
        Self { stage: SkippedStage::Main, reason: DetectionSkip::SameTree, tree: Some(tree) }
    }
}

/// 撃たなかったのはどの段か（record の `skipped=` と `kind=` の字面）。
///
/// **理由（[`DetectionSkip`]）とは別の軸**である（run 2 の裁定 2026-09-16）——`outside-scope` は
/// 検出線を省く周にも再 gate を省く周にも同じ意味で立つので、理由の enum に段を足すと
/// 2 つの軸が 1 つの列に潰れる。値は 3 つで、本 file の外へは出ない。
#[derive(Debug, Clone, Copy)]
enum SkippedStage {
    /// 検出線の段（gate / 主実測の中の 1 行）。
    Detection,
    /// 追随の再 gate 1 周（設計 §33 (i)）。
    Regate,
    /// land の主実測 1 周（設計 gate-cost.md §27・ADR-0043 §2.1）。
    Main,
}

impl SkippedStage {
    /// record の `skipped=` の字面。
    fn as_str(self) -> &'static str {
        match self {
            Self::Detection => "detection",
            Self::Regate => "regate",
            Self::Main => KIND_MAIN,
        }
    }

    /// record の `kind=` の字面（段の名＝verify 行の kind か、gate 1 周 / 主実測 1 周そのものか）。
    fn kind(self) -> &'static str {
        match self {
            Self::Detection => Check::Detection.as_str(),
            Self::Regate => KIND_GATE,
            Self::Main => KIND_MAIN,
        }
    }
}

/// gate 1 周を省いた record の `kind=`（verify 行の段ではないので [`Check`] の値を使わない）。
const KIND_GATE: &str = "gate";

/// land の主実測 1 周を省いた record の `kind=` と `skipped=`（同じ 1 語・verify 行の段ではない）。
const KIND_MAIN: &str = "main";

/// 書く record 1 本（通し番号 `n`・本文・撃った段なら元の [`Step`]）。
pub struct Record<'a> {
    /// `verify.jsonl` の `n`（1 始まり・skip record も数える）。
    pub n: u64,
    /// 1 行の JSON。
    pub body: String,
    /// 撃った段（skip record は `None`）。
    pub step: Option<&'a Step>,
}

impl Record<'_> {
    /// この record の段の診断（見出し + stderr の写し）を `path` へ append する（gate の `verify.stderr.log` と
    /// 主実測の `verify-main.stderr.log` が**同じ 1 本**で書く・設計 pipeline.md §35 (3)）。
    ///
    /// 残すのは撃って赤かった段と撃ち直した段だけで、skip record と遮断器が閉じて撃たなかった段は何も書かない。
    pub fn diagnose(&self, path: &Path, policy: LockPolicy) -> Result<(), String> {
        match self.step {
            Some(step) if !step.is_closed() => append_diagnosis(path, policy, self.n, step),
            _ => Ok(()),
        }
    }
}

/// 撃った段の record 列を組む（`verify.jsonl` と land の `verify-main.jsonl` が**同じ形・同じ位置**で書く）。
///
/// 検出線を省いた周は、その段の位置（[`Check::Contract`] の直前・契約の行が無ければ末尾）に skip record を
/// 1 本挟み、`n` は挟んだ record も含めて通しで振る（**撃たなかった事実を黙って落とさない**・設計 §30）。
pub fn records_of<'a>(steps: &'a [Step], skipped: Option<Skipped<'_>>) -> Vec<Record<'a>> {
    records_placed(steps, skipped.map(Placed::Skip), Marks { patch: None, pure_move: None })
}

/// 撃った検出線の record だけが持つ印（便の diff の patch-id と、純移動の周に落とした `+` 行の本数）。
#[derive(Clone, Copy)]
struct Marks<'a> {
    /// `patch_id=`（設計 §40 形 (b)）。
    patch: Option<&'a str>,
    /// `pure-move=`（設計 gate-cost.md §14 約束 3・要約にならない便は `None`＝field を欠く）。
    pure_move: Option<usize>,
}

/// 検出線の段の位置に置く、撃っていない record 1 本（skip か、前周の写し）。
enum Placed<'a> {
    /// 省いた周（[`skip_record`]）。
    Skip(Skipped<'a>),
    /// 前周の record を持ち越した周（[`carried_record`]・設計 §40 形 (b)）。
    Carried(&'a Carried),
}

impl Placed<'_> {
    /// `n` 番の record の本文。
    fn body(&self, number: u64) -> String {
        match self {
            Self::Skip(skip) => skip_record(number, *skip),
            Self::Carried(carried) => carried_record(number, carried),
        }
    }
}

/// [`records_of`] の本体（置く record は skip か写し・撃った検出線の record は `marks` を持つ）。
fn records_placed<'a>(steps: &'a [Step], placed: Option<Placed<'_>>, marks: Marks<'_>) -> Vec<Record<'a>> {
    let mut records: Vec<Record<'a>> = Vec::new();
    let mut pending = placed;
    for step in steps {
        if step.stage == Check::Contract {
            if let Some(place) = pending.take() {
                let n = next_number(records.len());
                records.push(Record { n, body: place.body(n), step: None });
            }
        }
        let n = next_number(records.len());
        records.push(Record { n, body: marked_record(n, step, marks), step: Some(step) });
    }
    if let Some(place) = pending {
        let n = next_number(records.len());
        records.push(Record { n, body: place.body(n), step: None });
    }
    records
}

/// 撃った検出線の record が持つ便の diff の patch-id の field（設計 §40 形 (b)）。
const PATCH_ID_FIELD: &str = "patch_id";

/// 写しの record が持つ前周の `n` の field（写しと実測を区別する印・C10）。
const CARRIED_FIELD: &str = "carried";

/// 撃った 1 段の record（[`step_record`]）に、**検出線の段だけ** `patch_id=<id>` を足す（`patch` が `None` の周と
/// 他の段は [`step_record`] と同じ字面・schema は 1 のまま・設計 §40 形 (b)）。
pub fn detection_record(number: u64, step: &Step, patch: Option<&str>) -> String {
    marked_record(number, step, Marks { patch, pure_move: None })
}

/// 純移動の周に撃った検出線の record が持つ、落とした `+` 行の本数の field（設計 gate-cost.md §14 約束 3）。
const PURE_MOVE_FIELD: &str = "pure-move";

/// [`detection_record`] の本体: 検出線の段だけ `patch_id=` と、撃った周の `pure-move=` を足す（撃たなかった行は欠く）。
fn marked_record(number: u64, step: &Step, marks: Marks<'_>) -> String {
    let mut fields = step_fields(number, step);
    if step.stage == Check::Detection {
        if let Some(found) = marks.patch {
            fields.push((PATCH_ID_FIELD, Value::Str(found.to_owned())));
        }
        if let Some(dropped) = marks.pure_move.filter(|_| !step.is_closed()) {
            fields.push((PURE_MOVE_FIELD, Value::Num(u64::try_from(dropped).unwrap_or(u64::MAX))));
        }
    }
    json_lite::write_object(&fields)
}

/// 前周の検出線の record（持ち越しの材料・[`carried_from`] だけが作る・設計 §40 形 (b)）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Carried {
    /// 前周の record の `n`（写しの `carried=`）。
    n: u64,
    /// 前周の record の field（`carried` を除いて並びのまま・`n` は写す周に振り直す）。
    pairs: Vec<(String, Value)>,
}

/// 便の `verify.jsonl` から、`patch` と同じ `patch_id` を持つ前周の検出線の record を引く（設計 §40 形 (b)）。
///
/// 前周の record は**最後に撃った（または写した）検出線の record**（`kind=detection` ∧ `skipped` を持たない）である。
/// 引けるのはその record が `patch_id` を持ち、`patch` と同じで、rc 0 の周だけ。`patch` が無い・file を読めない・
/// 1 行でも JSON として読めない・record が無い・`patch_id` が無い・違う周は `None`（呼び手は撃つ＝fail-closed）。
pub fn carried_from(state_dir: &Path, run: &str, patch: Option<&str>) -> Option<Carried> {
    let patch = patch?;
    let text = std::fs::read_to_string(verify_log_path(state_dir, run)).ok()?;
    let mut last = None;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let pairs = json_lite::parse_object(line.trim()).ok()?;
        let detection = field(&pairs, "kind") == Some(&Value::Str(Check::Detection.as_str().to_owned()));
        if detection && field(&pairs, "skipped").is_none() {
            last = Some(pairs);
        }
    }
    let pairs = last?;
    if field(&pairs, PATCH_ID_FIELD) != Some(&Value::Str(patch.to_owned())) || field(&pairs, "rc") != Some(&Value::Num(0)) {
        return None;
    }
    let Some(Value::Num(n)) = field(&pairs, "n") else {
        return None;
    };
    let n = *n;
    let pairs = pairs.into_iter().filter(|(key, _)| key != CARRIED_FIELD).collect();
    Some(Carried { n, pairs })
}

/// record の field 1 つ（無ければ `None`）。
fn field<'a>(pairs: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    pairs.iter().find(|(found, _)| found == key).map(|(_, value)| value)
}

/// 持ち越した周の record（前周の record を `n` だけ振り直して写し、末尾に `carried=<前周の n>`・schema は 1 のまま）。
///
/// `cmd` / `rc` / `line` / `patch_id` は前周の値のまま（撃っていない＝新しい値が無い）で、実測との区別は
/// `carried` の有無が持つ（C10）。
pub fn carried_record(number: u64, carried: &Carried) -> String {
    let mut fields: Vec<(&str, Value)> = carried
        .pairs
        .iter()
        .map(|(key, value)| match key.as_str() {
            "n" => ("n", Value::Num(number)),
            found => (found, value.clone()),
        })
        .collect();
    fields.push((CARRIED_FIELD, Value::Num(carried.n)));
    json_lite::write_object(&fields)
}

/// 既に積んだ record 数から次の `n`（1 始まり）。
pub fn next_number(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX).saturating_add(1)
}

/// 撃たなかった段の record（`kind=<段> skipped=<段> [tree=<sha>] reason=<理由>`・schema は 1 のまま）。
///
/// 検出線を省いた周は `kind=detection skipped=detection`、追随の再 gate を省いて前周の判定を
/// 引き継いだ周は `kind=gate skipped=regate`（設計 §33 (2)）、land の主実測を省いた周は
/// `kind=main skipped=main`（設計 gate-cost.md §27）。どれも**撃たなかった事実を
/// 黙って落とさない**ための 1 本で、読み手は `skipped=` の非空でまとめて拾える。
pub fn skip_record(number: u64, skipped: Skipped<'_>) -> String {
    let mut fields = vec![
        ("schema", Value::Num(SCHEMA)),
        ("n", Value::Num(number)),
        ("kind", Value::Str(skipped.stage.kind().to_owned())),
        ("skipped", Value::Str(skipped.stage.as_str().to_owned())),
    ];
    if let Some(tree) = skipped.tree {
        fields.push(("tree", Value::Str(tree.to_owned())));
    }
    fields.push(("reason", Value::Str(skipped.reason.as_str().to_owned())));
    json_lite::write_object(&fields)
}

/// 撃った 1 段の record（`verify.jsonl` と land の `verify-main.jsonl` が**同じ形**で書く）。
///
/// **schema は 1 のまま任意 field を足す**（古い読み手は未知の field を無視する・
/// ADR-0017 §2.1 の event と同じ足し方・設計 gate-cost.md §5）。
///
/// `secs=` は**撃った段の全部**（kind と rc を問わず）に書く任意 field である（設計 §26 形 (1)・1 便の
/// 時間を器が測る）。`line=` は **kind と rc を問わず**、stdout の末尾 1 行が在った step 全部に書く（設計 §5.1・
/// planner 裁定 2026-09-14）。検出線の行だけに絞ると flip-check の `base-retried=N`（rc 0 の周の
/// stdout にしか出ない）が残らず、行の種類で分岐する形にもなる（C2）。
pub fn step_record(number: u64, step: &Step) -> String {
    json_lite::write_object(&step_fields(number, step))
}

/// [`step_record`] の field の並び（[`detection_record`] が末尾に `patch_id` を足す）。
fn step_fields(number: u64, step: &Step) -> Vec<(&'static str, Value)> {
    let mut fields = vec![
        ("schema", Value::Num(SCHEMA)),
        ("n", Value::Num(number)),
        ("rc", Value::Num(recorded_rc(step.rc))),
        ("cmd", Value::Str(step.cmd.clone())),
        ("jobs", Value::Num(step.jobs)),
        ("confined", Value::Bool(step.confined)),
        ("peak_mb", Value::Str(shown_peak(step.peak_mb))),
        ("kind", Value::Str(step.stage.as_str().to_owned())),
    ];
    if let Some(reason) = step.reason {
        fields.push(("reason", Value::Str(reason.as_str().to_owned())));
    }
    if let Some(slot) = &step.slot {
        fields.push(("slot", Value::Str(slot.clone())));
    }
    if let Some(why) = step.slot_why {
        fields.push(("slot_why", Value::Str(why.as_str().to_owned())));
    }
    if let Some(released) = step.scope {
        fields.push(("scope", Value::Str(released.as_str().to_owned())));
    }
    // 撃ち直した周だけ `retried=1`（設計 gate-cost.md §21・撃ち直さない周は field を欠く・C10）。
    if step.retried_from.is_some() {
        fields.push(("retried", Value::Num(1)));
    }
    if let Some(line) = &step.line {
        fields.push(("line", Value::Str(line.clone())));
    }
    // 段の壁時計（設計 gate-cost.md §26 形 (1)）。**撃った段だけ**が持つ——write-set 照合は撃つ
    // process を持たず（[`super::verify::Step::secs`] が `None`）、撃たなかった段は [`skip_record`] で、
    // どちらも field を欠く（0 と書かない＝「測って 0 秒」と弁別する・C10）。
    if let Some(secs) = step.secs {
        fields.push(("secs", Value::Num(secs)));
    }
    // 器の健康の遮断器の印（設計 gate-cost.md §32 約束 5 / 7・任意 field＝schema は 1 のまま）。空いていた周は欠く。
    if let Some(mark) = step.host {
        fields.push(("host", Value::Str(mark.as_str().to_owned())));
    }
    // nextest 形の stderr に `FAIL [` の行が在った周だけ（設計 pipeline.md §35・無い周は field を欠く・C10）。
    if let Some(failed) = &step.failed {
        fields.push(("failed", Value::Str(failed.name.clone())));
        if !failed.sections.is_empty() {
            fields.push(("failed_stderr", Value::Str(failed.sections.clone())));
        }
    }
    fields
}

/// `verify.jsonl` の数え上げ（赤の本数と、2 つの「測れなかった」）。
pub(super) struct Counted {
    /// rc≠0 だった verify 行の本数（**測れた行**だけを数える）。
    pub(super) red: u64,
    /// 段①（write-set 照合）で diff の path を読めなかったか。
    pub(super) unreadable: bool,
    /// 箱の中で殺された行の理由（在れば）。
    pub(super) killed: Option<Reason>,
    /// 検出線が rc 2（測れなかった）で終えた行の `n`（最初の 1 行・在れば）。
    pub(super) detection_unmeasured: Option<u64>,
    /// 器の健康の遮断器が閉じて撃たなかった行の `n`（最初の 1 行・在れば・設計 gate-cost.md §32 約束 5）。
    pub(super) busy: Option<u64>,
}

/// 検出線（`Check::Detection`）の **rc 2 = 測れなかった**か（`s2-07l.331`・設計 pipeline.md §5.3）。
///
/// `cargo xtask mutants-diff` の rc は 3 値である: 0 = 測定・1 = deny 昇格後の赤（R-C12-1・裁定後だけ）・
/// 2 = 測れなかった（baseline が落ちた・道具が起こせない）。段を問わず rc≠0 を赤に数えると、2 が
/// FAIL（判定に届いた便の終端）に化ける（`s2-07l.329` run 1 の実測）。**rc 1 の検出線と、検出線以外の
/// rc 2 は従来どおり赤**——除外は「検出線 ∧ rc 2」の 1 点だけで、rc の意味は道具の側が持つ。
///
/// 撃ち直すか（[`super::verify::run_checks_admitted`]・設計 gate-cost.md §21）も**同じ 1 点**で見る
/// （判定と撃ち直しの条件を 2 面に持たない）。
pub(super) fn detection_unmeasured(step: &Step) -> bool {
    step.stage == Check::Detection && step.rc == 2
}

/// peak の字面。**読めない周は `-`**（0 と書かない＝「測って 0」と弁別する）。
fn shown_peak(peak_mb: Option<u64>) -> String {
    peak_mb.map_or_else(|| "-".to_owned(), |mb| mb.to_string())
}

/// 箱の中で殺された段か（殺された理由・設計 gate-cost.md §4.2 / §4.3）。
///
/// **rc では見ない**——kernel が殺した process の rc は行の内容次第で何にでもなる。根拠は
/// 包みが出した `memory.events` の `oom_kill` で、包みごと死んで終端行を出せなかった周は
/// signal 死をその代理にする。**包めなかった周は当たらない**（素の行が外から kill された
/// 周を「箱が溢れた」と読まない）。
///
/// **検出線の行の `oom_kill` は数えない**（`s2-07l.228`・設計 §4.2）。変異ごとの test process を
/// 箱の中で起こす道具は、無限 loop の変異 1 つが kernel に殺されてもその死を吸収して完走する
/// ——測れた周であり、判定は rc と outcomes が持つ。record の `reason=oom-kill` は残る
/// （[`step_record`] は `step.reason` をそのまま書く）。包みごとの signal 死は終端行が無い＝
/// 測れていないので、検出線でも従来どおり数える。
fn box_kill(step: &Step) -> Option<Reason> {
    if !step.confined {
        return None;
    }
    let absorbs_oom = step.stage == Check::Detection;
    step.reason.filter(|found| {
        matches!(*found, Reason::Signal) || (matches!(*found, Reason::OomKill) && !absorbs_oom)
    })
}

/// 便の写し（共通 verify と検出線）を読む。
///
/// **読むのは `<state_dir>/pipe/<run>/vessel.toml` だけ**である——repo や worktree の
/// `.vessel.toml` を読み直すと、便の実装が自分の検証を書き換えられる（ADR-0010 §2.4）。
fn frozen_copy(entry: &Gate<'_>) -> Result<Effective, String> {
    let path = vessel_path(entry.state_dir, entry.run);
    Effective::load(&path)
        .map_err(|errors| {
            let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
            format!("{} を読めない: {}", path.display(), lines.join(" / "))
        })
}

/// 見出し 1 行と stderr の末尾を診断 file へ 1 件 append する。
///
/// **見出しは呼び手が組む**。n / rc / cmd をそのまま渡す形にすると引数が 5 個を超え、
/// C4 の線（`too_many_arguments`）に当たる。書き口は [`append_line`] の 1 本のまま
/// （lock を持つ writer を 2 本にしない・憲法 C6.3）。
fn append_stderr(path: &Path, policy: LockPolicy, head: &str, stderr: &str) -> Result<(), String> {
    append_line(path, &format!("{head}\n{stderr}"), policy).map_err(|err| err.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{excerpt_of, notice_line, skip_record, Skipped, STDERR_TAIL_LINES};
    use crate::pipe::move_proof::{LensInput, NOT_PURE};

    /// nextest 形の stderr（cargo-nextest 0.9.143 の実出力の形）: 落ちた歯 2 本が即時の区間を持ち、1 本目は末尾から
    /// 遠い位置（後ろに PASS の進捗行が 30 本）・2 本目の区間は上限より長い・Summary の後に `FAIL [` が 2 本繰り返す。
    fn nextest_stderr() -> String {
        let mut lines: Vec<String> = vec![
            "────────────".to_owned(),
            " Nextest run ID 0 with nextest profile: default".to_owned(),
            "    Starting 33 tests across 1 binary".to_owned(),
            "        FAIL [   0.003s] (  1/33) scribe2 pipe::alpha::breaks".to_owned(),
            "  stdout ───".to_owned(),
            String::new(),
            "    running 1 test".to_owned(),
            "    stdout-only-line".to_owned(),
            String::new(),
            "  stderr ───".to_owned(),
            "    thread 'pipe::alpha::breaks' panicked at src/alpha.rs:9:5:".to_owned(),
            "    assertion failed: alpha-panic-body".to_owned(),
            String::new(),
        ];
        for index in 2..32 {
            lines.push(format!("        PASS [   0.001s] ({index:>3}/33) scribe2 pipe::green::t{index}"));
        }
        lines.push("        FAIL [   0.004s] ( 32/33) scribe2 pipe::beta::breaks".to_owned());
        lines.push("  stderr ───".to_owned());
        for index in 0..25 {
            lines.push(format!("    beta-line-{index:02}"));
        }
        lines.push("    assertion failed: beta-panic-body".to_owned());
        lines.push(String::new());
        lines.push("        PASS [   0.001s] ( 33/33) scribe2 pipe::green::last".to_owned());
        lines.push("────────────".to_owned());
        lines.push("     Summary [   0.004s] 33 tests run: 31 passed, 2 failed, 0 skipped".to_owned());
        lines.push("        FAIL [   0.003s] (  1/33) scribe2 pipe::alpha::breaks".to_owned());
        lines.push("        FAIL [   0.004s] ( 32/33) scribe2 pipe::beta::breaks".to_owned());
        lines.push("error: test run failed".to_owned());
        lines.join("\n")
    }

    /// `failed=` は最初の `FAIL [` の歯の名で、区間は歯ごとに順に残る（1 本目の panic は末尾 N 行の外でも残る・
    /// 区間は次の進捗行の直前で閉じる・歯 1 本あたり末尾 [`STDERR_TAIL_LINES`] 行が上限・設計 pipeline.md §35）。
    #[test]
    fn pipe_verify_failed_names_the_first_tooth_and_keeps_each_section() {
        let stderr = nextest_stderr();
        let tail: Vec<&str> = stderr.lines().rev().take(STDERR_TAIL_LINES).collect();
        assert!(!tail.iter().any(|line| line.contains("alpha-panic-body")), "前提: 1 本目の panic は末尾 N 行の外");
        let excerpt = excerpt_of(&stderr);
        let failed = excerpt.failed.expect("FAIL [ の行が在る周は failed を持つ");
        assert_eq!(failed.name, "pipe::alpha::breaks", "最初の落ちた歯: {failed:?}");
        let sections: Vec<&str> = failed.sections.lines().collect();
        let heads: Vec<&str> = sections.iter().copied().filter(|line| line.starts_with("--- failed=")).collect();
        assert_eq!(
            heads,
            ["--- failed=pipe::alpha::breaks", "--- failed=pipe::beta::breaks"],
            "落ちた歯ごとに順に 1 区間（Summary の後の繰り返しは区間を持たない）: {}",
            failed.sections
        );
        assert_eq!(
            sections.get(1..3),
            Some(&["    thread 'pipe::alpha::breaks' panicked at src/alpha.rs:9:5:", "    assertion failed: alpha-panic-body"][..]),
            "1 本目の区間は stderr の小見出しの次から・末尾の空行は落ちる: {}",
            failed.sections
        );
        assert_eq!(sections.get(3).copied(), heads.get(1).copied(), "区間は次の進捗行の直前で閉じる（PASS 行を含まない）");
        let beta: Vec<&str> = sections.get(4..).unwrap_or_default().to_vec();
        assert_eq!(beta.len(), STDERR_TAIL_LINES, "2 本目の区間は上限の行数: {beta:?}");
        assert_eq!(beta.last().copied(), Some("    assertion failed: beta-panic-body"), "上限は区間の末尾から数える");
        assert!(!failed.sections.contains("stdout-only-line"), "stdout の小見出しの中身は区間に入らない");
        assert!(excerpt.text.starts_with(&failed.sections), "写しは区間が先: {}", excerpt.text);
        assert!(excerpt.text.ends_with("error: test run failed"), "写しは末尾 N 行も持つ: {}", excerpt.text);
        assert!(excerpt.text.contains("alpha-panic-body"), "末尾に入らない歯の panic も写しに残る");
    }

    /// `FAIL [` の無い stderr（clippy 等の行）は `failed` を持たず、写しは従来どおり末尾 [`STDERR_TAIL_LINES`] 行だけ。
    /// `stderr ───` の字面が在っても `FAIL [` の後でなければ区間にしない。
    #[test]
    fn pipe_verify_failed_absent_without_fail_lines_keeps_only_the_tail() {
        let mut lines: Vec<String> = vec!["  stderr ───".to_owned()];
        lines.extend((0..30).map(|index| format!("warning: lint-{index:02}")));
        let stderr = format!("{}\n", lines.join("\n"));
        let excerpt = excerpt_of(&stderr);
        assert_eq!(excerpt.failed, None, "FAIL [ の行が無い周は failed を書かない");
        let want: Vec<String> = (10..30).map(|index| format!("warning: lint-{index:02}")).collect();
        assert_eq!(excerpt.text, want.join("\n"), "末尾 N 行だけ（末尾の改行は区切りとして落ちる）");
        assert_eq!(excerpt_of("").text, "", "空の stderr は空の写し");
    }

    /// 通知の字面は `# lens-input=diff reason=<語>` で、語は `NotPure` の全 variant（母集団 5）が
    /// **それぞれ別の字面**で載る（設計 §21 (4)・要約の周の `-` は e2e の `pipe_gate_notice_summary_` が測る）。
    #[test]
    fn notice_line_carries_the_kind_and_the_reason() {
        let lines: Vec<String> = NOT_PURE
            .iter()
            .map(|why| notice_line(&LensInput::Diff(*why), (0, 0)))
            .collect();
        assert_eq!(
            lines.first().map(String::as_str),
            Some("# lens-input=diff reason=unreadable"),
            "頭 + kind + 理由の語: {lines:?}"
        );
        let unique: std::collections::BTreeSet<&String> = lines.iter().collect();
        assert_eq!(unique.len(), NOT_PURE.len(), "理由の語は variant ごとに違う: {lines:?}");
    }

    /// (d) 主実測の口で作った record の字面は `kind=main skipped=main tree=<sha> reason=same-tree` の順で固定
    /// （設計 gate-cost.md §27・ADR-0043 §2.2・`s2-07l.464`）。木は口が `&str` で必ず取る＝木の無い構築は型が拒む。
    #[test]
    fn main_skip_record_line_is_fixed() {
        let line = skip_record(1, Skipped::main("0123456789abcdef0123456789abcdef01234567"));
        assert_eq!(
            line,
            r#"{"schema":1,"n":1,"kind":"main","skipped":"main","tree":"0123456789abcdef0123456789abcdef01234567","reason":"same-tree"}"#
        );
    }
}
