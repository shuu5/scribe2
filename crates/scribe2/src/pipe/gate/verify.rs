//! gate の verify 行の実行（段の列 [`Check`] と、行を撃つ 1 本 [`run_line_captured`]・
//! [`super`] から純移動・`s2-07l.286`）。判定の順と終端は親（[`super::gate`]）が持つ。

use super::record::{detection_unmeasured, STDERR_TAIL_LINES, USAGE_HEAD};
use super::{UNADMITTED_JOBS, WRITE_SET_CMD};
use crate::pipe::admission::{self, Grant};
use crate::pipe::confine::{self, Confinement, Reason, Released, Usage};
use crate::pipe::contract::Contract;
use crate::pipe::declaration::{BASE_HOLE, JOBS_HOLE};
use crate::pipe::git_bytes;
use crate::pipe::refuse;
use crate::seat::RuleRead;
use std::path::Path;

/// 機械検証の段。**適用順序は [`CHECKS`] の並びが唯一の権威**である（憲法 C2）。
///
/// 順序を散文の注記で持たないための形である——enum が段の集合を閉じ、[`run_checks`] の
/// 網羅 match が新しい variant を必ずこの並びへ置かせる（置き忘れは compile error）。
/// 閉じた enum と全 variant の並びを対で持つのは器の既定の形である（[`super::VERDICTS`] /
/// `rules::ALL` と同型）。並びが宣言順のままであることは
/// [`crate::order::is_declaration_order`] を通す歯が測る（ADR-0013 §2.2・限界は §2.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// diff が契約の write-set の内に収まっているか（ADR-0009 §2.4）。
    WriteSet,
    /// 便の写し `vessel.toml` の共通 verify（`{base}` を置換して撃つ）。
    Common,
    /// 便の写しの検出線（`detection-verify`・穴は共通 verify と同じ・設計 gate-cost.md §5）。
    Detection,
    /// 契約の verify（穴を持たない）。
    Contract,
}

/// [`Check`] の全 variant。**この並びが適用順序である**。
pub const CHECKS: &[Check] = &[Check::WriteSet, Check::Common, Check::Detection, Check::Contract];

impl Check {
    /// 段の名（scope の unit 名と record の `kind` に載せる字面）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WriteSet => WRITE_SET_CMD,
            Self::Common => "common",
            Self::Detection => "detection",
            Self::Contract => "contract",
        }
    }
}

/// 撃った 1 段の結果。
pub struct Step {
    /// どの段か（record の `kind=`）。
    pub stage: Check,
    /// 実行した行の字面（置換後）。write-set 照合は [`WRITE_SET_CMD`]。
    pub cmd: String,
    /// process の rc（起動できない周は -1）。
    pub rc: i32,
    /// stderr の末尾（緑の段は空）。
    pub stderr: String,
    /// cgroup の scope で包めたか（record の `confined=`・設計 gate-cost.md §4）。
    pub confined: bool,
    /// 包めなかった理由 か 箱の中で起きたこと（record の `reason=`・閉じた enum）。
    pub reason: Option<Reason>,
    /// scope の peak（MiB）。**読めない周は `None`**＝record は `-`（0 と書かない）。
    pub peak_mb: Option<u64>,
    /// 段の壁時計（秒・record の `secs=`・設計 gate-cost.md §26 形 (1)）。
    ///
    /// **撃つ process を持たない段（[`unwrapped`]）は `None`**＝record は field を欠く（0 と書かない
    /// ＝「測って 0 秒」と弁別する・C10）。撃った段は起動できなかった周も秒を持つ（[`Fired::secs`]）。
    pub secs: Option<u64>,
    /// この行に渡した実効 jobs（record の `jobs=`）。
    pub jobs: u64,
    /// 受付の結果（record の `slot=`・受付を通らない行は `None`）。
    pub slot: Option<String>,
    /// 受付が測れなかった理由（record の `slot_why=`・測れた周と受付を通らない行は `None`）。
    pub slot_why: Option<admission::Unreadable>,
    /// 行の終端で scope を片付けた結果（record の `scope=`・包めなかった周と `Gone` は `None`）。
    pub scope: Option<Released>,
    /// 行が stdout に出した末尾の非空 1 行（record の `line=`・逐語・設計 gate-cost.md §5.1）。
    ///
    /// **kind と rc を問わず**運ぶ（xtask の検出線の 1 行も flip-check の判定行も、rc 0 で通った周の
    /// stdout にしか現れない）。無い周は `None`＝field を欠く（空文字を書かない・C10）。
    pub line: Option<String>,
    /// 撃ち直した周の **1 回目の rc と stderr の末尾**（record の `retried=1`・診断 file の 1 段目・設計
    /// gate-cost.md §21）。
    ///
    /// 検出線が rc 2（測れなかった）で終えた周だけ同じ材料でもう 1 回撃ち、**2 回目の `Step` がこの欄に
    /// 1 回目を運ぶ**（1 回目の `Step` は列に積まない・撃ち直した事実を record に残す・C10）。
    /// 「撃ち直した」はこの欄の有無に畳む（別の bool を持たない）。撃ち直さない行と 1 回目は `None`。
    pub retried_from: Option<(i32, String)>,
}

/// 撃つ process を持たない段（write-set 照合）の封じ込め欄。
///
/// 包めなかったのではなく**包む対象が無い**（Rust で照合するだけで子 process を起こさない）。
/// 理由を持たせないのはそのためである。**秒を持たないのも同じ理由**である（測る process が無い
/// ＝0 秒で撃ったのではない・設計 gate-cost.md §26 形 (1)）。
fn unwrapped(cmd: String, rc: i32, stderr: String) -> Step {
    Step {
        stage: Check::WriteSet,
        cmd,
        rc,
        stderr,
        confined: false,
        reason: None,
        peak_mb: None,
        secs: None,
        jobs: UNADMITTED_JOBS,
        slot: None,
        slot_why: None,
        scope: None,
        line: None,
        retried_from: None,
    }
}

/// 受付の材料（gate だけが持つ・land の main 実測は受付を通らない）。
pub struct Admit<'a> {
    /// 置き場（host の slot dir はこの親から導く）。
    pub state_dir: &'a Path,
    /// 便 id（札の名に載る）。
    pub run: &'a str,
    /// rules 行の値。
    pub rules: admission::Rules,
}

/// 検証を撃つ材料。
pub struct Checks<'a> {
    /// 撃つ場所。
    pub worktree: &'a Path,
    /// 便の base（`{base}` の実値）。
    pub base: &'a str,
    /// 読み込み済みの契約。
    pub contract: &'a Contract,
    /// **便の写しの**共通 verify（repo / worktree の宣言は読み直さない）。
    pub common: &'a [String],
    /// **便の写しの**検出線。land の main 実測は木が gate と同じ周に空を渡す（ADR-0021 §2.4）。
    pub detection: &'a [String],
}

/// 全段を**順序どおり**に撃つ。
///
/// **gate も land もこの 1 本を通る**——2 本になると gate が通した行と main で撃った行の
/// 意味が静かにずれる（行を撃つ実装を [`run_line_captured`] 1 本に保っているのと同じ理由）。
pub fn run_checks(checks: &Checks<'_>) -> Vec<Step> {
    run_checks_admitted(checks, None)
}

/// 全段を**順序どおり**に撃つ（受付を持つ形）。[`run_checks`] はこれの受付なしの形である。
///
/// `admit` が在る周だけ、`{jobs}` を持つ共通 verify の行が host の受付を通る（設計 gate-cost.md
/// §3.2・§3.3）。行を撃つ実装はこの 1 本のままである。
///
/// **検出線の行だけ**、rc 2（測れなかった・[`detection_unmeasured`]）で終えた周は同じ材料で
/// もう 1 回撃つ（[`refire`]・設計 gate-cost.md §21）。gate も land もここを通るので撃ち直しの
/// 挙動も 1 本である（land の木が同じ周は検出線を撃たないので撃ち直しも起きない）。
pub fn run_checks_admitted(checks: &Checks<'_>, admit: Option<&Admit<'_>>) -> Vec<Step> {
    // 封じ込めの 3 線は 1 便で 1 度だけ読む（行ごとに manifest を開き直さない）。
    let caps = confine::Caps::embedded();
    let mut steps = Vec::new();
    for check in CHECKS {
        let (lines, holes): (&[String], bool) = match *check {
            Check::WriteSet => {
                steps.push(check_write_set(checks));
                continue;
            }
            Check::Common => (checks.common, true),
            Check::Detection => (checks.detection, true),
            Check::Contract => (&checks.contract.verify, false),
        };
        for line in lines {
            let n = steps.len().saturating_add(1);
            let entry = Fire { checks, raw: line.as_str(), holes, stage: *check, n };
            let first = fire(&entry, caps, admit);
            let step = if detection_unmeasured(&first) { refire(first, &entry, caps, admit) } else { first };
            steps.push(step);
        }
    }
    steps
}

/// 測れなかった検出線を**同じ [`Fire`] で 1 回だけ**撃ち直し、2 回目の [`Step`] に 1 回目の rc と
/// stderr の末尾を運ばせる（設計 gate-cost.md §21・C10）。
///
/// 撃ち直しは 1 行につき 1 回である——2 回目も rc 2 なら従来どおり「測れなかった」
/// （[`detection_unmeasured`]）で、3 回目は撃たない（値の線を足さない・極性不変・C11.2）。
/// rc 1 の検出線と検出線以外の rc 2 は呼び手が撃ち直さない（判定は [`detection_unmeasured`] の 1 点）。
fn refire(
    first: Step,
    entry: &Fire<'_>,
    caps: Result<confine::Caps, RuleRead>,
    admit: Option<&Admit<'_>>,
) -> Step {
    let mut second = fire(entry, caps, admit);
    second.retried_from = Some((first.rc, first.stderr));
    second
}

/// 共通 verify の行の穴を実値へ置く（**契約の行には置換しない**）。
///
/// **1 走査で埋めない**のは、穴の値が sha と数字だけで、互いの字面を含まないためである
/// （`{worktree}` のように外から来る path を埋める面とは条件が違う）。
fn fill_holes(line: &str, base: &str, jobs: u64) -> String {
    line.replace(BASE_HOLE, base)
        .replace(JOBS_HOLE, &jobs.to_string())
}

/// 1 行を撃つ材料。
struct Fire<'a> {
    /// 撃つ場所と材料。
    checks: &'a Checks<'a>,
    /// **置換前**の行（どの箱に入れるか・受付を通るかはここから決まる）。
    raw: &'a str,
    /// 穴を置換する段か（共通 verify だけ・契約の行は穴を持たない）。
    holes: bool,
    /// 段（scope の unit 名に載る）。
    stage: Check,
    /// `verify.jsonl` の record 番号（scope の unit 名に載る）。
    n: usize,
}

/// 1 行を scope に包んで撃ち、結果を組む。
///
/// `{jobs}` を持つ共通 verify の行は、**撃つ前に受付で枠を取り、撃った後に返す**
/// （設計 §3.2）。実効 jobs = `min(gate.mutants_jobs, 受け付けた枠)`。
fn fire(entry: &Fire<'_>, caps: Result<confine::Caps, RuleRead>, admit: Option<&Admit<'_>>) -> Step {
    let place = entry
        .checks
        .worktree
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let unit = confine::unit_name(&place, entry.stage.as_str(), entry.n);
    let grant = admitted(entry, caps, &unit, admit);
    let jobs = grant.as_ref().map_or(UNADMITTED_JOBS, |held| held.jobs);
    let cmd = if entry.holes {
        fill_holes(entry.raw, entry.checks.base, jobs)
    } else {
        entry.raw.to_owned()
    };
    let wrap = confine::Wrap {
        unit: &unit,
        limit: confine::limit_of(entry.raw, jobs),
        caps,
    };
    let fired = run_line_captured(entry.checks.worktree, &cmd, &wrap);
    let (confined, reason, peak_mb) =
        (fired.confinement.confined(), fired.reason(), fired.usage.peak_mb);
    let (slot, slot_why) = grant
        .as_ref()
        .map_or((None, None), |held| (Some(held.detail.clone()), held.why));
    if let Some(held) = grant {
        admission::release(held);
    }
    Step {
        stage: entry.stage,
        cmd,
        rc: fired.rc,
        stderr: fired.stderr,
        confined,
        reason,
        peak_mb,
        secs: Some(fired.secs),
        jobs,
        slot,
        slot_why,
        scope: fired.scope,
        line: fired.stdout_tail,
        retried_from: None,
    }
}

/// 行が受付を通るなら枠を取る（通らない行は `None`）。
///
/// 通るのは **`{jobs}` を持つ宣言の行**（共通 verify・検出線）だけである（`{jobs}` を持たない行は枠を取らない
/// ＝mutants を持たない consumer は費用を払わない・設計 §3.3）。**包めない周は 1 枠だけを
/// 取りにいく**——箱の無い行に並列度を上げると、溢れたときに殺されるのが席の側になる。
fn admitted(
    entry: &Fire<'_>,
    caps: Result<confine::Caps, RuleRead>,
    unit: &str,
    admit: Option<&Admit<'_>>,
) -> Option<Grant> {
    let admit = admit?;
    if !entry.holes || !entry.raw.contains(JOBS_HOLE) {
        return None;
    }
    // 包めるかは箱の大きさ（jobs ≥ 1）に依らない。撃つ前に同じ判定を 1 度だけ引く。
    let probe = confine::Wrap { unit, limit: confine::limit_of(entry.raw, UNADMITTED_JOBS), caps };
    let (_, confinement) = confine::wrap_line(entry.raw, &probe);
    let want = if confinement.confined() { admit.rules.cap } else { UNADMITTED_JOBS };
    let mut grant = admission::admit(admit.state_dir, admit.run, want, &admit.rules);
    grant.jobs = admit.rules.cap.min(grant.jobs).max(UNADMITTED_JOBS);
    Some(grant)
}

/// diff の path が契約の write-set に収まっているか（ADR-0009 §2.4）。
///
/// hook の guard とは**面が違う**: あちらは編集時に実体（symlink）まで解いて 1 件ずつ止める
/// backstop で、こちらは便が終わった後に git が出した名前を数える gate である。
fn check_write_set(checks: &Checks<'_>) -> Step {
    let cmd = WRITE_SET_CMD.to_owned();
    let range = format!("{}..HEAD", checks.base);
    // **`-z`**（NUL 区切り・quote しない）で受ける。既定の `--name-only` は非 ASCII の path を
    // `"…"` へ quote するので、字面照合が偽の RED を出す（本 repo は日本語の doc を持つ）。
    let Some(bytes) = git_bytes(checks.worktree, &["diff", "--name-only", "-z", &range]) else {
        return unwrapped(cmd, -1, "diff の path を読めない".to_owned());
    };
    let text = String::from_utf8_lossy(&bytes);
    let outside: Vec<&str> = text
        .split('\0')
        .filter(|path| !path.is_empty() && !listed(path, &checks.contract.write_set))
        .collect();
    if outside.is_empty() {
        return unwrapped(cmd, 0, String::new());
    }
    unwrapped(
        cmd,
        1,
        format!("契約の write-set の外へ出た path:\n{}", outside.join("\n")),
    )
}

/// path が write-set のいずれか（file の一致 か dir の prefix）に含まれるか。
///
/// 項目は [`refuse::normalize`] に通してから比べる（接頭辞 `+` / `-` は受付の宣言であって path の一部ではない
/// ＝diff の素の path と照合する・設計 contract-source.md §3・剥がす規則を 2 か所に持たない・`s2-07l.291`）。
fn listed(path: &str, write_set: &[String]) -> bool {
    write_set.iter().any(|entry| {
        let plain = refuse::normalize(entry);
        let trimmed = plain.trim_end_matches('/');
        path == trimmed || path.starts_with(&format!("{trimmed}/"))
    })
}

/// 段①（write-set 照合）を**読めなかった**段か（`s2-07l.65`）。
///
/// 位置（`CHECKS` の宣言順）ではなく **段の名と rc** で見る（順序が変わっても診断が黙って消えない）。
/// rc だけで見ない: 撃った sh が signal で死んだ周も `code()` が無く -1 になる（[`recorded_rc`]）ので、
/// rc -1 の全数を「読めなかった」に倒すと**走って死んだ赤**が「測れなかった」に化ける。
/// gate と land が**同じ 1 本**で判定する（極性を 2 面に持たない・`s2-07l.103`）。
pub fn is_unreadable(step: &Step) -> bool {
    step.cmd == WRITE_SET_CMD && step.rc == -1
}

/// verify 1 行を撃った結果。
pub struct Fired {
    /// process の rc（起動できない周は -1）。
    pub rc: i32,
    /// stderr の末尾（[`STDERR_TAIL_LINES`] 行）。
    pub stderr: String,
    /// 包みが stdout の終端に出した数（包めなかった周は既定）。
    pub usage: Usage,
    /// 行の壁時計（秒・[`Step::secs`] 経由で record の `secs=`・設計 gate-cost.md §26 形 (1)）。
    ///
    /// 測るのは **process の起動から終了まで**で、起動できなかった周も（起動に失敗するまでの）
    /// 秒を持つ——判定は rc のままで、秒は費用の値である（1 便の時間を器が測る・C10）。
    /// 行の終端で scope を片付ける時間（[`confine::release_scope`]）は行の費用ではないので入れない。
    pub secs: u64,
    /// 包めたか。
    pub confinement: Confinement,
    /// 行の終端で scope を片付けた結果（record に書く周だけ `Some`・[`confine::release_scope`]）。
    pub scope: Option<Released>,
    /// stdout の末尾の非空 1 行（包みの終端行を剥がした残り・逐語・無ければ `None`・[`last_line`]）。
    ///
    /// **判定には使わない**（判定は rc である）。record の `line=` へ運ぶだけの値で、起動できなかった
    /// 周は stdout が器の外に無いので `None`。
    pub stdout_tail: Option<String>,
}

impl Fired {
    /// record の `reason=`。
    ///
    /// 包めなかった周はその理由、包めた周は**箱の中で起きたこと**を載せる——`oom_kill` が
    /// 立った周と、包みごと signal で死んだ周（`memory.events` を読む前に死ぬので oom の
    /// 代理・設計 §4.3）を、外からの kill と弁別するためである（lens-132d L1）。
    fn reason(&self) -> Option<Reason> {
        if let Some(found) = self.confinement.reason() {
            return Some(found);
        }
        if self.usage.oom_kill.is_some_and(|count| count >= 1) {
            return Some(Reason::OomKill);
        }
        (self.rc < 0).then_some(Reason::Signal)
    }
}

/// verify 1 行を **cgroup の scope に包んで**撃ち、rc・stderr の末尾・包みの数を得る。
///
/// **撃つ実装はここ 1 本だけ**である（gate も land も [`run_checks`] 経由でここへ来る）。
/// 出力の要る側と要らない側で `Command` を 2 本に割ると、gate が通した行と land が
/// main で撃った行が別の実装になり、意味が静かにずれる。包む口も同じ理由で 1 本である。
///
/// stdout を読むのは**包みの終端行と record の `line=` のため**だけで、判定には使わない（判定は rc である）。
pub fn run_line_captured(worktree: &Path, line: &str, wrap: &confine::Wrap<'_>) -> Fired {
    let (mut command, confinement) = confine::wrap_line(line, wrap);
    // 壁時計は**起動の直前から終了の直後まで**の 1 対で取る（設計 gate-cost.md §26 形 (1)）。
    // 秒は起動できた周も起動できなかった周も同じこの 1 つを運ぶ（下の 2 つの返り口）。
    let started = std::time::Instant::now();
    let spawned = command.current_dir(worktree).output();
    let secs = started.elapsed().as_secs();
    // **行の終端で scope を片付ける**（設計 gate-cost.md §4.4 errata・`s2-07l.234`）。行が孤児の
    // process を残すと scope は active のまま残り、同じ名の次の周の相手になる。判定は変えない。
    let scope = confine::release_scope(&confinement);
    let Ok(out) = spawned else {
        // 起動できなかった周は rc も stderr も**器の外に無い**。空を「何も言わなかった」
        // として返し、極性は従来どおり RED 側（-1）へ倒す。
        return Fired {
            rc: -1,
            stderr: String::new(),
            usage: Usage::default(),
            secs,
            confinement,
            scope,
            stdout_tail: None,
        };
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    // **包めなかった周の stdout は測定として読まない**。素の行が出した `confine-usage` の字面を
    // 包みの測定として読むと、撃たれた行が自分の peak を名乗れてしまう。
    let usage = if confinement.confined() {
        confine::read_usage(&stdout)
    } else {
        Usage::default()
    };
    Fired {
        rc: out.status.code().unwrap_or(-1),
        stderr: tail_of(&String::from_utf8_lossy(&out.stderr)),
        usage,
        secs,
        confinement,
        scope,
        stdout_tail: last_line(&stdout),
    }
}

/// 末尾 [`STDERR_TAIL_LINES`] 行を改行で継いで返す（末尾の改行は行の区切りとして落ちる）。
fn tail_of(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.len().saturating_sub(STDERR_TAIL_LINES);
    lines.get(from..).unwrap_or_default().join("\n")
}

/// stdout の**末尾の非空 1 行**（record の `line=`・設計 gate-cost.md §5.1・pure）。
///
/// 包みの終端行（[`USAGE_HEAD`] で始まる行）は**剥がしてから**取る＝道具の判定行が終端行の
/// 直前に在る周（包めた周の常）にその行を返す。行の中身は逐語（CRLF の `\r` だけ `lines` が
/// 区切りとして落とす）。空白だけの行は非空に数えない。非空の行が 1 つも無い周は `None`
/// （空文字を書かない）。
fn last_line(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .rev()
        .find(|line| {
            let head = line.trim_start();
            !head.is_empty() && !head.starts_with(USAGE_HEAD)
        })
        .map(str::to_owned)
}

/// rc を JSON の非負整数へ写す。`sh` が signal で落ちた周（負）は 255 に畳む。
pub(super) fn recorded_rc(rc: i32) -> u64 {
    u64::try_from(rc).unwrap_or(u64::from(u8::MAX))
}

/// byte 数を数える。
pub(super) fn byte_count(bytes: &[u8]) -> u64 {
    u64::try_from(bytes.len()).unwrap_or(u64::MAX)
}

#[cfg(test)]
pub(crate) mod tests {
    // flip-check: moved s2-07l.286
    use super::super::record::USAGE_HEAD;
    use super::{last_line, listed, run_line_captured, unwrapped, WRITE_SET_CMD};
    use crate::pipe::confine::{read_usage, Limit, Reason, Wrap};
    use crate::seat::RuleRead;
    use std::path::{Path, PathBuf};

    /// write-set の項目は接頭辞（`+` 新規 / `-` 縮む面）を剥がした素の path で照合し、dir 項目（末尾 `/`）は配下を
    /// segment 境界で含む（`s2-07l.291`）。diff の path に接頭辞の字面は来ない＝`+x` の path は `+x` の項目に当たらない。
    #[test]
    fn pipe_gate_write_set_prefixed_items_are_listed_as_plain_paths() {
        let set = |items: &[&str]| items.iter().map(|item| (*item).to_owned()).collect::<Vec<String>>();
        assert!(listed("x", &set(&["+x"])), "`+x` は新規 file `x` の宣言");
        assert!(listed("x", &set(&["-x"])), "`-x` は縮む面 `x` の宣言");
        assert!(listed("dir/a.rs", &set(&["dir/"])), "dir 項目は配下を含む");
        assert!(!listed("dir.rs", &set(&["dir/"])), "dir 項目は segment 境界で外れる");
        assert!(!listed("+x", &set(&["+x"])), "接頭辞は path の一部ではない");
        assert!(!listed("y", &set(&["+x", "-x", "dir/"])), "剥がしても外の path は外");
    }

    /// record の `line=` は stdout の**末尾の非空 1 行**（設計 gate-cost.md §5.1）: 空 / 空白だけ → `None`・
    /// 末尾改行は区切り・包みの終端行は剥がしてその直前の行・CRLF の `\r` は落ちる・空行を跨いで遡る。
    #[test]
    fn pipe_record_last_line_is_the_trailing_nonblank_line_before_the_usage_line() {
        assert_eq!(last_line(""), None, "空は None（空文字を書かない）");
        assert_eq!(last_line("\n  \n"), None, "空白だけの行は非空に数えない");
        assert_eq!(last_line("one\ntwo\n"), Some("two".to_owned()), "末尾改行は区切り");
        assert_eq!(last_line("one\ntwo"), Some("two".to_owned()), "末尾改行の無い周も同じ");
        assert_eq!(
            last_line("noise\nmutants-diff: total=3 caught=2\nconfine-usage peak_bytes=1048576 oom_kill=0\n"),
            Some("mutants-diff: total=3 caught=2".to_owned()),
            "包みの終端行を剥がした直前の行"
        );
        assert_eq!(
            last_line("confine-usage peak_bytes=- oom_kill=0\n"),
            None,
            "終端行しか無い周は None（終端行を判定行に化けさせない）"
        );
        assert_eq!(last_line("one\r\ntwo\r\n"), Some("two".to_owned()), "CRLF の \\r は落ちる");
        assert_eq!(last_line("last\n\n\n"), Some("last".to_owned()), "空行を跨いで遡る");
        assert_eq!(last_line("  padded  \n"), Some("  padded  ".to_owned()), "行の中身は逐語（trim しない）");
    }

    /// ここで剥がす見出しは [`read_usage`] が読む見出しと**同じ字面**である（ずれると終端行が `line` に化ける）。
    #[test]
    fn pipe_record_usage_head_matches_the_confine_reader() {
        let usage = read_usage(&format!("{USAGE_HEAD} peak_bytes=2097152 oom_kill=1\n"));
        assert_eq!(usage.peak_mb, Some(2), "同じ見出しを包みの読み手が測定として読む");
        assert_eq!(usage.oom_kill, Some(1));
    }

    // flip-check: retroactive s2-07l.222
    /// 起動できなかった行は **rc -1**（RED 側の極性・`-` を消すと rc 1 に化ける）で、stderr は空・scope は撃たない。
    /// 起動を Err にする fixture は**存在しない cwd**である——PATH に無い command 名の行は `sh -c` が起動して
    /// rc 127 を返す（spawn は Err にならない・下の対で pin する）。包めない `Wrap`（rules の読めない周）で撃つ
    /// ので `systemd-run` も `systemctl` も起こさない。
    #[test]
    fn mutant_in_pipe_run_line_captured_unspawnable_line_is_minus_one() {
        let root = scratch("unspawnable");
        let wrap = Wrap { unit: "scribe2-mutant-unit", limit: Limit::HostReserve, caps: Err(RuleRead::Missing) };
        let fired = run_line_captured(&root.join("absent-worktree"), "true", &wrap);
        assert_eq!(fired.rc, -1, "起動できない周は -1");
        assert_eq!(fired.stderr, "", "器の外に stderr は無い");
        assert_eq!(fired.confinement.reason(), Some(Reason::NoRules), "包まずに撃った");
        assert_eq!(fired.scope, None, "scope を片付けない");
        assert_eq!(fired.stdout_tail, None, "stdout も器の外に無い");
        let missing = run_line_captured(&root, "scribe2-mutant-no-such-command", &wrap);
        assert_eq!(missing.rc, 127, "PATH に無い command は sh が起動して 127（-1 ではない）");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 段の秒は **process の起動から終了まで**の壁時計である（設計 gate-cost.md §26 形 (1)）: 1 秒眠る行の
    /// [`super::Fired`] は 1 以上を運び、起動できなかった周（存在しない cwd・rc -1 は不変）も秒を持ち、
    /// 撃つ process を持たない段（[`unwrapped`]・write-set 照合）は秒を持たない（`None`＝record は field を欠く）。
    ///
    /// 上限に当てるのは**外から測った秒**だけである——固定の壁時計 bound は負荷の高い host で偽に落ちる
    /// （設計 gate-cost.md §25 の実測）。包めない `Wrap` で撃つので `systemd-run` も `systemctl` も起こさない。
    #[test]
    fn gate_secs_fired_measures_the_wall_clock_of_the_process() {
        let root = scratch("secs");
        let wrap = Wrap { unit: "scribe2-secs-unit", limit: Limit::HostReserve, caps: Err(RuleRead::Missing) };
        let slept = run_line_captured(&root, "sleep 1", &wrap);
        assert_eq!(slept.rc, 0, "行は完走した");
        assert!(slept.secs >= 1, "1 秒眠った行の壁時計は 1 以上: {}", slept.secs);
        let started = std::time::Instant::now();
        let unspawnable = run_line_captured(&root.join("absent-worktree"), "true", &wrap);
        let outer = started.elapsed().as_secs();
        assert_eq!(unspawnable.rc, -1, "起動できない周の極性は -1 のまま");
        assert!(unspawnable.secs <= outer, "起動失敗までの壁時計（外から測った秒を超えない）: {}", unspawnable.secs);
        let matched = unwrapped(WRITE_SET_CMD.to_owned(), 0, String::new());
        assert_eq!(matched.secs, None, "撃つ process を持たない段は秒を持たない（0 と書かない）");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 歯ごとの空の tmp dir（in-file の歯の置き場・env を読まないのは器の本体の規律〔C2.2〕）。
    pub(in crate::pipe::gate) fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gate-verdict-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// dir の直下の名前（名前順）。
    pub(in crate::pipe::gate) fn names(dir: &Path) -> Vec<String> {
        let mut found: Vec<String> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        found.sort();
        found
    }
}
